use super::*;
use crate::audio_settings::AudioSettings;
use crate::call_recovery::{CallRecovery, RecoveryAction, ResumeResult};
use crate::critical_intents::{CriticalAction, CriticalIntents, IntentScope};
use crate::protocol::{encode, Decoder};
use futures_util::{SinkExt, StreamExt};
use libwebrtc::data_channel::DataChannelInit;
use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

const CALL: &str = "0123456789abcdef0123456789abcdef";

enum Reply {
    Value(Value),
    Error,
    Hold,
    Delay(Duration),
    Close,
}

struct Fixture {
    endpoint: String,
    seen: mpsc::UnboundedReceiver<Value>,
    release: Arc<Notify>,
    server: tokio::task::JoinHandle<Vec<Value>>,
}

async fn fixture(replies: impl IntoIterator<Item = Reply>) -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/zabor_v3", listener.local_addr().unwrap());
    let mut replies: VecDeque<_> = replies.into_iter().collect();
    let (seen, received) = mpsc::unbounded_channel();
    let release = Arc::new(Notify::new());
    let server_release = release.clone();
    let server = tokio::spawn(async move {
        let (mut http, _) = listener.accept().await.unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            header.push(http.read_u8().await.unwrap());
            assert!(header.len() < 8192);
        }
        assert!(String::from_utf8(header)
            .unwrap()
            .starts_with("POST /zabor_v3/negotiate?negotiateVersion=1 "));
        let body = json!({"connectionToken":"local-stream-restore","negotiateVersion":1,"availableTransports":[{"transport":"WebSockets","transferFormats":["Text"]}]}).to_string();
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
        let mut requests = Vec::new();
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            for request in decoder.push(text.as_bytes()).unwrap() {
                if request["type"] == 6 {
                    continue;
                }
                seen.send(request.clone()).unwrap();
                requests.push(request.clone());
                if request.get("invocationId").is_none() {
                    continue;
                }
                let result = match request["target"].as_str().unwrap() {
                    "StartStream" => match replies.pop_front().expect("Unexpected StartStream") {
                        Reply::Value(value) => value,
                        Reply::Error => {
                            let error = json!({"type":3,"invocationId":request["invocationId"],"error":"fixture refusal"});
                            if socket
                                .send(Message::text(encode(&error).unwrap()))
                                .await
                                .is_err()
                            {
                                return requests;
                            }
                            continue;
                        }
                        Reply::Hold => {
                            server_release.notified().await;
                            json!(true)
                        }
                        Reply::Delay(duration) => {
                            tokio::time::sleep(duration).await;
                            json!(true)
                        }
                        Reply::Close => {
                            socket.close(None).await.unwrap();
                            return requests;
                        }
                    },
                    "ResumeCall" => {
                        assert_eq!(request["arguments"], json!([CALL]));
                        json!({"state":"resumed","peer":{"id":"peer","username":"fixture","displayName":"Fixture"}})
                    }
                    other => panic!("Unexpected invoke {other}"),
                };
                let response =
                    json!({"type":3,"invocationId":request["invocationId"],"result":result});
                if socket
                    .send(Message::text(encode(&response).unwrap()))
                    .await
                    .is_err()
                {
                    return requests;
                }
            }
        }
        requests
    });
    Fixture {
        endpoint,
        seen: received,
        release,
        server,
    }
}

fn session() -> VoiceSession {
    let controls = Arc::new(AudioControls::new(AudioSettings::default()));
    let shared = SharedAudio::new(controls.clone(), CancellationToken::new(), |_, _, _| {
        Box::pin(async { panic!("Stream restore fixture must not open physical audio") })
    });
    let (engine, _events) = MediaEngine::new("owner".into());
    VoiceSession {
        engine,
        local_id: "owner".into(),
        peers: ["peer".into()].into_iter().collect(),
        channel: Some("fixture-channel".into()),
        call_peer: None,
        relay_only: false,
        capture: None,
        controls,
        audio_runtime: shared,
        audio: None,
        playback: None,
    }
}

fn share(session: &mut VoiceSession, quality: &str) -> (Arc<AtomicBool>, Arc<AtomicBool>) {
    let (capture, stop, failed) = ScreenCapture::synthetic();
    session
        .engine
        .set_video_with_quality(capture.source.clone(), quality)
        .unwrap();
    session.capture = Some(ScreenShare {
        capture: ShareCapture::Desktop(capture),
        quality: quality.into(),
    });
    (stop, failed)
}

fn peer(session: &mut VoiceSession) -> PeerLifetime {
    session.engine.authorize_peers(["peer".into()]);
    session
        .engine
        .create_data_channel("peer", "local-stream-restore", DataChannelInit::default())
        .unwrap();
    session.engine.peer_lifetime("peer").unwrap()
}

fn assert_retained(
    session: &VoiceSession,
    track: &libwebrtc::video_track::RtcVideoTrack,
    lifetime: &PeerLifetime,
) {
    assert!(track.enabled());
    assert!(session.engine.has_live_video());
    assert!(lifetime.is_alive());
    assert!(session
        .engine
        .peer_lifetime("peer")
        .unwrap()
        .is_same(lifetime));
    assert!(session.is_sharing());
}

#[tokio::test]
async fn stream_reannouncement_keeps_original_quality_capture_track_and_restored_peer() {
    timeout(Duration::from_secs(15), async {
        for call in [false, true] {
            for quality in ["low", "medium", "high"] {
                let fixture = fixture([Reply::Value(json!(true)), Reply::Value(json!(true))]).await;
                let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
                    .await
                    .unwrap();
                let mut session = session();
                if call {
                    session.channel = None;
                    session.call_peer = Some("peer".into());
                }
                share(&mut session, quality);
                let lifetime = peer(&mut session);
                let track = session.engine.local_video().unwrap();
                let original = &session.capture.as_ref().unwrap().capture;
                for _ in 0..2 {
                    assert!(session.restore_share(&hub).await.unwrap());
                    assert_retained(&session, &track, &lifetime);
                    assert!(std::ptr::eq(
                        original,
                        &session.capture.as_ref().unwrap().capture
                    ));
                }
                hub.close();
                let requests = fixture.server.await.unwrap();
                assert_eq!(requests.len(), 2);
                for request in requests {
                    assert_eq!(request["type"], 1);
                    assert_eq!(request["target"], "StartStream");
                    assert_eq!(request["arguments"], json!([quality]));
                    assert!(request["invocationId"].is_string());
                }
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn refusal_and_invalid_completion_preserve_membership_capture_and_retry_quality() {
    timeout(Duration::from_secs(10), async {
        let fixture = fixture([
            Reply::Value(json!(false)),
            Reply::Error,
            Reply::Value(Value::Null),
            Reply::Value(json!("true")),
            Reply::Value(json!(true)),
        ])
        .await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let mut session = session();
        share(&mut session, "high");
        let lifetime = peer(&mut session);
        let track = session.engine.local_video().unwrap();
        for _ in 0..4 {
            assert!(matches!(
                session.restore_share(&hub).await,
                Err(MediaError::Backend)
            ));
            assert_retained(&session, &track, &lifetime);
            assert_eq!(session.channel.as_deref(), Some("fixture-channel"));
        }
        assert!(session.restore_share(&hub).await.unwrap());
        assert_retained(&session, &track, &lifetime);
        hub.close();
        let requests = fixture.server.await.unwrap();
        assert_eq!(requests.len(), 5);
        assert!(requests
            .iter()
            .all(|request| request["target"] == "StartStream"
                && request["arguments"] == json!(["high"])));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn network_failure_does_not_retire_capture_or_peer_and_a_new_hub_can_retry() {
    timeout(Duration::from_secs(10), async {
        let mut session = session();
        share(&mut session, "medium");
        let lifetime = peer(&mut session);
        let track = session.engine.local_video().unwrap();
        let closed = fixture([Reply::Close]).await;
        let (hub, _events) = HubConnection::connect(&closed.endpoint, None)
            .await
            .unwrap();
        assert!(matches!(
            session.restore_share(&hub).await,
            Err(MediaError::Backend)
        ));
        assert_retained(&session, &track, &lifetime);
        assert_eq!(closed.server.await.unwrap().len(), 1);
        hub.close();
        let retry = fixture([Reply::Value(json!(true))]).await;
        let (hub, _events) = HubConnection::connect(&retry.endpoint, None).await.unwrap();
        assert!(session.restore_share(&hub).await.unwrap());
        assert_retained(&session, &track, &lifetime);
        hub.close();
        let requests = retry.server.await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["arguments"], json!(["medium"]));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn absent_failed_stopped_disabled_removed_and_unowned_streams_are_not_announced() {
    timeout(Duration::from_secs(10), async {
        let fixture = fixture([]).await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let mut session = session();
        assert!(!session.restore_share(&hub).await.unwrap());
        let (stop, failed) = share(&mut session, "low");
        failed.store(true, Ordering::Release);
        assert!(!session.restore_share(&hub).await.unwrap());
        failed.store(false, Ordering::Release);
        stop.store(true, Ordering::Release);
        assert!(!session.restore_share(&hub).await.unwrap());
        share(&mut session, "low");
        let track = session.engine.local_video().unwrap();
        track.set_enabled(false);
        assert!(!session.restore_share(&hub).await.unwrap());
        track.set_enabled(true);
        session.engine.set_video(None).unwrap();
        assert!(!session.restore_share(&hub).await.unwrap());
        share(&mut session, "low");
        session.channel = None;
        assert!(!session.restore_share(&hub).await.unwrap());
        session.channel = Some("fixture-channel".into());
        session.stop_share(&hub).await;
        assert!(!session.is_sharing());
        assert!(!session.restore_share(&hub).await.unwrap());
        hub.close();
        let requests = fixture.server.await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["target"], "StopStream");
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn leave_and_logout_cancel_stream_wait_under_the_original_voice_ticket() {
    timeout(Duration::from_secs(10), async {
        for action in [CriticalAction::Leave, CriticalAction::Logout] {
            for before in [false, true] {
                let mut fixture = fixture([Reply::Hold]).await;
                let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
                    .await
                    .unwrap();
                let mut session = session();
                share(&mut session, "low");
                let lifetime = peer(&mut session);
                let track = session.engine.local_video().unwrap();
                let intents = CriticalIntents::default();
                let ticket = intents.ticket();
                let owner = intents.session_token(ticket);
                let result = if before {
                    intents.request(action);
                    intents
                        .run(ticket, IntentScope::Voice, session.restore_share(&hub))
                        .await
                } else {
                    let (result, ()) = tokio::join!(
                        intents.run(ticket, IntentScope::Voice, session.restore_share(&hub)),
                        async {
                            let request = fixture.seen.recv().await.unwrap();
                            assert_eq!(request["target"], "StartStream");
                            intents.request(action);
                        }
                    );
                    result
                };
                assert!(result.is_none());
                assert!(!intents.is_current(ticket, IntentScope::Voice));
                assert_eq!(owner.is_cancelled(), action == CriticalAction::Logout);
                assert_retained(&session, &track, &lifetime);
                fixture.release.notify_one();
                hub.close();
                let requests = fixture.server.await.unwrap();
                assert_eq!(requests.len(), usize::from(!before));
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn capture_failure_during_stream_wait_is_not_reported_as_a_live_restoration() {
    timeout(Duration::from_secs(10), async {
        let mut fixture = fixture([Reply::Hold]).await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let mut session = session();
        let (_, failed) = share(&mut session, "high");
        let lifetime = peer(&mut session);
        let track = session.engine.local_video().unwrap();
        let (result, ()) = tokio::join!(session.restore_share(&hub), async {
            assert_eq!(fixture.seen.recv().await.unwrap()["target"], "StartStream");
            failed.store(true, Ordering::Release);
            fixture.release.notify_one();
        });
        assert!(!result.unwrap());
        assert!(session.share_failed());
        assert_retained(&session, &track, &lifetime);
        session.stop_share(&hub).await;
        assert!(!session.restore_share(&hub).await.unwrap());
        hub.close();
        let requests = fixture.server.await.unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["target"] == "StartStream")
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["target"] == "StopStream")
                .count(),
            1
        );
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn resumed_call_is_committed_before_slow_stream_restoration_outside_call_budget() {
    timeout(Duration::from_secs(20), async {
        for scheduled in [false, true] {
            let fixture = fixture([Reply::Delay(Duration::from_millis(5200))]).await;
            let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
                .await
                .unwrap();
            let mut session = session();
            session.channel = None;
            session.call_peer = Some("peer".into());
            share(&mut session, "high");
            let previous = peer(&mut session);
            let track = session.engine.local_video().unwrap();
            let mut recovery = CallRecovery::default();
            recovery.begin("peer");
            assert!(recovery.connected("peer"));
            assert!(recovery.suspended(CALL, 45_000, Instant::now()));
            if scheduled {
                let result = crate::call_recovery::resume(&hub, CALL, Duration::from_secs(5))
                    .await
                    .unwrap();
                assert!(matches!(result, ResumeResult::Resumed { peer } if peer.id == "peer"));
            }
            timeout(
                Duration::from_secs(5),
                session.resume_call(&hub, "peer", vec![]),
            )
            .await
            .unwrap()
            .unwrap();
            assert!(!previous.is_alive());
            let restored = session.engine.peer_lifetime("peer").unwrap();
            assert!(recovery.restored(CALL, "peer", Instant::now()));
            let started = Instant::now();
            assert!(session.restore_share(&hub).await.unwrap());
            assert!(started.elapsed() >= Duration::from_secs(5));
            assert_eq!(recovery.poll(Instant::now(), true), RecoveryAction::Wait);
            assert_eq!(recovery.next_deadline(), None);
            assert_retained(&session, &track, &restored);
            assert_eq!(session.call_peer.as_deref(), Some("peer"));
            hub.close();
            let requests = fixture.server.await.unwrap();
            let methods: Vec<_> = requests
                .iter()
                .map(|request| request["target"].as_str().unwrap())
                .collect();
            assert_eq!(
                methods,
                if scheduled {
                    vec!["ResumeCall", "SendWebRTCOffer", "StartStream"]
                } else {
                    vec!["SendWebRTCOffer", "StartStream"]
                }
            );
            assert_eq!(requests.last().unwrap()["arguments"], json!(["high"]));
        }
    })
    .await
    .unwrap();
}

#[cfg(feature = "camera-test-source")]
#[tokio::test]
async fn camera_start_and_reannouncement_keep_default_quality_original_capture_and_track() {
    timeout(Duration::from_secs(10), async {
        for call in [false, true] {
            let fixture = fixture([Reply::Value(json!(true)), Reply::Value(json!(true))]).await;
            let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
                .await
                .unwrap();
            let mut session = session();
            session.peers.clear();
            if call {
                session.channel = None;
                session.call_peer = Some("peer".into());
            }
            let runtime = session.audio_runtime.clone();
            let capture = CameraCapture::synthetic(runtime.cleanup_tracker(), 2, 150)
                .await
                .unwrap();
            session
                .announce_share(&hub, ShareCapture::Camera(capture), "camera")
                .await
                .unwrap();
            let lifetime = peer(&mut session);
            let track = session.engine.local_video().unwrap();
            let original = &session.capture.as_ref().unwrap().capture;
            assert!(session.restore_share(&hub).await.unwrap());
            assert!(std::ptr::eq(
                original,
                &session.capture.as_ref().unwrap().capture
            ));
            assert_retained(&session, &track, &lifetime);
            session.stop_share(&hub).await;
            assert!(!session.is_sharing());
            assert!(!track.enabled());
            runtime.seal_cleanup().unwrap();
            runtime.wait_for_cleanup().await.unwrap();
            hub.close();
            let requests = fixture.server.await.unwrap();
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| request["target"] == "StartStream")
                    .count(),
                2
            );
            assert!(requests
                .iter()
                .filter(|request| request["target"] == "StartStream")
                .all(|request| request["arguments"] == json!(["camera"])));
            assert_eq!(requests.last().unwrap()["target"], "StopStream");
        }
    })
    .await
    .unwrap();
}

#[cfg(feature = "camera-test-source")]
#[tokio::test]
async fn camera_stream_refusal_preserves_membership_and_retained_camera_can_reannounce() {
    timeout(Duration::from_secs(10), async {
        let fixture = fixture([
            Reply::Value(json!(true)),
            Reply::Value(json!(false)),
            Reply::Value(json!(true)),
        ])
        .await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let mut session = session();
        session.peers.clear();
        let runtime = session.audio_runtime.clone();
        let capture = CameraCapture::synthetic(runtime.cleanup_tracker(), 0, 0)
            .await
            .unwrap();
        session
            .announce_share(&hub, ShareCapture::Camera(capture), "camera")
            .await
            .unwrap();
        let lifetime = peer(&mut session);
        let track = session.engine.local_video().unwrap();
        assert!(session.restore_share(&hub).await.is_err());
        assert_retained(&session, &track, &lifetime);
        assert_eq!(session.channel.as_deref(), Some("fixture-channel"));
        assert!(session.restore_share(&hub).await.unwrap());
        assert_retained(&session, &track, &lifetime);
        drop(session);
        runtime.seal_cleanup().unwrap();
        runtime.wait_for_cleanup().await.unwrap();
        hub.close();
        let requests = fixture.server.await.unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests
            .iter()
            .all(|request| request["target"] == "StartStream"
                && request["arguments"] == json!(["camera"])));
    })
    .await
    .unwrap();
}
