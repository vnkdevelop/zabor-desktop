use super::{
    main_session, reset_audio_session, restore_audio, restore_audio_session,
    try_reset_audio_session, worker, AuthClient, Command, CommandQueue, MainResult, SessionIo,
    SessionWork, UiEvent, VoiceLeave, VoiceSession, VoiceWork, WorkerMedia,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
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
use zabor_native::audio_preferences::AudioPreferences;
use zabor_native::audio_runtime::SharedAudio;
use zabor_native::auth::{Credentials, LoginOutcome, Registration, User};
use zabor_native::call_recovery::CallRecovery;
use zabor_native::critical_intents::{CriticalAction, CriticalIntents, IntentScope, IntentTicket};
use zabor_native::hub::HubConnection;
use zabor_native::native_audio::NativeAudio;
use zabor_native::profile::Profile;
use zabor_native::protocol::{encode, Decoder};
use zabor_native::roster::NotificationKind;
use zabor_native::video::StreamDropGrace;
use zeroize::Zeroizing;

const RESUME_SESSION: &str = "0123456789abcdef0123456789abcdef";

struct Fixture {
    endpoint: String,
    held: Arc<Notify>,
    release: Arc<Notify>,
    answered: Arc<Notify>,
    hints: mpsc::Receiver<String>,
    server: tokio::task::JoinHandle<Vec<String>>,
}

fn credentials() -> Credentials {
    Credentials {
        username: "fixture".into(),
        password: Zeroizing::new("fixture-only".into()),
    }
}

fn user() -> User {
    User {
        id: "fixture-owner".into(),
        username: "fixture".into(),
        display_name: "Fixture".into(),
        ..Default::default()
    }
}

fn answer(method: &str) -> Value {
    match method {
        "Login" | "LoginAndResume" | "Register" => json!(user()),
        "GetChannels" | "GetFriends" | "GetFriendRequests" | "GetChannelInvites" => json!([]),
        "GetAudioSettings" => json!("{\"language\":\"en\",\"inputVolume\":73}"),
        "GetJokeOfTheDay" => json!("fixture"),
        "GetIceServers" => json!({"iceServers":[]}),
        "GetRegistrationCaptcha" => json!({"id":"fixture-captcha","imageBase64":"fixture-image"}),
        "CheckUserExists"
        | "EnableCallResume"
        | "CreateChannel"
        | "SendFriendRequest"
        | "AcceptFriendRequest"
        | "DeclineFriendRequest"
        | "DeclineChannelInvite" => json!(true),
        _ => panic!("Unexpected main-session fixture method"),
    }
}

async fn fixture(hold: &'static str) -> Fixture {
    fixture_with_login(hold, json!(user())).await
}

async fn fixture_with_login(hold: &'static str, login_result: Value) -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/zabor_v3", listener.local_addr().unwrap());
    let held = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let answered = Arc::new(Notify::new());
    let (hint_sender, hints) = mpsc::channel(16);
    let server_held = held.clone();
    let server_release = release.clone();
    let server_answered = answered.clone();
    let server = tokio::spawn(async move {
        let (mut http, _) = listener.accept().await.unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            header.push(http.read_u8().await.unwrap());
            assert!(header.len() < 8192);
        }
        let body = json!({"connectionToken":"main-session-fixture","negotiateVersion":1,"availableTransports":[{"transport":"WebSockets","transferFormats":["Text"]}]}).to_string();
        if hold == "negotiate" {
            server_held.notify_one();
            server_release.notified().await;
        }
        let response = http.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
        if hold == "negotiate" && response.is_err() {
            return vec!["negotiate".into()];
        }
        response.unwrap();
        drop(http);
        let accepted = timeout(Duration::from_secs(2), listener.accept()).await;
        if hold == "negotiate" && accepted.is_err() {
            return vec!["negotiate".into()];
        }
        let (tcp, _) = accepted.unwrap().unwrap();
        let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap().into_text().unwrap(),
            "{\"protocol\":\"json\",\"version\":1}\u{1e}"
        );
        socket.send(Message::text("{}\u{1e}")).await.unwrap();
        let mut decoder = Decoder::default();
        let mut waiting: Option<Value> = None;
        let mut methods = Vec::new();
        loop {
            tokio::select! {
                _ = server_release.notified(), if waiting.is_some() => {
                    let request = waiting.take().unwrap();
                     let result = if hold == "Login" { login_result.clone() } else { answer(hold) };
                     let _ = socket.send(Message::text(encode(&json!({"type":3,"invocationId":request["invocationId"],"result":result})).unwrap())).await;
                    server_answered.notify_one();
                },
                message = socket.next() => {
                    let Some(Ok(Message::Text(text))) = message else { break; };
                    for request in decoder.push(text.as_bytes()).unwrap() {
                        if request["type"] == 6 { continue; }
                        let method = request["target"].as_str().unwrap();
                        methods.push(method.into());
                        if matches!(method, "EndCall" | "LeaveChannel" | "StopStream") {
                            assert!(request.get("invocationId").is_none());
                            assert_eq!(request["arguments"], json!([]));
                            hint_sender.send(method.into()).await.unwrap();
                        } else if method == "UpdateUserState" {
                            assert!(request.get("invocationId").is_none());
                            assert_eq!(request["arguments"], json!([{"userId":"fixture-owner","isMuted":true,"isDeafened":false}]));
                        } else {
                            if method == "LoginAndResume" { assert_eq!(request["arguments"].as_array().unwrap().len(), 3); assert_eq!(request["arguments"][2], RESUME_SESSION); }
                            if method == "Login" { assert_eq!(request["arguments"].as_array().unwrap().len(), 2); }
                            if matches!(method, "CreateChannel" | "SendFriendRequest") { assert_eq!(request["arguments"], json!(["Fixture"])); }
                            if matches!(method, "AcceptFriendRequest" | "DeclineFriendRequest" | "DeclineChannelInvite") { assert_eq!(request["arguments"], json!(["fixture-id"])); }
                            if method == hold {
                                assert!(waiting.is_none());
                                waiting = Some(request);
                                server_held.notify_one();
                            } else {
                                socket.send(Message::text(encode(&json!({"type":3,"invocationId":request["invocationId"],"result":answer(method)})).unwrap())).await.unwrap();
                            }
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
        answered,
        hints,
        server,
    }
}

fn audio(stop: &CancellationToken) -> Arc<SharedAudio<NativeAudio>> {
    let controls = AudioPreferences::new(&json!({})).controls.clone();
    let audio = SharedAudio::new(controls, stop.child_token(), |_, _, _| {
        Box::pin(async { panic!("Physical audio must not start in a main-session fixture") })
    });
    let lease = audio.demand.reserve().unwrap();
    assert!(audio.demand.activate(lease));
    audio
}

async fn drive<T>(
    work: &SessionWork<'_>,
    ticket: IntentTicket,
    audio: &Arc<SharedAudio<NativeAudio>>,
    generation: &AtomicU64,
    hub: Option<&HubConnection>,
    future: impl Future<Output = T>,
) -> Option<T> {
    let mut voice = None;
    let mut media_events = None;
    if let Some(hub) = hub {
        let (mut session, events) = VoiceSession::new(
            hub,
            &user(),
            false,
            AudioPreferences::new(&json!({})).controls.clone(),
            audio.clone(),
        )
        .await
        .unwrap();
        session.call_peer = Some("peer".into());
        voice = Some(session);
        media_events = Some(events);
    }
    let mut voice_ticket = Some(ticket);
    let mut pending_join = Some("old-channel".into());
    let mut videos = BTreeMap::new();
    let mut paused = BTreeSet::new();
    let mut drops = StreamDropGrace::default();
    let mut recovery = CallRecovery::default();
    let mut abandoned = false;
    let mut pending_stop_stream = false;
    let mut call_connected = true;
    let mut sent_audio = Some((false, false));
    let mut sent_speaking = true;
    let mut release = Some(tokio::time::Instant::now());
    let mut local_speaking = true;
    work.run_with_leave(
        future,
        &mut VoiceLeave {
            voice: &mut voice,
            media_events: &mut media_events,
            voice_ticket: &mut voice_ticket,
            pending_join: &mut pending_join,
            video_jobs: &mut videos,
            paused_video: &mut paused,
            stream_drops: &mut drops,
            call_recovery: &mut recovery,
            abandoned_call: &mut abandoned,
            pending_stop_stream: &mut pending_stop_stream,
            call_connected: &mut call_connected,
            sent_audio_state: &mut sent_audio,
            sent_speaking: &mut sent_speaking,
            speaking_release: &mut release,
            local_speaking: &mut local_speaking,
            video_generation: generation,
            audio_demand: &audio.demand,
        },
        hub,
    )
    .await
}

fn cases() -> Vec<(&'static str, Command)> {
    let mut cases = [
        "CheckUserExists",
        "Login",
        "EnableCallResume",
        "GetChannels",
        "GetAudioSettings",
    ]
    .into_iter()
    .map(|held| {
        (
            held,
            Command::Authenticate {
                credentials: credentials(),
                registration: None,
            },
        )
    })
    .collect::<Vec<_>>();
    cases.extend([
        (
            "Register",
            Command::Authenticate {
                credentials: credentials(),
                registration: Some(Registration {
                    name: "Fixture".into(),
                    avatar: None,
                    color: "blue".into(),
                    captcha_id: "fixture-captcha".into(),
                    answer: "1234".into(),
                }),
            },
        ),
        ("GetRegistrationCaptcha", Command::Captcha),
        (
            "CreateChannel",
            Command::Modal {
                kind: 1,
                value: " Fixture ".into(),
            },
        ),
        (
            "SendFriendRequest",
            Command::Modal {
                kind: 2,
                value: " Fixture ".into(),
            },
        ),
        (
            "GetFriends",
            Command::Modal {
                kind: 1,
                value: " Fixture ".into(),
            },
        ),
        (
            "AcceptFriendRequest",
            Command::Notification(NotificationKind::Friend, "fixture-id".into(), true),
        ),
        (
            "DeclineFriendRequest",
            Command::Notification(NotificationKind::Friend, "fixture-id".into(), false),
        ),
        (
            "DeclineChannelInvite",
            Command::Notification(NotificationKind::Channel, "fixture-id".into(), false),
        ),
    ]);
    cases
}

#[tokio::test]
async fn actual_stalled_connection_negotiation_keeps_single_session_future_on_leave_and_cancels_on_logout(
) {
    timeout(Duration::from_secs(10), async {
        for logout in [false, true] {
            let fixture = fixture("negotiate").await;
            let intents = Arc::new(CriticalIntents::default());
            let ticket = intents.ticket();
            let stop = CancellationToken::new();
            let audio = audio(&stop);
            let generation = Arc::new(AtomicU64::new(1));
            let (sender, mut published) = mpsc::unbounded_channel();
            let task = {
                let intents = intents.clone();
                let audio = audio.clone();
                let generation = generation.clone();
                let endpoint = fixture.endpoint.clone();
                tokio::spawn(async move {
                    let update = |event| {
                        let _ = sender.send(event);
                    };
                    let work = SessionWork::new(&intents, ticket, &stop, &update);
                    let connection = drive(
                        &work,
                        ticket,
                        &audio,
                        &generation,
                        None,
                        HubConnection::connect(&endpoint, None),
                    )
                    .await;
                    if let Some(Ok((connection, _events))) = connection {
                        assert!(work.commit(|| true).unwrap());
                        assert!(!intents.is_current(ticket, IntentScope::Voice));
                        connection.close();
                        true
                    } else {
                        false
                    }
                })
            };
            fixture.held.notified().await;
            if logout {
                intents.request(CriticalAction::Logout);
            } else {
                intents.request(CriticalAction::Leave);
                assert!(matches!(
                    timeout(Duration::from_millis(1500), published.recv())
                        .await
                        .unwrap()
                        .unwrap()
                        .for_current(&intents),
                    Some(UiEvent::VoiceEnded)
                ));
                assert_eq!(audio.demand.active(), 0);
                assert_eq!(generation.load(Ordering::Acquire), 2);
                assert!(!task.is_finished());
            }
            if logout {
                assert!(!timeout(Duration::from_millis(1500), task)
                    .await
                    .unwrap()
                    .unwrap());
                fixture.release.notify_one();
                assert_eq!(intents.next().await, CriticalAction::Logout);
            } else {
                fixture.release.notify_one();
                assert!(task.await.unwrap());
            }
            fixture.server.await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn main_session_protocol_waits_service_repeated_priority_leave_without_restarting_auth_or_modals(
) {
    timeout(Duration::from_secs(45), async {
        for (held, command) in cases() {
            assert!(main_session::is_session_command(&command));
            let mut fixture = fixture(held).await;
            let (hub, _events) = HubConnection::connect(&fixture.endpoint, None).await.unwrap();
            let intents = Arc::new(CriticalIntents::default());
            let ticket = intents.ticket();
            let stop = CancellationToken::new();
            let audio = audio(&stop);
            let monitor = audio.monitor(true);
            let generation = Arc::new(AtomicU64::new(1));
            let invoked = Arc::new(AtomicUsize::new(0));
            let published = Arc::new(Mutex::new(Vec::new()));
            let (sender, mut commands) = mpsc::channel(16);
            let queue = CommandQueue { sender, intents: intents.clone() };
            for _ in 0..16 { queue.try_send(Command::Join("old-channel".into())).unwrap_or_else(|_| panic!("Queue fixture failed")); }
            let task = {
                let hub = hub.clone(); let intents = intents.clone(); let audio = audio.clone(); let generation = generation.clone(); let invoked = invoked.clone(); let published = published.clone();
                tokio::spawn(async move {
                    let update = |event| published.lock().unwrap().push(event);
                    let work = SessionWork::new(&intents, ticket, &stop, &update);
                    let auth = AuthClient::new(hub.clone());
                    let result = drive(&work, ticket, &audio, &generation, Some(&hub), async {
                        invoked.fetch_add(1, Ordering::AcqRel);
                        main_session::compute(command, &auth, &hub, Some(user())).await
                    }).await;
                    if let Some(Ok(result)) = result {
                        match result {
                            MainResult::Authentication(LoginOutcome::SignedIn(data), _) => { assert_eq!(data.user.id, "fixture-owner"); work.publish(UiEvent::SignedIn(data)); },
                            MainResult::Event(event) => work.publish(event),
                            _ => panic!("Unexpected main-session result"),
                        }
                    } else { panic!("Leave lost main-session result"); }
                })
            };
            fixture.held.notified().await;
            let started = Instant::now();
            queue.try_send(Command::Leave).unwrap_or_else(|_| panic!("Priority Leave failed"));
            assert_eq!(timeout(Duration::from_millis(1500), fixture.hints.recv()).await.unwrap().as_deref(), Some("EndCall"));
            assert_eq!(audio.demand.active(), 0);
            assert_eq!(generation.load(Ordering::Acquire), 2);
            assert!(!task.is_finished());
            monitor.configure(false); monitor.configure(true);
            assert_eq!(invoked.load(Ordering::Acquire), 1);
            queue.try_send(Command::Leave).unwrap_or_else(|_| panic!("Repeated Leave failed"));
            assert_eq!(timeout(Duration::from_millis(1500), fixture.hints.recv()).await.unwrap().as_deref(), Some("LeaveChannel"));
            fixture.release.notify_one();
            task.await.unwrap();
            assert_eq!(invoked.load(Ordering::Acquire), 1);
            for _ in 0..16 { let queued = commands.try_recv().unwrap(); assert!(!intents.is_current(queued.ticket, queued.command.scope())); }
            assert!(published.lock().unwrap().pop().unwrap().for_current(&intents).is_some());
            hub.close();
            let methods = fixture.server.await.unwrap();
            assert_eq!(methods.iter().filter(|method| method.as_str() == held).count(), 1);
            println!("main session held={held} leave_ms={} invocations=1 queued_voice_commands=16 repeats=1 physical_audio=false", started.elapsed().as_millis());
        }
    }).await.unwrap();
}

#[tokio::test]
async fn logout_during_actual_main_session_waits_discards_result_before_profile_or_settings_commit()
{
    timeout(Duration::from_secs(45), async {
        for (held, command) in cases() {
            let mut fixture = fixture(held).await;
            let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
                .await
                .unwrap();
            let intents = Arc::new(CriticalIntents::default());
            let ticket = intents.ticket();
            let commits = Arc::new(AtomicUsize::new(0));
            let task = {
                let hub = hub.clone();
                let intents = intents.clone();
                let commits = commits.clone();
                tokio::spawn(async move {
                    let stop = CancellationToken::new();
                    let audio = audio(&stop);
                    let generation = AtomicU64::new(1);
                    let update = |_| {};
                    let work = SessionWork::new(&intents, ticket, &stop, &update);
                    let auth = AuthClient::new(hub.clone());
                    let result = drive(
                        &work,
                        ticket,
                        &audio,
                        &generation,
                        Some(&hub),
                        main_session::compute(command, &auth, &hub, Some(user())),
                    )
                    .await;
                    if result.is_some() {
                        work.commit(|| commits.fetch_add(1, Ordering::AcqRel));
                    }
                    result.is_none()
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
            intents.request(CriticalAction::Logout);
            assert!(timeout(Duration::from_millis(1500), task)
                .await
                .unwrap()
                .unwrap());
            assert_eq!(commits.load(Ordering::Acquire), 0);
            assert_eq!(intents.next().await, CriticalAction::Logout);
            fixture.release.notify_one();
            fixture.answered.notified().await;
            hub.close();
            let methods = fixture.server.await.unwrap();
            assert_eq!(
                methods
                    .iter()
                    .filter(|method| method.as_str() == held)
                    .count(),
                1
            );
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn login_resume_is_session_work_and_preserves_wire_contract_without_restoring_revoked_voice()
{
    timeout(Duration::from_secs(10), async {
        let mut fixture = fixture("LoginAndResume").await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let intents = Arc::new(CriticalIntents::default());
        let ticket = intents.ticket();
        let task = {
            let hub = hub.clone();
            let intents = intents.clone();
            tokio::spawn(async move {
                let stop = CancellationToken::new();
                let audio = audio(&stop);
                let generation = AtomicU64::new(1);
                let update = |_| {};
                let work = SessionWork::new(&intents, ticket, &stop, &update);
                let auth = AuthClient::new(hub.clone());
                let credentials = credentials();
                let result = drive(
                    &work,
                    ticket,
                    &audio,
                    &generation,
                    Some(&hub),
                    auth.login_with_resume(&credentials, Some(RESUME_SESSION)),
                )
                .await;
                assert!(matches!(result, Some(Ok(LoginOutcome::SignedIn(_)))));
                let voice_work = VoiceWork::new(&intents, ticket, &stop, &update);
                assert!(voice_work
                    .run(async { panic!("Old voice was reticketed") })
                    .await
                    .is_none());
                assert!(work.commit(|| true).unwrap());
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
        fixture.release.notify_one();
        task.await.unwrap();
        hub.close();
        let methods = fixture.server.await.unwrap();
        assert_eq!(
            methods
                .iter()
                .filter(|method| method.as_str() == "LoginAndResume")
                .count(),
            1
        );
        assert!(!methods.iter().any(|method| method == "Login"));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn prepared_dpapi_session_survives_leave_but_is_removed_on_logout_before_guarded_commit() {
    timeout(Duration::from_secs(10), async {
        let mut fixture = fixture("unused").await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let profile = Arc::new(Profile::open(root.path()).unwrap());
        profile
            .save_session("original-owner", &credentials())
            .unwrap();
        profile.write_settings(&json!({"language":"ru"})).unwrap();
        let original = std::fs::read(root.path().join("session.enc")).unwrap();
        let prefs = AudioPreferences::new(&json!({"language":"ru"}));
        let intents = Arc::new(CriticalIntents::default());
        let ticket = intents.ticket();
        let staged = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let task = {
            let hub = hub.clone();
            let intents = intents.clone();
            let profile = profile.clone();
            let prefs = prefs.clone();
            let staged = staged.clone();
            let release = release.clone();
            tokio::spawn(async move {
                let stop = CancellationToken::new();
                let audio = audio(&stop);
                let generation = AtomicU64::new(1);
                let update = |_| {};
                let work = SessionWork::new(&intents, ticket, &stop, &update);
                let preparation_profile = profile.clone();
                let prepared = drive(&work, ticket, &audio, &generation, Some(&hub), async {
                    let prepared = tokio::task::spawn_blocking(move || {
                        preparation_profile.prepare_session("fixture-owner", &credentials())
                    })
                    .await
                    .unwrap()
                    .unwrap();
                    staged.notify_one();
                    release.notified().await;
                    prepared
                })
                .await;
                if let Some(prepared) = prepared {
                    work.commit(|| profile.commit_session(prepared));
                }
                assert!(work
                    .commit(|| restore_audio(&prefs, &hub, &Default::default()))
                    .is_none());
            })
        };
        staged.notified().await;
        assert_eq!(
            std::fs::read_dir(root.path())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
                .count(),
            1
        );
        intents.request(CriticalAction::Leave);
        assert_eq!(
            timeout(Duration::from_millis(1500), fixture.hints.recv())
                .await
                .unwrap()
                .as_deref(),
            Some("EndCall")
        );
        assert!(!task.is_finished());
        assert_eq!(
            std::fs::read(root.path().join("session.enc")).unwrap(),
            original
        );
        intents.request(CriticalAction::Logout);
        task.await.unwrap();
        assert_eq!(
            std::fs::read(root.path().join("session.enc")).unwrap(),
            original
        );
        assert_eq!(profile.read_settings().unwrap()["language"], "ru");
        assert_eq!(prefs.settings().unwrap().language, "ru");
        assert_eq!(
            std::fs::read_dir(root.path())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
                .count(),
            0
        );
        hub.close();
        fixture.server.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn late_resume_selector_keeps_auth_future_but_uses_plain_login_after_leave_during_user_check()
{
    timeout(Duration::from_secs(10), async {
        for leave in [false, true] {
            let mut fixture = fixture("CheckUserExists").await;
            let (hub, _events) = HubConnection::connect(&fixture.endpoint, None).await.unwrap();
            let intents = Arc::new(CriticalIntents::default()); let ticket = intents.ticket();
            let task = {
                let hub = hub.clone(); let intents = intents.clone();
                tokio::spawn(async move {
                    let stop = CancellationToken::new(); let audio = audio(&stop); let generation = AtomicU64::new(1); let update = |_| {};
                    let work = SessionWork::new(&intents, ticket, &stop, &update);
                    let voice_work = VoiceWork::new(&intents, ticket, &stop, &update);
                    let auth = AuthClient::new(hub.clone()); let credentials = credentials();
                    let outcome = drive(&work, ticket, &audio, &generation, Some(&hub), auth.login_with_resume_if(&credentials, Some(RESUME_SESSION), || voice_work.is_current())).await;
                    assert!(matches!(outcome, Some(Ok(LoginOutcome::SignedIn(_)))));
                    assert!(work.commit(|| true).unwrap());
                })
            };
            fixture.held.notified().await;
            if leave {
                intents.request(CriticalAction::Leave);
                assert_eq!(timeout(Duration::from_millis(1500), fixture.hints.recv()).await.unwrap().as_deref(), Some("EndCall"));
                assert!(!task.is_finished());
            }
            fixture.release.notify_one(); task.await.unwrap(); hub.close();
            let methods = fixture.server.await.unwrap();
            assert_eq!(methods.iter().filter(|method| method.as_str() == "CheckUserExists").count(), 1);
            let dispatched = if leave { "Login" } else { "LoginAndResume" };
            let rejected = if leave { "LoginAndResume" } else { "Login" };
            assert_eq!(methods.iter().filter(|method| method.as_str() == dispatched).count(), 1);
            assert!(!methods.iter().any(|method| method == rejected));
            println!("main resume held=CheckUserExists leave={leave} dispatched={dispatched} invocations=1 original_voice_ticket=true physical_audio=false");
        }
    }).await.unwrap();
}

#[tokio::test]
async fn owner_guarded_profile_and_audio_commits_survive_leave_but_cannot_replace_next_owner_after_logout(
) {
    timeout(Duration::from_secs(10), async {
        let fixture = fixture("unused").await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let profile = Profile::open(root.path()).unwrap();
        let prefs = AudioPreferences::new(&json!({"language":"ru"}));
        let intents = CriticalIntents::default();
        let ticket = intents.ticket();
        let stop = CancellationToken::new();
        let update = |_| {};
        let work = SessionWork::new(&intents, ticket, &stop, &update);
        let prepared = profile
            .prepare_session("fixture-owner", &credentials())
            .unwrap();
        intents.request(CriticalAction::Leave);
        assert!(matches!(
            work.commit(|| profile.commit_session(prepared)),
            Some(Ok(()))
        ));
        let data = zabor_native::auth::Bootstrap {
            user: user(),
            audio_settings: Some(json!({"language":"en","inputVolume":73})),
            ..Default::default()
        };
        assert_eq!(
            work.commit(|| restore_audio(&prefs, &hub, &data)),
            Some(true)
        );
        assert_eq!(
            profile.load_session().unwrap().unwrap().user_id,
            "fixture-owner"
        );
        assert_eq!(prefs.settings().unwrap().input_volume, 73.0);
        intents.request(CriticalAction::Logout);
        assert_eq!(intents.next().await, CriticalAction::Logout);
        let fresh = SessionWork::new(&intents, intents.ticket(), &stop, &update);
        let prepared = profile
            .prepare_session("next-owner", &credentials())
            .unwrap();
        assert!(matches!(
            fresh.commit(|| profile.commit_session(prepared)),
            Some(Ok(()))
        ));
        let next = zabor_native::auth::Bootstrap {
            user: User {
                id: "next-owner".into(),
                ..Default::default()
            },
            audio_settings: Some(json!({"language":"ru","inputVolume":31})),
            ..Default::default()
        };
        assert_eq!(
            fresh.commit(|| restore_audio(&prefs, &hub, &next)),
            Some(true)
        );
        let stale = profile
            .prepare_session("fixture-owner", &credentials())
            .unwrap();
        assert!(work.commit(|| profile.commit_session(stale)).is_none());
        assert!(work.commit(|| restore_audio(&prefs, &hub, &data)).is_none());
        assert_eq!(
            profile.load_session().unwrap().unwrap().user_id,
            "next-owner"
        );
        assert_eq!(prefs.settings().unwrap().input_volume, 31.0);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
        hub.close();
        fixture.server.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn audio_session_result_and_local_mute_do_not_reuse_revoked_voice_ticket() {
    timeout(Duration::from_secs(10), async {
        let fixture = fixture("unused").await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let prefs = AudioPreferences::new(&json!({}));
        assert!(prefs.begin_session("fixture-owner", hub.clone(), None));
        prefs.controls.set_user_state(true, false);
        let intents = CriticalIntents::default();
        let ticket = intents.ticket();
        let stop = CancellationToken::new();
        let update = |_| {};
        let work = SessionWork::new(&intents, ticket, &stop, &update);
        let audio = audio(&stop);
        let (mut voice, _events) =
            VoiceSession::new(&hub, &user(), false, prefs.controls.clone(), audio)
                .await
                .unwrap();
        let mut local = user();
        intents.request(CriticalAction::Leave);
        assert!(work
            .commit(|| {
                let (muted, deafened) = prefs.controls.user_state();
                local.is_muted = muted;
                local.is_deafened = deafened;
            })
            .is_some());
        assert!(local.is_muted);
        assert!(!local.is_deafened);
        let voice_work = VoiceWork::new(&intents, ticket, &stop, &update);
        assert!(voice_work
            .run(voice.mute(&hub, true, false))
            .await
            .is_none());
        assert_eq!(intents.next().await, CriticalAction::Leave);
        assert!(voice_work
            .run(voice.mute(&hub, true, false))
            .await
            .is_none());
        let fresh = VoiceWork::new(&intents, intents.ticket(), &stop, &update);
        assert!(matches!(
            fresh.run(voice.mute(&hub, true, false)).await,
            Some(Ok(()))
        ));
        intents.request(CriticalAction::Logout);
        assert!(work
            .commit(|| {
                local.is_muted = false;
            })
            .is_none());
        assert!(local.is_muted);
        hub.close();
        let methods = fixture.server.await.unwrap();
        assert_eq!(
            methods
                .iter()
                .filter(|method| method.as_str() == "UpdateUserState")
                .count(),
            1
        );
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn revoked_inflight_profile_preparation_finishes_without_commit_and_cleans_detached_output() {
    struct Finished(Option<tokio::sync::oneshot::Sender<()>>);
    impl Drop for Finished {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }
    timeout(Duration::from_secs(10), async {
        let fixture = fixture("unused").await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
            .await
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let profile = Arc::new(Profile::open(root.path()).unwrap());
        profile
            .save_session("original-owner", &credentials())
            .unwrap();
        let original = std::fs::read(root.path().join("session.enc")).unwrap();
        let intents = Arc::new(CriticalIntents::default());
        let ticket = intents.ticket();
        let started = Arc::new(Notify::new());
        let (release, blocked) = std::sync::mpsc::channel();
        let (finished, cleaned) = tokio::sync::oneshot::channel();
        let task = {
            let hub = hub.clone();
            let profile = profile.clone();
            let intents = intents.clone();
            let started = started.clone();
            tokio::spawn(async move {
                let stop = CancellationToken::new();
                let audio = audio(&stop);
                let generation = AtomicU64::new(1);
                let update = |_| {};
                let work = SessionWork::new(&intents, ticket, &stop, &update);
                let job = tokio::task::spawn_blocking(move || {
                    started.notify_one();
                    blocked.recv().unwrap();
                    (
                        profile
                            .prepare_session("prepared-owner", &credentials())
                            .unwrap(),
                        Finished(Some(finished)),
                    )
                });
                drive(&work, ticket, &audio, &generation, Some(&hub), job)
                    .await
                    .is_none()
            })
        };
        started.notified().await;
        intents.request(CriticalAction::Logout);
        assert!(timeout(Duration::from_millis(1500), task)
            .await
            .unwrap()
            .unwrap());
        release.send(()).unwrap();
        cleaned.await.unwrap();
        assert_eq!(
            std::fs::read(root.path().join("session.enc")).unwrap(),
            original
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
        hub.close();
        fixture.server.await.unwrap();
    })
    .await
    .unwrap();
}

#[test]
fn main_session_routing_preserves_channel_accept_voice_scope_and_guarded_commits_reject_stale_owners(
) {
    for (_, command) in cases() {
        assert!(main_session::is_session_command(&command));
        assert_eq!(command.scope(), IntentScope::Session);
    }
    let invite = Command::Notification(NotificationKind::Channel, "fixture-id".into(), true);
    assert!(!main_session::is_session_command(&invite));
    assert_eq!(invite.scope(), IntentScope::Voice);
    assert!(!main_session::is_session_command(&Command::Audio));
    let intents = CriticalIntents::default();
    let ticket = intents.ticket();
    let stop = CancellationToken::new();
    let update = |_| {};
    let work = SessionWork::new(&intents, ticket, &stop, &update);
    assert_eq!(work.commit(|| 42), Some(42));
    intents.request(CriticalAction::Leave);
    assert_eq!(work.commit(|| 43), Some(43));
    intents.request(CriticalAction::Logout);
    assert!(work.commit(|| panic!("Stale owner commit ran")).is_none());
    let fresh = SessionWork::new(&intents, intents.ticket(), &stop, &update);
    assert!(fresh
        .commit(|| panic!("Dominant Logout commit ran"))
        .is_none());
    stop.cancel();
    assert!(work.commit(|| panic!("Stopped commit ran")).is_none());
}

#[tokio::test]
async fn held_audio_settings_restore_services_leave_and_cancels_logout_or_window_without_late_owner_commit(
) {
    timeout(Duration::from_secs(15), async {
        for action in ["leave", "logout", "window"] {
            let fixture = fixture("unused").await;
            let (hub, _events) = HubConnection::connect(&fixture.endpoint, None).await.unwrap();
            let preferences = AudioPreferences::new(&json!({"language":"en","inputVolume":99}));
            assert!(preferences.begin_session("original-owner", hub.clone(), None));
            let original = preferences.settings().unwrap();
            let held = Arc::new(Notify::new());
            let (release, blocked) = std::sync::mpsc::channel();
            let holder = {
                let preferences = preferences.clone();
                let held = held.clone();
                tokio::task::spawn_blocking(move || {
                    let _settings = preferences.controls.settings.lock().unwrap();
                    held.notify_one();
                    blocked.recv().unwrap();
                })
            };
            held.notified().await;
            let intents = Arc::new(CriticalIntents::default());
            let ticket = intents.ticket();
            let stop = CancellationToken::new();
            let audio = audio(&stop);
            let monitor = audio.monitor(true);
            let generation = Arc::new(AtomicU64::new(1));
            let started = Arc::new(Notify::new());
            let commits = Arc::new(AtomicUsize::new(0));
            let (sender, mut published) = mpsc::unbounded_channel();
            let (commands, mut queued) = mpsc::channel(16);
            let queue = CommandQueue { sender: commands, intents: intents.clone() };
            for _ in 0..16 { queue.try_send(Command::Join("old-channel".into())).unwrap_or_else(|_| panic!("Voice FIFO fixture failed")); }
            let task = {
                let preferences = preferences.clone();
                let hub = hub.clone();
                let intents = intents.clone();
                let stop = stop.clone();
                let audio = audio.clone();
                let generation = generation.clone();
                let started = started.clone();
                let commits = commits.clone();
                tokio::spawn(async move {
                    let update = |event| { let _ = sender.send(event); };
                    let work = SessionWork::new(&intents, ticket, &stop, &update);
                    let data = zabor_native::auth::Bootstrap {
                        user: User { id: "restored-owner".into(), is_muted: true, is_server_deafened: true, ..Default::default() },
                        audio_settings: Some(json!({"inputVolume":73,"selectedInput":"retained-external-device"})),
                        ..Default::default()
                    };
                    let future = restore_audio_session(&work, &preferences, &hub, &data);
                    tokio::pin!(future);
                    let mut first = true;
                    let started_future = std::future::poll_fn(|cx| {
                        let result = future.as_mut().poll(cx);
                        if first { first = false; started.notify_one(); }
                        result
                    });
                    let result = drive(&work, ticket, &audio, &generation, None, started_future).await;
                    if let Some(Some(true)) = result {
                        assert!(work.commit(|| commits.fetch_add(1, Ordering::AcqRel)).is_some());
                    }
                    result
                })
            };
            started.notified().await;
            assert_eq!(commits.load(Ordering::Acquire), 0);
            let measured = Instant::now();
            match action {
                "leave" => {
                    for _ in 0..2 {
                        queue.try_send(Command::Leave).unwrap_or_else(|_| panic!("Priority Leave failed"));
                        assert!(matches!(timeout(Duration::from_millis(1500), published.recv()).await.unwrap().unwrap().for_current(&intents), Some(UiEvent::VoiceEnded)));
                        assert_eq!(audio.demand.active(), 0);
                        assert!(monitor.is_current());
                        monitor.configure(false);
                        monitor.configure(true);
                        assert!(!task.is_finished());
                        assert_eq!(preferences.settings().unwrap(), original);
                    }
                    release.send(()).unwrap();
                    holder.await.unwrap();
                    assert_eq!(task.await.unwrap(), Some(Some(true)));
                    assert_eq!(commits.load(Ordering::Acquire), 1);
                    assert_eq!(preferences.settings().unwrap().input_volume, 73.0);
                    assert_eq!(preferences.settings().unwrap().selected_input, "retained-external-device");
                    assert_eq!(preferences.controls.user_state(), (true, false));
                    assert_eq!(preferences.controls.server_state(), (false, true));
                    for _ in 0..16 { let command = queued.try_recv().unwrap(); assert!(!intents.is_current(command.ticket, command.command.scope())); }
                }
                "logout" | "window" => {
                    if action == "logout" { queue.try_send(Command::Logout).unwrap_or_else(|_| panic!("Priority Logout failed")); } else { stop.cancel(); }
                    assert!(timeout(Duration::from_millis(1500), task).await.unwrap().unwrap().is_none());
                    assert_eq!(commits.load(Ordering::Acquire), 0);
                    assert_eq!(preferences.settings().unwrap(), original);
                    release.send(()).unwrap();
                    holder.await.unwrap();
                    assert_eq!(preferences.controls.user_state(), (false, false));
                    assert_eq!(preferences.controls.server_state(), (false, false));
                    if action == "logout" {
                        assert_eq!(intents.next().await, CriticalAction::Logout);
                        let update = |_| {};
                        let fresh = SessionWork::new(&intents, intents.ticket(), &stop, &update);
                        let data = zabor_native::auth::Bootstrap { user: User { id: "next-owner".into(), ..Default::default() }, audio_settings: Some(json!({"inputVolume":31})), ..Default::default() };
                        assert_eq!(restore_audio_session(&fresh, &preferences, &hub, &data).await, Some(true));
                        assert_eq!(preferences.settings().unwrap().input_volume, 31.0);
                        assert_eq!(commits.load(Ordering::Acquire), 0);
                    }
                }
                _ => unreachable!(),
            }
            println!("audio restore held=settings-lock action={action} fixture_ms={} original_ticket=true detached_jobs=0 physical_audio=false", measured.elapsed().as_millis());
            hub.close();
            fixture.server.await.unwrap();
        }
    }).await.unwrap();
}

#[tokio::test]
async fn guarded_audio_restore_failure_keeps_existing_settings_and_stopped_work_never_applies_user_state(
) {
    let fixture = fixture("unused").await;
    let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
        .await
        .unwrap();
    let preferences = AudioPreferences::new(&json!({"inputVolume":99}));
    let original = preferences.settings().unwrap();
    let intents = CriticalIntents::default();
    let ticket = intents.ticket();
    let stop = CancellationToken::new();
    let update = |_| {};
    let work = SessionWork::new(&intents, ticket, &stop, &update);
    let data = zabor_native::auth::Bootstrap {
        user: User {
            is_muted: true,
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(
        restore_audio_session(&work, &preferences, &hub, &data).await,
        Some(false)
    );
    assert_eq!(preferences.settings().unwrap(), original);
    assert_eq!(preferences.controls.user_state(), (true, false));
    preferences.controls.set_user_state(false, false);
    stop.cancel();
    assert!(restore_audio_session(&work, &preferences, &hub, &data)
        .await
        .is_none());
    assert_eq!(preferences.controls.user_state(), (false, false));
    hub.close();
    fixture.server.await.unwrap();
}

#[tokio::test]
async fn logout_between_audio_acceptance_and_worker_acceptance_rejects_original_ticket_and_releases_resource_guards(
) {
    let fixture = fixture("unused").await;
    let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
        .await
        .unwrap();
    let preferences = AudioPreferences::new(&json!({"inputVolume":99}));
    let intents = CriticalIntents::default();
    let ticket = intents.ticket();
    let stop = CancellationToken::new();
    let update = |_| {};
    let work = SessionWork::new(&intents, ticket, &stop, &update);
    let data = zabor_native::auth::Bootstrap {
        user: User {
            id: "original-owner".into(),
            ..Default::default()
        },
        audio_settings: Some(json!({"inputVolume":73})),
        ..Default::default()
    };
    assert_eq!(
        restore_audio_session(&work, &preferences, &hub, &data).await,
        Some(true)
    );
    assert!(preferences.controls.settings.try_lock().is_ok());
    assert!(preferences
        .try_prepare_session("probe-owner", hub.clone(), None)
        .unwrap()
        .is_some());
    intents.request(CriticalAction::Logout);
    assert!(work
        .commit(|| panic!("Stale downstream worker acceptance ran"))
        .is_none());
    assert_eq!(intents.next().await, CriticalAction::Logout);
    let fresh = SessionWork::new(&intents, intents.ticket(), &stop, &update);
    let next = zabor_native::auth::Bootstrap {
        user: User {
            id: "next-owner".into(),
            ..Default::default()
        },
        audio_settings: Some(json!({"inputVolume":31})),
        ..Default::default()
    };
    assert_eq!(
        restore_audio_session(&fresh, &preferences, &hub, &next).await,
        Some(true)
    );
    assert!(restore_audio_session(&work, &preferences, &hub, &data)
        .await
        .is_none());
    assert_eq!(preferences.settings().unwrap().input_volume, 31.0);
    hub.close();
    fixture.server.await.unwrap();
}

#[tokio::test]
async fn held_failed_auth_preferences_services_leave_and_cancels_logout_or_window_before_reset() {
    timeout(Duration::from_secs(15), async {
        for action in ["leave", "logout", "window"] {
            let fixture = fixture("unused").await;
            let (hub, _events) = HubConnection::connect(&fixture.endpoint, None).await.unwrap();
            let preferences = AudioPreferences::new(&json!({"inputVolume":99}));
            assert!(preferences.begin_session("original-owner", hub.clone(), None));
            let held = Arc::new(Notify::new());
            let (release, blocked) = std::sync::mpsc::channel();
            let holder = {
                let preferences = preferences.clone();
                let held = held.clone();
                tokio::task::spawn_blocking(move || {
                    let _held = preferences.try_prepare_logout().unwrap().unwrap();
                    held.notify_one();
                    blocked.recv().unwrap();
                })
            };
            held.notified().await;
            let intents = Arc::new(CriticalIntents::default());
            let ticket = intents.ticket();
            let stop = CancellationToken::new();
            let audio = audio(&stop);
            let monitor = audio.monitor(true);
            let now = Instant::now();
            let capture = audio.mic_test.begin(now).unwrap();
            audio.mic_test.prepared(capture, now);
            audio.mic_test.snapshot(now + zabor_native::mic_test::MONITOR_SETTLE);
            for index in 0..500 {
                assert!(audio.mic_test.capture(
                    capture, &[0.1; 480], -20.0,
                    now + zabor_native::mic_test::MONITOR_SETTLE + zabor_native::mic_test::PIPELINE_FLUSH + Duration::from_millis(index * 10),
                ));
            }
            let original = audio.mic_test.snapshot(now);
            let started = Arc::new(Notify::new());
            let commits = Arc::new(AtomicUsize::new(0));
            let (sender, mut published) = mpsc::unbounded_channel();
            let (commands, mut queued) = mpsc::channel(16);
            let queue = CommandQueue { sender: commands, intents: intents.clone() };
            for _ in 0..16 { queue.try_send(Command::Join("old-channel".into())).unwrap_or_else(|_| panic!("Voice FIFO fixture failed")); }
            let task = {
                let preferences = preferences.clone();
                let audio = audio.clone();
                let intents = intents.clone();
                let stop = stop.clone();
                let started = started.clone();
                let commits = commits.clone();
                tokio::spawn(async move {
                    let update = |event| { let _ = sender.send(event); };
                    let work = SessionWork::new(&intents, ticket, &stop, &update);
                    let future = reset_audio_session(&work, &preferences, &audio);
                    tokio::pin!(future);
                    let mut first = true;
                    let started_future = std::future::poll_fn(|cx| {
                        let result = future.as_mut().poll(cx);
                        if first { first = false; started.notify_one(); }
                        result
                    });
                    let generation = AtomicU64::new(1);
                    let result = drive(&work, ticket, &audio, &generation, None, started_future).await;
                    if let Some(Some(true)) = result {
                        assert!(work.commit(|| commits.fetch_add(1, Ordering::AcqRel)).is_some());
                    }
                    result
                })
            };
            started.notified().await;
            let measured = Instant::now();
            if action == "leave" {
                for _ in 0..2 {
                    queue.try_send(Command::Leave).unwrap_or_else(|_| panic!("Priority Leave failed"));
                    assert!(matches!(timeout(Duration::from_millis(1500), published.recv()).await.unwrap().unwrap().for_current(&intents), Some(UiEvent::VoiceEnded)));
                    assert_eq!(audio.demand.active(), 0);
                    assert!(monitor.is_current());
                    assert_eq!(audio.mic_test.snapshot(now), original);
                    assert!(!task.is_finished());
                }
                release.send(()).unwrap();
                holder.await.unwrap();
                assert_eq!(task.await.unwrap(), Some(Some(true)));
                assert_eq!(commits.load(Ordering::Acquire), 1);
                assert!(!monitor.is_current());
                assert!(!audio.mic_test.has_demand());
                assert_eq!(audio.mic_test.snapshot(now).stored_samples, 0);
                assert!(!preferences.edit("original-owner", "outputVolume", json!(140)));
                for _ in 0..16 { let command = queued.try_recv().unwrap(); assert!(!intents.is_current(command.ticket, command.command.scope())); }
            } else {
                if action == "logout" { queue.try_send(Command::Logout).unwrap_or_else(|_| panic!("Priority Logout failed")); } else { stop.cancel(); }
                assert!(timeout(Duration::from_millis(1500), task).await.unwrap().unwrap().is_none());
                assert_eq!(commits.load(Ordering::Acquire), 0);
                assert_eq!(audio.mic_test.snapshot(now), original);
                release.send(()).unwrap();
                holder.await.unwrap();
                assert!(preferences.edit("original-owner", "outputVolume", json!(140)));
            }
            println!("audio reset held=preferences-lock action={action} fixture_ms={} original_ticket=true detached_jobs=0 physical_audio=false", measured.elapsed().as_millis());
            hub.close();
            fixture.server.await.unwrap();
        }
    }).await.unwrap();
}

#[tokio::test]
async fn failed_auth_reset_releases_partial_resources_and_rechecks_original_ticket_before_worker_cleanup(
) {
    let fixture = fixture("unused").await;
    let (hub, _events) = HubConnection::connect(&fixture.endpoint, None)
        .await
        .unwrap();
    let preferences = AudioPreferences::new(&json!({"inputVolume":99}));
    assert!(preferences.begin_session("original-owner", hub.clone(), None));
    let intents = CriticalIntents::default();
    let ticket = intents.ticket();
    let stop = CancellationToken::new();
    let update = |_| {};
    let work = SessionWork::new(&intents, ticket, &stop, &update);
    let audio = audio(&stop);
    let old = audio.monitor(true);
    {
        let held = audio.try_prepare_reset().unwrap().unwrap();
        assert!(try_reset_audio_session(&work, &preferences, &audio).is_none());
        assert!(preferences.try_prepare_logout().unwrap().is_some());
        drop(held);
    }
    assert!(old.is_current());
    assert_eq!(
        reset_audio_session(&work, &preferences, &audio).await,
        Some(true)
    );
    assert!(!old.is_current());
    assert!(preferences.try_prepare_logout().unwrap().is_some());
    assert!(audio.try_prepare_reset().unwrap().is_some());
    intents.request(CriticalAction::Logout);
    assert!(work
        .commit(|| panic!("Stale failed-auth worker cleanup ran"))
        .is_none());
    assert_eq!(intents.next().await, CriticalAction::Logout);
    assert!(preferences.begin_session("next-owner", hub.clone(), Some(&json!({"inputVolume":31}))));
    let current = audio.monitor(true);
    let generation = audio.demand.reserve().unwrap();
    audio.demand.activate(generation);
    assert!(reset_audio_session(&work, &preferences, &audio)
        .await
        .is_none());
    assert!(current.is_current());
    assert_eq!(audio.demand.active(), generation);
    assert!(preferences.edit("next-owner", "outputVolume", json!(140)));
    hub.close();
    fixture.server.await.unwrap();
}

#[tokio::test]
async fn failed_auth_preference_poison_still_resets_runtime_and_reports_incomplete_cleanup() {
    let preferences = AudioPreferences::new(&json!({"inputVolume":99}));
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = preferences.try_prepare_logout().unwrap().unwrap();
        panic!("Expected preference fixture poison");
    }))
    .is_err());
    let intents = CriticalIntents::default();
    let ticket = intents.ticket();
    let stop = CancellationToken::new();
    let update = |_| {};
    let work = SessionWork::new(&intents, ticket, &stop, &update);
    let audio = audio(&stop);
    let old = audio.monitor(true);
    assert_eq!(
        reset_audio_session(&work, &preferences, &audio).await,
        Some(false)
    );
    assert_eq!(audio.demand.active(), 0);
    assert!(!old.is_current());
    assert!(!audio.mic_test.has_demand());
    assert!(preferences.try_prepare_logout().is_err());
    intents.request(CriticalAction::Logout);
    let current = audio.monitor(true);
    assert!(reset_audio_session(&work, &preferences, &audio)
        .await
        .is_none());
    assert!(current.is_current());
    println!("audio reset poison=preferences legacy_partial=true poison_retained=true complete=false physical_audio=false");
}

#[tokio::test]
async fn native_voice_extraction_accepts_only_original_worker_resource_ticket_and_preserves_shared_monitor(
) {
    timeout(Duration::from_secs(15), async {
        let fixture = fixture("unused").await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None).await.unwrap();
        for mode in ["live", "leave", "logout", "window", "mismatch", "missing"] {
            let intents = CriticalIntents::default();
            let original = intents.ticket();
            let stop = CancellationToken::new();
            let controls = AudioPreferences::new(&json!({})).controls.clone();
            let audio = SharedAudio::new(controls.clone(), stop.child_token(), |_, _, _| {
                Box::pin(async { panic!("Voice extraction fixture must not start physical audio") })
            });
            let generation = audio.demand.reserve().unwrap();
            assert!(audio.demand.activate(generation));
            let monitor = audio.monitor(true);
            let (mut session, receiver) = VoiceSession::new(&hub, &user(), false, controls.clone(), audio.clone()).await.unwrap();
            session.call_peer = Some("fixture-peer".into());
            let mut voice = Some(session);
            let mut events = Some(receiver);
            let ticket = match mode {
                "leave" | "mismatch" => {
                    intents.request(CriticalAction::Leave);
                    assert_eq!(intents.next().await, CriticalAction::Leave);
                    if mode == "mismatch" { intents.ticket() } else { original }
                },
                "logout" => {
                    intents.request(CriticalAction::Logout);
                    assert_eq!(intents.next().await, CriticalAction::Logout);
                    original
                },
                "window" => { stop.cancel(); original },
                _ => original,
            };
            let resource_ticket = (mode != "missing").then_some(original);
            let monitor_current = monitor.is_current();
            controls.speaking.store(true, Ordering::Release);
            controls.mic_db.store((-17.0_f32).to_bits(), Ordering::Release);
            let epoch = controls.capture_epoch.load(Ordering::Acquire);
            let snapshot = audio.mic_test.snapshot(Instant::now());
            let update = |_| panic!("Extraction must not publish while holding the intent gate");
            let work = VoiceWork::new(&intents, ticket, &stop, &update);
            let mut accepted = 0;
            let mut connected = true;
            let disposed = work.take_voice(&mut voice, &mut events, resource_ticket, |session| {
                assert_eq!(session.unwrap().call_peer.as_deref(), Some("fixture-peer"));
                accepted += 1;
                connected = false;
            });
            assert_eq!(disposed.is_some(), mode == "live");
            assert_eq!(accepted, usize::from(mode == "live"));
            assert_eq!(connected, mode != "live");
            assert_eq!(voice.is_none(), mode == "live");
            assert_eq!(events.is_none(), mode == "live");
            if let Some(disposed) = disposed {
                assert_eq!(disposed.0.as_ref().unwrap().call_peer.as_deref(), Some("fixture-peer"));
                assert!(!disposed.1.as_ref().unwrap().is_closed());
                assert_eq!(intents.ticket(), original);
                drop(disposed);
            } else {
                assert_eq!(voice.as_ref().unwrap().call_peer.as_deref(), Some("fixture-peer"));
                assert!(!events.as_ref().unwrap().is_closed());
                drop(voice);
                drop(events);
            }
            assert_eq!(audio.demand.active(), generation);
            assert!(audio.demand.is_reserved(generation));
            assert!(controls.speaking.load(Ordering::Acquire));
            assert_eq!(f32::from_bits(controls.mic_db.load(Ordering::Acquire)), -17.0);
            assert_eq!(controls.capture_epoch.load(Ordering::Acquire), epoch);
            assert_eq!(monitor.is_current(), monitor_current);
            assert_eq!(audio.mic_test.snapshot(Instant::now()), snapshot);
            audio.wait_for_cleanup().await.unwrap();
            println!("voice extraction native={mode} original_resource_ticket=true accepted={} callback_and_slots_guarded=true status_monitor_unchanged=true physical_audio=false", mode == "live");
        }
        hub.close();
        assert_eq!(fixture.server.await.unwrap(), vec!["GetIceServers"; 6]);
    }).await.unwrap();
}

struct VoiceExtractionDropCheck {
    intents: Arc<CriticalIntents>,
    drops: Arc<AtomicUsize>,
}

impl Drop for VoiceExtractionDropCheck {
    fn drop(&mut self) {
        let _ = self.intents.ticket();
        self.drops.fetch_add(1, Ordering::AcqRel);
    }
}

#[tokio::test]
async fn native_voice_extraction_releases_intent_gate_before_rejected_callback_drop_and_accepted_unwind(
) {
    timeout(Duration::from_secs(10), async {
        let fixture = fixture("unused").await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None).await.unwrap();
        for mode in ["live", "leave", "window", "mismatch", "panic"] {
            let intents = Arc::new(CriticalIntents::default());
            let original = intents.ticket();
            let stop = CancellationToken::new();
            let controls = AudioPreferences::new(&json!({})).controls.clone();
            let audio = SharedAudio::new(controls.clone(), stop.child_token(), |_, _, _| {
                Box::pin(async { panic!("Voice extraction fixture must not start physical audio") })
            });
            let generation = audio.demand.reserve().unwrap();
            assert!(audio.demand.activate(generation));
            let monitor = audio.monitor(true);
            let (session, receiver) = VoiceSession::new(&hub, &user(), false, controls, audio.clone()).await.unwrap();
            let mut voice = Some(session);
            let mut events = Some(receiver);
            let ticket = if matches!(mode, "leave" | "mismatch") {
                intents.request(CriticalAction::Leave);
                assert_eq!(intents.next().await, CriticalAction::Leave);
                if mode == "mismatch" { intents.ticket() } else { original }
            } else { original };
            if mode == "window" { stop.cancel(); }
            let monitor_current = monitor.is_current();
            let drops = Arc::new(AtomicUsize::new(0));
            let probe = VoiceExtractionDropCheck { intents: intents.clone(), drops: drops.clone() };
            let update = |_| {};
            let work = VoiceWork::new(&intents, ticket, &stop, &update);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                work.take_voice(&mut voice, &mut events, Some(original), move |_| {
                    let _ = &probe;
                    assert_ne!(mode, "panic", "Expected accepted Voice extraction callback panic");
                })
            }));
            assert_eq!(drops.load(Ordering::Acquire), 1);
            assert_eq!(result.is_err(), mode == "panic");
            assert_eq!(voice.is_none(), matches!(mode, "live" | "panic"));
            assert_eq!(events.is_none(), matches!(mode, "live" | "panic"));
            if mode == "panic" {
                assert_eq!(work.commit(|| 42), Some(42));
            }
            drop(result);
            drop(voice);
            drop(events);
            assert_eq!(monitor.is_current(), monitor_current);
            assert_eq!(audio.demand.active(), 1);
            audio.wait_for_cleanup().await.unwrap();
            println!("voice extraction callback_drop={mode} gate_reentry=true owned_resources_outside_gate=true native_drop_order_unchanged=true physical_audio=false");
        }
        hub.close();
        assert_eq!(fixture.server.await.unwrap(), vec!["GetIceServers"; 5]);
    }).await.unwrap();
}

#[tokio::test]
async fn native_voice_installation_rechecks_ready_resources_and_atomically_replaces_original_ticket_slots(
) {
    timeout(Duration::from_secs(20), async {
        let fixture = fixture("unused").await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None).await.unwrap();
        let mut requests = 0;
        for kind in ["join", "call", "answer"] {
            for slots in ["empty", "voice", "receiver"] {
                for mode in ["live", "leave-pending", "leave-consumed", "logout-pending", "logout-consumed", "window"] {
                    let intents = CriticalIntents::default();
                    let old_ticket = intents.ticket();
                    intents.request(CriticalAction::Leave);
                    assert_eq!(intents.next().await, CriticalAction::Leave);
                    let ticket = intents.ticket();
                    let stop = CancellationToken::new();
                    let controls = AudioPreferences::new(&json!({})).controls.clone();
                    let audio = SharedAudio::new(controls.clone(), stop.child_token(), |_, _, _| {
                        Box::pin(async { panic!("Voice installation fixture must not start physical audio") })
                    });
                    let generation = audio.demand.reserve().unwrap();
                    assert!(audio.demand.activate(generation));
                    let monitor = audio.monitor(true);
                    let (mut voice, mut events) = if slots == "empty" { (None, None) } else {
                        let (mut old, events) = VoiceSession::new(&hub, &user(), false, controls.clone(), audio.clone()).await.unwrap();
                        requests += 1;
                        old.channel = Some("old-channel".into());
                        if slots == "receiver" { drop(old); (None, Some(events)) } else { (Some(old), Some(events)) }
                    };
                    let mut resource_ticket = (slots != "empty").then_some(old_ticket);
                    let original_resource_ticket = resource_ticket;
                    let update = |_| panic!("Voice installation must not publish inside the gate");
                    let work = VoiceWork::new(&intents, ticket, &stop, &update);
                    let prepared = VoiceSession::new(&hub, &user(), false, controls.clone(), audio.clone()).await.unwrap();
                    requests += 1;
                    let mut prepared = work.run(async { prepared }).await.unwrap();
                    if kind != "join" { prepared.0.call_peer = Some("prepared-peer".into()); }
                    match mode {
                        "leave-pending" | "leave-consumed" => intents.request(CriticalAction::Leave),
                        "logout-pending" | "logout-consumed" => intents.request(CriticalAction::Logout),
                        "window" => stop.cancel(),
                        _ => {}
                    }
                    if mode.ends_with("consumed") { intents.next().await; }
                    let monitor_current = monitor.is_current();
                    controls.speaking.store(true, Ordering::Release);
                    controls.mic_db.store((-17.0_f32).to_bits(), Ordering::Release);
                    let epoch = controls.capture_epoch.load(Ordering::Acquire);
                    let snapshot = audio.mic_test.snapshot(Instant::now());
                    let mut accepted = 0;
                    let mut connected = false;
                    let mut recovery = CallRecovery::default();
                    let replaced = work.install_voice(&mut voice, &mut events, &mut resource_ticket, prepared, || {
                        accepted += 1;
                        if kind != "join" { recovery.begin("prepared-peer"); }
                        if kind == "answer" { recovery.connected("prepared-peer"); connected = true; }
                    });
                    let installed = mode == "live";
                    assert_eq!(replaced.is_some(), installed);
                    assert_eq!(accepted, usize::from(installed));
                    assert_eq!(connected, installed && kind == "answer");
                    assert_eq!(recovery.set_session(RESUME_SESSION), installed && kind == "answer");
                    if let Some(replaced) = replaced {
                        assert_eq!(resource_ticket, Some(ticket));
                        assert_ne!(resource_ticket, original_resource_ticket);
                        assert_eq!(voice.as_ref().unwrap().channel, None);
                        assert_eq!(voice.as_ref().unwrap().call_peer.as_deref(), (kind != "join").then_some("prepared-peer"));
                        assert!(!events.as_ref().unwrap().is_closed());
                        assert_eq!(replaced.0.as_ref().map(|old| old.channel.as_deref()), (slots == "voice").then_some(Some("old-channel")));
                        assert_eq!(replaced.1.is_some(), slots != "empty");
                        if let Some(receiver) = replaced.1.as_ref() { assert_eq!(receiver.is_closed(), slots == "receiver"); }
                        drop(replaced);
                        assert!(!events.as_ref().unwrap().is_closed());
                    } else {
                        assert_eq!(resource_ticket, original_resource_ticket);
                        assert_eq!(voice.as_ref().map(|old| old.channel.as_deref()), (slots == "voice").then_some(Some("old-channel")));
                        assert_eq!(events.is_some(), slots != "empty");
                        if let Some(receiver) = events.as_ref() { assert_eq!(receiver.is_closed(), slots == "receiver"); }
                    }
                    drop(voice);
                    drop(events);
                    assert_eq!(audio.demand.active(), generation);
                    assert!(audio.demand.is_reserved(generation));
                    assert_eq!(monitor.is_current(), monitor_current);
                    assert!(controls.speaking.load(Ordering::Acquire));
                    assert_eq!(f32::from_bits(controls.mic_db.load(Ordering::Acquire)), -17.0);
                    assert_eq!(controls.capture_epoch.load(Ordering::Acquire), epoch);
                    assert_eq!(audio.mic_test.snapshot(Instant::now()), snapshot);
                    audio.wait_for_cleanup().await.unwrap();
                    println!("voice installation kind={kind} slots={slots} mode={mode} accepted={installed} original_ticket=true ready_result_rechecked=true old_resources_returned=true monitor_unchanged=true physical_audio=false");
                }
            }
        }
        hub.close();
        assert_eq!(fixture.server.await.unwrap(), vec!["GetIceServers"; requests]);
    }).await.unwrap();
}

#[tokio::test]
async fn native_voice_installation_keeps_rejected_and_replaced_resources_and_callback_unwind_outside_gate(
) {
    timeout(Duration::from_secs(15), async {
        let fixture = fixture("unused").await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None).await.unwrap();
        for mode in ["live", "leave", "logout", "window", "panic"] {
            let intents = Arc::new(CriticalIntents::default());
            let old_ticket = intents.ticket();
            intents.request(CriticalAction::Leave);
            assert_eq!(intents.next().await, CriticalAction::Leave);
            let ticket = intents.ticket();
            let stop = CancellationToken::new();
            let controls = AudioPreferences::new(&json!({})).controls.clone();
            let audio = SharedAudio::new(controls.clone(), stop.child_token(), |_, _, _| {
                Box::pin(async { panic!("Voice installation unwind fixture must not start physical audio") })
            });
            let generation = audio.demand.reserve().unwrap();
            assert!(audio.demand.activate(generation));
            let monitor = audio.monitor(true);
            let (mut old, receiver) = VoiceSession::new(&hub, &user(), false, controls.clone(), audio.clone()).await.unwrap();
            old.channel = Some("old-channel".into());
            let mut voice = Some(old);
            let mut events = Some(receiver);
            let mut resource_ticket = Some(old_ticket);
            let mut prepared = VoiceSession::new(&hub, &user(), false, controls, audio.clone()).await.unwrap();
            prepared.0.call_peer = Some("prepared-peer".into());
            match mode {
                "leave" => intents.request(CriticalAction::Leave),
                "logout" => intents.request(CriticalAction::Logout),
                "window" => stop.cancel(),
                _ => {}
            }
            let monitor_current = monitor.is_current();
            let drops = Arc::new(AtomicUsize::new(0));
            let probe = VoiceExtractionDropCheck { intents: intents.clone(), drops: drops.clone() };
            let update = |_| {};
            let work = VoiceWork::new(&intents, ticket, &stop, &update);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                work.install_voice(&mut voice, &mut events, &mut resource_ticket, prepared, move || {
                    let _ = &probe;
                    assert_ne!(mode, "panic", "Expected accepted Voice installation callback panic");
                })
            }));
            assert_eq!(drops.load(Ordering::Acquire), 1);
            assert_eq!(result.is_err(), mode == "panic");
            let installed = matches!(mode, "live" | "panic");
            assert_eq!(resource_ticket, Some(if installed { ticket } else { old_ticket }));
            assert_eq!(voice.as_ref().unwrap().call_peer.as_deref(), installed.then_some("prepared-peer"));
            assert_eq!(voice.as_ref().unwrap().channel.as_deref(), (!installed).then_some("old-channel"));
            assert!(!events.as_ref().unwrap().is_closed());
            if mode == "panic" { assert_eq!(work.commit(|| 42), Some(42)); }
            if let Ok(Some(replaced)) = &result {
                assert_eq!(replaced.0.as_ref().unwrap().channel.as_deref(), Some("old-channel"));
                assert!(!replaced.1.as_ref().unwrap().is_closed());
            }
            drop(result);
            assert!(!events.as_ref().unwrap().is_closed());
            drop(voice);
            drop(events);
            assert_eq!(monitor.is_current(), monitor_current);
            assert_eq!(audio.demand.active(), generation);
            audio.wait_for_cleanup().await.unwrap();
            println!("voice installation callback_drop={mode} gate_reentry=true native_resources_outside_gate=true original_ticket=true physical_audio=false");
        }
        hub.close();
        assert_eq!(fixture.server.await.unwrap(), vec!["GetIceServers"; 10]);
    }).await.unwrap();
}

#[tokio::test]
async fn native_voice_installation_linearizes_priority_and_rechecks_stop_after_waiting_for_intent_gate(
) {
    timeout(Duration::from_secs(15), async {
        let fixture = fixture("unused").await;
        let (hub, _events) = HubConnection::connect(&fixture.endpoint, None).await.unwrap();
        for mode in ["leave", "logout", "window"] {
            let intents = CriticalIntents::default();
            let ticket = intents.ticket();
            let stop = CancellationToken::new();
            let audio = audio(&stop);
            let monitor = audio.monitor(true);
            let prepared = VoiceSession::new(&hub, &user(), false, AudioPreferences::new(&json!({})).controls.clone(), audio.clone()).await.unwrap();
            let update = |_| {};
            let work = VoiceWork::new(&intents, ticket, &stop, &update);
            let (entered, waiting) = std::sync::mpsc::channel();
            let (release, held) = std::sync::mpsc::channel();
            let (started, starting) = std::sync::mpsc::channel();
            let (done, finished) = std::sync::mpsc::channel();
            let metadata = AtomicUsize::new(0);
            let (voice, events, resource_ticket, replaced) = std::thread::scope(|scope| {
                let (intents, work, metadata) = (&intents, &work, &metadata);
                if mode == "window" {
                    let holder = scope.spawn(move || intents.commit(ticket, IntentScope::Session, || {
                        entered.send(()).unwrap();
                        held.recv_timeout(Duration::from_secs(3)).unwrap();
                    }));
                    waiting.recv_timeout(Duration::from_secs(1)).unwrap();
                    let installer = scope.spawn(move || {
                        let (mut voice, mut events, mut resource_ticket) = (None, None, None);
                        started.send(()).unwrap();
                        let replaced = work.install_voice(&mut voice, &mut events, &mut resource_ticket, prepared, || panic!("Stopped installer must not mutate slots"));
                        done.send(()).unwrap();
                        (voice, events, resource_ticket, replaced)
                    });
                    starting.recv_timeout(Duration::from_secs(1)).unwrap();
                    assert!(finished.recv_timeout(Duration::from_millis(30)).is_err());
                    stop.cancel();
                    release.send(()).unwrap();
                    holder.join().unwrap().unwrap();
                    finished.recv_timeout(Duration::from_secs(1)).unwrap();
                    installer.join().unwrap()
                } else {
                    let installer = scope.spawn(move || {
                        let (mut voice, mut events, mut resource_ticket) = (None, None, None);
                        let replaced = work.install_voice(&mut voice, &mut events, &mut resource_ticket, prepared, || {
                            entered.send(()).unwrap();
                            held.recv_timeout(Duration::from_secs(3)).unwrap();
                            metadata.store(1, Ordering::Release);
                        });
                        (voice, events, resource_ticket, replaced)
                    });
                    waiting.recv_timeout(Duration::from_secs(1)).unwrap();
                    let requester = scope.spawn(move || {
                        started.send(()).unwrap();
                        intents.request(if mode == "leave" { CriticalAction::Leave } else { CriticalAction::Logout });
                        assert_eq!(metadata.load(Ordering::Acquire), 1);
                        done.send(()).unwrap();
                    });
                    starting.recv_timeout(Duration::from_secs(1)).unwrap();
                    assert!(finished.recv_timeout(Duration::from_millis(30)).is_err());
                    release.send(()).unwrap();
                    finished.recv_timeout(Duration::from_secs(1)).unwrap();
                    requester.join().unwrap();
                    installer.join().unwrap()
                }
            });
            assert_eq!(voice.is_some(), mode != "window");
            assert_eq!(events.is_some(), mode != "window");
            assert_eq!(resource_ticket, (mode != "window").then_some(ticket));
            assert_eq!(replaced.is_some(), mode != "window");
            assert_eq!(metadata.load(Ordering::Acquire), usize::from(mode != "window"));
            assert!(work.commit(|| panic!("Revoked installation ticket must not revive")).is_none());
            drop(replaced);
            drop(voice);
            drop(events);
            assert_eq!(monitor.is_current(), mode != "window");
            assert_eq!(audio.demand.active(), 1);
            audio.wait_for_cleanup().await.unwrap();
            println!("voice installation gate={mode} strict_linearization=true stop_after_gate_wait_rechecked=true original_ticket=true physical_audio=false");
        }
        hub.close();
        assert_eq!(fixture.server.await.unwrap(), vec!["GetIceServers"; 3]);
    }).await.unwrap();
}

async fn hold_logout_preferences(
    preferences: Arc<AudioPreferences>,
) -> (std::sync::mpsc::Sender<()>, tokio::task::JoinHandle<()>) {
    let entered = Arc::new(Notify::new());
    let (release, blocked) = std::sync::mpsc::channel();
    let held = entered.clone();
    let holder = tokio::task::spawn_blocking(move || {
        let _held = preferences.try_prepare_logout().unwrap().unwrap();
        held.notify_one();
        blocked.recv_timeout(Duration::from_secs(4)).unwrap();
    });
    entered.notified().await;
    (release, holder)
}

async fn wait_for_session_removal(profile: &Profile) {
    timeout(Duration::from_millis(1500), async {
        while profile.root().join("session.enc").exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_logout_worker_registers_durable_clear_before_blocked_resource_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let profile = Arc::new(Profile::open(root.path()).unwrap());
    profile
        .save_session("original-owner", &credentials())
        .unwrap();
    let intents = Arc::new(CriticalIntents::default());
    let stop = CancellationToken::new();
    let session_io = SessionIo::new(Some(profile.clone()), intents.clone(), stop.clone());
    let preferences = AudioPreferences::new(&json!({"inputVolume":99}));
    let audio = audio(&stop);
    let monitor = audio.monitor(true);
    let (release, holder) = hold_logout_preferences(preferences.clone()).await;
    let (sender, commands) = mpsc::channel(16);
    let queue = CommandQueue {
        sender,
        intents: intents.clone(),
    };
    queue
        .try_send(Command::Logout)
        .unwrap_or_else(|_| panic!("Priority Logout failed"));
    let (updates, mut published) = mpsc::unbounded_channel();
    let task = tokio::spawn(worker(
        "http://127.0.0.1:1/zabor_v3".into(),
        commands,
        Arc::new(move |event| {
            let _ = updates.send(event);
        }),
        stop.clone(),
        Some(profile.clone()),
        WorkerMedia {
            video_sink: Arc::new(|_| panic!("Logout fixture must not produce video")),
            video_generation: Arc::new(AtomicU64::new(1)),
            audio_preferences: preferences.clone(),
            audio_runtime: audio.clone(),
            intents: intents.clone(),
            session_io: session_io.clone(),
        },
    ));
    let measured = Instant::now();
    wait_for_session_removal(&profile).await;
    assert!(!task.is_finished());
    assert!(monitor.is_current());
    assert!(published.try_recv().is_err());
    let mic_state = audio.mic_test.snapshot(Instant::now());
    stop.cancel();
    timeout(Duration::from_millis(1500), task)
        .await
        .unwrap()
        .unwrap();
    release.send(()).unwrap();
    holder.await.unwrap();
    session_io.finish().await.unwrap();
    assert!(profile.load_session().unwrap().is_none());
    assert!(!monitor.is_current());
    assert!(audio.mic_test.has_demand());
    assert_eq!(audio.demand.active(), 1);
    assert_eq!(audio.mic_test.snapshot(Instant::now()), mic_state);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    println!("logout admission held=preferences-lock actual_worker=true clear_before_cleanup=true window_drain=true fixture_ms={} physical_audio=false", measured.elapsed().as_millis());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_normal_logout_wait_services_repeated_leave_and_rechecks_replacement_owner() {
    timeout(Duration::from_secs(15), async {
        for action in ["release", "window", "replacement"] {
            let fixture = fixture("unused").await;
            let (hub, _events) = HubConnection::connect(&fixture.endpoint, None).await.unwrap();
            let root = tempfile::tempdir().unwrap();
            let profile = Arc::new(Profile::open(root.path()).unwrap());
            profile.save_session("original-owner", &credentials()).unwrap();
            let intents = Arc::new(CriticalIntents::default());
            let stop = CancellationToken::new();
            let session_io = SessionIo::new(Some(profile.clone()), intents.clone(), stop.clone());
            let preferences = AudioPreferences::new(&json!({"inputVolume":99}));
            assert!(preferences.begin_session("original-owner", hub.clone(), None));
            let audio = audio(&stop);
            let monitor = audio.monitor(true);
            let original = audio.mic_test.snapshot(Instant::now());
            let generation = Arc::new(AtomicU64::new(1));
            let (release, holder) = hold_logout_preferences(preferences.clone()).await;
            let (sender, commands) = mpsc::channel(16);
            let queue = CommandQueue { sender, intents: intents.clone() };
            for _ in 0..16 {
                queue.try_send(Command::Join("old-channel".into())).unwrap_or_else(|_| panic!("Voice FIFO fixture failed"));
            }
            queue.try_send(Command::Logout).unwrap_or_else(|_| panic!("Priority Logout failed"));
            let original_ticket = intents.ticket();
            let (updates, mut published) = mpsc::unbounded_channel();
            let task = tokio::spawn(worker(
                "http://127.0.0.1:1/zabor_v3".into(), commands,
                Arc::new(move |event| { let _ = updates.send(event); }), stop.clone(), Some(profile.clone()),
                WorkerMedia {
                    video_sink: Arc::new(|_| panic!("Logout fixture must not produce video")),
                    video_generation: generation.clone(), audio_preferences: preferences.clone(),
                    audio_runtime: audio.clone(), intents: intents.clone(), session_io: session_io.clone(),
                },
            ));
            wait_for_session_removal(&profile).await;
            let measured = Instant::now();
            for _ in 0..2 {
                queue.try_send(Command::Leave).unwrap_or_else(|_| panic!("Priority Leave failed"));
                assert!(matches!(timeout(Duration::from_millis(1500), published.recv()).await.unwrap().unwrap().for_current(&intents), Some(UiEvent::VoiceEnded)));
                assert_eq!(audio.demand.active(), 0);
                assert!(monitor.is_current());
                assert_eq!(audio.mic_test.snapshot(Instant::now()), original);
                assert!(!task.is_finished());
            }
            assert_eq!(generation.load(Ordering::Acquire), 3);
            if action == "window" {
                stop.cancel();
                timeout(Duration::from_millis(1500), task).await.unwrap().unwrap();
                assert!(published.try_recv().is_err());
                release.send(()).unwrap();
                holder.await.unwrap();
                assert!(preferences.edit("original-owner", "outputVolume", json!(140)));
                assert!(audio.mic_test.has_demand());
                assert_eq!(audio.mic_test.snapshot(Instant::now()), original);
                assert_eq!(generation.load(Ordering::Acquire), 4);
            } else {
                if action == "replacement" {
                    queue.try_send(Command::Logout).unwrap_or_else(|_| panic!("Replacement Logout failed"));
                    let update = |_| {};
                    let old = SessionWork::new(&intents, original_ticket, &stop, &update);
                    assert!(old.commit(|| panic!("Replaced Logout worker acceptance ran")).is_none());
                }
                release.send(()).unwrap();
                holder.await.unwrap();
                assert!(matches!(timeout(Duration::from_millis(1500), published.recv()).await.unwrap().unwrap().for_current(&intents), Some(UiEvent::VoiceEnded)));
                assert!(matches!(timeout(Duration::from_millis(1500), published.recv()).await.unwrap().unwrap().for_current(&intents), Some(UiEvent::SignedOut)));
                assert!(published.try_recv().is_err());
                assert_eq!(generation.load(Ordering::Acquire), 4);
                assert!(!monitor.is_current());
                assert!(!audio.mic_test.has_demand());
                assert!(!preferences.edit("original-owner", "outputVolume", json!(140)));
                drop(queue);
                timeout(Duration::from_millis(1500), task).await.unwrap().unwrap();
                assert_eq!(generation.load(Ordering::Acquire), 5);
            }
            session_io.finish().await.unwrap();
            assert!(profile.load_session().unwrap().is_none());
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
            hub.close();
            fixture.server.await.unwrap();
            println!("normal logout held=preferences-lock action={action} actual_worker=true repeated_leave=2 queued_voice_commands=16 accepted_cleanup={} fixture_ms={} physical_audio=false", usize::from(action != "window"), measured.elapsed().as_millis());
        }
    }).await.unwrap();
}

#[tokio::test]
async fn actual_normal_logout_preserves_partial_poison_reset_and_reports_cleanup_failure() {
    let preferences = AudioPreferences::new(&json!({"inputVolume":99}));
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = preferences.try_prepare_logout().unwrap().unwrap();
        panic!("Expected normal Logout preference fixture poison");
    }))
    .is_err());
    let intents = Arc::new(CriticalIntents::default());
    let stop = CancellationToken::new();
    let audio = audio(&stop);
    let monitor = audio.monitor(true);
    let generation = Arc::new(AtomicU64::new(1));
    let session_io = SessionIo::new(None, intents.clone(), stop.clone());
    let (sender, commands) = mpsc::channel(16);
    let queue = CommandQueue {
        sender,
        intents: intents.clone(),
    };
    queue
        .try_send(Command::Logout)
        .unwrap_or_else(|_| panic!("Priority Logout failed"));
    let (updates, mut published) = mpsc::unbounded_channel();
    let task = tokio::spawn(worker(
        "http://127.0.0.1:1/zabor_v3".into(),
        commands,
        Arc::new(move |event| {
            let _ = updates.send(event);
        }),
        stop.clone(),
        None,
        WorkerMedia {
            video_sink: Arc::new(|_| panic!("Poison fixture must not produce video")),
            video_generation: generation.clone(),
            audio_preferences: preferences.clone(),
            audio_runtime: audio.clone(),
            intents: intents.clone(),
            session_io: session_io.clone(),
        },
    ));
    assert!(matches!(
        timeout(Duration::from_millis(1500), published.recv())
            .await
            .unwrap()
            .unwrap()
            .for_current(&intents),
        Some(UiEvent::VoiceEnded)
    ));
    assert!(matches!(
        timeout(Duration::from_millis(1500), published.recv())
            .await
            .unwrap()
            .unwrap()
            .for_current(&intents),
        Some(UiEvent::SignedOut)
    ));
    assert!(matches!(
        timeout(Duration::from_millis(1500), published.recv())
            .await
            .unwrap()
            .unwrap()
            .for_current(&intents),
        Some(UiEvent::Toast("validation.sessionSaveError"))
    ));
    assert!(!monitor.is_current());
    assert_eq!(audio.demand.active(), 0);
    assert!(!audio.mic_test.has_demand());
    assert_eq!(generation.load(Ordering::Acquire), 2);
    assert!(preferences.try_prepare_logout().is_err());
    drop(queue);
    timeout(Duration::from_millis(1500), task)
        .await
        .unwrap()
        .unwrap();
    session_io.finish().await.unwrap();
    println!("normal logout poison=preferences actual_worker=true legacy_partial=true poison_retained=true complete=false physical_audio=false");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_failed_auth_worker_registers_durable_clear_before_reset_wait_and_window_exit() {
    let fixture = fixture_with_login("Login", Value::Null).await;
    let root = tempfile::tempdir().unwrap();
    let profile = Arc::new(Profile::open(root.path()).unwrap());
    profile
        .save_session("original-owner", &credentials())
        .unwrap();
    let intents = Arc::new(CriticalIntents::default());
    let stop = CancellationToken::new();
    let session_io = SessionIo::new(Some(profile.clone()), intents.clone(), stop.clone());
    let preferences = AudioPreferences::new(&json!({"inputVolume":99}));
    let audio = audio(&stop);
    let monitor = audio.monitor(true);
    let (sender, commands) = mpsc::channel(16);
    let queue = CommandQueue {
        sender,
        intents: intents.clone(),
    };
    let (updates, mut published) = mpsc::unbounded_channel();
    let task = tokio::spawn(worker(
        fixture.endpoint.clone(),
        commands,
        Arc::new(move |event| {
            let _ = updates.send(event);
        }),
        stop.clone(),
        Some(profile.clone()),
        WorkerMedia {
            video_sink: Arc::new(|_| panic!("Failed-auth fixture must not produce video")),
            video_generation: Arc::new(AtomicU64::new(1)),
            audio_preferences: preferences.clone(),
            audio_runtime: audio.clone(),
            intents: intents.clone(),
            session_io: session_io.clone(),
        },
    ));
    timeout(Duration::from_secs(3), fixture.held.notified())
        .await
        .unwrap();
    assert!(matches!(
        published.recv().await.unwrap().for_current(&intents),
        Some(UiEvent::Connected)
    ));
    let (release, holder) = hold_logout_preferences(preferences.clone()).await;
    fixture.release.notify_one();
    let measured = Instant::now();
    wait_for_session_removal(&profile).await;
    assert!(!task.is_finished());
    assert!(monitor.is_current());
    assert!(published.try_recv().is_err());
    let mic_state = audio.mic_test.snapshot(Instant::now());
    stop.cancel();
    timeout(Duration::from_millis(1500), task)
        .await
        .unwrap()
        .unwrap();
    release.send(()).unwrap();
    holder.await.unwrap();
    drop(queue);
    session_io.finish().await.unwrap();
    assert!(profile.load_session().unwrap().is_none());
    assert!(!monitor.is_current());
    assert!(audio.mic_test.has_demand());
    assert_eq!(audio.demand.active(), 1);
    assert_eq!(audio.mic_test.snapshot(Instant::now()), mic_state);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    assert_eq!(
        timeout(Duration::from_millis(1500), fixture.server)
            .await
            .unwrap()
            .unwrap(),
        vec!["CheckUserExists", "Login"]
    );
    println!("failed auth admission held=preferences-lock actual_worker=true clear_before_reset=true window_drain=true fixture_ms={} physical_audio=false", measured.elapsed().as_millis());
}
