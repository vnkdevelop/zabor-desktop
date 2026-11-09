use super::*;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::PhysicalSize;
use tokio::sync::mpsc;
use zabor_native::strings::Strings;

struct Headless;

impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        window.set_size(PhysicalSize::new(1100, 760));
        Ok(window)
    }
}

type Fixture = (
    NativeApp,
    Rc<RefCell<MemberUi>>,
    Rc<RefCell<AppUi>>,
    Rc<RefCell<Bootstrap>>,
    mpsc::Receiver<QueuedCommand>,
    Arc<CriticalIntents>,
    Timer,
);

fn member() -> User {
    User {
        id: "member".into(),
        username: "Fixture".into(),
        display_name: "Member".into(),
        last_online: Some("2024-03-10T09:59:59Z".into()),
        ..Default::default()
    }
}

fn fixture() -> Fixture {
    fixture_with_adapter(|_, _, _, _, _| {})
}

fn fixture_with_adapter(
    install: impl FnOnce(
        &NativeApp,
        &Rc<RefCell<AppUi>>,
        &Rc<RefCell<Bootstrap>>,
        &CommandQueue,
        &Arc<CriticalIntents>,
    ),
) -> Fixture {
    slint::platform::set_platform(Box::new(Headless)).unwrap();
    let ui = NativeApp::new().unwrap();
    ui.on_tr(|key, language| {
        Strings::new(language.as_str())
            .unwrap()
            .text(key.as_str())
            .into()
    });
    ui.set_user_id("owner".into());
    ui.set_authenticated(true);
    ui.set_connected(true);
    let mut data = Bootstrap::default();
    data.user.id = "owner".into();
    data.channels.push(Channel {
        id: "channel".into(),
        name: "Channel".into(),
        owner_id: "owner".into(),
        ..Default::default()
    });
    let data = Rc::new(RefCell::new(data));
    let state = Rc::new(RefCell::new(MemberUi::default()));
    let app = Rc::new(RefCell::new(AppUi::default()));
    let intents = Arc::new(CriticalIntents::default());
    let (sender, receiver) = mpsc::channel(16);
    let commands = CommandQueue {
        sender,
        intents: intents.clone(),
    };
    let timer = bind(
        &ui,
        state.clone(),
        app.clone(),
        commands.clone(),
        intents.clone(),
        data.clone(),
    );
    install(&ui, &app, &data, &commands, &intents);
    (ui, state, app, data, receiver, intents, timer)
}

fn load(
    ui: &NativeApp,
    state: &RefCell<MemberUi>,
    app: &RefCell<AppUi>,
    data: &RefCell<Bootstrap>,
    receiver: &mut mpsc::Receiver<QueuedCommand>,
    intents: &CriticalIntents,
) {
    ui.invoke_open_channel_members("channel".into());
    let queued = receiver.try_recv().unwrap();
    let Command::MemberAction(request) = queued.command else {
        panic!("member callback did not queue original operation")
    };
    assert_eq!(queued.ticket, request.ticket);
    state.borrow_mut().handle(
        ui,
        app,
        &data.borrow(),
        Event {
            request,
            result: Ok(Response::Members(vec![member()])),
        },
        intents,
    );
}

#[test]
fn member_snapshot_survives_first_original_app_context_callback_before_periodic_refresh() {
    std::thread::spawn(|| {
        let mut app_timer = None;
        let (ui, state, app, data, mut receiver, intents, _timer) =
            fixture_with_adapter(|ui, app, data, commands, intents| {
                app_timer = Some(super::super::app_ui::bind(
                    ui,
                    app.clone(),
                    commands,
                    intents.clone(),
                    data.clone(),
                    Rc::new(RefCell::new(super::super::avatar_ui::AvatarUi::new())),
                ));
            });
        load(&ui, &state, &app, &data, &mut receiver, &intents);
        ui.invoke_open_member_profile("member".into());
        let Command::LookupProfile(query) = receiver.try_recv().unwrap().command else {
            panic!("missing original member lookup")
        };
        ui.invoke_app_context_changed();
        assert_eq!(app.borrow().member_profile, Some(member()));
        let mut fresh = member();
        fresh.display_name = "Fresh Member".into();
        state
            .borrow_mut()
            .profile_lookup(&ui, &app, &query, Ok(Some(fresh.clone())), &intents);
        ui.invoke_app_context_changed();
        assert_eq!(app.borrow().member_profile, Some(fresh));
        ui.set_user_id("replacement".into());
        ui.invoke_app_context_changed();
        assert!(app.borrow().member_profile.is_none());
        drop(app_timer);
    })
    .join()
    .unwrap();
}

#[test]
fn original_avatar_friend_and_self_profiles_do_not_reapply_over_batched_member_navigation() {
    std::thread::spawn(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let avatars = Rc::new(RefCell::new(super::super::avatar_ui::AvatarUi::new()));
        let setup = Rc::new(RefCell::new(None));
        let stop = tokio_util::sync::CancellationToken::new();
        let mut avatar_timer = None;
        let (ui, state, app, data, mut receiver, intents, _timer) =
            fixture_with_adapter(|ui, _, data, commands, intents| {
                avatar_timer = Some(super::super::avatar_ui::bind(
                    ui,
                    avatars.clone(),
                    data.clone(),
                    setup.clone(),
                    super::super::avatar_ui::Bindings {
                        commands: commands.clone(),
                        runtime: runtime.handle().clone(),
                        update: Arc::new(|_| panic!("adapter fixture must not start avatar IO")),
                        intents: intents.clone(),
                        stop: stop.clone(),
                    },
                ));
            });
        let prior = User {
            id: "prior-friend".into(),
            username: "Prior".into(),
            display_name: "Prior Friend".into(),
            ..Default::default()
        };
        data.borrow_mut().friends.push(prior);
        data.borrow_mut().user.username = "Owner".into();
        data.borrow_mut().user.display_name = "Owner".into();
        for id in ["prior-friend", "owner"] {
            ui.set_profile_source(if id == "owner" { "none" } else { "friends" }.into());
            ui.invoke_open_profile(id.into());
            if id != "owner" {
                receiver.try_recv().unwrap();
            }
            assert_eq!(ui.get_profile_id(), id);
            load(&ui, &state, &app, &data, &mut receiver, &intents);
            ui.invoke_open_member_profile("member".into());
            receiver.try_recv().unwrap();
            ui.invoke_avatar_context_changed();
            avatars
                .borrow_mut()
                .refresh(&ui, &data.borrow(), None, &intents);
            assert_eq!(ui.get_profile_id(), "member");
            assert_eq!(ui.get_profile_name(), "Member");
            assert!(!ui.get_profile_friend());
            assert_eq!(ui.get_profile_source(), "channelMembers");
            assert!(data.borrow().channel_users.is_empty());
            ui.invoke_close_member_profile();
            ui.invoke_avatar_context_changed();
        }
        stop.cancel();
        let (jobs, failed) = avatars.borrow_mut().take_jobs(&ui);
        assert!(!failed && jobs.is_empty());
        drop(avatar_timer);
    })
    .join()
    .unwrap();
}

#[test]
fn member_origin_friend_callbacks_use_actual_original_worker_and_reject_late_closed_results() {
    for mode in [
        "true", "false", "null", "leave", "logout", "view", "window", "friend",
    ] {
        std::thread::spawn(move || {
            let (ui, state, app, data, mut receiver, intents, _timer) = fixture();
            ui.set_user_id("fixture-owner".into());
            data.borrow_mut().user.id = "fixture-owner".into();
            ui.invoke_open_channel_members("channel".into());
            let Command::MemberAction(request) = receiver.try_recv().unwrap().command else {
                panic!("member-origin list request missing")
            };
            let peer = User {
                id: "fixture-peer".into(),
                ..member()
            };
            state.borrow_mut().handle(
                &ui,
                &app,
                &data.borrow(),
                Event {
                    request,
                    result: Ok(Response::Members(vec![peer])),
                },
                &intents,
            );
            ui.invoke_open_member_profile("fixture-peer".into());
            let Command::LookupProfile(lookup) = receiver.try_recv().unwrap().command else {
                panic!("member-origin lookup missing")
            };
            ui.invoke_member_profile_action("friend".into());
            let queued = receiver.try_recv().unwrap();
            let Command::ProfileAddFriend(query) = queued.command else {
                panic!("member-origin friend request missing")
            };
            assert_eq!(queued.ticket, query.ticket);
            assert_eq!(ui.get_profile_request_status(), 1);
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            let result = runtime.block_on(
                super::super::main_session_tests::member_origin_friend_original_worker(
                    query.clone(),
                    intents.clone(),
                    mode,
                    || {
                        if mode == "view" {
                            ui.invoke_close_member_profile();
                        }
                    },
                ),
            );
            assert_eq!(
                result,
                match mode {
                    "true" | "leave" => Some(true),
                    "logout" | "view" | "window" => None,
                    _ => Some(false),
                }
            );
            if let Some(sent) = result {
                state
                    .borrow_mut()
                    .friend_result(&ui, &query, sent, &intents);
                assert_eq!(ui.get_profile_request_status(), if sent { 2 } else { 0 });
            } else if mode != "window" {
                state
                    .borrow_mut()
                    .friend_result(&ui, &query, true, &intents);
                assert_ne!(ui.get_profile_request_status(), 2);
            } else {
                assert_eq!(ui.get_profile_request_status(), 1);
            }
            if mode == "view" {
                assert!(!lookup.alive() && !query.alive());
            }
            assert!(data.borrow().friends.is_empty() && data.borrow().channel_users.is_empty());
            assert!(receiver.try_recv().is_err());
        })
        .join()
        .unwrap();
    }
}

#[test]
fn selected_member_outgoing_request_is_displayed_for_current_owner() {
    std::thread::spawn(|| {
        let (ui, state, app, data, mut receiver, intents, _timer) = fixture();
        ui.invoke_open_channel_members("channel".into());
        let Command::MemberAction(request) = receiver.try_recv().unwrap().command else {
            panic!("selected outgoing list missing")
        };
        let user = User {
            friend_requests_received: vec!["owner".into()],
            ..member()
        };
        state.borrow_mut().handle(
            &ui,
            &app,
            &data.borrow(),
            Event {
                request,
                result: Ok(Response::Members(vec![user])),
            },
            &intents,
        );
        ui.invoke_open_member_profile("member".into());
        receiver.try_recv().unwrap();
        assert_eq!(ui.get_profile_request_status(), 2);
        ui.invoke_member_profile_action("friend".into());
        assert!(receiver.try_recv().is_err());
    })
    .join()
    .unwrap();
}

#[test]
fn fresh_and_cached_member_outgoing_status_preserves_original_pending_and_replacement_owner() {
    std::thread::spawn(|| {
        let (ui, state, app, data, mut receiver, intents, _timer) = fixture();
        load(&ui, &state, &app, &data, &mut receiver, &intents);
        ui.invoke_open_member_profile("member".into());
        let Command::LookupProfile(query) = receiver.try_recv().unwrap().command else {
            panic!("fresh outgoing lookup missing")
        };
        let outgoing = User {
            friend_requests_received: vec!["owner".into()],
            ..member()
        };
        state
            .borrow_mut()
            .profile_lookup(&ui, &app, &query, Ok(Some(outgoing.clone())), &intents);
        assert_eq!(ui.get_profile_request_status(), 2);
        ui.invoke_close_member_profile();
        ui.invoke_open_member_profile("member".into());
        assert!(receiver.try_recv().is_err());
        assert_eq!(ui.get_profile_request_status(), 2);
        ui.set_user_id("replacement".into());
        data.borrow_mut().user.id = "replacement".into();
        load(&ui, &state, &app, &data, &mut receiver, &intents);
        ui.invoke_open_member_profile("member".into());
        let Command::LookupProfile(current) = receiver.try_recv().unwrap().command else {
            panic!("replacement outgoing lookup missing")
        };
        assert_eq!(ui.get_profile_request_status(), 0);
        state
            .borrow_mut()
            .profile_lookup(&ui, &app, &query, Ok(Some(outgoing.clone())), &intents);
        assert_eq!(ui.get_profile_request_status(), 0);
        ui.invoke_member_profile_action("friend".into());
        let Command::ProfileAddFriend(pending) = receiver.try_recv().unwrap().command else {
            panic!("replacement original pending request missing")
        };
        let outgoing = User {
            friend_requests_received: vec!["replacement".into()],
            ..outgoing
        };
        state
            .borrow_mut()
            .profile_lookup(&ui, &app, &current, Ok(Some(outgoing)), &intents);
        assert_eq!(ui.get_profile_request_status(), 1);
        assert!(state
            .borrow()
            .friend_request
            .as_ref()
            .unwrap()
            .same(&pending));
        ui.invoke_close_member_profile();
        assert!(!pending.alive());
        ui.invoke_open_member_profile("member".into());
        assert_eq!(ui.get_profile_request_status(), 2);
        state
            .borrow_mut()
            .friend_result(&ui, &pending, false, &intents);
        assert_eq!(ui.get_profile_request_status(), 2);
    })
    .join()
    .unwrap();
}

#[test]
fn actual_member_callbacks_use_registered_source_fresh_profile_and_return_without_roster_injection()
{
    std::thread::spawn(|| {
        let (ui, state, app, data, mut receiver, intents, _timer) = fixture();
        load(&ui, &state, &app, &data, &mut receiver, &intents);
        assert_eq!(ui.get_modal(), 11);
        assert_eq!(ui.get_channel_members().row_count(), 1);
        assert!(!ui.get_members_loading());
        assert!(data.borrow().channel_users.is_empty() && data.borrow().friends.is_empty());
        ui.invoke_open_member_profile("member".into());
        assert_eq!(ui.get_modal(), 9);
        assert_eq!(ui.get_profile_source(), "channelMembers");
        assert_eq!(ui.get_profile_id(), "member");
        let queued = receiver.try_recv().unwrap();
        let Command::LookupProfile(query) = queued.command else {
            panic!("member profile did not request fresh original lookup")
        };
        assert_eq!(queued.ticket, query.ticket);
        let mut fresh = member();
        fresh.display_name = "Fresh Member".into();
        fresh.is_online = true;
        fresh.last_online = None;
        state
            .borrow_mut()
            .profile_lookup(&ui, &app, &query, Ok(Some(fresh.clone())), &intents);
        assert_eq!(ui.get_profile_name(), "Fresh Member");
        assert!(ui.get_profile_online());
        assert_eq!(app.borrow().member_profile, Some(fresh));
        assert_eq!(ui.get_friends().row_count(), 0);
        ui.invoke_close_member_profile();
        assert_eq!(ui.get_modal(), 11);
        state
            .borrow_mut()
            .profile_lookup(&ui, &app, &query, Ok(Some(member())), &intents);
        assert_eq!(ui.get_profile_name(), "Fresh Member");
        assert!(data.borrow().channel_users.is_empty());
    })
    .join()
    .unwrap();
}

#[test]
fn actual_member_actions_guard_offline_invite_thirty_seconds_confirmation_and_friend_only_rollback()
{
    std::thread::spawn(|| {
        let (ui, state, app, data, mut receiver, intents, _timer) = fixture();
        data.borrow_mut().friends.push(member());
        ui.set_friends(super::super::friend_rows(&data.borrow().friends));
        load(&ui, &state, &app, &data, &mut receiver, &intents);
        ui.invoke_open_member_profile("member".into());
        let Command::LookupProfile(query) = receiver.try_recv().unwrap().command else {
            panic!("missing profile lookup")
        };
        ui.set_active_channel("channel".into());
        ui.set_voice_active(true);
        ui.invoke_member_profile_action("invite".into());
        assert!(receiver.try_recv().is_err());
        assert!(!ui.get_app_feedback().is_empty());
        let mut fresh = member();
        fresh.is_online = true;
        state
            .borrow_mut()
            .profile_lookup(&ui, &app, &query, Ok(Some(fresh)), &intents);
        ui.invoke_member_profile_action("invite".into());
        let Command::MemberAction(invite) = receiver.try_recv().unwrap().command else {
            panic!("missing invite")
        };
        assert_eq!(invite.scope(), IntentScope::Voice);
        assert!(ui.get_member_invited());
        state.borrow_mut().handle(
            &ui,
            &app,
            &data.borrow(),
            Event {
                request: invite,
                result: Ok(Response::Confirmed),
            },
            &intents,
        );
        ui.invoke_member_profile_action("invite".into());
        assert!(receiver.try_recv().is_err());
        let deadline = *state.borrow().sent.values().next().unwrap();
        state
            .borrow_mut()
            .context(&ui, &data.borrow(), &intents, deadline);
        assert!(!ui.get_member_invited());
        ui.invoke_member_profile_action("kick".into());
        assert_eq!(ui.get_modal(), 12);
        assert!(receiver.try_recv().is_err());
        ui.invoke_confirm_member_kick(false);
        assert_eq!(ui.get_modal(), 11);
        ui.invoke_open_member_profile("member".into());
        ui.invoke_member_profile_action("kick".into());
        ui.invoke_confirm_member_kick(true);
        let Command::MemberAction(kick) = receiver.try_recv().unwrap().command else {
            panic!("missing confirmed kick")
        };
        assert_eq!(ui.get_channel_members().row_count(), 0);
        state.borrow_mut().handle(
            &ui,
            &app,
            &data.borrow(),
            Event {
                request: kick,
                result: Err(AuthError::Network),
            },
            &intents,
        );
        assert_eq!(ui.get_channel_members().row_count(), 1);
        ui.invoke_open_member_profile("member".into());
        ui.invoke_member_profile_action("remove".into());
        let Command::MemberAction(remove) = receiver.try_recv().unwrap().command else {
            panic!("missing friend-only removal")
        };
        assert_eq!(ui.get_friends().row_count(), 0);
        assert_eq!(data.borrow().friends.len(), 1);
        assert!(data.borrow().channel_users.is_empty());
        state.borrow_mut().handle(
            &ui,
            &app,
            &data.borrow(),
            Event {
                request: remove.clone(),
                result: Err(AuthError::Network),
            },
            &intents,
        );
        assert_eq!(ui.get_friends().row_count(), 1);
        ui.invoke_open_member_profile("member".into());
        ui.invoke_member_profile_action("remove".into());
        let Command::MemberAction(confirmed) = receiver.try_recv().unwrap().command else {
            panic!("missing second friend-only removal")
        };
        state.borrow_mut().handle(
            &ui,
            &app,
            &data.borrow(),
            Event {
                request: confirmed,
                result: Ok(Response::Confirmed),
            },
            &intents,
        );
        ui.set_friends(super::super::friend_rows(&data.borrow().friends));
        state
            .borrow_mut()
            .context(&ui, &data.borrow(), &intents, Instant::now());
        assert_eq!(ui.get_friends().row_count(), 0);
        data.borrow_mut().friends.clear();
        state
            .borrow_mut()
            .context(&ui, &data.borrow(), &intents, Instant::now());
        assert!(state.borrow().removed.is_empty());
        data.borrow_mut().friends.push(member());
        ui.set_friends(super::super::friend_rows(&data.borrow().friends));
        state
            .borrow_mut()
            .context(&ui, &data.borrow(), &intents, Instant::now());
        assert_eq!(ui.get_friends().row_count(), 1);
        ui.set_user_id("replacement".into());
        data.borrow_mut().user.id = "replacement".into();
        ui.set_friends(Rc::new(slint::VecModel::from(Vec::<crate::FriendItem>::new())).into());
        state
            .borrow_mut()
            .context(&ui, &data.borrow(), &intents, Instant::now());
        state.borrow_mut().handle(
            &ui,
            &app,
            &data.borrow(),
            Event {
                request: remove,
                result: Err(AuthError::Network),
            },
            &intents,
        );
        assert_eq!(ui.get_friends().row_count(), 0);
    })
    .join()
    .unwrap();
}

#[test]
fn closed_member_profile_releases_original_lookup_and_friend_requests_before_same_target_reopens() {
    std::thread::spawn(|| {
        let (ui, state, app, data, mut receiver, intents, _timer) = fixture();
        load(&ui, &state, &app, &data, &mut receiver, &intents);
        ui.invoke_open_member_profile("member".into());
        let Command::LookupProfile(previous) = receiver.try_recv().unwrap().command else {
            panic!("missing first lookup")
        };
        ui.invoke_member_profile_action("friend".into());
        let Command::ProfileAddFriend(friend) = receiver.try_recv().unwrap().command else {
            panic!("missing original friend request")
        };
        assert_eq!(ui.get_profile_request_status(), 1);
        ui.invoke_close_member_profile();
        assert!(!previous.alive() && !friend.alive());
        ui.invoke_open_member_profile("member".into());
        let Command::LookupProfile(current) = receiver.try_recv().unwrap().command else {
            panic!("cancelled lookup retained a dead pending entry")
        };
        assert!(!previous.same(&current));
        state
            .borrow_mut()
            .friend_result(&ui, &friend, true, &intents);
        assert_eq!(ui.get_profile_request_status(), 0);
        state
            .borrow_mut()
            .profile_lookup(&ui, &app, &previous, Ok(Some(member())), &intents);
        assert!(state.borrow().lookup.as_ref().unwrap().same(&current));
        ui.invoke_member_profile_action("friend".into());
        let Command::ProfileAddFriend(replacement) = receiver.try_recv().unwrap().command else {
            panic!("stale friend request blocked replacement")
        };
        assert!(!friend.same(&replacement));
        ui.set_profile_id("other".into());
        state
            .borrow_mut()
            .friend_result(&ui, &replacement, true, &intents);
        assert_eq!(ui.get_profile_request_status(), 1);
        state
            .borrow_mut()
            .context(&ui, &data.borrow(), &intents, Instant::now());
        assert!(!current.alive() && !replacement.alive());
        assert!(state.borrow().lookup.is_none() && state.borrow().friend_request.is_none());
    })
    .join()
    .unwrap();
}

#[test]
fn member_kick_confirmation_rechecks_current_owner_and_invalidates_older_member_refresh() {
    std::thread::spawn(|| {
        let (ui, state, app, data, mut receiver, intents, _timer) = fixture();
        load(&ui, &state, &app, &data, &mut receiver, &intents);
        ui.invoke_open_channel_members("channel".into());
        let Command::MemberAction(refresh) = receiver.try_recv().unwrap().command else {
            panic!("missing original member refresh")
        };
        ui.invoke_open_member_profile("member".into());
        receiver.try_recv().unwrap();
        ui.invoke_member_profile_action("kick".into());
        data.borrow_mut().channels[0].owner_id = "replacement".into();
        ui.invoke_confirm_member_kick(true);
        assert!(receiver.try_recv().is_err());
        assert_eq!(ui.get_channel_members().row_count(), 1);
        data.borrow_mut().channels[0].owner_id = "owner".into();
        ui.invoke_open_member_profile("member".into());
        receiver.try_recv().unwrap();
        ui.invoke_member_profile_action("kick".into());
        ui.invoke_confirm_member_kick(true);
        let Command::MemberAction(kick) = receiver.try_recv().unwrap().command else {
            panic!("missing fresh owner-confirmed kick")
        };
        assert!(refresh.stop.is_cancelled());
        state.borrow_mut().handle(
            &ui,
            &app,
            &data.borrow(),
            Event {
                request: refresh,
                result: Ok(Response::Members(vec![member()])),
            },
            &intents,
        );
        assert_eq!(ui.get_channel_members().row_count(), 0);
        ui.invoke_open_channel_members("channel".into());
        let Command::MemberAction(fresh) = receiver.try_recv().unwrap().command else {
            panic!("missing new selected-view refresh")
        };
        let mut current = member();
        current.id = "current".into();
        state.borrow_mut().handle(
            &ui,
            &app,
            &data.borrow(),
            Event {
                request: fresh,
                result: Ok(Response::Members(vec![current])),
            },
            &intents,
        );
        state.borrow_mut().handle(
            &ui,
            &app,
            &data.borrow(),
            Event {
                request: kick,
                result: Err(AuthError::Network),
            },
            &intents,
        );
        assert_eq!(ui.get_channel_members().row_data(0).unwrap().id, "current");
    })
    .join()
    .unwrap();
}

#[test]
fn headless_members_are_not_overpainted_and_keyboard_kick_defaults_to_cancel_in_both_languages() {
    std::thread::spawn(|| {
        use slint::platform::WindowEvent;
        let (ui, state, app, data, mut receiver, intents, _timer) = fixture();
        ui.show().unwrap();
        let key = |text: slint::SharedString| {
            ui.window()
                .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
            ui.window()
                .dispatch_event(WindowEvent::KeyReleased { text });
        };
        for language in ["ru", "en"] {
            ui.set_language(language.into());
            load(&ui, &state, &app, &data, &mut receiver, &intents);
            slint::platform::update_timers_and_animations();
            let image = ui.window().take_snapshot().unwrap();
            let pixel = image.as_slice()[220 * image.width() as usize + 340];
            assert!(
                pixel.r >= 14 && pixel.g >= 14,
                "generic overlay overpainted members"
            );
            key(slint::platform::Key::Return.into());
            assert_eq!(ui.get_modal(), 9);
            receiver.try_recv().unwrap();
            ui.invoke_member_profile_action("kick".into());
            slint::platform::update_timers_and_animations();
            ui.window().take_snapshot().unwrap();
            key(slint::platform::Key::Return.into());
            assert_eq!(ui.get_modal(), 11);
            assert!(receiver.try_recv().is_err());
            assert_eq!(ui.get_channel_members().row_count(), 1);
            ui.invoke_open_member_profile("member".into());
            receiver.try_recv().unwrap();
            ui.invoke_member_profile_action("kick".into());
            slint::platform::update_timers_and_animations();
            ui.window().take_snapshot().unwrap();
            key(slint::platform::Key::Tab.into());
            key(slint::platform::Key::Return.into());
            assert_eq!(ui.get_modal(), 11);
            assert!(receiver.try_recv().is_err());
            ui.invoke_close_channel_members();
        }
        ui.hide().unwrap();
    })
    .join()
    .unwrap();
}
