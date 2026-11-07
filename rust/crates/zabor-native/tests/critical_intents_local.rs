#![cfg(feature = "native-media")]

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Notify};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use zabor_native::audio_mix::AudioControls;
use zabor_native::audio_runtime::AudioDemand;
use zabor_native::audio_settings::AudioSettings;
use zabor_native::critical_intents::{CriticalAction, CriticalIntents, IntentScope};
use zabor_native::hub::HubConnection;
use zabor_native::protocol::{encode, Decoder};

struct Resource(Arc<AtomicUsize>);

impl Drop for Resource {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }
}

#[tokio::test]
async fn leave_drops_inflight_voice_resource_and_fresh_intent_still_runs() {
    timeout(Duration::from_secs(5), async {
        let intents = Arc::new(CriticalIntents::default());
        let ticket = intents.ticket();
        let started = Arc::new(Notify::new());
        let drops = Arc::new(AtomicUsize::new(0));
        let task = {
            let intents = intents.clone();
            let started = started.clone();
            let drops = drops.clone();
            tokio::spawn(async move {
                intents
                    .run(ticket, IntentScope::Voice, async move {
                        let _resource = Resource(drops);
                        started.notify_one();
                        std::future::pending::<u32>().await
                    })
                    .await
            })
        };
        started.notified().await;
        intents.request(CriticalAction::Leave);
        assert_eq!(
            timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap(),
            None
        );
        assert_eq!(drops.load(Ordering::Acquire), 1);
        assert_eq!(intents.next().await, CriticalAction::Leave);
        assert_eq!(
            intents
                .run(intents.ticket(), IntentScope::Voice, async { 42 })
                .await,
            Some(42)
        );
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn logout_cancels_session_resource_even_after_a_pending_leave() {
    timeout(Duration::from_secs(5), async {
        let intents = Arc::new(CriticalIntents::default());
        let ticket = intents.ticket();
        let started = Arc::new(Notify::new());
        let drops = Arc::new(AtomicUsize::new(0));
        let task = {
            let intents = intents.clone();
            let started = started.clone();
            let drops = drops.clone();
            tokio::spawn(async move {
                intents
                    .run(ticket, IntentScope::Session, async move {
                        let _resource = Resource(drops);
                        started.notify_one();
                        std::future::pending::<u32>().await
                    })
                    .await
            })
        };
        started.notified().await;
        intents.request(CriticalAction::Leave);
        assert!(intents.is_current(ticket, IntentScope::Session));
        intents.request(CriticalAction::Logout);
        assert_eq!(
            timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap(),
            None
        );
        assert_eq!(drops.load(Ordering::Acquire), 1);
        assert_eq!(intents.next().await, CriticalAction::Logout);
    })
    .await
    .unwrap();
}

#[test]
fn immediate_voice_demand_reset_preserves_settings_and_old_drop_cannot_revoke_replacement() {
    let controls = Arc::new(AudioControls::new(AudioSettings::default()));
    let demand = AudioDemand::new(controls.clone());
    let old = demand.reserve().unwrap();
    assert!(demand.activate(old));
    let state = controls.user_state();
    let playback_epoch = controls.playback_epoch.load(Ordering::Acquire);
    demand.reset();
    assert_eq!(demand.active(), 0);
    assert!(!demand.has_voice());
    assert_eq!(demand.with_voice(old, || true), None);
    assert_eq!(controls.user_state(), state);
    assert_eq!(
        controls.playback_epoch.load(Ordering::Acquire),
        playback_epoch
    );
    let fresh = demand.reserve().unwrap();
    assert!(demand.activate(fresh));
    demand.revoke(old);
    assert_eq!(demand.with_voice(fresh, || true), Some(true));
}

type Fixture = (
    String,
    Arc<Notify>,
    Arc<Notify>,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<Vec<String>>,
);

async fn fixture() -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/zabor_v3", listener.local_addr().unwrap());
    let seen = Arc::new(Notify::new());
    let ended = Arc::new(Notify::new());
    let seen_server = seen.clone();
    let ended_server = ended.clone();
    let (release, release_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let mut release_rx = Some(release_rx);
        let (mut http, _) = listener.accept().await.unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            header.push(http.read_u8().await.unwrap());
            assert!(header.len() < 8192);
        }
        let body = json!({"connectionToken":"critical-fixture","negotiateVersion":1,"availableTransports":[{"transport":"WebSockets","transferFormats":["Text"]}]}).to_string();
        http.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        drop(http);
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap().into_text().unwrap(),
            "{\"protocol\":\"json\",\"version\":1}\u{1e}"
        );
        socket.send(Message::text("{}\u{1e}")).await.unwrap();
        let mut decoder = Decoder::default();
        let mut methods = Vec::new();
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            for request in decoder.push(text.as_bytes()).unwrap() {
                if request["type"] == 6 {
                    continue;
                }
                let method = request["target"].as_str().unwrap();
                methods.push(method.into());
                match method {
                    "Hold" => {
                        seen_server.notify_one();
                        release_rx.take().unwrap().await.unwrap();
                        socket.send(Message::text(encode(&json!({"type":3,"invocationId":request["invocationId"],"result":true})).unwrap())).await.unwrap();
                    }
                    "EndCall" => {
                        assert!(request.get("invocationId").is_none());
                        ended_server.notify_one();
                    }
                    _ => panic!("Unexpected critical fixture request"),
                }
            }
        }
        methods
    });
    (endpoint, seen, ended, release, task)
}

#[tokio::test]
async fn canceled_actual_hub_voice_wait_ignores_late_completion_and_endcall_is_sent() {
    timeout(Duration::from_secs(10), async {
        let (endpoint, seen, ended, release, server) = fixture().await;
        let (hub, _events) = HubConnection::connect(&endpoint, None).await.unwrap();
        let intents = Arc::new(CriticalIntents::default());
        let ticket = intents.ticket();
        let task = {
            let intents = intents.clone();
            let hub = hub.clone();
            tokio::spawn(async move {
                intents
                    .run(
                        ticket,
                        IntentScope::Voice,
                        hub.invoke("Hold", vec![], Duration::from_secs(30)),
                    )
                    .await
            })
        };
        seen.notified().await;
        intents.request(CriticalAction::Leave);
        assert!(timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .is_none());
        assert_eq!(intents.next().await, CriticalAction::Leave);
        hub.send("EndCall", vec![]).await.unwrap();
        release.send(()).unwrap();
        ended.notified().await;
        hub.close();
        assert_eq!(server.await.unwrap(), vec!["Hold", "EndCall"]);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn leave_does_not_restart_or_cancel_unrelated_actual_hub_session_wait() {
    timeout(Duration::from_secs(10), async {
        let (endpoint, seen, _ended, release, server) = fixture().await;
        let (hub, _events) = HubConnection::connect(&endpoint, None).await.unwrap();
        let intents = Arc::new(CriticalIntents::default());
        let ticket = intents.ticket();
        let task = {
            let intents = intents.clone();
            let hub = hub.clone();
            tokio::spawn(async move {
                intents
                    .run(
                        ticket,
                        IntentScope::Session,
                        hub.invoke("Hold", vec![], Duration::from_secs(30)),
                    )
                    .await
            })
        };
        seen.notified().await;
        intents.request(CriticalAction::Leave);
        assert_eq!(intents.next().await, CriticalAction::Leave);
        release.send(()).unwrap();
        assert_eq!(task.await.unwrap(), Some(Ok(json!(true))));
        hub.close();
        assert_eq!(server.await.unwrap(), vec!["Hold"]);
    })
    .await
    .unwrap();
}
