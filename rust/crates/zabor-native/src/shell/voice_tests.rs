use super::{FrameUpdate, UiEvent, VoiceWork};
use futures_util::{SinkExt, StreamExt};
use libwebrtc::data_channel::DataChannelInit;
use libwebrtc::media_stream_track::MediaStreamTrack;
use libwebrtc::video_frame::{I420Buffer, VideoFrame, VideoRotation};
use libwebrtc::video_source::{native::NativeVideoSource, VideoResolution};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Notify};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use zabor_native::critical_intents::{CriticalAction, CriticalIntents, IntentScope, IntentTicket};
use zabor_native::hub::HubConnection;
use zabor_native::media::{MediaEngine, MediaEvent, PeerLifetime};
use zabor_native::protocol::Decoder;
use zabor_native::video::{VideoDelivery, VideoReceiver};

struct Fixture {
    endpoint: String,
    held: Arc<Notify>,
    server: tokio::task::JoinHandle<Vec<Value>>,
}

async fn fixture() -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/zabor_v3", listener.local_addr().unwrap());
    let held = Arc::new(Notify::new());
    let server_held = held.clone();
    let server = tokio::spawn(async move {
        let (mut http, _) = listener.accept().await.unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            header.push(http.read_u8().await.unwrap());
            assert!(header.len() < 8192);
        }
        let body = json!({"connectionToken":"voice-work-fixture","negotiateVersion":1,"availableTransports":[{"transport":"WebSockets","transferFormats":["Text"]}]}).to_string();
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
                if request["target"] == "Hold" {
                    server_held.notify_one();
                }
                requests.push(request);
            }
        }
        requests
    });
    Fixture {
        endpoint,
        held,
        server,
    }
}

fn peer() -> (MediaEngine, PeerLifetime) {
    let (mut engine, _) = MediaEngine::new_data_only("owner".into());
    engine.authorize_peers(["peer".into()]);
    engine
        .create_data_channel("peer", "voice-work-fixture", DataChannelInit::default())
        .unwrap();
    let lifetime = engine.peer_lifetime("peer").unwrap();
    (engine, lifetime)
}

#[tokio::test]
async fn leave_and_logout_cancel_actual_stalled_voice_wait_without_running_its_tail() {
    timeout(Duration::from_secs(10), async {
        for action in [CriticalAction::Leave, CriticalAction::Logout] {
            let fixture = fixture().await;
            let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
                .await
                .unwrap();
            let (mut engine, lifetime) = peer();
            let intents = Arc::new(CriticalIntents::default());
            let ticket = intents.ticket();
            let owner = intents.session_token(ticket);
            let tails = Arc::new(AtomicUsize::new(0));
            let task = {
                let hub = hub.clone();
                let intents = intents.clone();
                let tails = tails.clone();
                tokio::spawn(async move {
                    let stop = CancellationToken::new();
                    let update = |_| panic!("Revoked voice work must not publish");
                    let work = VoiceWork::new(&intents, ticket, &stop, &update);
                    work.run_peer(&lifetime, async {
                        hub.invoke("Hold", vec![], Duration::from_secs(30))
                            .await
                            .unwrap();
                        tails.fetch_add(1, Ordering::AcqRel);
                        work.publish_peer(&lifetime, UiEvent::Sharing(true));
                    })
                    .await
                })
            };
            fixture.held.notified().await;
            let started = Instant::now();
            intents.request(action);
            assert_eq!(
                timeout(Duration::from_millis(1500), task)
                    .await
                    .unwrap()
                    .unwrap(),
                None
            );
            assert_eq!(tails.load(Ordering::Acquire), 0);
            assert_eq!(owner.is_cancelled(), action == CriticalAction::Logout);
            assert_eq!(intents.next().await, action);
            hub.close();
            let requests = fixture.server.await.unwrap();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0]["target"], "Hold");
            engine.close();
            println!(
                "voice wait action={action:?} cancellation_ms={} tail_runs=0 physical_audio=false",
                started.elapsed().as_millis()
            );
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn peer_replacement_cancels_stalled_voice_wait_without_revoking_owner_or_new_peer() {
    timeout(Duration::from_secs(10), async {
        let fixture = fixture().await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let (mut engine, lifetime) = peer();
        let intents = Arc::new(CriticalIntents::default());
        let ticket = intents.ticket();
        let owner = intents.session_token(ticket);
        let task = {
            let intents = intents.clone();
            let hub = hub.clone();
            let lifetime = lifetime.clone();
            tokio::spawn(async move {
                let stop = CancellationToken::new();
                let update = |_| panic!("Dead peer must not publish");
                let work = VoiceWork::new(&intents, ticket, &stop, &update);
                work.run_peer(&lifetime, async {
                    hub.invoke("Hold", vec![], Duration::from_secs(30))
                        .await
                        .unwrap();
                    panic!("Invalidated peer wait tail ran");
                })
                .await
            })
        };
        fixture.held.notified().await;
        let started = Instant::now();
        engine.disconnect_peer("peer");
        engine
            .create_data_channel("peer", "replacement", DataChannelInit::default())
            .unwrap();
        let replacement = engine.peer_lifetime("peer").unwrap();
        assert_eq!(
            timeout(Duration::from_millis(1500), task)
                .await
                .unwrap()
                .unwrap(),
            None::<()>
        );
        assert!(!lifetime.is_alive());
        timeout(Duration::from_millis(100), lifetime.cancelled())
            .await
            .unwrap();
        assert!(replacement.is_alive());
        assert!(!owner.is_cancelled());
        assert!(intents.is_current(ticket, IntentScope::Voice));
        hub.close();
        assert_eq!(fixture.server.await.unwrap().len(), 1);
        engine.close();
        assert!(!replacement.is_alive());
        println!(
            "voice peer replacement cancellation_ms={} owner_preserved=true physical_audio=false",
            started.elapsed().as_millis()
        );
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn voice_and_peer_guards_reject_same_poll_revocation_and_never_poll_already_stale_work() {
    let (mut engine, lifetime) = peer();
    let intents = CriticalIntents::default();
    let ticket = intents.ticket();
    let stop = CancellationToken::new();
    let update = |_| panic!("Stale voice work published");
    let work = VoiceWork::new(&intents, ticket, &stop, &update);
    assert_eq!(
        work.run_peer(&lifetime, async {
            engine.disconnect_peer("peer");
            42
        })
        .await,
        None
    );
    assert_eq!(
        work.run_peer(&lifetime, async {
            panic!("Dead peer future polled");
        })
        .await,
        None::<()>
    );
    assert_eq!(
        work.run(async {
            intents.request(CriticalAction::Leave);
            42
        })
        .await,
        None
    );
    assert_eq!(intents.next().await, CriticalAction::Leave);
    assert_eq!(
        work.run(async {
            panic!("Old voice future polled");
        })
        .await,
        None::<()>
    );
    let fresh = VoiceWork::new(&intents, intents.ticket(), &stop, &update);
    assert_eq!(
        fresh
            .run(async {
                stop.cancel();
                42
            })
            .await,
        None
    );
    assert_eq!(
        fresh
            .run(async {
                panic!("Stopped future polled");
            })
            .await,
        None::<()>
    );
    engine.close();
}

#[tokio::test]
async fn queued_voice_ui_rejects_original_ticket_and_peer_lifetime_but_preserves_session_roster() {
    let (mut engine, lifetime) = peer();
    let intents = CriticalIntents::default();
    let ticket = intents.ticket();
    let stop = CancellationToken::new();
    let queued = Mutex::new(Vec::new());
    let update = |event| queued.lock().unwrap().push(event);
    let work = VoiceWork::new(&intents, ticket, &stop, &update);
    work.publish_peer(&lifetime, UiEvent::Toast("toasts.micUnknownError"));
    let peer_event = queued
        .lock()
        .unwrap()
        .pop()
        .unwrap()
        .guarded(ticket, IntentScope::Session);
    engine.disconnect_peer("peer");
    assert!(peer_event.for_current(&intents).is_none());
    for event in [
        UiEvent::Sharing(true),
        UiEvent::RemoveVideo("peer".into()),
        UiEvent::VoiceEnded,
        UiEvent::Hub(zabor_native::hub::HubEvent {
            target: "UserSpeaking".into(),
            arguments: vec![json!("owner"), json!(true)],
        }),
    ] {
        work.publish(event);
    }
    intents.request(CriticalAction::Leave);
    assert!(queued
        .lock()
        .unwrap()
        .drain(..)
        .all(|event| event.for_current(&intents).is_none()));
    let roster = UiEvent::Hub(zabor_native::hub::HubEvent {
        target: "FriendAdded".into(),
        arguments: vec![],
    })
    .guarded(ticket, IntentScope::Session);
    assert!(roster.for_current(&intents).is_some());
    assert_eq!(intents.next().await, CriticalAction::Leave);
    work.publish(UiEvent::Sharing(true));
    assert!(queued.lock().unwrap().is_empty());
    let fresh = VoiceWork::new(&intents, intents.ticket(), &stop, &update);
    fresh.publish(UiEvent::Sharing(true));
    intents.request(CriticalAction::Logout);
    assert!(queued
        .lock()
        .unwrap()
        .pop()
        .unwrap()
        .for_current(&intents)
        .is_none());
    engine.close();
}

#[tokio::test]
async fn guarded_ice_and_stream_view_notifications_preserve_wire_shape_and_skip_revoked_dispatch() {
    timeout(Duration::from_secs(10), async {
        let fixture = fixture().await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let (mut engine, lifetime) = peer();
        let intents = CriticalIntents::default();
        let ticket = intents.ticket();
        let stop = CancellationToken::new();
        let update = |_| panic!("Notification fixture published UI");
        let work = VoiceWork::new(&intents, ticket, &stop, &update);
        let ice = json!({"candidate":"fixture","sdpMid":"0","sdpMLineIndex":0}).to_string();
        assert_eq!(
            work.send_peer(
                &lifetime,
                &hub,
                "SendIceCandidate",
                vec![json!("peer"), json!(ice)]
            )
            .await,
            Some(Ok(()))
        );
        assert_eq!(
            work.send_peer(
                &lifetime,
                &hub,
                "SendStreamViewState",
                vec![json!("peer"), json!("watching")]
            )
            .await,
            Some(Ok(()))
        );
        intents.request(CriticalAction::Leave);
        assert_eq!(
            work.send_peer(&lifetime, &hub, "SendIceCandidate", vec![json!("stale")])
                .await,
            None
        );
        assert_eq!(intents.next().await, CriticalAction::Leave);
        assert_eq!(
            work.send_peer(&lifetime, &hub, "SendStreamViewState", vec![json!("stale")])
                .await,
            None
        );
        let fresh = VoiceWork::new(&intents, intents.ticket(), &stop, &update);
        engine.disconnect_peer("peer");
        assert_eq!(
            fresh
                .send_peer(&lifetime, &hub, "SendIceCandidate", vec![json!("dead")])
                .await,
            None
        );
        hub.close();
        let requests = fixture.server.await.unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0],
            json!({"type":1,"target":"SendIceCandidate","arguments":["peer",ice]})
        );
        assert_eq!(
            requests[1],
            json!({"type":1,"target":"SendStreamViewState","arguments":["peer","watching"]})
        );
        engine.close();
    })
    .await
    .unwrap();
}

fn frame_sink(
    sender: mpsc::Sender<FrameUpdate>,
    ticket: IntentTicket,
    generation: u64,
) -> Arc<dyn Fn(VideoDelivery) + Send + Sync> {
    Arc::new(move |delivery| {
        let _ = sender.try_send(FrameUpdate {
            peer: "alice".into(),
            ticket,
            generation,
            lease: delivery.lease,
            frame: delivery.frame,
        });
    })
}

#[tokio::test]
async fn actual_rtp_mailbox_frames_reject_leave_before_teardown_and_accept_only_fresh_voice_subscription(
) {
    timeout(Duration::from_secs(20), async {
        let (mut alice, mut alice_events) = MediaEngine::new("alice".into());
        let (mut bob, mut bob_events) = MediaEngine::new("bob".into());
        alice.authorize_peers(["bob".into()]);
        bob.authorize_peers(["alice".into()]);
        let source = NativeVideoSource::new(VideoResolution { width: 160, height: 96 }, true);
        alice.set_video(Some(source.clone())).unwrap();
        let offer = alice.offer("bob", false).await.unwrap();
        let answer = bob.receive_offer("alice", &offer).await.unwrap().unwrap();
        alice.receive_answer("bob", &answer).await.unwrap();
        let (track, lifetime) = loop {
            tokio::select! {
                event = alice_events.recv() => { if let Some(MediaEvent::Ice { payload, .. }) = event { bob.receive_ice("alice", &payload).await.unwrap(); } },
                event = bob_events.recv() => match event {
                    Some(MediaEvent::Ice { payload, .. }) => alice.receive_ice("bob", &payload).await.unwrap(),
                    Some(MediaEvent::Track { track: MediaStreamTrack::Video(track), lifetime, .. }) => break (track, lifetime),
                    _ => {},
                },
            }
        };
        let intents = CriticalIntents::default();
        let old = intents.ticket();
        let generation = AtomicU64::new(7);
        let stop = CancellationToken::new();
        let (sender, mut frames) = mpsc::channel(8);
        let mut receiver = VideoReceiver::new(track.clone(), lifetime.clone(), frame_sink(sender.clone(), old, 7));
        let mut ticker = tokio::time::interval(Duration::from_millis(33));
        for fresh in [false, true] {
            receiver.start();
            let frame = loop {
                tokio::select! {
                    _ = ticker.tick() => { source.capture_frame(&VideoFrame::new(VideoRotation::VideoRotation0, I420Buffer::new_black(160, 96))); },
                    frame = frames.recv() => { let frame = frame.unwrap(); if frame.lease.is_alive() { break frame; } },
                    event = alice_events.recv() => { if let Some(MediaEvent::Ice { payload, .. }) = event { bob.receive_ice("alice", &payload).await.unwrap(); } },
                    event = bob_events.recv() => { if let Some(MediaEvent::Ice { payload, .. }) = event { alice.receive_ice("bob", &payload).await.unwrap(); } },
                }
            };
            assert_eq!((frame.frame.width, frame.frame.height), (160, 96));
            assert!(frame.is_current(&intents, &generation, &stop));
            if !fresh {
                intents.request(CriticalAction::Leave);
                assert!(frame.lease.is_alive());
                assert!(receiver.is_playing());
                assert_eq!(generation.load(Ordering::Acquire), 7);
                assert!(!frame.is_current(&intents, &generation, &stop));
                assert_eq!(intents.next().await, CriticalAction::Leave);
                assert!(!frame.is_current(&intents, &generation, &stop));
                receiver.stop();
                receiver = VideoReceiver::new(track.clone(), lifetime.clone(), frame_sink(sender.clone(), intents.ticket(), 7));
            } else {
                generation.store(8, Ordering::Release);
                assert!(!frame.is_current(&intents, &generation, &stop));
                generation.store(7, Ordering::Release);
                stop.cancel();
                assert!(!frame.is_current(&intents, &generation, &stop));
                let live_stop = CancellationToken::new();
                assert!(frame.is_current(&intents, &generation, &live_stop));
                intents.request(CriticalAction::Logout);
                assert!(!frame.is_current(&intents, &generation, &live_stop));
                receiver.stop();
                assert!(!frame.lease.is_alive());
            }
        }
        bob.disconnect_peer("alice");
        assert!(!lifetime.is_alive());
        alice.close(); bob.close();
        println!("voice RTP mailbox original_ticket=true leave_before_teardown_rejected=true fresh_subscription=true physical_audio=false");
    }).await.unwrap();
}
