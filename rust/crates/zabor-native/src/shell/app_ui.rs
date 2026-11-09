use super::{Command, CommandQueue, NativeApp};
use slint::{ComponentHandle, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use zabor_native::achievements::{
    AchievementData, AchievementError, AchievementQuery, AchievementTarget, AchievementsView,
    HiddenCatalog, HiddenPersistence, STANDARD,
};
use zabor_native::auth::{Bootstrap, User};
use zabor_native::critical_intents::{CriticalIntents, IntentScope};
use zabor_native::presence::{format_last_online, PresenceLanguage, PresenceText};
use zabor_native::strings::Strings;

pub(super) struct AchievementResult {
    pub query: AchievementQuery,
    pub result: Result<AchievementData, AchievementError>,
    pub catalog: Option<HiddenCatalog>,
    pub storage_failed: bool,
}

pub(super) enum Event {
    CallFailed {
        owner: String,
        peer: String,
    },
    Achievements(Box<AchievementResult>),
    AchievementProposal {
        result: Box<AchievementResult>,
        plan: HiddenPersistence,
        acknowledge: tokio::sync::oneshot::Sender<Option<HiddenCatalog>>,
    },
    AchievementsStored {
        query: AchievementQuery,
        result: Result<Option<HiddenCatalog>, zabor_native::profile::ProfileError>,
    },
}

#[derive(Default)]
pub(super) struct AppUi {
    view: AchievementsView,
    catalog: Option<HiddenCatalog>,
    owner: String,
    language: String,
    copied: Option<(String, String, Instant)>,
    feedback_until: Option<Instant>,
    pub(super) member_profile: Option<User>,
}

impl AppUi {
    pub(super) fn context(&mut self, ui: &NativeApp) {
        if !ui.get_authenticated() || self.owner != ui.get_user_id().as_str() {
            self.view.close();
            self.catalog = None;
            self.owner = ui.get_user_id().to_string();
            self.copied = None;
            self.feedback_until = None;
            self.member_profile = None;
            ui.set_app_feedback("".into());
        }
        if ui.get_modal() != 10
            || self
                .view
                .query()
                .is_some_and(|query| !self.current(ui, query))
        {
            self.view.close();
        }
        if self.copied.as_ref().is_some_and(|(owner, target, _)| {
            owner != ui.get_user_id().as_str()
                || target != ui.get_profile_id().as_str()
                || ui.get_modal() != 9
        }) {
            self.copied = None;
        }
        ui.set_profile_login_copied(self.copied.is_some());
    }

    pub(super) fn handle(&mut self, ui: &NativeApp, event: Event, intents: &CriticalIntents) {
        match event {
            Event::CallFailed { owner, peer } => {
                if ui.get_authenticated()
                    && ui.get_user_id().as_str() == owner
                    && (ui.get_modal() == 0
                        || (ui.get_modal() == 9 && ui.get_profile_id().as_str() == peer))
                {
                    self.feedback(ui, "validation.connectionError", Instant::now());
                }
            }
            Event::Achievements(result) => {
                self.accept_result(ui, *result, intents);
            }
            Event::AchievementProposal {
                result,
                plan,
                acknowledge,
            } => {
                let accepted = if self.accept_result(ui, *result, intents) {
                    plan.commit_if_current(intents, &self.view)
                } else {
                    None
                };
                let _ = acknowledge.send(accepted);
            }
            Event::AchievementsStored { query, result } => {
                if self.current(ui, &query)
                    && self.view.query() == Some(&query)
                    && intents
                        .commit(query.ticket(), IntentScope::Session, || ())
                        .is_some()
                {
                    match result {
                        Ok(Some(catalog)) => self.catalog = Some(catalog),
                        Err(_) => self.feedback(ui, "validation.sessionSaveError", Instant::now()),
                        Ok(None) => {}
                    }
                    self.render(ui);
                }
            }
        }
    }

    fn current(&self, ui: &NativeApp, query: &AchievementQuery) -> bool {
        ui.get_authenticated()
            && ui.get_modal() == 10
            && ui.get_user_id().as_str() == query.owner()
            && match query.target() {
                AchievementTarget::Own => ui.get_profile_id().as_str() == query.owner(),
                AchievementTarget::Peer(id) => ui.get_profile_id().as_str() == id.as_str(),
            }
            && query.alive()
    }

    fn accept_result(
        &mut self,
        ui: &NativeApp,
        result: AchievementResult,
        intents: &CriticalIntents,
    ) -> bool {
        if !self.current(ui, &result.query)
            || !self.view.accept(intents, &result.query, result.result)
        {
            return false;
        }
        if matches!(result.query.target(), AchievementTarget::Own) {
            self.catalog = result.catalog;
        }
        if result.storage_failed {
            self.feedback(ui, "validation.sessionSaveError", Instant::now());
        }
        self.render(ui);
        true
    }

    fn render(&mut self, ui: &NativeApp) {
        let Ok(strings) = Strings::new(ui.get_language().as_str()) else {
            return;
        };
        self.language = ui.get_language().to_string();
        let rows = self.view.rows(
            ui.get_language().as_str(),
            |key| strings.text(key),
            self.catalog.as_ref(),
        );
        ui.set_achievement_items(
            Rc::new(VecModel::from(
                rows.into_iter()
                    .map(|row| crate::AchievementItem {
                        id: row.id.into(),
                        title: row.title.into(),
                        description: row.description.into(),
                        icon: row.icon.into(),
                        hidden: row.hidden,
                        unknown: row.unknown,
                        unlocked: row.unlocked,
                        progress: row.progress as f32,
                        progress_text: row.progress_text.into(),
                    })
                    .collect::<Vec<_>>(),
            ))
            .into(),
        );
        ui.set_achievements_loading(self.view.data().is_none());
        ui.set_achievements_summary(
            self.view
                .data()
                .map(|data| {
                    strings.format(
                        "achievements.summary",
                        &[
                            ("unlocked", &data.unlocked_ids.len().to_string()),
                            (
                                "total",
                                &(STANDARD.len() + data.total_hidden_count).to_string(),
                            ),
                        ],
                    )
                })
                .unwrap_or_default()
                .into(),
        );
    }

    pub(super) fn feedback(&mut self, ui: &NativeApp, key: &str, now: Instant) {
        if let Ok(strings) = Strings::new(ui.get_language().as_str()) {
            ui.set_app_feedback(strings.text(key).into());
        }
        self.feedback_until = Some(now + Duration::from_secs(3));
    }

    fn tick(&mut self, ui: &NativeApp, data: &Bootstrap, now: Instant) {
        self.context(ui);
        if self
            .copied
            .as_ref()
            .is_some_and(|(_, _, until)| now >= *until)
        {
            self.copied = None;
            ui.set_profile_login_copied(false);
        }
        if self.feedback_until.is_some_and(|until| now >= until) {
            self.feedback_until = None;
            ui.set_app_feedback("".into());
        }
        if ui.get_modal() == 10 && self.language != ui.get_language().as_str() {
            self.render(ui);
        }
        if ui.get_modal() != 9 || ui.get_profile_editing() {
            ui.set_profile_presence("".into());
            return;
        }
        let id = ui.get_profile_id();
        let user = self
            .member_profile
            .as_ref()
            .filter(|user| ui.get_profile_source() == "channelMembers" && user.id == id.as_str())
            .or_else(|| {
                std::iter::once(&data.user)
                    .chain(data.friends.iter())
                    .chain(data.channel_users.values().flatten())
                    .chain(data.call_user.iter())
                    .find(|user| user.id == id.as_str())
            });
        let Ok(strings) = Strings::new(ui.get_language().as_str()) else {
            return;
        };
        let online = if id == ui.get_user_id() {
            ui.get_connected()
        } else {
            ui.get_profile_online()
        };
        let text = if online {
            strings.text("presence.online")
        } else {
            let last = super::presence_clock::last_online(
                user.and_then(|user| user.last_online.as_deref()),
            );
            let text = super::presence_clock::resolve(super::now_ms())
                .map(|now| format_last_online(last, now))
                .unwrap_or(PresenceText::Offline);
            text.render(
                if ui.get_language() == "en" {
                    PresenceLanguage::En
                } else {
                    PresenceLanguage::Ru
                },
                |key| strings.text(key),
            )
        };
        ui.set_profile_presence(text.into());
    }
}

pub(super) fn bind(
    ui: &NativeApp,
    state: Rc<RefCell<AppUi>>,
    commands: &CommandQueue,
    intents: Arc<CriticalIntents>,
    latest: Rc<RefCell<Bootstrap>>,
) -> Timer {
    let weak = ui.as_weak();
    let context = state.clone();
    ui.on_app_context_changed(move || {
        if let Some(ui) = weak.upgrade() {
            context.borrow_mut().context(&ui);
        }
    });
    let weak = ui.as_weak();
    let opening = state.clone();
    let queue = commands.clone();
    let original = intents.clone();
    ui.on_open_achievements(move || {
        let Some(ui) = weak.upgrade().filter(|ui| {
            ui.get_authenticated()
                && ui.get_connected()
                && ui.get_modal() == 9
                && !ui.get_profile_editing()
                && !ui.get_busy()
        }) else {
            return;
        };
        let target = if ui.get_profile_id() == ui.get_user_id() {
            AchievementTarget::Own
        } else {
            AchievementTarget::Peer(ui.get_profile_id().to_string())
        };
        opening.borrow_mut().context(&ui);
        ui.set_achievements_own(matches!(target, AchievementTarget::Own));
        ui.set_achievements_peer_name(ui.get_profile_name());
        ui.set_modal(10);
        let Ok(query) = opening.borrow_mut().view.begin(
            &original,
            original.ticket(),
            ui.get_user_id().as_str(),
            target,
        ) else {
            return;
        };
        if queue
            .sender
            .try_send(super::QueuedCommand {
                command: Command::Achievements(query.clone()),
                ticket: query.ticket(),
            })
            .is_err()
        {
            opening
                .borrow_mut()
                .view
                .accept(&original, &query, Err(AchievementError::Busy));
        }
        opening.borrow_mut().render(&ui);
        ui.set_achievements_focus_reset(ui.get_achievements_focus_reset().wrapping_add(1));
    });
    let weak = ui.as_weak();
    ui.on_close_app_modal(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.get_profile_source() == "channelMembers" {
                ui.invoke_close_member_profile();
            } else {
                ui.set_modal(0);
            }
            ui.set_profile_restore_reset(ui.get_profile_restore_reset().wrapping_add(1));
        }
    });
    let weak = ui.as_weak();
    let copying = state.clone();
    let original = intents;
    ui.on_copy_profile_login(move || {
        let Some(ui) = weak.upgrade().filter(|ui| {
            ui.get_authenticated() && ui.get_modal() == 9 && !ui.get_profile_login().is_empty()
        }) else {
            return;
        };
        let mut state = copying.borrow_mut();
        state.context(&ui);
        if state.copied.is_some() || !original.is_current(original.ticket(), IntentScope::Session) {
            return;
        }
        ui.invoke_copy_username();
        state.copied = Some((
            ui.get_user_id().to_string(),
            ui.get_profile_id().to_string(),
            Instant::now() + Duration::from_secs(2),
        ));
        ui.set_profile_login_copied(true);
    });
    let weak = ui.as_weak();
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(250), move || {
        if let Some(ui) = weak.upgrade() {
            state
                .borrow_mut()
                .tick(&ui, &latest.borrow(), Instant::now());
        }
    });
    timer
}

#[cfg(test)]
#[path = "app_ui_tests.rs"]
mod tests;
