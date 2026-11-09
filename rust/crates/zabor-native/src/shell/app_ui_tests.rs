use super::*;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{Model, PhysicalSize};
use std::cell::Cell;
use tokio::sync::mpsc;
use zabor_native::achievements::parse_result;
use zabor_native::critical_intents::CriticalAction;

struct Headless;

impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        window.set_size(PhysicalSize::new(1100, 760));
        Ok(window)
    }

    fn set_clipboard_text(&self, text: &str, _: slint::platform::Clipboard) {
        CLIPBOARD.with(|clipboard| *clipboard.borrow_mut() = text.into());
    }

    fn clipboard_text(&self, _: slint::platform::Clipboard) -> Option<String> {
        Some(CLIPBOARD.with(|clipboard| clipboard.borrow().clone()))
    }
}

thread_local! {
    static CLIPBOARD: RefCell<String> = const { RefCell::new(String::new()) };
    static PLATFORM_READY: Cell<bool> = const { Cell::new(false) };
}

type Fixture = (
    NativeApp,
    Rc<RefCell<AppUi>>,
    CommandQueue,
    mpsc::Receiver<super::super::QueuedCommand>,
    Arc<CriticalIntents>,
    Timer,
);

fn fixture() -> Fixture {
    PLATFORM_READY.with(|ready| {
        if !ready.replace(true) {
            slint::platform::set_platform(Box::new(Headless)).unwrap();
        }
    });
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
    ui.set_profile_id("owner".into());
    ui.set_profile_login("username".into());
    ui.set_modal(9);
    let state = Rc::new(RefCell::new(AppUi::default()));
    let intents = Arc::new(CriticalIntents::default());
    let (sender, receiver) = mpsc::channel(4);
    let commands = CommandQueue {
        sender,
        intents: intents.clone(),
    };
    let timer = bind(
        &ui,
        state.clone(),
        &commands,
        intents.clone(),
        Rc::new(RefCell::new(Bootstrap::default())),
    );
    (ui, state, commands, receiver, intents, timer)
}

fn result(query: AchievementQuery) -> Event {
    Event::Achievements(Box::new(AchievementResult {
        query,
        result: parse_result(&serde_json::json!(
            "{\"unlockedIds\":[\"first_friend\"],\"totalHiddenCount\":0}"
        )),
        catalog: None,
        storage_failed: false,
    }))
}

#[test]
fn actual_app_callbacks_load_own_and_peer_models_and_reject_closed_owner_and_logout_results() {
    let (ui, state, _queue, mut receiver, intents, _timer) = fixture();
    for language in ["ru", "en"] {
        ui.set_language(language.into());
        ui.set_modal(9);
        ui.set_profile_id("owner".into());
        ui.invoke_open_achievements();
        assert_eq!(ui.get_modal(), 10);
        assert!(ui.get_achievements_loading());
        let queued = receiver.try_recv().unwrap();
        let Command::Achievements(query) = queued.command else {
            panic!("wrong app command")
        };
        assert_eq!(queued.ticket, query.ticket());
        assert_eq!(query.target(), &AchievementTarget::Own);
        state
            .borrow_mut()
            .handle(&ui, result(query.clone()), &intents);
        assert!(!ui.get_achievements_loading());
        assert_eq!(ui.get_achievement_items().row_count(), 15);
        assert_eq!(
            ui.get_achievement_items().row_data(0).unwrap().id,
            "first_friend"
        );
        assert!(ui.get_achievements_summary().contains("15"));
        ui.invoke_close_app_modal();
        state.borrow_mut().context(&ui);
        assert!(state.borrow().view.query().is_none());
        ui.set_profile_id("peer".into());
        ui.set_modal(9);
        ui.invoke_open_achievements();
        let Command::Achievements(peer) = receiver.try_recv().unwrap().command else {
            panic!("wrong peer command")
        };
        assert_eq!(peer.target(), &AchievementTarget::Peer("peer".into()));
        state.borrow_mut().handle(&ui, result(query), &intents);
        assert!(state.borrow().view.data().is_none());
        state.borrow_mut().handle(&ui, result(peer), &intents);
        assert_eq!(ui.get_achievement_items().row_count(), 15);
    }
    ui.set_modal(9);
    ui.invoke_open_achievements();
    let Command::Achievements(query) = receiver.try_recv().unwrap().command else {
        panic!("wrong app command")
    };
    ui.set_user_id("replacement".into());
    state.borrow_mut().handle(&ui, result(query), &intents);
    assert!(state.borrow().view.data().is_none());
    state.borrow_mut().context(&ui);
    assert!(state.borrow().catalog.is_none());
    ui.set_modal(9);
    ui.set_profile_id("replacement".into());
    ui.invoke_open_achievements();
    let Command::Achievements(query) = receiver.try_recv().unwrap().command else {
        panic!("wrong app command")
    };
    intents.request(CriticalAction::Logout);
    state.borrow_mut().handle(&ui, result(query), &intents);
    assert!(state.borrow().view.data().is_none());
}

#[test]
fn actual_app_clipboard_is_optimistic_suppressed_for_two_seconds_and_selection_scoped() {
    let (ui, state, _queue, _receiver, intents, _timer) = fixture();
    ui.invoke_copy_profile_login();
    assert!(ui.get_profile_login_copied());
    assert_eq!(CLIPBOARD.with(|value| value.borrow().clone()), "username");
    let until = state.borrow().copied.as_ref().unwrap().2;
    ui.invoke_copy_profile_login();
    assert_eq!(state.borrow().copied.as_ref().unwrap().2, until);
    state
        .borrow_mut()
        .tick(&ui, &Bootstrap::default(), until - Duration::from_nanos(1));
    assert!(ui.get_profile_login_copied());
    state.borrow_mut().tick(&ui, &Bootstrap::default(), until);
    assert!(!ui.get_profile_login_copied());
    ui.invoke_copy_profile_login();
    ui.set_profile_id("other".into());
    state.borrow_mut().context(&ui);
    assert!(!ui.get_profile_login_copied());
    intents.request(CriticalAction::Logout);
    ui.invoke_copy_profile_login();
    assert!(state.borrow().copied.is_none());
    let now = Instant::now();
    state.borrow_mut().feedback(&ui, "profile.userOffline", now);
    state
        .borrow_mut()
        .tick(&ui, &Bootstrap::default(), now + Duration::from_secs(3));
    assert!(ui.get_app_feedback().is_empty());
}

#[test]
fn actual_app_storage_ack_is_view_owner_and_logout_guarded_and_catalog_waits_for_completion() {
    let (ui, state, _queue, mut receiver, intents, _timer) = fixture();
    for mode in [
        "accept", "closed", "replaced", "owner", "selected", "logout",
    ] {
        ui.set_user_id("owner".into());
        ui.set_profile_id("owner".into());
        ui.set_modal(9);
        ui.invoke_open_achievements();
        let Command::Achievements(query) = receiver.try_recv().unwrap().command else {
            panic!("wrong storage proposal command")
        };
        let data = parse_result(&serde_json::json!("{\"unlockedHiddenDefs\":[{\"id\":\"hidden-one\",\"icon\":\"x\",\"maxValue\":1,\"statKey\":\"hiddenStat\",\"titleRu\":\"r\",\"descriptionRu\":\"r\",\"titleEn\":\"e\",\"descriptionEn\":\"e\"}]}" )).unwrap();
        let catalog = HiddenCatalog::empty("owner").unwrap();
        let plan = catalog.prepare_response(&query, &data).unwrap().unwrap();
        match mode {
            "closed" => ui.invoke_close_app_modal(),
            "replaced" => {
                ui.invoke_close_app_modal();
                ui.set_modal(9);
                ui.invoke_open_achievements();
                receiver.try_recv().unwrap();
            }
            "owner" => ui.set_user_id("replacement".into()),
            "selected" => ui.set_profile_id("replacement".into()),
            "logout" => {
                intents.request(CriticalAction::Logout);
            }
            _ => {}
        }
        let (acknowledge, mut receipt) = tokio::sync::oneshot::channel();
        state.borrow_mut().handle(
            &ui,
            Event::AchievementProposal {
                result: Box::new(AchievementResult {
                    query: query.clone(),
                    result: Ok(data),
                    catalog: Some(catalog.clone()),
                    storage_failed: false,
                }),
                plan,
                acknowledge,
            },
            &intents,
        );
        let accepted = receipt.try_recv().unwrap();
        assert_eq!(accepted.is_some(), mode == "accept");
        if let Some(accepted) = accepted {
            assert_eq!(state.borrow().catalog, Some(catalog));
            state.borrow_mut().handle(
                &ui,
                Event::AchievementsStored {
                    query: query.clone(),
                    result: Ok(Some(accepted.clone())),
                },
                &intents,
            );
            assert_eq!(state.borrow().catalog, Some(accepted));
            ui.invoke_close_app_modal();
            state.borrow_mut().context(&ui);
            state.borrow_mut().handle(
                &ui,
                Event::AchievementsStored {
                    query,
                    result: Ok(Some(HiddenCatalog::empty("stale").unwrap())),
                },
                &intents,
            );
            assert_ne!(
                state.borrow().catalog,
                Some(HiddenCatalog::empty("stale").unwrap())
            );
        }
    }
}

#[test]
fn call_failure_is_owner_selection_scoped_for_three_seconds_and_fresh_member_presence_stays_redacted(
) {
    let (ui, state, _queue, _receiver, intents, _timer) = fixture();
    ui.set_profile_id("peer".into());
    ui.set_profile_source("channelMembers".into());
    ui.set_profile_online(false);
    let mut data = Bootstrap::default();
    data.friends.push(User {
        id: "peer".into(),
        last_online: Some("2024-03-10T09:59:59Z".into()),
        ..Default::default()
    });
    state.borrow_mut().tick(&ui, &data, Instant::now());
    state.borrow_mut().member_profile = Some(User {
        id: "peer".into(),
        last_online: None,
        ..Default::default()
    });
    state.borrow_mut().tick(&ui, &data, Instant::now());
    assert_eq!(
        ui.get_profile_presence().as_str(),
        Strings::new(ui.get_language().as_str())
            .unwrap()
            .text("presence.offline")
    );
    for (owner, peer) in [("replacement", "peer"), ("owner", "other")] {
        state.borrow_mut().handle(
            &ui,
            Event::CallFailed {
                owner: owner.into(),
                peer: peer.into(),
            },
            &intents,
        );
        assert!(ui.get_app_feedback().is_empty());
    }
    state.borrow_mut().handle(
        &ui,
        Event::CallFailed {
            owner: "owner".into(),
            peer: "peer".into(),
        },
        &intents,
    );
    let until = state.borrow().feedback_until.unwrap();
    state
        .borrow_mut()
        .tick(&ui, &data, until - Duration::from_nanos(1));
    assert!(!ui.get_app_feedback().is_empty());
    state.borrow_mut().tick(&ui, &data, until);
    assert!(ui.get_app_feedback().is_empty());
}
