use super::{
    main_session, restore_audio, AuthClient, Command, CommandQueue, MainResult, SessionWork,
    UiEvent, VoiceLeave, VoiceSession, VoiceWork,
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
                    let _ = socket.send(Message::text(encode(&json!({"type":3,"invocationId":request["invocationId"],"result":answer(hold)})).unwrap())).await;
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
