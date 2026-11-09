use super::*;
use crate::shell::member_actions::{Operation, Request, Response};
use zabor_native::auth::Channel;
use zabor_native::native_audio::test_source::{self, Lifecycle};

async fn joined(
    endpoint: &str,
    intents: Arc<CriticalIntents>,
    expected_login_channel: Option<&str>,
) -> (MemberWorkerFixture, Arc<Lifecycle>, String, Vec<User>) {
    let stop = CancellationToken::new();
    let preferences = AudioPreferences::new(&json!({}));
    let (audio, lifecycle) = test_source::shared(preferences.controls.clone(), stop.child_token());
    let mut worker =
        MemberWorkerFixture::start_with_audio(endpoint, intents.clone(), stop, audio, preferences)
            .await;
    assert_eq!(
        worker.signed_in_user.current_channel_id.as_deref(),
        expected_login_channel
    );
    let (channel, users) = join(&mut worker, &intents).await;
    assert_eq!(lifecycle.started(), 2);
    assert_eq!(lifecycle.exited(), 0);
    (worker, lifecycle, channel, users)
}

async fn observe_state(fixture: &mut Fixture) {
    assert_eq!(
        timeout(Duration::from_secs(2), fixture.hints.recv())
            .await
            .unwrap()
            .unwrap(),
        "UpdateUserState"
    );
}

async fn join(worker: &mut MemberWorkerFixture, intents: &CriticalIntents) -> (String, Vec<User>) {
    worker
        .queue
        .try_send(Command::Join("fixture-channel".into()))
        .unwrap_or_else(|_| panic!("actual post-Join queue failed"));
    loop {
        match worker.events.recv().await.unwrap().for_current(intents) {
            Some(UiEvent::Voice(channel, users)) => {
                assert_eq!(channel, "fixture-channel");
                assert_eq!(users.len(), 1);
                assert_eq!(users[0].id, "fixture-owner");
                return (channel, users);
            }
            Some(UiEvent::Toast(key)) => panic!("actual original Join failed: {key}"),
            Some(UiEvent::Members(event)) => {
                assert!(!matches!(event.result, Ok(Response::Confirmed)))
            }
            _ => {}
        }
    }
}

fn request(intents: &CriticalIntents, mode: &str) -> Request {
    Request::new(
        if mode == "owner" {
            "other-owner"
        } else {
            "fixture-owner"
        },
        Operation::Invite {
            channel: Channel {
                id: if mode == "channel" {
                    "other-channel"
                } else {
                    "fixture-channel"
                }
                .into(),
                name: "Fixture".into(),
                owner_id: "fixture-owner".into(),
                ..Default::default()
            },
            target: Box::new(User {
                id: "fixture-peer".into(),
                username: "Fixture".into(),
                is_online: mode != "offline",
                ..Default::default()
            }),
        },
        intents,
    )
    .unwrap()
}

fn dispatch(worker: &MemberWorkerFixture, request: &Request) {
    worker
        .queue
        .sender
        .try_send(super::super::QueuedCommand {
            ticket: request.ticket,
            command: Command::MemberAction(request.clone()),
        })
        .unwrap_or_else(|_| panic!("actual post-Join Invite queue failed"));
}

async fn response(
    worker: &mut MemberWorkerFixture,
    intents: &CriticalIntents,
    request: &Request,
) -> bool {
    loop {
        if let Some(UiEvent::Members(event)) =
            worker.events.recv().await.unwrap().for_current(intents)
        {
            assert!(event.request.same(request));
            assert_eq!(event.request.ticket, request.ticket);
            return matches!(event.result, Ok(Response::Confirmed));
        }
    }
}

#[tokio::test]
async fn actual_post_join_invite_uses_original_resource_not_absent_or_stale_login_channel() {
    timeout(Duration::from_secs(30), async {
        for stale in [false, true] {
            for mode in ["true", "false", "null", "channel", "offline", "owner"] {
                let mut login = user();
                login.current_channel_id = stale.then(|| "stale-login-channel".into());
                let mut fixture = fixture_with_audio_response(
                    "CallToChannel",
                    json!(login),
                    Some(match mode {
                        "false" => json!(false),
                        "null" => Value::Null,
                        _ => json!(true),
                    }),
                    Vec::new(),
                    true,
                ).await;
                let intents = Arc::new(CriticalIntents::default());
                let (mut worker, lifecycle, _, _) = joined(&fixture.endpoint, intents.clone(), stale.then_some("stale-login-channel")).await;
                observe_state(&mut fixture).await;
                let request = request(&intents, mode);
                dispatch(&worker, &request);
                let invoked = !matches!(mode, "channel" | "offline" | "owner");
                if invoked {
                    fixture.held.notified().await;
                    fixture.release.notify_one();
                }
                assert_eq!(response(&mut worker, &intents, &request).await, mode == "true");
                worker.finish().await;
                assert_eq!(lifecycle.started(), lifecycle.exited());
                let methods = fixture.server.await.unwrap();
                assert_eq!(methods.iter().filter(|method| *method == "JoinChannel").count(), 1);
                assert_eq!(methods.iter().filter(|method| *method == "UpdateUserState").count(), 1);
                assert_eq!(methods.iter().filter(|method| *method == "CallToChannel").count(), usize::from(invoked));
                assert!(!methods.iter().any(|method| matches!(method.as_str(), "StartCall" | "SendWebRTCOffer")));
                println!("post_join_invite mode={mode} stale_login={stale} original_join=true original_resource=true original_request=true device_owner_threads_joined=true physical_audio=false");
            }
        }
    }).await.unwrap();
}

#[tokio::test]
async fn actual_post_join_invite_rejects_held_leave_logout_view_window_and_old_request_after_rejoin(
) {
    timeout(Duration::from_secs(30), async {
        for mode in ["leave", "logout", "view", "window", "rejoin"] {
            let mut fixture = fixture_with_audio_response("CallToChannel", json!(user()), Some(json!(true)), Vec::new(), true).await;
            let intents = Arc::new(CriticalIntents::default());
            let (mut worker, lifecycle, _, _) = joined(&fixture.endpoint, intents.clone(), None).await;
            observe_state(&mut fixture).await;
            let original = request(&intents, "true");
            dispatch(&worker, &original);
            fixture.held.notified().await;
            let mut rejected = false;
            match mode {
                "leave" | "rejoin" => {
                    worker.queue.try_send(Command::Leave).unwrap_or_else(|_| panic!("Invite Leave queue failed"));
                    loop {
                        match worker.events.recv().await.unwrap().for_current(&intents) {
                            Some(UiEvent::VoiceEnded) => break,
                            Some(UiEvent::Members(event)) => {
                                assert!(event.request.same(&original));
                                assert_eq!(event.request.ticket, original.ticket);
                                assert!(!matches!(event.result, Ok(Response::Confirmed)));
                                rejected = true;
                            }
                            _ => {}
                        }
                    }
                    assert!(original.stop.is_cancelled());
                    assert_eq!(timeout(Duration::from_secs(2), fixture.hints.recv()).await.unwrap().unwrap(), "LeaveChannel");
                }
                "logout" => { worker.queue.try_send(Command::Logout).unwrap_or_else(|_| panic!("Invite Logout queue failed")); }
                "view" => original.stop.cancel(),
                "window" => worker.stop.cancel(),
                _ => unreachable!(),
            }
            fixture.release.notify_one();
            if matches!(mode, "leave" | "rejoin") && !rejected {
                assert!(!response(&mut worker, &intents, &original).await);
            }
            if mode == "view" {
                assert!(!response(&mut worker, &intents, &original).await);
            } else if mode == "logout" {
                loop {
                    if matches!(worker.events.recv().await.unwrap().for_current(&intents), Some(UiEvent::SignedOut)) { break; }
                }
            } else if mode == "rejoin" {
                join(&mut worker, &intents).await;
                observe_state(&mut fixture).await;
                assert!(!intents.is_current(original.ticket, IntentScope::Voice));
                dispatch(&worker, &original);
                worker.queue.try_send(Command::Modal { kind: 1, value: "Fixture".into() }).unwrap_or_else(|_| panic!("old Invite rejection barrier failed"));
                loop {
                    match worker.events.recv().await.unwrap().for_current(&intents) {
                        Some(UiEvent::Members(event)) => assert!(!event.request.same(&original) || !matches!(event.result, Ok(Response::Confirmed))),
                        Some(UiEvent::Data(_)) => break,
                        _ => {}
                    }
                }
            }
            worker.finish().await;
            assert_eq!(lifecycle.started(), lifecycle.exited());
            let methods = fixture.server.await.unwrap();
            assert_eq!(methods.iter().filter(|method| *method == "CallToChannel").count(), 1);
            assert_eq!(methods.iter().filter(|method| *method == "JoinChannel").count(), if mode == "rejoin" { 2 } else { 1 });
            assert_eq!(methods.iter().filter(|method| *method == "UpdateUserState").count(), if mode == "rejoin" { 2 } else { 1 });
            println!("post_join_invite held={mode} original_request_revoked=true original_join=true original_io_audio_worker_server_joined=true physical_audio=false");
        }
    }).await.unwrap();
}

pub(in crate::shell) async fn member_origin_invite_original_worker(
    intents: Arc<CriticalIntents>,
    mode: &'static str,
    on_voice: impl FnOnce(String, Vec<User>) -> Request,
    on_held: impl FnOnce(),
) -> crate::shell::member_ui::Event {
    timeout(Duration::from_secs(10), async {
        let mut fixture = fixture_with_audio_response(
            "CallToChannel",
            json!(user()),
            Some(match mode {
                "false" => json!(false),
                "null" => Value::Null,
                _ => json!(true),
            }),
            Vec::new(),
            true,
        ).await;
        let (mut worker, lifecycle, channel, users) = joined(&fixture.endpoint, intents.clone(), None).await;
        observe_state(&mut fixture).await;
        let original = on_voice(channel, users);
        dispatch(&worker, &original);
        fixture.held.notified().await;
        on_held();
        fixture.release.notify_one();
        let event = loop {
            if let Some(UiEvent::Members(event)) = worker.events.recv().await.unwrap().for_current(&intents) {
                assert!(event.request.same(&original));
                assert_eq!(event.request.ticket, original.ticket);
                break event;
            }
        };
        worker.finish().await;
        assert_eq!(lifecycle.started(), lifecycle.exited());
        let methods = fixture.server.await.unwrap();
        assert_eq!(methods.iter().filter(|method| *method == "JoinChannel").count(), 1);
        assert_eq!(methods.iter().filter(|method| *method == "UpdateUserState").count(), 1);
        assert_eq!(methods.iter().filter(|method| *method == "CallToChannel").count(), 1);
        assert!(!methods.iter().any(|method| matches!(method.as_str(), "StartCall" | "SendWebRTCOffer")));
        println!("member_origin_invite mode={mode} actual_original_join=true original_member_request=true original_worker_io_audio_device_threads_server_joined=true physical_audio=false");
        event
    }).await.unwrap()
}
