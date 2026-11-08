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

#[tokio::test]
async fn voice_commit_rechecks_original_ticket_after_ready_result_leave_logout_and_window_stop() {
    for mode in ["live", "leave", "logout", "window"] {
        let intents = CriticalIntents::default();
        let ticket = intents.ticket();
        let stop = CancellationToken::new();
        let update = |_| panic!("Commit fixture must not publish");
        let work = VoiceWork::new(&intents, ticket, &stop, &update);
        let result = work.run(async { 42 }).await.unwrap();
        match mode {
            "leave" => intents.request(CriticalAction::Leave),
            "logout" => intents.request(CriticalAction::Logout),
            "window" => stop.cancel(),
            _ => {}
        }
        let mut accepted = 0;
        assert_eq!(
            work.commit(|| {
                accepted += 1;
                result
            }),
            (mode == "live").then_some(42)
        );
        assert_eq!(accepted, usize::from(mode == "live"));
        if matches!(mode, "leave" | "logout") {
            assert_eq!(
                intents.next().await,
                if mode == "leave" {
                    CriticalAction::Leave
                } else {
                    CriticalAction::Logout
                }
            );
            assert!(work
                .commit(|| panic!("Consumed priority must not revive original Voice ticket"))
                .is_none());
            let fresh = VoiceWork::new(&intents, intents.ticket(), &stop, &update);
            assert_eq!(fresh.commit(|| 43), Some(43));
        }
        println!("voice extraction commit={mode} original_ticket=true ready_result_rechecked=true stale_tail_rejected=true physical_audio=false");
    }
}

#[test]
fn voice_commit_holds_original_intent_gate_and_rechecks_window_stop_after_waiting_for_it() {
    for action in [CriticalAction::Leave, CriticalAction::Logout] {
        let intents = CriticalIntents::default();
        let ticket = intents.ticket();
        let stop = CancellationToken::new();
        let update = |_| {};
        let work = VoiceWork::new(&intents, ticket, &stop, &update);
        let (entered, waiting) = std::sync::mpsc::channel();
        let (release, held) = std::sync::mpsc::channel();
        let (requesting, requested) = std::sync::mpsc::channel();
        let (done, finished) = std::sync::mpsc::channel();
        let value = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let work = &work;
            let value = &value;
            let commit = scope.spawn(move || {
                work.commit(|| {
                    entered.send(()).unwrap();
                    held.recv_timeout(Duration::from_secs(3)).unwrap();
                    value.store(1, Ordering::Release);
                    42
                })
            });
            waiting.recv_timeout(Duration::from_secs(1)).unwrap();
            let requester = scope.spawn(|| {
                requesting.send(()).unwrap();
                intents.request(action);
                assert_eq!(value.load(Ordering::Acquire), 1);
                done.send(()).unwrap();
            });
            requested.recv_timeout(Duration::from_secs(1)).unwrap();
            assert!(finished.recv_timeout(Duration::from_millis(30)).is_err());
            release.send(()).unwrap();
            assert_eq!(commit.join().unwrap(), Some(42));
            finished.recv_timeout(Duration::from_secs(1)).unwrap();
            requester.join().unwrap();
        });
        assert!(work
            .commit(|| panic!("Original Voice ticket revived after priority"))
            .is_none());
        println!("voice extraction gate={action:?} linearized=true mutation_before_priority=true arbitrary_commit_not_unlocked=true physical_audio=false");
    }
    let intents = CriticalIntents::default();
    let ticket = intents.ticket();
    let stop = CancellationToken::new();
    let update = |_| {};
    let work = VoiceWork::new(&intents, ticket, &stop, &update);
    let (entered, waiting) = std::sync::mpsc::channel();
    let (release, held) = std::sync::mpsc::channel();
    let (started, starting) = std::sync::mpsc::channel();
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        let intents = &intents;
        let holder = scope.spawn(move || {
            intents.commit(ticket, IntentScope::Session, || {
                entered.send(()).unwrap();
                held.recv_timeout(Duration::from_secs(3)).unwrap();
            })
        });
        waiting.recv_timeout(Duration::from_secs(1)).unwrap();
        let commit = scope.spawn(|| {
            started.send(()).unwrap();
            let result =
                work.commit(|| panic!("Window stop must be checked after acquiring the gate"));
            done.send(result).unwrap();
        });
        starting.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(finished.recv_timeout(Duration::from_millis(30)).is_err());
        stop.cancel();
        release.send(()).unwrap();
        holder.join().unwrap().unwrap();
        assert_eq!(
            finished.recv_timeout(Duration::from_secs(1)).unwrap(),
            None::<()>
        );
        commit.join().unwrap();
    });
    println!("voice extraction gate=window stop_after_gate_wait_rechecked=true stale_mutation_rejected=true physical_audio=false");
}

#[tokio::test]
async fn resource_metadata_commit_rejects_missing_reticketed_and_revoked_ready_results() {
    for phase in [
        "sent-audio",
        "speaking",
        "sent-speaking",
        "join-reset",
        "call-connected",
        "call-session",
        "call-suspended",
        "call-disconnected",
        "resume-poll",
        "resume-retry",
        "resume-ended",
        "resume-restored",
    ] {
        for mode in [
            "live",
            "leave-pending",
            "leave-consumed",
            "logout-pending",
            "logout-consumed",
            "window",
            "missing",
            "reticket",
        ] {
            let intents = CriticalIntents::default();
            let original = intents.ticket();
            let stop = CancellationToken::new();
            let update = |_| panic!("Resource metadata must not publish inside the intent gate");
            let ready = VoiceWork::new(&intents, original, &stop, &update);
            assert_eq!(ready.run(async { 42 }).await, Some(42));
            match mode {
                "leave-pending" | "leave-consumed" | "reticket" => {
                    intents.request(CriticalAction::Leave)
                }
                "logout-pending" | "logout-consumed" => intents.request(CriticalAction::Logout),
                "window" => stop.cancel(),
                _ => {}
            }
            if mode.ends_with("consumed") || mode == "reticket" {
                intents.next().await;
            }
            let ticket = if mode == "reticket" {
                intents.ticket()
            } else {
                original
            };
            let resource_ticket = (mode != "missing").then_some(original);
            let work = VoiceWork::new(&intents, ticket, &stop, &update);
            let mut values = (
                Some((false, false)),
                false,
                Some(tokio::time::Instant::now()),
                false,
            );
            let before = values;
            let mut mutations = 0;
            let result = work.commit_resource(resource_ticket, || {
                values = (None, true, None, true);
                mutations += 1;
                42
            });
            assert_eq!(result, (mode == "live").then_some(42));
            assert_eq!(mutations, usize::from(mode == "live"));
            assert_eq!(
                values,
                if mode == "live" {
                    (None, true, None, true)
                } else {
                    before
                }
            );
            assert_eq!(resource_ticket, (mode != "missing").then_some(original));
            if mode != "live" {
                assert!(work
                    .commit_resource(resource_ticket, || panic!("Rejected metadata tail ran"))
                    .is_none());
            }
            println!("resource metadata phase={phase} mode={mode} ready_result=true original_ticket=true accepted={} physical_audio=false", mode == "live");
        }
    }
}

#[test]
fn resource_metadata_commit_keeps_fnmut_capture_drop_and_unwind_outside_strict_gate() {
    struct Probe {
        intents: Arc<CriticalIntents>,
        drops: Arc<AtomicUsize>,
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            let _ = self.intents.ticket();
            self.drops.fetch_add(1, Ordering::AcqRel);
        }
    }
    for mode in [
        "live", "leave", "logout", "window", "missing", "reticket", "panic",
    ] {
        let intents = Arc::new(CriticalIntents::default());
        let original = intents.ticket();
        let stop = CancellationToken::new();
        if mode == "leave" || mode == "reticket" {
            intents.request(CriticalAction::Leave);
        }
        if mode == "logout" {
            intents.request(CriticalAction::Logout);
        }
        if mode == "window" {
            stop.cancel();
        }
        if mode == "reticket" {
            assert!(intents.take_leave(original).is_some());
        }
        let ticket = if mode == "reticket" {
            intents.ticket()
        } else {
            original
        };
        let drops = Arc::new(AtomicUsize::new(0));
        let probe = Probe {
            intents: intents.clone(),
            drops: drops.clone(),
        };
        let update = |_| {};
        let work = VoiceWork::new(&intents, ticket, &stop, &update);
        let mut accepted = false;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            work.commit_resource((mode != "missing").then_some(original), move || {
                let _ = &probe;
                assert_ne!(mode, "panic", "Expected resource metadata callback unwind");
                42
            })
        }));
        assert_eq!(drops.load(Ordering::Acquire), 1);
        assert_eq!(result.is_err(), mode == "panic");
        if let Ok(result) = result {
            assert_eq!(result, (mode == "live").then_some(42));
        }
        if matches!(mode, "live" | "panic") {
            assert_eq!(
                work.commit_resource(Some(original), || {
                    accepted = true;
                    43
                }),
                Some(43)
            );
            assert!(accepted);
        }
        println!("resource metadata capture={mode} drop_gate_reentry=true fnmut_borrowed=true physical_audio=false");
    }
    for mode in ["leave", "logout", "window"] {
        let intents = CriticalIntents::default();
        let ticket = intents.ticket();
        let stop = CancellationToken::new();
        let update = |_| {};
        let work = VoiceWork::new(&intents, ticket, &stop, &update);
        let (entered, waiting) = std::sync::mpsc::channel();
        let (release, held) = std::sync::mpsc::channel();
        let (started, starting) = std::sync::mpsc::channel();
        let (done, finished) = std::sync::mpsc::channel();
        let value = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let intents = &intents;
            let work = &work;
            let value = &value;
            if mode == "window" {
                let holder = scope.spawn(move || {
                    intents.commit(ticket, IntentScope::Session, || {
                        entered.send(()).unwrap();
                        held.recv_timeout(Duration::from_secs(3)).unwrap();
                    })
                });
                waiting.recv_timeout(Duration::from_secs(1)).unwrap();
                let commit = scope.spawn(move || {
                    started.send(()).unwrap();
                    let result = work.commit_resource(Some(ticket), || {
                        panic!("Window stop after gate wait accepted metadata")
                    });
                    done.send(result).unwrap();
                });
                starting.recv_timeout(Duration::from_secs(1)).unwrap();
                assert!(finished.recv_timeout(Duration::from_millis(30)).is_err());
                stop.cancel();
                release.send(()).unwrap();
                holder.join().unwrap().unwrap();
                assert_eq!(
                    finished.recv_timeout(Duration::from_secs(1)).unwrap(),
                    None::<()>
                );
                commit.join().unwrap();
            } else {
                let commit = scope.spawn(move || {
                    work.commit_resource(Some(ticket), || {
                        entered.send(()).unwrap();
                        held.recv_timeout(Duration::from_secs(3)).unwrap();
                        value.store(1, Ordering::Release);
                    })
                });
                waiting.recv_timeout(Duration::from_secs(1)).unwrap();
                let requester = scope.spawn(move || {
                    started.send(()).unwrap();
                    intents.request(if mode == "leave" {
                        CriticalAction::Leave
                    } else {
                        CriticalAction::Logout
                    });
                    assert_eq!(value.load(Ordering::Acquire), 1);
                    done.send(None).unwrap();
                });
                starting.recv_timeout(Duration::from_secs(1)).unwrap();
                assert!(finished.recv_timeout(Duration::from_millis(30)).is_err());
                release.send(()).unwrap();
                assert_eq!(commit.join().unwrap(), Some(()));
                finished.recv_timeout(Duration::from_secs(1)).unwrap();
                requester.join().unwrap();
            }
        });
        assert!(work
            .commit_resource(Some(ticket), || panic!(
                "Priority-revoked metadata accepted"
            ))
            .is_none());
        println!("resource metadata gate={mode} strict_linearization=true stop_after_gate_wait=true physical_audio=false");
    }
}

#[tokio::test]
async fn native_call_restore_resource_commit_keeps_retired_video_drop_outside_gate() {
    use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
    use zabor_native::call_recovery::CallRecovery;
    const SESSION: &str = "0123456789abcdef0123456789abcdef";
    struct SinkProbe {
        intents: Arc<CriticalIntents>,
        drops: Arc<AtomicUsize>,
    }
    impl Drop for SinkProbe {
        fn drop(&mut self) {
            let _ = self.intents.ticket();
            self.drops.fetch_add(1, Ordering::AcqRel);
        }
    }
    let (mut engine, lifetime) = peer();
    let factory = libwebrtc::peer_connection_factory::PeerConnectionFactory::default();
    let source = NativeVideoSource::new(
        VideoResolution {
            width: 160,
            height: 96,
        },
        true,
    );
    for path in ["event", "command"] {
        for mode in [
            "live",
            "leave-pending",
            "leave-consumed",
            "logout-pending",
            "logout-consumed",
            "window",
            "missing",
            "reticket",
            "panic",
            "expired",
            "wrong-peer",
            "failed",
        ] {
            let intents = Arc::new(CriticalIntents::default());
            let original = intents.ticket();
            let stop = CancellationToken::new();
            let now = Instant::now();
            let mut recovery = CallRecovery::default();
            recovery.begin("peer");
            assert!(recovery.connected("peer"));
            assert!(recovery.suspended(SESSION, 45_000, now));
            let deadline = recovery.next_deadline();
            let drops = Arc::new(AtomicUsize::new(0));
            let probe = SinkProbe {
                intents: intents.clone(),
                drops: drops.clone(),
            };
            let receiver = VideoReceiver::new(
                factory.create_video_track("metadata-fixture", source.clone()),
                lifetime.clone(),
                Arc::new(move |_| {
                    let _ = &probe;
                }),
            );
            assert!(!receiver.is_playing());
            let mut videos = std::collections::BTreeMap::from([("peer".to_owned(), receiver)]);
            let mut paused = std::collections::BTreeSet::from(["peer".to_owned()]);
            let mut grace = zabor_native::video::StreamDropGrace::default();
            grace.schedule("peer".into(), now);
            let update = |_| panic!("Call restoration must publish outside the acceptance gate");
            let ready = VoiceWork::new(&intents, original, &stop, &update);
            assert_eq!(ready.run(async { 42 }).await, Some(42));
            match mode {
                "leave-pending" | "leave-consumed" | "reticket" => {
                    intents.request(CriticalAction::Leave)
                }
                "logout-pending" | "logout-consumed" => intents.request(CriticalAction::Logout),
                "window" => stop.cancel(),
                _ => {}
            }
            if mode.ends_with("consumed") || mode == "reticket" {
                intents.next().await;
            }
            let work = VoiceWork::new(
                &intents,
                if mode == "reticket" {
                    intents.ticket()
                } else {
                    original
                },
                &stop,
                &update,
            );
            let resource_ticket = (mode != "missing").then_some(original);
            let restored_at = now + Duration::from_secs(if mode == "expired" { 45 } else { 1 });
            let mut retired_video = None;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                work.commit_resource(resource_ticket, || {
                    if mode != "failed"
                        && recovery.restored(
                            SESSION,
                            if mode == "wrong-peer" {
                                "other"
                            } else {
                                "peer"
                            },
                            restored_at,
                        )
                    {
                        retired_video = videos.remove("peer");
                        paused.remove("peer");
                        assert_ne!(mode, "panic", "Expected restored video metadata unwind");
                        true
                    } else {
                        recovery.retry(restored_at);
                        false
                    }
                })
            }));
            let extracted = matches!(mode, "live" | "panic");
            assert_eq!(result.is_err(), mode == "panic");
            assert_eq!(retired_video.is_some(), extracted);
            assert_eq!(videos.is_empty(), extracted);
            assert_eq!(paused.is_empty(), extracted);
            assert_eq!(drops.load(Ordering::Acquire), 0);
            assert_eq!(grace.next_deadline(), Some(now + Duration::from_secs(30)));
            if let Ok(Some(restored)) = result {
                assert_eq!(restored, mode == "live");
                drop(retired_video.take());
                if restored {
                    grace.cancel("peer");
                }
                assert_eq!(grace.next_deadline().is_none(), restored);
            } else if mode != "panic" {
                assert_eq!(recovery.next_deadline(), deadline);
                assert!(recovery.can_restore(SESSION, "peer", now));
            }
            drop(retired_video);
            drop(videos);
            assert_eq!(drops.load(Ordering::Acquire), 1);
            assert!(lifetime.is_alive());
            assert_eq!(resource_ticket, (mode != "missing").then_some(original));
            println!("resource metadata video path={path} mode={mode} receiver_drop_gate_reentry=true extracted={extracted} grace_cancel_outside_gate=true physical_audio=false");
        }
    }
    engine.close();
}

#[tokio::test]
async fn peer_resource_ready_commits_require_original_voice_ticket_and_peer_identity() {
    for mode in [
        "live",
        "leave-pending",
        "leave-consumed",
        "logout-pending",
        "logout-consumed",
        "window",
        "missing",
        "reticket",
        "peer",
        "replacement",
        "poison",
    ] {
        let (mut engine, lifetime) = peer();
        let intents = CriticalIntents::default();
        let original = intents.ticket();
        let stop = CancellationToken::new();
        let update = |_| panic!("Peer metadata must not publish under either gate");
        assert_eq!(
            VoiceWork::new(&intents, original, &stop, &update)
                .run_peer(&lifetime, async { 42 })
                .await,
            Some(42)
        );
        match mode {
            "leave-pending" | "leave-consumed" | "reticket" => {
                intents.request(CriticalAction::Leave)
            }
            "logout-pending" | "logout-consumed" => intents.request(CriticalAction::Logout),
            "window" => stop.cancel(),
            "peer" | "replacement" => {
                engine.disconnect_peer("peer");
                if mode == "replacement" {
                    engine
                        .create_data_channel(
                            "peer",
                            "peer-acceptance-replacement",
                            DataChannelInit::default(),
                        )
                        .unwrap();
                    let replacement = engine.peer_lifetime("peer").unwrap();
                    assert!(replacement.is_alive());
                    assert!(!replacement.is_same(&lifetime));
                }
            }
            "poison" => {
                assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || lifetime.commit(|| panic!("Expected peer admission poison"))
                ))
                .is_err());
            }
            _ => {}
        }
        if mode.ends_with("consumed") || mode == "reticket" {
            intents.next().await;
        }
        let ticket = if mode == "reticket" {
            intents.ticket()
        } else {
            original
        };
        let resource_ticket = (mode != "missing").then_some(original);
        let work = VoiceWork::new(&intents, ticket, &stop, &update);
        let mut value = 0;
        assert_eq!(
            work.commit_peer_resource(resource_ticket, &lifetime, || {
                value += 1;
                42
            }),
            (mode == "live").then_some(42)
        );
        assert_eq!(value, usize::from(mode == "live"));
        assert_eq!(resource_ticket, (mode != "missing").then_some(original));
        if mode != "live" {
            assert!(work
                .commit_peer_resource(resource_ticket, &lifetime, || panic!(
                    "Rejected original peer resource tail ran"
                ))
                .is_none());
        }
        engine.close();
        println!("peer acceptance resource mode={mode} ready_result=true original_voice_peer=true accepted={} physical_audio=false", mode == "live");
    }
}

#[test]
fn peer_resource_gate_orders_intent_before_peer_and_rechecks_window_after_peer_wait() {
    for mode in ["leave", "logout", "window"] {
        let (mut engine, lifetime) = peer();
        let intents = CriticalIntents::default();
        let ticket = intents.ticket();
        let stop = CancellationToken::new();
        let update = |_| {};
        let work = VoiceWork::new(&intents, ticket, &stop, &update);
        let (entered, waiting) = std::sync::mpsc::channel();
        let (release, held) = std::sync::mpsc::channel();
        let (started, starting) = std::sync::mpsc::channel();
        let (done, finished) = std::sync::mpsc::channel();
        let value = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let lifetime = &lifetime;
            let intents = &intents;
            let work = &work;
            let value = &value;
            if mode == "window" {
                let holder = scope.spawn(move || {
                    lifetime.commit(|| {
                        entered.send(()).unwrap();
                        held.recv_timeout(Duration::from_secs(3)).unwrap();
                    })
                });
                waiting.recv_timeout(Duration::from_secs(1)).unwrap();
                let commit = scope.spawn(move || {
                    started.send(()).unwrap();
                    done.send(work.commit_peer_resource(Some(ticket), lifetime, || {
                        panic!("Window stop while waiting for peer gate accepted resource")
                    }))
                    .unwrap();
                });
                starting.recv_timeout(Duration::from_secs(1)).unwrap();
                assert!(finished.recv_timeout(Duration::from_millis(30)).is_err());
                stop.cancel();
                release.send(()).unwrap();
                holder.join().unwrap().unwrap();
                assert_eq!(
                    finished.recv_timeout(Duration::from_secs(1)).unwrap(),
                    None::<()>
                );
                commit.join().unwrap();
            } else {
                let commit = scope.spawn(move || {
                    work.commit_peer_resource(Some(ticket), lifetime, || {
                        entered.send(()).unwrap();
                        held.recv_timeout(Duration::from_secs(3)).unwrap();
                        value.store(1, Ordering::Release);
                    })
                });
                waiting.recv_timeout(Duration::from_secs(1)).unwrap();
                let requester = scope.spawn(move || {
                    started.send(()).unwrap();
                    intents.request(if mode == "leave" {
                        CriticalAction::Leave
                    } else {
                        CriticalAction::Logout
                    });
                    assert_eq!(value.load(Ordering::Acquire), 1);
                    done.send(None).unwrap();
                });
                starting.recv_timeout(Duration::from_secs(1)).unwrap();
                assert!(finished.recv_timeout(Duration::from_millis(30)).is_err());
                release.send(()).unwrap();
                assert_eq!(commit.join().unwrap(), Some(()));
                finished.recv_timeout(Duration::from_secs(1)).unwrap();
                requester.join().unwrap();
            }
        });
        assert!(work
            .commit_peer_resource(Some(ticket), &lifetime, || panic!(
                "Revoked joint-gate resource accepted"
            ))
            .is_none());
        engine.close();
        println!("peer acceptance joint_gate={mode} strict_intent_peer_order=true stop_after_peer_wait=true physical_audio=false");
    }
}

#[tokio::test]
async fn peer_video_admission_and_install_keep_receivers_and_fnmut_captures_outside_both_gates() {
    use libwebrtc::peer_connection_factory::{
        native::PeerConnectionFactoryExt, PeerConnectionFactory,
    };
    struct Probe {
        intents: Arc<CriticalIntents>,
        lifetime: PeerLifetime,
        drops: Arc<AtomicUsize>,
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            let _ = self.intents.ticket();
            let _ = self.lifetime.commit(|| 43);
            self.drops.fetch_add(1, Ordering::AcqRel);
        }
    }
    let factory = PeerConnectionFactory::default();
    let source = NativeVideoSource::new(
        VideoResolution {
            width: 160,
            height: 96,
        },
        true,
    );
    for phase in ["admission", "installation"] {
        for occupied in [false, true] {
            for mode in [
                "live",
                "leave",
                "logout",
                "window",
                "missing",
                "reticket",
                "peer",
                "replacement",
                "poison",
                "wrong-peer",
                "panic",
            ] {
                let (mut engine, lifetime) = peer();
                let (mut other_engine, other_lifetime) = peer();
                let intents = Arc::new(CriticalIntents::default());
                let original = intents.ticket();
                let stop = CancellationToken::new();
                let old_drops = Arc::new(AtomicUsize::new(0));
                let new_drops = Arc::new(AtomicUsize::new(0));
                let callback_drops = Arc::new(AtomicUsize::new(0));
                let mut videos = std::collections::BTreeMap::new();
                if occupied {
                    let probe = Probe {
                        intents: intents.clone(),
                        lifetime: lifetime.clone(),
                        drops: old_drops.clone(),
                    };
                    videos.insert(
                        "peer".into(),
                        VideoReceiver::new(
                            factory.create_video_track("peer-old", source.clone()),
                            lifetime.clone(),
                            Arc::new(move |_| {
                                let _ = &probe;
                            }),
                        ),
                    );
                }
                let new_lifetime = if mode == "wrong-peer" {
                    other_lifetime.clone()
                } else {
                    lifetime.clone()
                };
                let probe = Probe {
                    intents: intents.clone(),
                    lifetime: new_lifetime.clone(),
                    drops: new_drops.clone(),
                };
                let prepared = VideoReceiver::new(
                    factory.create_video_track("peer-new", source.clone()),
                    new_lifetime,
                    Arc::new(move |_| {
                        let _ = &probe;
                    }),
                );
                let callback_probe = Probe {
                    intents: intents.clone(),
                    lifetime: lifetime.clone(),
                    drops: callback_drops.clone(),
                };
                let update = |_| {};
                assert_eq!(
                    VoiceWork::new(&intents, original, &stop, &update)
                        .run_peer(&lifetime, async { 42 })
                        .await,
                    Some(42)
                );
                match mode {
                    "leave" | "reticket" => {
                        intents.request(CriticalAction::Leave);
                        intents.next().await;
                    }
                    "logout" => {
                        intents.request(CriticalAction::Logout);
                        intents.next().await;
                    }
                    "window" => stop.cancel(),
                    "peer" | "replacement" => {
                        engine.disconnect_peer("peer");
                        if mode == "replacement" {
                            engine
                                .create_data_channel(
                                    "peer",
                                    "same-name-new-lifetime",
                                    DataChannelInit::default(),
                                )
                                .unwrap();
                            assert!(!engine.peer_lifetime("peer").unwrap().is_same(&lifetime));
                        }
                    }
                    "poison" => {
                        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            lifetime
                                .commit(|| panic!("Expected configured receiver admission poison"))
                        }))
                        .is_err());
                    }
                    _ => {}
                }
                let work = VoiceWork::new(
                    &intents,
                    if mode == "reticket" {
                        intents.ticket()
                    } else {
                        original
                    },
                    &stop,
                    &update,
                );
                let resource_ticket = (mode != "missing").then_some(original);
                if phase == "admission" {
                    let mut retired = None;
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        work.commit_peer_resource(resource_ticket, &lifetime, || {
                            retired = videos.remove("peer");
                            assert_ne!(
                                mode, "panic",
                                "Expected admitted receiver extraction unwind"
                            );
                            42
                        })
                    }));
                    assert_eq!(result.is_err(), mode == "panic");
                    let admitted = matches!(mode, "live" | "wrong-peer" | "panic");
                    if let Ok(result) = result {
                        assert_eq!(result, admitted.then_some(42));
                    }
                    assert_eq!(retired.is_some(), occupied && admitted);
                    assert_eq!(videos.is_empty(), !occupied || admitted);
                    assert_eq!(old_drops.load(Ordering::Acquire), 0);
                    drop(retired);
                    drop(prepared);
                    drop(callback_probe);
                } else {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        work.install_video(
                            &mut videos,
                            resource_ticket,
                            &lifetime,
                            ("peer".into(), prepared),
                            move || {
                                let _ = &callback_probe;
                                assert_ne!(
                                    mode, "panic",
                                    "Expected accepted video installation unwind"
                                );
                            },
                        )
                    }));
                    assert_eq!(result.is_err(), mode == "panic");
                    let installed = matches!(mode, "live" | "panic");
                    if let Ok(result) = result {
                        assert_eq!(result.is_some(), installed);
                        assert_eq!(
                            result.as_ref().is_some_and(Option::is_some),
                            occupied && installed
                        );
                        assert_eq!(old_drops.load(Ordering::Acquire), 0);
                        drop(result);
                    } else {
                        assert_eq!(old_drops.load(Ordering::Acquire), usize::from(occupied));
                    }
                    assert_eq!(videos.is_empty(), !occupied && !installed);
                    assert_eq!(new_drops.load(Ordering::Acquire), usize::from(!installed));
                    assert_eq!(callback_drops.load(Ordering::Acquire), 1);
                }
                drop(videos);
                assert_eq!(old_drops.load(Ordering::Acquire), usize::from(occupied));
                assert_eq!(new_drops.load(Ordering::Acquire), 1);
                assert_eq!(callback_drops.load(Ordering::Acquire), 1);
                assert_eq!(resource_ticket, (mode != "missing").then_some(original));
                engine.close();
                other_engine.close();
                println!("peer acceptance video phase={phase} occupied={occupied} mode={mode} resource_capture_drop_gate_reentry=true original_lifetime=true physical_audio=false");
            }
        }
    }
}

#[tokio::test]
async fn peer_video_active_native_preparation_rechecks_original_resources_before_installation() {
    use libwebrtc::peer_connection_factory::{
        native::PeerConnectionFactoryExt, PeerConnectionFactory,
    };
    timeout(Duration::from_secs(15), async {
        let factory = PeerConnectionFactory::default();
        let source = NativeVideoSource::new(VideoResolution { width: 160, height: 96 }, true);
        for mode in ["live", "leave", "logout", "window", "peer", "replacement"] {
            let (mut engine, lifetime) = peer();
            let intents = CriticalIntents::default();
            let original = intents.ticket();
            let stop = CancellationToken::new();
            let generation = AtomicU64::new(7);
            let update = |_| {};
            let work = VoiceWork::new(&intents, original, &stop, &update);
            let mut videos = std::collections::BTreeMap::<String, VideoReceiver>::new();
            let (sender, mut frames) = mpsc::channel(8);
            let track = factory.create_video_track("peer-active-preparation", source.clone());
            let mut receiver = VideoReceiver::new(track, lifetime.clone(), frame_sink(sender, original, 7));
            let mut retired = None;
            let watch = work.commit_peer_resource(Some(original), &lifetime, || {
                retired = videos.remove("alice");
                videos.values().filter(|receiver| receiver.is_playing()).count() < 3
            }).unwrap();
            drop(retired);
            assert!(watch);
            receiver.start();
            assert!(receiver.is_playing());
            let mut ticker = tokio::time::interval(Duration::from_millis(33));
            let frame = loop {
                tokio::select! {
                    _ = ticker.tick() => { source.capture_frame(&VideoFrame::new(VideoRotation::VideoRotation0, I420Buffer::new_black(160, 96))); },
                    frame = frames.recv() => break frame.unwrap(),
                }
            };
            assert!(frame.is_current(&intents, &generation, &stop));
            match mode {
                "leave" => { intents.request(CriticalAction::Leave); intents.next().await; },
                "logout" => { intents.request(CriticalAction::Logout); intents.next().await; },
                "window" => stop.cancel(),
                "peer" | "replacement" => {
                    engine.disconnect_peer("peer");
                    if mode == "replacement" { engine.create_data_channel("peer", "active-new-peer", DataChannelInit::default()).unwrap(); assert!(!engine.peer_lifetime("peer").unwrap().is_same(&lifetime)); }
                },
                _ => {},
            }
            let installed = work.install_video(&mut videos, Some(original), &lifetime, ("alice".into(), receiver), || {});
            assert_eq!(installed.is_some(), mode == "live");
            assert_eq!(videos.len(), usize::from(mode == "live"));
            assert_eq!(frame.lease.is_alive(), mode == "live");
            assert_eq!(frame.is_current(&intents, &generation, &stop), mode == "live");
            drop(installed);
            drop(videos);
            assert!(!frame.lease.is_alive());
            engine.close();
            println!("peer acceptance active_video={mode} native_subscription=true original_frame_voice_peer=true post_prepare_recheck=true physical_audio=false");
        }
    }).await.unwrap();
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
