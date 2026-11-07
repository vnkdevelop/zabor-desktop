use super::{send_chat_packets, SessionWork, UiEvent};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use zabor_native::chat::OutboundPacket;
use zabor_native::critical_intents::{CriticalAction, CriticalIntents, IntentScope};
use zabor_native::files::FileBroker;
use zabor_native::hub::HubConnection;
use zabor_native::protocol::{encode, Decoder};

struct Fixture {
    endpoint: String,
    seen: Arc<Notify>,
    release: Arc<Notify>,
    answered: Arc<Notify>,
    server: tokio::task::JoinHandle<Vec<String>>,
}

async fn fixture() -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/zabor_v3", listener.local_addr().unwrap());
    let seen = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let answered = Arc::new(Notify::new());
    let server_seen = seen.clone();
    let server_release = release.clone();
    let server_answered = answered.clone();
    let server = tokio::spawn(async move {
        let (mut http, _) = listener.accept().await.unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            header.push(http.read_u8().await.unwrap());
            assert!(header.len() < 8192);
        }
        let body = json!({"connectionToken":"session-fixture","negotiateVersion":1,"availableTransports":[{"transport":"WebSockets","transferFormats":["Text"]}]}).to_string();
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
                        server_seen.notify_one();
                        server_release.notified().await;
                        socket.send(Message::text(encode(&json!({"type":3,"invocationId":request["invocationId"],"result":true})).unwrap())).await.unwrap();
                        server_answered.notify_one();
                    }
                    "SendChatRelay" => {
                        assert!(request.get("invocationId").is_none());
                        assert_eq!(request["arguments"], json!(["peer", "session-packet"]));
                    }
                    _ => panic!("Unexpected session fixture request"),
                }
            }
        }
        methods
    });
    Fixture {
        endpoint,
        seen,
        release,
        answered,
        server,
    }
}

fn packet() -> OutboundPacket {
    OutboundPacket {
        peer: "peer".into(),
        payload: "session-packet".into(),
        direct: None,
    }
}

#[tokio::test]
async fn logout_cancels_actual_session_wait_and_drops_staged_file_without_tail_publication() {
    timeout(Duration::from_secs(10), async {
        let fixture = fixture().await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("original.bin");
        let cache = root.path().join("cache");
        std::fs::write(&source, b"original data").unwrap();
        let staged = FileBroker::new(&cache)
            .unwrap()
            .stage_cancellable(&source, &CancellationToken::new())
            .unwrap();
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 1);
        let intents = Arc::new(CriticalIntents::default());
        let ticket = intents.ticket();
        let published = Arc::new(Mutex::new(Vec::new()));
        let task = {
            let intents = intents.clone();
            let hub = hub.clone();
            let published = published.clone();
            tokio::spawn(async move {
                let stop = CancellationToken::new();
                let update = |event| published.lock().unwrap().push(event);
                let work = SessionWork::new(&intents, ticket, &stop, &update);
                work.run(async {
                    let _staged = staged;
                    let result = hub.invoke("Hold", vec![], Duration::from_secs(30)).await;
                    send_chat_packets(&work, &hub, None, vec![packet()]).await;
                    work.publish(UiEvent::ChooseDestination("peer".into(), "old-file".into()));
                    result
                })
                .await
            })
        };
        fixture.seen.notified().await;
        intents.request(CriticalAction::Logout);
        assert!(timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .is_none());
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
        assert_eq!(std::fs::read(&source).unwrap(), b"original data");
        assert!(published.lock().unwrap().is_empty());
        assert_eq!(intents.next().await, CriticalAction::Logout);
        fixture.release.notify_one();
        fixture.answered.notified().await;
        hub.close();
        assert_eq!(fixture.server.await.unwrap(), vec!["Hold"]);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn repeated_leave_preserves_actual_session_future_packets_and_guarded_publication() {
    timeout(Duration::from_secs(10), async {
        let fixture = fixture().await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None).await.unwrap();
        let intents = Arc::new(CriticalIntents::default());
        let ticket = intents.ticket();
        let published = Arc::new(Mutex::new(Vec::new()));
        let task = {
            let intents = intents.clone();
            let hub = hub.clone();
            let published = published.clone();
            tokio::spawn(async move {
                let stop = CancellationToken::new();
                let update = |event| published.lock().unwrap().push(event);
                let work = SessionWork::new(&intents, ticket, &stop, &update);
                work.run(async {
                    let result = hub.invoke("Hold", vec![], Duration::from_secs(30)).await;
                    send_chat_packets(&work, &hub, None, vec![packet()]).await;
                    work.publish(UiEvent::ChatMessages("peer".into(), vec![], true));
                    result
                }).await
            })
        };
        fixture.seen.notified().await;
        for _ in 0..3 {
            intents.request(CriticalAction::Leave);
            assert_eq!(intents.next().await, CriticalAction::Leave);
        }
        assert!(intents.is_current(ticket, IntentScope::Session));
        fixture.release.notify_one();
        assert_eq!(task.await.unwrap(), Some(Ok(Value::Bool(true))));
        let event = published.lock().unwrap().pop().unwrap();
        assert!(matches!(event.for_current(&intents), Some(UiEvent::ChatMessages(peer, _, true)) if peer == "peer"));
        hub.close();
        assert_eq!(fixture.server.await.unwrap(), vec!["Hold", "SendChatRelay"]);
    }).await.unwrap();
}

#[tokio::test]
async fn stale_session_packet_batch_never_reaches_actual_hub_after_logout_is_consumed() {
    timeout(Duration::from_secs(10), async {
        let fixture = fixture().await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let intents = CriticalIntents::default();
        let ticket = intents.ticket();
        let stop = CancellationToken::new();
        let update = |_| panic!("Stale session publication");
        let work = SessionWork::new(&intents, ticket, &stop, &update);
        intents.request(CriticalAction::Logout);
        assert_eq!(intents.next().await, CriticalAction::Logout);
        send_chat_packets(&work, &hub, None, vec![packet(), packet()]).await;
        work.publish(UiEvent::Toast("chat.file.failed"));
        hub.close();
        assert!(fixture.server.await.unwrap().is_empty());
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn stopped_window_cancels_actual_session_wait_without_tail_effects() {
    timeout(Duration::from_secs(10), async {
        let fixture = fixture().await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let stop = CancellationToken::new();
        let task = {
            let hub = hub.clone();
            let stop = stop.clone();
            tokio::spawn(async move {
                let intents = CriticalIntents::default();
                let update = |_| panic!("Stopped window publication");
                let work = SessionWork::new(&intents, intents.ticket(), &stop, &update);
                work.run(async {
                    hub.invoke("Hold", vec![], Duration::from_secs(30))
                        .await
                        .unwrap();
                    work.publish(UiEvent::FileProgress("peer".into(), "file".into(), 1.0));
                })
                .await
            })
        };
        fixture.seen.notified().await;
        stop.cancel();
        assert_eq!(
            timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap(),
            None
        );
        fixture.release.notify_one();
        fixture.answered.notified().await;
        hub.close();
        assert_eq!(fixture.server.await.unwrap(), vec!["Hold"]);
    })
    .await
    .unwrap();
}

#[test]
fn queued_session_ui_results_survive_leave_but_cannot_cross_owner_boundary() {
    let intents = CriticalIntents::default();
    let stop = CancellationToken::new();
    let published = Mutex::new(Vec::new());
    let update = |event| published.lock().unwrap().push(event);
    let work = SessionWork::new(&intents, intents.ticket(), &stop, &update);
    work.publish(UiEvent::ChatMessages("peer".into(), vec![], true));
    work.publish(UiEvent::ChooseDestination("peer".into(), "file".into()));
    work.publish(UiEvent::FileProgress("peer".into(), "file".into(), 0.5));
    work.publish(UiEvent::Toast("chat.keyChanged"));
    intents.request(CriticalAction::Leave);
    assert!(published.lock().unwrap().iter().all(|event| matches!(event, UiEvent::Guarded(ticket, IntentScope::Session, _) if intents.is_current(*ticket, IntentScope::Session))));
    intents.request(CriticalAction::Logout);
    for event in published.lock().unwrap().drain(..) {
        assert!(event.for_current(&intents).is_none());
    }
    work.publish(UiEvent::ChatMessages("peer".into(), vec![], true));
    assert!(published.lock().unwrap().is_empty());
}

#[tokio::test]
async fn stopped_ready_session_result_is_discarded_in_the_same_poll() {
    let intents = CriticalIntents::default();
    let stop = CancellationToken::new();
    let update = |_| panic!("Stopped session publication");
    let work = SessionWork::new(&intents, intents.ticket(), &stop, &update);
    assert_eq!(
        work.run(async {
            stop.cancel();
            42
        })
        .await,
        None
    );
    work.publish(UiEvent::ChatMessages("peer".into(), vec![], true));
}
