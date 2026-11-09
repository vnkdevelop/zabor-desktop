use super::app_ui::AppUi;
use super::member_actions::{Operation, Request, Response};
use super::profile_lookup::{Cache, Lookup, Request as ProfileRequest};
use super::{Command, CommandQueue, NativeApp, QueuedCommand};
use slint::{ComponentHandle, Model, Timer, TimerMode};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use zabor_native::auth::{AuthError, Bootstrap, Channel, User};
use zabor_native::critical_intents::{CriticalIntents, IntentScope, IntentTicket};

pub(super) struct Event {
    pub(super) request: Request,
    pub(super) result: Result<Response, AuthError>,
}

struct Pending {
    request: Request,
    previous: Vec<User>,
    revision: u64,
}

pub(super) struct MemberUi {
    owner: String,
    ticket: Option<IntentTicket>,
    channel: Option<Channel>,
    query: Option<Request>,
    members: Vec<User>,
    rows_dirty: bool,
    cache: BTreeMap<String, Vec<User>>,
    revision: u64,
    profile: Option<User>,
    profiles: Cache,
    lookup: Option<ProfileRequest>,
    friend_request: Option<ProfileRequest>,
    action: Option<Pending>,
    kick: Option<String>,
    sent: BTreeMap<(String, String), Instant>,
    removed: BTreeSet<String>,
}

impl Default for MemberUi {
    fn default() -> Self {
        Self {
            owner: String::new(),
            ticket: None,
            channel: None,
            query: None,
            members: Vec::new(),
            rows_dirty: true,
            cache: BTreeMap::new(),
            revision: 0,
            profile: None,
            profiles: Cache::new(),
            lookup: None,
            friend_request: None,
            action: None,
            kick: None,
            sent: BTreeMap::new(),
            removed: BTreeSet::new(),
        }
    }
}

fn queue(commands: &CommandQueue, request: Request) -> bool {
    commands
        .sender
        .try_send(QueuedCommand {
            ticket: request.ticket,
            command: Command::MemberAction(request),
        })
        .is_ok()
}

impl MemberUi {
    fn cancel_profile_requests(&mut self, intents: &CriticalIntents, now: Instant) {
        if let Some(request) = self.lookup.take() {
            request.stop.cancel();
            self.profiles
                .finish(&request, Err(AuthError::Network), intents, now);
        }
        if let Some(request) = self.friend_request.take() {
            request.stop.cancel();
        }
        if let Some(pending) = self
            .action
            .take_if(|pending| matches!(pending.request.operation, Operation::Invite { .. }))
        {
            pending.request.stop.cancel();
            if let Operation::Invite { channel, target } = pending.request.operation {
                self.sent.remove(&(channel.id, target.id));
            }
        }
    }

    fn profile_context(&self, ui: &NativeApp) -> bool {
        ui.get_authenticated()
            && ui.get_modal() == 9
            && ui.get_profile_source() == "channelMembers"
            && self.owner == ui.get_user_id().as_str()
            && self
                .profile
                .as_ref()
                .is_some_and(|user| user.id == ui.get_profile_id().as_str())
    }

    fn profile_request_status(&self, user: &User) -> i32 {
        if self.friend_request.is_some() {
            1
        } else if user
            .friend_requests_received
            .iter()
            .any(|owner| owner == &self.owner)
        {
            2
        } else {
            0
        }
    }

    fn visible(&self, ui: &NativeApp) -> bool {
        ui.get_modal() == 11
            || ui.get_modal() == 12
            || self.profile_context(ui)
            || (ui.get_modal() == 10 && ui.get_profile_source() == "channelMembers")
    }

    fn render(&mut self, ui: &NativeApp) {
        if self.rows_dirty {
            ui.set_channel_members(super::friend_rows(&self.members));
            ui.set_members_selection(
                ui.get_members_selection()
                    .clamp(0, self.members.len().saturating_sub(1) as i32),
            );
            self.rows_dirty = false;
        }
        ui.set_members_loading(self.query.is_some());
        ui.set_members_channel_name(
            self.channel
                .as_ref()
                .map(|channel| channel.name.clone())
                .unwrap_or_default()
                .into(),
        );
        ui.set_member_can_invite(self.channel.as_ref().is_some_and(|channel| {
            ui.get_active_channel().as_str() == channel.id && ui.get_voice_active()
        }));
        ui.set_member_can_kick(
            self.channel
                .as_ref()
                .is_some_and(|channel| channel.owner_id == self.owner)
                && self
                    .profile
                    .as_ref()
                    .is_some_and(|user| user.id != self.owner),
        );
        ui.set_member_action_pending(self.action.is_some());
        ui.set_member_invited(
            self.channel
                .as_ref()
                .zip(self.profile.as_ref())
                .is_some_and(|(channel, user)| {
                    self.sent
                        .contains_key(&(channel.id.clone(), user.id.clone()))
                }),
        );
    }

    pub(super) fn context(
        &mut self,
        ui: &NativeApp,
        data: &Bootstrap,
        intents: &CriticalIntents,
        now: Instant,
    ) {
        if !ui.get_authenticated()
            || self.owner != ui.get_user_id().as_str()
            || self
                .ticket
                .is_some_and(|ticket| !intents.is_current(ticket, IntentScope::Session))
        {
            if let Some(query) = self.query.take() {
                query.stop.cancel();
            }
            if let Some(action) = self.action.take() {
                action.request.stop.cancel();
            }
            self.cancel_profile_requests(intents, now);
            self.profiles.clear();
            self.channel = None;
            self.profile = None;
            self.members.clear();
            self.rows_dirty = true;
            self.cache.clear();
            self.kick = None;
            self.sent.clear();
            self.removed.clear();
            self.ticket = None;
            self.owner = ui.get_user_id().to_string();
        }
        if ui.get_authenticated() && self.ticket.is_none() {
            let ticket = intents.ticket();
            if intents.is_current(ticket, IntentScope::Session) {
                self.ticket = Some(ticket);
            }
        }
        self.profiles.sync(&self.owner, intents);
        self.sent.retain(|_, until| now < *until);
        if !self.visible(ui) {
            if let Some(query) = self.query.take() {
                query.stop.cancel();
            }
        }
        if !self.profile_context(ui) {
            self.cancel_profile_requests(intents, now);
        }
        if let Some(channel) = self.channel.as_mut() {
            if let Some(current) = data
                .channels
                .iter()
                .find(|current| current.id == channel.id)
            {
                *channel = current.clone();
            } else {
                if let Some(query) = self.query.take() {
                    query.stop.cancel();
                }
                if let Some(action) = self.action.take() {
                    action.request.stop.cancel();
                }
                self.channel = None;
                self.members.clear();
                self.rows_dirty = true;
                self.kick = None;
            }
        }
        if let Some(action) = &self.action {
            if !intents.is_current(action.request.ticket, action.request.scope()) {
                self.action = None;
            }
        }
        self.removed
            .retain(|target| data.friends.iter().any(|friend| &friend.id == target));
        let pending = self
            .action
            .as_ref()
            .and_then(|action| match &action.request.operation {
                Operation::RemoveFriend(target) => Some(target.as_str()),
                _ => None,
            });
        if pending.is_some() || !self.removed.is_empty() {
            let model = ui.get_friends();
            let rows = model
                .iter()
                .filter(|row| {
                    Some(row.id.as_str()) != pending && !self.removed.contains(row.id.as_str())
                })
                .collect::<Vec<_>>();
            if rows.len() != model.row_count() {
                ui.set_friends(Rc::new(slint::VecModel::from(rows)).into());
            }
        }
        if self.profile_context(ui) {
            ui.set_profile_friend(
                data.friends
                    .iter()
                    .any(|friend| friend.id == ui.get_profile_id().as_str())
                    && !self.removed.contains(ui.get_profile_id().as_str()),
            );
        }
        self.render(ui);
    }

    fn open(
        &mut self,
        ui: &NativeApp,
        data: &Bootstrap,
        commands: &CommandQueue,
        intents: &CriticalIntents,
        id: &str,
    ) {
        self.context(ui, data, intents, Instant::now());
        let Some(channel) = data
            .channels
            .iter()
            .find(|channel| channel.id == id)
            .cloned()
        else {
            return;
        };
        if !ui.get_authenticated() || !ui.get_connected() || ui.get_busy() {
            return;
        }
        let Some(request) = Request::new(&self.owner, Operation::Members(channel.clone()), intents)
        else {
            return;
        };
        if let Some(previous) = self.query.take() {
            previous.stop.cancel();
        }
        self.cancel_profile_requests(intents, Instant::now());
        self.members = self.cache.get(id).cloned().unwrap_or_default();
        self.rows_dirty = true;
        self.revision = self.revision.wrapping_add(1);
        self.channel = Some(channel);
        self.profile = None;
        self.query = Some(request.clone());
        ui.set_modal(11);
        ui.invoke_avatar_context_changed();
        ui.set_members_selection(0);
        ui.set_members_focus_reset(ui.get_members_focus_reset().wrapping_add(1));
        if !queue(commands, request) {
            self.query = None;
        }
        self.render(ui);
    }

    fn open_profile(
        &mut self,
        ui: &NativeApp,
        data: &Bootstrap,
        app: &RefCell<AppUi>,
        commands: &CommandQueue,
        intents: &CriticalIntents,
        id: &str,
    ) {
        if ui.get_modal() != 11 || self.owner != data.user.id {
            return;
        }
        let Some(mut user) = self.members.iter().find(|user| user.id == id).cloned() else {
            return;
        };
        if user.id == self.owner {
            user = data.user.clone();
            ui.invoke_open_profile(user.id.clone().into());
        }
        self.cancel_profile_requests(intents, Instant::now());
        self.profile = Some(user.clone());
        {
            let mut app = app.borrow_mut();
            app.context(ui);
            app.member_profile = Some(user.clone());
        }
        ui.set_profile_source("channelMembers".into());
        super::avatar_ui::apply_profile(ui, &user);
        ui.set_profile_friend(
            data.friends.iter().any(|friend| friend.id == user.id)
                && !self.removed.contains(&user.id),
        );
        ui.set_profile_editing(false);
        ui.set_profile_request_status(self.profile_request_status(&user));
        ui.set_modal(9);
        match self
            .profiles
            .prepare(&self.owner, &user, intents, Instant::now())
        {
            Some(Lookup::Cached(user)) => {
                self.profile = Some((*user).clone());
                app.borrow_mut().member_profile = Some((*user).clone());
                super::avatar_ui::apply_profile(ui, &user);
                ui.set_profile_request_status(self.profile_request_status(&user));
            }
            Some(Lookup::Pending(request)) => self.lookup = Some(request),
            Some(Lookup::Start(request)) => {
                if commands
                    .sender
                    .try_send(QueuedCommand {
                        ticket: request.ticket,
                        command: Command::LookupProfile(request.clone()),
                    })
                    .is_ok()
                {
                    self.lookup = Some(request);
                } else {
                    self.profiles.finish(
                        &request,
                        Err(AuthError::Network),
                        intents,
                        Instant::now(),
                    );
                }
            }
            None => {}
        }
        self.render(ui);
    }

    pub(super) fn profile_lookup(
        &mut self,
        ui: &NativeApp,
        app: &RefCell<AppUi>,
        request: &ProfileRequest,
        result: Result<Option<User>, AuthError>,
        intents: &CriticalIntents,
    ) {
        let user = self
            .profiles
            .finish(request, result, intents, Instant::now());
        if !self.profile_context(ui)
            || !self
                .lookup
                .as_ref()
                .is_some_and(|pending| pending.same(request))
            || !request.matches_profile(
                &self.owner,
                ui.get_profile_id().as_str(),
                ui.get_profile_login().as_str(),
            )
        {
            return;
        }
        self.lookup = None;
        if let Some(user) = user {
            self.profile = Some((*user).clone());
            app.borrow_mut().member_profile = Some((*user).clone());
            super::avatar_ui::apply_profile(ui, &user);
            ui.set_profile_request_status(self.profile_request_status(&user));
        }
    }

    pub(super) fn friend_result(
        &mut self,
        ui: &NativeApp,
        request: &ProfileRequest,
        sent: bool,
        intents: &CriticalIntents,
    ) {
        if self.profile_context(ui)
            && request.alive()
            && request.matches_profile(
                &self.owner,
                ui.get_profile_id().as_str(),
                ui.get_profile_login().as_str(),
            )
            && intents.is_current(request.ticket, IntentScope::Session)
            && self
                .friend_request
                .as_ref()
                .is_some_and(|pending| pending.same(request))
        {
            self.friend_request = None;
            ui.set_profile_request_status(if sent {
                2
            } else {
                self.profile
                    .as_ref()
                    .map_or(0, |user| self.profile_request_status(user))
            });
        }
    }

    pub(super) fn handle(
        &mut self,
        ui: &NativeApp,
        app: &RefCell<AppUi>,
        data: &Bootstrap,
        event: Event,
        intents: &CriticalIntents,
    ) {
        let request = event.request;
        if request.owner != self.owner
            || request.owner != ui.get_user_id().as_str()
            || request.stop.is_cancelled()
            || !intents.is_current(request.ticket, request.scope())
        {
            return;
        }
        if let Operation::Members(channel) = &request.operation {
            if !self.visible(ui)
                || !self
                    .query
                    .as_ref()
                    .is_some_and(|current| current.same(&request))
                || !self
                    .channel
                    .as_ref()
                    .is_some_and(|current| current.id == channel.id)
            {
                return;
            }
            self.query = None;
            if let Ok(Response::Members(members)) = event.result {
                self.members = members;
                self.rows_dirty = true;
                self.revision = self.revision.wrapping_add(1);
                if self.cache.len() >= 32 && !self.cache.contains_key(&channel.id) {
                    self.cache.clear();
                }
                self.cache.insert(channel.id.clone(), self.members.clone());
            } else {
                app.borrow_mut()
                    .feedback(ui, "validation.connectionError", Instant::now());
            }
        } else {
            let Some(pending) = self.action.take() else {
                return;
            };
            if !pending.request.same(&request) {
                self.action = Some(pending);
                return;
            }
            if !matches!(event.result, Ok(Response::Confirmed)) {
                match &request.operation {
                    Operation::Kick { channel, .. }
                        if self.revision == pending.revision
                            && self.channel.as_ref().is_some_and(|current| {
                                current.id == channel.id && current.owner_id == self.owner
                            }) =>
                    {
                        self.members = pending.previous;
                        self.rows_dirty = true;
                        self.cache.insert(channel.id.clone(), self.members.clone());
                        self.revision = self.revision.wrapping_add(1);
                    }
                    Operation::RemoveFriend(_) => ui.set_friends(super::friend_rows(&data.friends)),
                    Operation::Invite { channel, target } => {
                        self.sent.remove(&(channel.id.clone(), target.id.clone()));
                    }
                    _ => {}
                }
                app.borrow_mut()
                    .feedback(ui, "validation.connectionError", Instant::now());
            } else if let Operation::RemoveFriend(target) = &request.operation {
                self.removed.insert(target.clone());
            }
        }
        self.render(ui);
    }

    fn action(
        &mut self,
        ui: &NativeApp,
        app: &RefCell<AppUi>,
        data: &Bootstrap,
        commands: &CommandQueue,
        intents: &CriticalIntents,
        kind: &str,
    ) {
        self.context(ui, data, intents, Instant::now());
        if ui.get_modal() != 9
            || !ui.get_authenticated()
            || self.owner != data.user.id
            || self.action.is_some()
            || !ui.get_connected()
            || ui.get_busy()
        {
            return;
        }
        let member = self.profile_context(ui);
        if !member && kind != "remove" {
            return;
        }
        let selected = if member {
            self.profile.clone()
        } else {
            data.friends
                .iter()
                .find(|user| user.id == ui.get_profile_id().as_str())
                .cloned()
        };
        let Some(user) = selected.filter(|user| user.id != self.owner) else {
            return;
        };
        let operation = match kind {
            "invite" => {
                let Some(channel) = self.channel.clone().filter(|channel| {
                    ui.get_active_channel().as_str() == channel.id && ui.get_voice_active()
                }) else {
                    return;
                };
                if self
                    .sent
                    .contains_key(&(channel.id.clone(), user.id.clone()))
                {
                    return;
                }
                if !user.is_online {
                    app.borrow_mut()
                        .feedback(ui, "profile.userOffline", Instant::now());
                    return;
                }
                Operation::Invite {
                    channel,
                    target: Box::new(user),
                }
            }
            "kick" => {
                if self
                    .channel
                    .as_ref()
                    .is_some_and(|channel| channel.owner_id == self.owner)
                {
                    self.kick = Some(user.id.clone());
                    ui.set_member_kick_name(user.display_name.into());
                    ui.set_modal(12);
                    ui.set_member_kick_focus_reset(
                        ui.get_member_kick_focus_reset().wrapping_add(1),
                    );
                }
                return;
            }
            "remove"
                if data.friends.iter().any(|friend| friend.id == user.id)
                    && !self.removed.contains(&user.id) =>
            {
                Operation::RemoveFriend(user.id)
            }
            "friend"
                if !data.friends.iter().any(|friend| friend.id == user.id)
                    && !user
                        .friend_requests_received
                        .iter()
                        .any(|owner| owner == &self.owner)
                    && self.friend_request.is_none()
                    && ui.get_profile_request_status() == 0 =>
            {
                let Some(request) =
                    ProfileRequest::new(&self.owner, &user.id, &user.username, intents)
                else {
                    return;
                };
                if commands
                    .sender
                    .try_send(QueuedCommand {
                        ticket: request.ticket,
                        command: Command::ProfileAddFriend(request.clone()),
                    })
                    .is_ok()
                {
                    self.friend_request = Some(request);
                    ui.set_profile_request_status(1);
                }
                return;
            }
            _ => return,
        };
        self.start_action(ui, commands, intents, operation);
    }

    fn start_action(
        &mut self,
        ui: &NativeApp,
        commands: &CommandQueue,
        intents: &CriticalIntents,
        operation: Operation,
    ) {
        let Some(request) = Request::new(&self.owner, operation, intents) else {
            return;
        };
        if !queue(commands, request.clone()) {
            return;
        }
        let previous = self.members.clone();
        match &request.operation {
            Operation::Kick { channel, target } => {
                if let Some(query) = self.query.take() {
                    query.stop.cancel();
                }
                self.members.retain(|member| member.id != *target);
                self.rows_dirty = true;
                self.cache.insert(channel.id.clone(), self.members.clone());
                self.revision = self.revision.wrapping_add(1);
            }
            Operation::RemoveFriend(target) => {
                let rows = ui
                    .get_friends()
                    .iter()
                    .filter(|row| row.id.as_str() != target)
                    .collect::<Vec<_>>();
                ui.set_friends(Rc::new(slint::VecModel::from(rows)).into());
                ui.set_profile_friend(false);
                ui.set_modal(
                    if ui.get_profile_source() == "channelMembers" && self.channel.is_some() {
                        11
                    } else {
                        0
                    },
                );
            }
            Operation::Invite { channel, target } => {
                self.sent.insert(
                    (channel.id.clone(), target.id.clone()),
                    Instant::now() + Duration::from_secs(30),
                );
            }
            _ => {}
        }
        self.action = Some(Pending {
            request,
            previous,
            revision: self.revision,
        });
        self.render(ui);
    }
}

pub(super) fn bind(
    ui: &NativeApp,
    state: Rc<RefCell<MemberUi>>,
    app: Rc<RefCell<AppUi>>,
    commands: CommandQueue,
    intents: Arc<CriticalIntents>,
    latest: Rc<RefCell<Bootstrap>>,
) -> Timer {
    let weak = ui.as_weak();
    let opening = state.clone();
    let data = latest.clone();
    let queue = commands.clone();
    let controls = intents.clone();
    ui.on_open_channel_members(move |id| {
        if let Some(ui) = weak.upgrade() {
            opening
                .borrow_mut()
                .open(&ui, &data.borrow(), &queue, &controls, id.as_str());
        }
    });
    let weak = ui.as_weak();
    let closing = state.clone();
    let controls = intents.clone();
    ui.on_close_channel_members(move || {
        if let Some(ui) = weak.upgrade() {
            let mut state = closing.borrow_mut();
            if let Some(request) = state.query.take() {
                request.stop.cancel();
            }
            state.cancel_profile_requests(&controls, Instant::now());
            state.profile = None;
            ui.set_modal(0);
        }
    });
    let weak = ui.as_weak();
    let opening = state.clone();
    let data = latest.clone();
    let queue = commands.clone();
    let controls = intents.clone();
    let profiles = app.clone();
    ui.on_open_member_profile(move |id| {
        if let Some(ui) = weak.upgrade() {
            opening.borrow_mut().open_profile(
                &ui,
                &data.borrow(),
                &profiles,
                &queue,
                &controls,
                id.as_str(),
            );
        }
    });
    let weak = ui.as_weak();
    let returning = state.clone();
    let controls = intents.clone();
    ui.on_close_member_profile(move || {
        if let Some(ui) = weak.upgrade() {
            let mut state = returning.borrow_mut();
            state.cancel_profile_requests(&controls, Instant::now());
            ui.set_modal(if state.channel.is_some() { 11 } else { 0 });
            state.render(&ui);
            ui.set_profile_restore_reset(ui.get_profile_restore_reset().wrapping_add(1));
        }
    });
    let weak = ui.as_weak();
    let actions = state.clone();
    let data = latest.clone();
    let queue = commands.clone();
    let controls = intents.clone();
    ui.on_member_profile_action(move |kind| {
        if let Some(ui) = weak.upgrade() {
            actions.borrow_mut().action(
                &ui,
                &app,
                &data.borrow(),
                &queue,
                &controls,
                kind.as_str(),
            );
        }
    });
    let weak = ui.as_weak();
    let confirming = state.clone();
    let queue = commands;
    let controls = intents.clone();
    let data = latest.clone();
    ui.on_confirm_member_kick(move |confirm| {
        if let Some(ui) = weak.upgrade() {
            let mut state = confirming.borrow_mut();
            state.context(&ui, &data.borrow(), &controls, Instant::now());
            if ui.get_modal() != 12 || state.owner != ui.get_user_id().as_str() {
                return;
            }
            let target = state.kick.take();
            ui.set_modal(11);
            if confirm
                && state.action.is_none()
                && ui.get_authenticated()
                && ui.get_connected()
                && !ui.get_busy()
            {
                if let Some((channel, target)) = state
                    .channel
                    .clone()
                    .filter(|channel| channel.owner_id == state.owner)
                    .zip(
                        target.filter(|target| state.members.iter().any(|user| &user.id == target)),
                    )
                {
                    state.start_action(&ui, &queue, &controls, Operation::Kick { channel, target });
                }
            }
        }
    });
    let weak = ui.as_weak();
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(250), move || {
        if let Some(ui) = weak.upgrade() {
            state
                .borrow_mut()
                .context(&ui, &latest.borrow(), &intents, Instant::now());
        }
    });
    timer
}

#[cfg(test)]
#[path = "member_ui_tests.rs"]
mod tests;
