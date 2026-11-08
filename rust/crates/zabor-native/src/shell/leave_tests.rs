use super::{Command, CommandQueue, SessionWork, UiEvent, VoiceLeave, VoiceSession};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
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
use zabor_native::audio_mix::AudioControls;
use zabor_native::audio_runtime::SharedAudio;
use zabor_native::auth::User;
use zabor_native::call_recovery::CallRecovery;
use zabor_native::critical_intents::{CriticalAction, CriticalIntents, IntentTicket};
use zabor_native::hub::HubConnection;
use zabor_native::media::MediaEvent;
use zabor_native::native_audio::NativeAudio;
use zabor_native::protocol::{encode, Decoder};
use zabor_native::video::{StreamDropGrace, VideoReceiver};

struct Fixture {
    endpoint: String,
    held: Arc<Notify>,
    release: Arc<Notify>,
    hints: mpsc::Receiver<String>,
    server: tokio::task::JoinHandle<Vec<String>>,
}

async fn fixture() -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/zabor_v3", listener.local_addr().unwrap());
    let held = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let (hint_sender, hints) = mpsc::channel(16);
    let server_held = held.clone();
    let server_release = release.clone();
    let server = tokio::spawn(async move {
        let (mut http, _) = listener.accept().await.unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            header.push(http.read_u8().await.unwrap());
            assert!(header.len() < 8192);
        }
        let body = json!({"connectionToken":"leave-fixture","negotiateVersion":1,"availableTransports":[{"transport":"WebSockets","transferFormats":["Text"]}]}).to_string();
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
        let mut held_id: Option<Value> = None;
        let mut methods = Vec::new();
        loop {
            tokio::select! {
                _ = server_release.notified(), if held_id.is_some() => {
                    let id = held_id.take().unwrap();
                    socket.send(Message::text(encode(&json!({"type":3,"invocationId":id,"result":true})).unwrap())).await.unwrap();
                },
                message = socket.next() => {
                    let Some(Ok(Message::Text(text))) = message else { break; };
                    for request in decoder.push(text.as_bytes()).unwrap() {
                        if request["type"] == 6 { continue; }
                        let method = request["target"].as_str().unwrap();
                        methods.push(method.into());
                        match method {
                            "GetIceServers" => {
                                socket.send(Message::text(encode(&json!({"type":3,"invocationId":request["invocationId"],"result":{"iceServers":[]}})).unwrap())).await.unwrap();
                            },
                            "Hold" => {
                                assert!(held_id.is_none());
                                held_id = Some(request["invocationId"].clone());
                                server_held.notify_one();
                            },
                            "EndCall" | "LeaveChannel" | "StopStream" => {
                                assert!(request.get("invocationId").is_none());
                                assert_eq!(request["arguments"], json!([]));
                                hint_sender.send(method.into()).await.unwrap();
                            },
                            _ => panic!("Unexpected leave fixture request"),
                        }
                    }
                },
            }
        }
        methods
    });
    Fixture {
        endpoint,
        held,
        release,
        hints,
        server,
    }
}

struct VoiceState {
    voice: Option<VoiceSession>,
    events: Option<mpsc::Receiver<MediaEvent>>,
    ticket: Option<IntentTicket>,
    pending_join: Option<String>,
    videos: BTreeMap<String, VideoReceiver>,
    paused: BTreeSet<String>,
    drops: StreamDropGrace,
    recovery: CallRecovery,
    abandoned_call: bool,
    pending_stop_stream: bool,
    call_connected: bool,
    sent_audio: Option<(bool, bool)>,
    sent_speaking: bool,
    speaking_release: Option<tokio::time::Instant>,
    local_speaking: bool,
}

impl VoiceState {
    fn new(ticket: IntentTicket) -> Self {
        let mut drops = StreamDropGrace::default();
        drops.schedule("peer".into(), Instant::now());
        let mut recovery = CallRecovery::default();
        recovery.begin("peer");
        Self {
            voice: None,
            events: None,
            ticket: Some(ticket),
            pending_join: Some("old-channel".into()),
            videos: BTreeMap::new(),
            paused: ["peer".into()].into(),
            drops,
            recovery,
            abandoned_call: true,
            pending_stop_stream: false,
            call_connected: true,
            sent_audio: Some((false, false)),
            sent_speaking: true,
            speaking_release: Some(tokio::time::Instant::now()),
            local_speaking: true,
        }
    }

    fn leave<'a>(
        &'a mut self,
        generation: &'a AtomicU64,
        audio: &'a SharedAudio<NativeAudio>,
    ) -> VoiceLeave<'a> {
        VoiceLeave {
            voice: &mut self.voice,
            media_events: &mut self.events,
            voice_ticket: &mut self.ticket,
            pending_join: &mut self.pending_join,
            video_jobs: &mut self.videos,
            paused_video: &mut self.paused,
            stream_drops: &mut self.drops,
            call_recovery: &mut self.recovery,
            abandoned_call: &mut self.abandoned_call,
            pending_stop_stream: &mut self.pending_stop_stream,
            call_connected: &mut self.call_connected,
            sent_audio_state: &mut self.sent_audio,
            sent_speaking: &mut self.sent_speaking,
            speaking_release: &mut self.speaking_release,
            local_speaking: &mut self.local_speaking,
            video_generation: generation,
            audio_demand: &audio.demand,
        }
    }

    fn assert_released(&self) {
        assert!(self.voice.is_none());
        assert!(self.events.is_none());
        assert!(self.ticket.is_none());
        assert!(self.pending_join.is_none());
        assert!(self.videos.is_empty());
        assert!(self.paused.is_empty());
        assert!(self.drops.next_deadline().is_none());
        assert!(self.recovery.next_deadline().is_none());
        assert!(!self.call_connected);
        assert!(self.sent_audio.is_none());
        assert!(!self.sent_speaking);
        assert!(self.speaking_release.is_none());
        assert!(!self.local_speaking);
    }
}

fn audio(stop: &CancellationToken) -> Arc<SharedAudio<NativeAudio>> {
    let audio = SharedAudio::new(
        Arc::new(AudioControls::new(Default::default())),
        stop.child_token(),
        |_, _, _| Box::pin(async { panic!("Physical audio must not start in a leave fixture") }),
    );
    let voice = audio.demand.reserve().unwrap();
    assert!(audio.demand.activate(voice));
    audio
}

#[tokio::test]
async fn leave_releases_native_voice_and_dequeues_priority_during_one_stalled_session_invocation() {
    timeout(Duration::from_secs(10), async {
        let mut fixture = fixture().await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None).await.unwrap();
        let intents = Arc::new(CriticalIntents::default());
        let old = intents.ticket();
        let stop = CancellationToken::new();
        let audio = audio(&stop);
        let monitor = audio.monitor(true);
        let clip_now = Instant::now();
        let token = audio.mic_test.begin(clip_now).unwrap();
        assert!(audio.mic_test.prepared(token, clip_now));
        audio.mic_test.snapshot(clip_now + zabor_native::mic_test::MONITOR_SETTLE);
        for index in 0..500 {
            assert!(audio.mic_test.capture(token, &[0.1; 480], -20.0, clip_now + zabor_native::mic_test::MONITOR_SETTLE + zabor_native::mic_test::PIPELINE_FLUSH + Duration::from_millis(index * 10)));
        }
        assert_eq!(audio.mic_test.snapshot(clip_now + Duration::from_secs(6)).stored_samples, zabor_native::mic_test::CLIP_SAMPLES);
        let generation = Arc::new(AtomicU64::new(1));
        let (sender, mut commands) = mpsc::channel(16);
        let queue = CommandQueue { sender, intents: intents.clone() };
        for _ in 0..16 { queue.try_send(Command::Join("old-channel".into())).unwrap_or_else(|_| panic!("Queue fixture failed")); }
        let invoked = Arc::new(AtomicUsize::new(0));
        let published = Arc::new(Mutex::new(Vec::new()));
        let task = {
            let intents = intents.clone(); let hub = hub.clone(); let audio = audio.clone();
            let generation = generation.clone(); let invoked = invoked.clone(); let published = published.clone();
            tokio::spawn(async move {
                let mut state = VoiceState::new(old);
                let (mut voice, events) = VoiceSession::new(&hub, &User { id: "owner".into(), ..Default::default() }, false, Arc::new(AudioControls::new(Default::default())), audio.clone()).await.unwrap();
                voice.call_peer = Some("peer".into());
                state.voice = Some(voice); state.events = Some(events);
                let update = |event| published.lock().unwrap().push(event);
                let work = SessionWork::new(&intents, old, &stop, &update);
                let result = work.run_with_leave(async {
                    invoked.fetch_add(1, Ordering::AcqRel);
                    let result = hub.invoke("Hold", vec![], Duration::from_secs(30)).await;
                    work.publish(UiEvent::ChatMessages("peer".into(), vec![], false));
                    result
                }, &mut state.leave(&generation, &audio), Some(&hub)).await;
                state.assert_released();
                assert!(!state.abandoned_call);
                result
            })
        };
        fixture.held.notified().await;
        let requested = Instant::now();
        queue.try_send(Command::Leave).unwrap_or_else(|_| panic!("Priority leave failed"));
        assert_eq!(timeout(Duration::from_millis(1500), fixture.hints.recv()).await.unwrap().as_deref(), Some("EndCall"));
        let cleanup_ms = requested.elapsed().as_millis();
        assert_eq!(audio.demand.active(), 0);
        assert!(!audio.demand.has_voice());
        assert_eq!(generation.load(Ordering::Acquire), 2);
        assert!(!task.is_finished());
        assert_eq!(invoked.load(Ordering::Acquire), 1);
        assert_eq!(audio.mic_test.snapshot(clip_now + Duration::from_secs(6)).stored_samples, zabor_native::mic_test::CLIP_SAMPLES);
        monitor.configure(false);
        monitor.configure(true);
        assert!(audio.mic_test.has_demand());
        for _ in 0..3 {
            queue.try_send(Command::Leave).unwrap_or_else(|_| panic!("Repeated leave failed"));
            assert_eq!(timeout(Duration::from_millis(1500), fixture.hints.recv()).await.unwrap().as_deref(), Some("LeaveChannel"));
            assert!(!task.is_finished());
        }
        fixture.release.notify_one();
        assert_eq!(task.await.unwrap(), Some(Ok(Value::Bool(true))));
        assert_eq!(invoked.load(Ordering::Acquire), 1);
        for _ in 0..16 {
            let queued = commands.try_recv().unwrap();
            assert!(!intents.is_current(queued.ticket, queued.command.scope()));
        }
        assert!(matches!(published.lock().unwrap().pop().unwrap().for_current(&intents), Some(UiEvent::ChatMessages(_, _, false))));
        hub.close();
        assert_eq!(fixture.server.await.unwrap(), vec!["GetIceServers", "Hold", "EndCall", "LeaveChannel", "LeaveChannel", "LeaveChannel"]);
        println!("leave stalled-session cleanup_ms={cleanup_ms} invocations=1 queued_voice_commands=16 repeats=3 retained_clip_samples=240000 physical_audio=false");
    }).await.unwrap();
}

#[tokio::test]
async fn leave_ready_result_is_drained_and_logout_after_cleanup_discards_owner_result() {
    let intents = CriticalIntents::default();
    let old = intents.ticket();
    let stop = CancellationToken::new();
    let audio = audio(&stop);
    let generation = AtomicU64::new(1);
    let mut state = VoiceState::new(old);
    let update = |_| intents.request(CriticalAction::Logout);
    let work = SessionWork::new(&intents, old, &stop, &update);
    let result = work
        .run_with_leave(
            async {
                intents.request(CriticalAction::Leave);
                42
            },
            &mut state.leave(&generation, &audio),
            None,
        )
        .await;
    assert_eq!(result, None);
    state.assert_released();
    assert_eq!(intents.next().await, CriticalAction::Logout);
}

#[tokio::test]
async fn logout_dominance_is_not_consumed_by_stalled_session_leave_driver() {
    let intents = CriticalIntents::default();
    let old = intents.ticket();
    let stop = CancellationToken::new();
    let audio = audio(&stop);
    let generation = AtomicU64::new(1);
    let mut state = VoiceState::new(old);
    let update = |_| panic!("Dominant Logout must not publish Leave");
    let work = SessionWork::new(&intents, old, &stop, &update);
    intents.request(CriticalAction::Leave);
    intents.request(CriticalAction::Logout);
    assert_eq!(
        work.run_with_leave(
            async { panic!("Revoked session polled") },
            &mut state.leave(&generation, &audio),
            None
        )
        .await,
        None::<()>
    );
    assert_eq!(generation.load(Ordering::Acquire), 1);
    assert_eq!(intents.next().await, CriticalAction::Logout);
}

#[tokio::test]
async fn leave_driver_keeps_logout_cancellation_of_the_actual_stalled_session_wait() {
    timeout(Duration::from_secs(10), async {
        let mut fixture = fixture().await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let intents = Arc::new(CriticalIntents::default());
        let old = intents.ticket();
        let task = {
            let intents = intents.clone();
            let hub = hub.clone();
            tokio::spawn(async move {
                let stop = CancellationToken::new();
                let audio = audio(&stop);
                let generation = AtomicU64::new(1);
                let mut state = VoiceState::new(old);
                let update = |event| {
                    if matches!(event, UiEvent::Guarded(_, _, _)) {
                    } else {
                        panic!("Unexpected unguarded leave event")
                    }
                };
                let work = SessionWork::new(&intents, old, &stop, &update);
                work.run_with_leave(
                    async {
                        hub.invoke("Hold", vec![], Duration::from_secs(30))
                            .await
                            .unwrap();
                        panic!("Logout must discard the stalled session tail");
                    },
                    &mut state.leave(&generation, &audio),
                    Some(&hub),
                )
                .await
            })
        };
        fixture.held.notified().await;
        intents.request(CriticalAction::Leave);
        assert_eq!(
            timeout(Duration::from_millis(1500), fixture.hints.recv())
                .await
                .unwrap()
                .as_deref(),
            Some("EndCall")
        );
        assert!(!task.is_finished());
        intents.request(CriticalAction::Logout);
        assert_eq!(
            timeout(Duration::from_millis(1500), task)
                .await
                .unwrap()
                .unwrap(),
            None::<()>
        );
        assert_eq!(intents.next().await, CriticalAction::Logout);
        hub.close();
        assert_eq!(fixture.server.await.unwrap(), vec!["Hold", "EndCall"]);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn stopped_stream_hint_is_bounded_and_does_not_omit_abandoned_end_call() {
    use super::leave_work::send_leave_hints;
    let intents = CriticalIntents::default();
    let stop = CancellationToken::new();
    let attempted = Mutex::new(Vec::new());
    let started = Instant::now();
    let result = timeout(
        Duration::from_millis(1500),
        send_leave_hints(&intents, intents.ticket(), &stop, true, true, |target| {
            let attempted = &attempted;
            async move {
                attempted.lock().unwrap().push(target);
                if target == "StopStream" {
                    std::future::pending::<()>().await;
                }
                true
            }
        }),
    )
    .await
    .unwrap();
    assert!(!result.stream_stopped);
    assert!(result.left);
    assert_eq!(*attempted.lock().unwrap(), vec!["StopStream", "EndCall"]);
    println!(
        "leave stopped-stream deadline_ms=250 measured_ms={} end_call_attempted=true",
        started.elapsed().as_millis()
    );
}

#[tokio::test]
async fn repeated_leave_revokes_stalled_cleanup_without_losing_pending_stream_and_call_hints() {
    use super::leave_work::send_leave_hints;
    timeout(Duration::from_secs(5), async {
        let intents = Arc::new(CriticalIntents::default());
        let old = intents.ticket();
        let started = Arc::new(Notify::new());
        let task = {
            let intents = intents.clone();
            let started = started.clone();
            tokio::spawn(async move {
                let stop = CancellationToken::new();
                send_leave_hints(&intents, old, &stop, true, true, |target| {
                    let started = started.clone();
                    async move {
                        assert_eq!(target, "StopStream");
                        started.notify_one();
                        std::future::pending::<bool>().await
                    }
                })
                .await
            })
        };
        started.notified().await;
        intents.request(CriticalAction::Leave);
        let result = task.await.unwrap();
        assert!(!result.stream_stopped);
        assert!(!result.left);
        let fresh = intents.next_leave(old).await.unwrap();
        let stop = CancellationToken::new();
        let attempted = Mutex::new(Vec::new());
        let result = send_leave_hints(&intents, fresh, &stop, true, true, |target| {
            let attempted = &attempted;
            async move {
                attempted.lock().unwrap().push(target);
                true
            }
        })
        .await;
        assert!(result.stream_stopped && result.left);
        assert_eq!(*attempted.lock().unwrap(), vec!["StopStream", "EndCall"]);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn logout_hint_deadlines_drop_stalled_sends_and_preserve_abandoned_end_call_attempt() {
    use super::leave_work::send_logout_hints;
    struct PendingHint<'a>(&'a AtomicUsize);
    impl Drop for PendingHint<'_> {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }
    for (stream, call, stalled) in [
        (true, true, "StopStream"),
        (false, true, "EndCall"),
        (true, true, "both"),
        (false, false, "none"),
    ] {
        let stop = CancellationToken::new();
        let attempted = Mutex::new(Vec::new());
        let dropped = AtomicUsize::new(0);
        let measured = Instant::now();
        let result = timeout(
            Duration::from_millis(1500),
            send_logout_hints(&stop, stream, call, |target| {
                let attempted = &attempted;
                let dropped = &dropped;
                async move {
                    let _pending = PendingHint(dropped);
                    attempted.lock().unwrap().push(target);
                    if target == stalled || stalled == "both" {
                        std::future::pending::<()>().await;
                    }
                    true
                }
            }),
        )
        .await
        .unwrap();
        let mut expected = Vec::new();
        if stream {
            expected.push("StopStream");
        }
        if call {
            expected.push("EndCall");
        }
        assert_eq!(*attempted.lock().unwrap(), expected);
        assert_eq!(dropped.load(Ordering::Acquire), expected.len());
        assert_eq!(
            result.stream_stopped,
            !stream || stalled != "StopStream" && stalled != "both"
        );
        assert_eq!(
            result.left,
            !call || stalled != "EndCall" && stalled != "both"
        );
        println!("logout hints stalled={stalled} fixture_ms={} stream_budget_ms=250 call_budget_ms=750 detached_jobs=0 physical_audio=false", measured.elapsed().as_millis());
    }
}

#[tokio::test]
async fn logout_hint_failures_and_window_cancellation_do_not_poll_or_restart_old_sends() {
    use super::leave_work::send_logout_hints;
    for cancelled in [false, true] {
        let stop = CancellationToken::new();
        if cancelled {
            stop.cancel();
        }
        let attempted = Mutex::new(Vec::new());
        let result = send_logout_hints(&stop, true, true, |target| {
            let attempted = &attempted;
            async move {
                attempted.lock().unwrap().push(target);
                false
            }
        })
        .await;
        assert!(!result.stream_stopped && !result.left);
        assert_eq!(
            *attempted.lock().unwrap(),
            if cancelled {
                vec![]
            } else {
                vec!["StopStream", "EndCall"]
            }
        );
    }
    for cancelling in ["StopStream", "EndCall"] {
        let stop = CancellationToken::new();
        let entered = Notify::new();
        let attempted = Mutex::new(Vec::new());
        let hints = send_logout_hints(&stop, true, true, |target| {
            let stop = &stop;
            let entered = &entered;
            let attempted = &attempted;
            async move {
                attempted.lock().unwrap().push(target);
                if target == cancelling {
                    entered.notify_one();
                    stop.cancelled().await;
                    std::future::pending::<()>().await;
                }
                true
            }
        });
        let cancelling = async {
            entered.notified().await;
            stop.cancel();
        };
        let (result, ()) = timeout(Duration::from_millis(1500), async {
            tokio::join!(hints, cancelling)
        })
        .await
        .unwrap();
        assert!(!result.left);
        assert_eq!(result.stream_stopped, attempted.lock().unwrap().len() == 2);
    }
}

#[tokio::test]
async fn logout_old_connection_sends_retained_stream_and_call_hints_without_a_live_voice() {
    use super::leave_work::close_logout_connection;
    let mut fixture = fixture().await;
    let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
        .await
        .unwrap();
    let intents = CriticalIntents::default();
    let old = intents.ticket();
    let mut state = VoiceState::new(old);
    state.pending_stop_stream = true;
    assert!(state.voice.is_none());
    intents.request(CriticalAction::Leave);
    intents.request(CriticalAction::Logout);
    assert_eq!(intents.next().await, CriticalAction::Logout);
    let result = close_logout_connection(
        hub.clone(),
        &CancellationToken::new(),
        state.pending_stop_stream,
        state.abandoned_call,
    )
    .await;
    assert!(result.stream_stopped && result.left);
    assert!(!hub.is_connected());
    for target in ["StopStream", "EndCall"] {
        assert_eq!(
            timeout(Duration::from_millis(1500), fixture.hints.recv())
                .await
                .unwrap()
                .as_deref(),
            Some(target)
        );
    }
    assert_eq!(fixture.server.await.unwrap(), vec!["StopStream", "EndCall"]);
    println!("logout connection captured_old=true retained_stream=true abandoned_call=true live_voice=false protocol_checked=true physical_audio=false");
}

#[tokio::test]
async fn abandoning_polled_logout_cleanup_closes_the_captured_connection_even_with_live_clones() {
    use super::leave_work::close_logout_connection;
    let fixture = fixture().await;
    let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
        .await
        .unwrap();
    let stop = CancellationToken::new();
    let mut cleanup = Box::pin(close_logout_connection(hub.clone(), &stop, true, true));
    assert!(futures_util::poll!(cleanup.as_mut()).is_pending());
    drop(cleanup);
    assert!(!hub.is_connected());
    assert!(timeout(Duration::from_millis(1500), fixture.server)
        .await
        .unwrap()
        .unwrap()
        .is_empty());
    println!("logout connection abandoned_polled=true old_clones_closed=true detached_jobs=0 physical_audio=false");
}
