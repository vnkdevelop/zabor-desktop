use super::{color, show_error, Bootstrap, Command, CommandQueue, NativeApp, UiEvent};
use sha2::{Digest, Sha256};
use slint::{ComponentHandle, Model, Timer, TimerMode};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use zabor_native::auth::{AuthError, ProfileUpdate, User};
use zabor_native::avatar::{Avatar, AvatarError, Crop, Selection, Source};
use zabor_native::critical_intents::{CriticalIntents, IntentScope, IntentTicket};

type Key = [u8; 32];

#[derive(Clone, PartialEq, Eq)]
enum Context {
    Setup(String),
    Profile(String),
}

#[derive(Clone)]
pub(super) struct Query {
    identity: Arc<()>,
    pub(super) stop: CancellationToken,
    pub(super) ticket: IntentTicket,
    context: Context,
}

impl Query {
    fn new(intents: &CriticalIntents, context: Context) -> Self {
        let ticket = intents.ticket();
        Self {
            identity: Arc::new(()),
            stop: intents.session_token(ticket),
            ticket,
            context,
        }
    }

    pub(super) fn alive(&self) -> bool {
        !self.stop.is_cancelled()
    }

    fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity)
    }

    pub(super) fn profile(intents: &CriticalIntents, owner: String) -> Self {
        Self::new(intents, Context::Profile(owner))
    }
}

pub(super) enum Event {
    Loaded(Query, Option<Result<Arc<Source>, AvatarError>>),
    Cropped(Query, Result<Selection, AvatarError>),
    Decoded(Key, Result<Arc<Avatar>, AvatarError>),
}

impl Event {
    pub(super) fn alive(&self) -> bool {
        match self {
            Self::Loaded(query, _) | Self::Cropped(query, _) => query.alive(),
            Self::Decoded(..) => true,
        }
    }
}

struct Cached {
    avatar: Arc<Avatar>,
    born: Instant,
    index: Option<usize>,
    image: slint::Image,
}

pub(super) struct AvatarUi {
    query: Option<Query>,
    saving: Option<Query>,
    source: Option<Arc<Source>>,
    source_born: Instant,
    source_index: Option<usize>,
    profile: Option<User>,
    draft: Option<String>,
    slots: BTreeMap<String, Key>,
    wires: BTreeMap<Key, String>,
    cache: BTreeMap<Key, Cached>,
    failed: BTreeSet<Key>,
    decoding: Option<Key>,
    decode_stop: Option<CancellationToken>,
    cache_ticket: Option<IntentTicket>,
    jobs: Vec<JoinHandle<()>>,
    lane: Arc<Semaphore>,
    needs_refresh: bool,
    join_failed: bool,
    worker_panics: Arc<AtomicBool>,
}

impl AvatarUi {
    pub(super) fn new() -> Self {
        Self {
            query: None,
            saving: None,
            source: None,
            source_born: Instant::now(),
            source_index: None,
            profile: None,
            draft: None,
            slots: BTreeMap::new(),
            wires: BTreeMap::new(),
            cache: BTreeMap::new(),
            failed: BTreeSet::new(),
            decoding: None,
            decode_stop: None,
            cache_ticket: None,
            jobs: Vec::new(),
            lane: Arc::new(Semaphore::new(1)),
            needs_refresh: true,
            join_failed: false,
            worker_panics: Arc::new(AtomicBool::new(false)),
        }
    }

    fn context(ui: &NativeApp) -> Option<Context> {
        if !ui.get_authenticated() && ui.get_auth_step() == 2 && ui.get_modal() == 0 {
            Some(Context::Setup(ui.get_login().to_string()))
        } else if ui.get_authenticated()
            && ui.get_modal() == 9
            && ui.get_profile_editing()
            && ui.get_profile_id() == ui.get_user_id()
        {
            Some(Context::Profile(ui.get_user_id().to_string()))
        } else {
            None
        }
    }

    fn matches(&self, query: &Query, ui: &NativeApp, intents: &CriticalIntents) -> bool {
        query.alive()
            && intents.is_current(query.ticket, IntentScope::Session)
            && Self::context(ui).as_ref() == Some(&query.context)
            && self
                .query
                .as_ref()
                .is_some_and(|current| current.same(query))
    }

    pub(super) fn cancel(&mut self, ui: &NativeApp) {
        if let Some(query) = self.query.take() {
            query.stop.cancel();
        }
        self.source = None;
        self.source_index = None;
        ui.set_crop_active(false);
        ui.set_avatar_loading(false);
        ui.set_crop_image(slint::Image::default());
    }

    fn context_changed(&mut self, ui: &NativeApp) {
        let context = Self::context(ui);
        if self
            .query
            .as_ref()
            .is_some_and(|query| Some(&query.context) != context.as_ref())
        {
            self.cancel(ui);
        }
        if self
            .saving
            .as_ref()
            .is_some_and(|query| Some(&query.context) != context.as_ref())
        {
            self.saving.take().unwrap().stop.cancel();
            ui.set_profile_saving(false);
        }
        if ui.get_modal() != 9 || !ui.get_authenticated() {
            self.draft = None;
            self.profile = None;
        }
    }

    pub(super) fn handle(
        &mut self,
        event: Event,
        ui: &NativeApp,
        intents: &CriticalIntents,
        setup: &RefCell<Option<String>>,
    ) {
        match event {
            Event::Loaded(query, result) => {
                if !self.matches(&query, ui, intents) {
                    return;
                }
                ui.set_avatar_loading(false);
                match result {
                    Some(Ok(source)) => {
                        ui.set_crop_scale(1.0);
                        ui.set_crop_x(0.0);
                        ui.set_crop_y(0.0);
                        ui.set_crop_image(image(&source.frames[0].pixels));
                        self.source = Some(source);
                        self.source_born = Instant::now();
                        self.source_index = Some(0);
                        ui.set_crop_active(true);
                    }
                    Some(Err(error)) => {
                        self.cancel(ui);
                        show_error(ui, AuthError::Validation(error.key()));
                    }
                    None => self.cancel(ui),
                }
            }
            Event::Cropped(query, result) => {
                if !self.matches(&query, ui, intents) || !ui.get_crop_active() {
                    return;
                }
                match result {
                    Ok(selection) => {
                        let key = key(&selection.wire);
                        let wire = selection.wire;
                        let new_color = color(&selection.color);
                        let image = image(&selection.avatar.frames[0].pixels);
                        if intents
                            .commit(query.ticket, IntentScope::Session, || {
                                match &query.context {
                                    Context::Setup(_) => *setup.borrow_mut() = Some(wire.clone()),
                                    Context::Profile(_) => self.draft = Some(wire.clone()),
                                }
                            })
                            .is_none()
                        {
                            return;
                        }
                        match &query.context {
                            Context::Setup(_) => {
                                ui.set_setup_avatar(image);
                                ui.set_avatar_color(new_color);
                            }
                            Context::Profile(_) => {
                                ui.set_profile_avatar(image);
                                ui.set_profile_color(new_color);
                            }
                        }
                        let needed = selection.avatar.bytes();
                        while self
                            .cache
                            .values()
                            .map(|cached| cached.avatar.bytes())
                            .sum::<usize>()
                            + needed
                            > 64 * 1024 * 1024
                        {
                            let Some(oldest) = self
                                .cache
                                .iter()
                                .min_by_key(|(_, cached)| cached.born)
                                .map(|(key, _)| *key)
                            else {
                                break;
                            };
                            self.cache.remove(&oldest);
                        }
                        self.cache.insert(
                            key,
                            Cached {
                                avatar: Arc::new(selection.avatar),
                                born: Instant::now(),
                                index: Some(0),
                                image: slint::Image::default(),
                            },
                        );
                        self.cancel(ui);
                        self.needs_refresh = true;
                    }
                    Err(error) => {
                        ui.set_avatar_loading(false);
                        show_error(ui, AuthError::Validation(error.key()));
                    }
                }
            }
            Event::Decoded(key, result) => {
                if self.decoding != Some(key) {
                    return;
                }
                self.decoding = None;
                self.decode_stop = None;
                if !self.wires.contains_key(&key) {
                    return;
                }
                match result {
                    Ok(avatar) => {
                        let bytes: usize = self
                            .cache
                            .values()
                            .map(|cached| cached.avatar.bytes())
                            .sum();
                        if bytes + avatar.bytes() <= 64 * 1024 * 1024 {
                            self.cache.insert(
                                key,
                                Cached {
                                    avatar,
                                    born: Instant::now(),
                                    index: None,
                                    image: slint::Image::default(),
                                },
                            );
                        } else {
                            self.failed.insert(key);
                        }
                    }
                    Err(_) => {
                        self.failed.insert(key);
                    }
                }
            }
        }
    }

    pub(super) fn refresh(
        &mut self,
        ui: &NativeApp,
        data: &Bootstrap,
        setup: Option<&str>,
        intents: &CriticalIntents,
    ) {
        self.needs_refresh = false;
        self.context_changed(ui);
        if self
            .cache_ticket
            .is_some_and(|ticket| !intents.is_current(ticket, IntentScope::Session))
        {
            self.cache.clear();
            self.failed.clear();
            self.decoding = None;
        }
        self.cache_ticket = Some(intents.ticket());
        if let Some(profile) = self.profile.as_mut() {
            ui.set_profile_friend(data.friends.iter().any(|user| user.id == profile.id));
            if !ui.get_profile_editing() {
                if let Some(user) = find_user(data, &profile.id) {
                    *profile = user.clone();
                    apply_profile(ui, user);
                }
            }
        }
        let mut slots = BTreeMap::new();
        let mut wires = BTreeMap::new();
        let mut used_bytes = 0;
        let mut add = |slot: String, wire: Option<&str>| {
            let Some(wire) = wire.filter(|wire| wire.len() <= zabor_native::avatar::MAX_WIRE_BYTES)
            else {
                return;
            };
            if wires.len() >= 32 || used_bytes + wire.len() > 64 * 1024 * 1024 {
                return;
            }
            let key = key(wire);
            slots.insert(slot, key);
            if let std::collections::btree_map::Entry::Vacant(entry) = wires.entry(key) {
                used_bytes += wire.len();
                entry.insert(wire.into());
            }
        };
        add("setup".into(), setup);
        if ui.get_authenticated() {
            add("self".into(), data.user.avatar_base64.as_deref());
            for user in &data.friends {
                add(format!("friend:{}", user.id), user.avatar_base64.as_deref());
            }
            for user in &data.requests {
                add(
                    format!("request:{}", user.id),
                    user.avatar_base64.as_deref(),
                );
            }
            for row in ui.get_voice_users().iter() {
                if let Some(user) = find_user(data, row.id.as_str()) {
                    add(format!("voice:{}", user.id), user.avatar_base64.as_deref());
                }
            }
            if let Some(profile) = &self.profile {
                add(
                    "profile".into(),
                    self.draft.as_deref().or(profile.avatar_base64.as_deref()),
                );
            }
        }
        self.slots = slots;
        self.wires = wires;
        self.cache.retain(|key, _| self.wires.contains_key(key));
        self.failed.retain(|key| self.wires.contains_key(key));
    }

    fn tick(
        &mut self,
        ui: &NativeApp,
        runtime: &tokio::runtime::Handle,
        update: Arc<dyn Fn(UiEvent) + Send + Sync>,
        intents: &CriticalIntents,
        stop: &CancellationToken,
    ) {
        if stop.is_cancelled() {
            return;
        }
        self.context_changed(ui);
        if self
            .cache_ticket
            .is_some_and(|ticket| !intents.is_current(ticket, IntentScope::Session))
        {
            self.cache.clear();
            self.slots.clear();
            self.wires.clear();
            self.failed.clear();
            self.decoding = None;
        }
        let now = Instant::now();
        for cached in self.cache.values_mut() {
            let index = cached.avatar.frame_index(
                now.duration_since(cached.born)
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64,
                ui.get_reduced_motion(),
            );
            if cached.index != Some(index) || cached.image.size().width == 0 {
                cached.image = image(&cached.avatar.frames[index].pixels);
                cached.index = Some(index);
            }
        }
        let current = |slot: &str| {
            self.slots
                .get(slot)
                .and_then(|key| self.cache.get(key))
                .map(|cached| cached.image.clone())
        };
        if let Some(image) = current("self").filter(|image| *image != ui.get_user_avatar()) {
            ui.set_user_avatar(image);
        }
        if let Some(image) = current("setup").filter(|image| *image != ui.get_setup_avatar()) {
            ui.set_setup_avatar(image);
        }
        if let Some(image) = current("profile").filter(|image| *image != ui.get_profile_avatar()) {
            ui.set_profile_avatar(image);
        }
        for (prefix, model) in [
            ("friend", ui.get_friends()),
            ("voice", ui.get_voice_users()),
        ] {
            for (index, mut row) in model.iter().enumerate() {
                if let Some(image) =
                    current(&format!("{prefix}:{}", row.id)).filter(|image| *image != row.avatar)
                {
                    row.avatar = image;
                    model.set_row_data(index, row);
                }
            }
        }
        let model = ui.get_notifications();
        for (index, mut row) in model.iter().enumerate() {
            if row.kind == 0 {
                if let Some(image) =
                    current(&format!("request:{}", row.id)).filter(|image| *image != row.avatar)
                {
                    row.avatar = image;
                    model.set_row_data(index, row);
                }
            }
        }
        if let Some(source) = self
            .source
            .as_ref()
            .filter(|_| ui.get_crop_active() && self.query.as_ref().is_some_and(Query::alive))
        {
            let index = source.frame_index(
                now.duration_since(self.source_born)
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64,
                ui.get_reduced_motion(),
            );
            if self.source_index != Some(index) {
                ui.set_crop_image(image(&source.frames[index].pixels));
                self.source_index = Some(index);
            }
        }
        if self.decoding.is_none() && self.jobs.len() < 8 {
            if let Some((key, wire)) = self
                .wires
                .iter()
                .find(|(key, _)| !self.cache.contains_key(*key) && !self.failed.contains(*key))
            {
                let key = *key;
                let wire = wire.clone();
                let ticket = intents.ticket();
                let cancelled = intents.session_token(ticket);
                self.decode_stop = Some(cancelled.clone());
                let shutdown = stop.clone();
                let lane = self.lane.clone();
                let panics = self.worker_panics.clone();
                self.decoding = Some(key);
                self.jobs.push(runtime.spawn(async move {
                    let permit = tokio::select! { biased; _ = cancelled.cancelled() => return, _ = shutdown.cancelled() => return, permit = lane.acquire_owned() => permit };
                    let Ok(permit) = permit else { return; };
                    let work_stop = cancelled.clone();
                    let result = tokio::task::spawn_blocking(move || { let _permit = permit; Avatar::from_wire(&wire, &work_stop).map(Arc::new) }).await.unwrap_or_else(|_| { panics.store(true, Ordering::Release); Err(AvatarError::Invalid) });
                    if !cancelled.is_cancelled() && !shutdown.is_cancelled() { update(UiEvent::Avatar(Event::Decoded(key, result)).guarded(ticket, IntentScope::Session)); }
                }));
            }
        }
        self.reap();
    }

    fn reap(&mut self) {
        use futures_util::FutureExt;
        let mut active = Vec::new();
        for job in self.jobs.drain(..) {
            if job.is_finished() {
                self.join_failed |= !matches!(job.now_or_never(), Some(Ok(())));
            } else {
                active.push(job);
            }
        }
        self.jobs = active;
    }

    pub(super) fn take_jobs(&mut self, ui: &NativeApp) -> (Vec<JoinHandle<()>>, bool) {
        self.cancel(ui);
        if let Some(stop) = self.decode_stop.take() {
            stop.cancel();
        }
        if let Some(query) = self.saving.take() {
            query.stop.cancel();
        }
        (
            std::mem::take(&mut self.jobs),
            self.join_failed || self.worker_panics.load(Ordering::Acquire),
        )
    }

    pub(super) fn failed(&self) -> bool {
        self.join_failed || self.worker_panics.load(Ordering::Acquire)
    }

    pub(super) fn profile_saved(&mut self, ui: &NativeApp, user: &User, query: &Query) {
        if query.alive()
            && self
                .saving
                .as_ref()
                .is_some_and(|current| current.same(query))
            && AvatarUi::context(ui) == Some(query.context.clone())
        {
            self.profile = Some(user.clone());
            self.draft = None;
            self.saving = None;
            apply_profile(ui, user);
            ui.set_profile_saving(false);
            ui.set_profile_editing(false);
            self.needs_refresh = true;
        }
    }

    pub(super) fn profile_error(&mut self, ui: &NativeApp, error: AuthError, query: &Query) {
        if query.alive()
            && self
                .saving
                .as_ref()
                .is_some_and(|current| current.same(query))
            && AvatarUi::context(ui) == Some(query.context.clone())
        {
            self.saving = None;
            ui.set_profile_saving(false);
            show_error(ui, error);
        }
    }
}

pub(super) struct Bindings {
    pub(super) commands: CommandQueue,
    pub(super) runtime: tokio::runtime::Handle,
    pub(super) update: Arc<dyn Fn(UiEvent) + Send + Sync>,
    pub(super) intents: Arc<CriticalIntents>,
    pub(super) stop: CancellationToken,
}

pub(super) fn bind(
    ui: &NativeApp,
    state: Rc<RefCell<AvatarUi>>,
    latest: Rc<RefCell<Bootstrap>>,
    setup: Rc<RefCell<Option<String>>>,
    bindings: Bindings,
) -> Timer {
    let Bindings {
        commands,
        runtime,
        update,
        intents,
        stop,
    } = bindings;
    let weak = ui.as_weak();
    let changed = state.clone();
    ui.on_avatar_context_changed(move || {
        if let Some(ui) = weak.upgrade() {
            changed.borrow_mut().context_changed(&ui);
        }
    });
    let weak = ui.as_weak();
    let cancelled = state.clone();
    ui.on_cancel_crop(move || {
        if let Some(ui) = weak.upgrade() {
            cancelled.borrow_mut().cancel(&ui);
        }
    });
    let weak = ui.as_weak();
    let selected = state.clone();
    let handle = runtime.clone();
    let updates = update.clone();
    let original = intents.clone();
    let shutdown = stop.clone();
    ui.on_choose_avatar(move || {
        let Some(ui) = weak.upgrade().filter(|ui| !ui.get_busy() && !ui.get_avatar_loading() && !ui.get_profile_saving()) else { return; };
        let Some(context) = AvatarUi::context(&ui) else { return; };
        let mut state = selected.borrow_mut();
        state.reap(); if state.jobs.len() >= 8 { return; }
        state.cancel(&ui); let query = Query::new(&original, context); state.query = Some(query.clone()); ui.set_avatar_loading(true);
        let update = updates.clone(); let stop = shutdown.clone(); let lane = state.lane.clone(); let panics = state.worker_panics.clone();
        state.jobs.push(handle.spawn(async move {
            let file = rfd::AsyncFileDialog::new().add_filter("images", &["png", "jpg", "jpeg", "webp", "gif"]).pick_file().await;
            if !query.alive() || stop.is_cancelled() { return; }
            let Some(file) = file else { update(UiEvent::Avatar(Event::Loaded(query.clone(), None)).guarded(query.ticket, IntentScope::Session)); return; };
            let path = file.path().to_owned(); let work_stop = query.stop.clone();
            let permit = tokio::select! { biased; _ = query.stop.cancelled() => return, _ = stop.cancelled() => return, permit = lane.acquire_owned() => permit };
            let Ok(permit) = permit else { return; };
            let result = tokio::task::spawn_blocking(move || { let _permit = permit; Source::from_file(&path, &work_stop).map(Arc::new) }).await.unwrap_or_else(|_| { panics.store(true, Ordering::Release); Err(AvatarError::Invalid) });
            if query.alive() && !stop.is_cancelled() { update(UiEvent::Avatar(Event::Loaded(query.clone(), Some(result))).guarded(query.ticket, IntentScope::Session)); }
        }));
    });
    let weak = ui.as_weak();
    let cropped = state.clone();
    let handle = runtime.clone();
    let updates = update.clone();
    let original = intents.clone();
    let shutdown = stop.clone();
    ui.on_apply_crop(move || {
        let Some(ui) = weak.upgrade().filter(|ui| ui.get_crop_active() && !ui.get_avatar_loading()) else { return; };
        let mut state = cropped.borrow_mut(); state.reap(); if state.jobs.len() >= 8 { return; }
        let Some(query) = state.query.clone().filter(|query| state.matches(query, &ui, &original)) else { return; };
        let Some(source) = state.source.clone() else { return; };
        let frame = state.source_index.unwrap_or(0);
        let crop = Crop { scale: f64::from(ui.get_crop_scale()), x: f64::from(ui.get_crop_x()), y: f64::from(ui.get_crop_y()) };
        if !crop.valid() { return; };
        ui.set_avatar_loading(true); let update = updates.clone(); let stop = shutdown.clone(); let work_stop = query.stop.clone(); let lane = state.lane.clone(); let panics = state.worker_panics.clone();
        state.jobs.push(handle.spawn(async move {
            let permit = tokio::select! { biased; _ = query.stop.cancelled() => return, _ = stop.cancelled() => return, permit = lane.acquire_owned() => permit };
            let Ok(permit) = permit else { return; };
            let result = tokio::task::spawn_blocking(move || { let _permit = permit; source.select_frame(crop, frame, &work_stop) }).await.unwrap_or_else(|_| { panics.store(true, Ordering::Release); Err(AvatarError::Invalid) });
            if query.alive() && !stop.is_cancelled() { update(UiEvent::Avatar(Event::Cropped(query.clone(), result)).guarded(query.ticket, IntentScope::Session)); }
        }));
    });
    let weak = ui.as_weak();
    let profiles = state.clone();
    let data = latest.clone();
    ui.on_open_profile(move |id| {
        let Some(ui) = weak
            .upgrade()
            .filter(|ui| ui.get_authenticated() && !ui.get_busy())
        else {
            return;
        };
        let data = data.borrow();
        let Some(user) = find_user(&data, id.as_str()) else {
            return;
        };
        let mut state = profiles.borrow_mut();
        state.cancel(&ui);
        if let Some(query) = state.saving.take() {
            query.stop.cancel();
        }
        state.draft = None;
        state.profile = Some(user.clone());
        state.needs_refresh = true;
        ui.set_profile_friend(data.friends.iter().any(|friend| friend.id == user.id));
        apply_profile(&ui, user);
        ui.set_profile_editing(false);
        ui.set_profile_saving(false);
        ui.set_error("".into());
        ui.set_modal(9);
    });
    let weak = ui.as_weak();
    let profiles = state.clone();
    ui.on_edit_profile(move || {
        if let Some(ui) = weak
            .upgrade()
            .filter(|ui| ui.get_authenticated() && ui.get_profile_id() == ui.get_user_id())
        {
            if let Some(user) = &profiles.borrow().profile {
                apply_profile(&ui, user);
                ui.set_profile_editing(true);
                ui.set_error("".into());
            }
        }
    });
    let weak = ui.as_weak();
    let profiles = state.clone();
    ui.on_cancel_profile_edit(move || {
        if let Some(ui) = weak.upgrade() {
            let mut state = profiles.borrow_mut();
            state.cancel(&ui);
            state.draft = None;
            if let Some(user) = &state.profile {
                apply_profile(&ui, user);
            }
            ui.set_profile_editing(false);
            ui.set_error("".into());
            state.needs_refresh = true;
        }
    });
    let weak = ui.as_weak();
    let profiles = state.clone();
    let original = intents.clone();
    ui.on_save_profile(move || {
        let Some(ui) = weak.upgrade().filter(|ui| {
            ui.get_authenticated()
                && ui.get_modal() == 9
                && ui.get_profile_editing()
                && !ui.get_profile_saving()
                && !ui.get_avatar_loading()
                && !ui.get_crop_active()
                && ui.get_profile_id() == ui.get_user_id()
        }) else {
            return;
        };
        let mut state = profiles.borrow_mut();
        let Some(user) = &state.profile else {
            return;
        };
        let selected_color = ui.get_profile_color();
        let request = ProfileUpdate {
            owner: ui.get_user_id().to_string(),
            name: ui.get_profile_name().to_string(),
            avatar: state.draft.clone().or_else(|| user.avatar_base64.clone()),
            color: if state.draft.is_some() {
                format!(
                    "#{:02x}{:02x}{:02x}",
                    selected_color.red(),
                    selected_color.green(),
                    selected_color.blue()
                )
            } else {
                user.avatar_color.clone()
            },
            about: ui.get_profile_about().to_string(),
        };
        if let Err(error) = request.validate(user) {
            show_error(&ui, error);
            return;
        }
        let query = Query::profile(&original, request.owner.clone());
        state.saving = Some(query.clone());
        ui.set_profile_saving(true);
        ui.set_error("".into());
        if queue_profile(&commands, request, query).is_err() {
            ui.set_profile_saving(false);
            show_error(&ui, AuthError::Network);
        }
    });
    let timer = Timer::default();
    let weak = ui.as_weak();
    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        if let Some(ui) = weak.upgrade() {
            let mut state = state.borrow_mut();
            if state.needs_refresh {
                state.refresh(&ui, &latest.borrow(), setup.borrow().as_deref(), &intents);
            }
            state.tick(&ui, &runtime, update.clone(), &intents, &stop);
        }
    });
    timer
}

pub(super) fn apply_profile(ui: &NativeApp, user: &User) {
    ui.set_profile_id(user.id.clone().into());
    ui.set_profile_name(user.display_name.clone().into());
    ui.set_profile_login(user.username.clone().into());
    ui.set_profile_about(user.about_me.clone().unwrap_or_default().into());
    ui.set_profile_color(color(&user.avatar_color));
    ui.set_profile_avatar(
        user.avatar_base64
            .as_deref()
            .map(super::decode_image)
            .unwrap_or_default(),
    );
    ui.set_profile_online(user.is_online);
}

fn find_user<'a>(data: &'a Bootstrap, id: &str) -> Option<&'a User> {
    if data.user.id == id {
        return Some(&data.user);
    }
    data.friends
        .iter()
        .chain(data.requests.iter())
        .chain(data.channel_users.values().flatten())
        .chain(data.call_user.iter())
        .find(|user| user.id == id)
}

fn key(wire: &str) -> Key {
    Sha256::digest(wire.as_bytes()).into()
}

fn queue_profile(
    queue: &CommandQueue,
    request: ProfileUpdate,
    query: Query,
) -> Result<(), Box<tokio::sync::mpsc::error::TrySendError<super::QueuedCommand>>> {
    let ticket = query.ticket;
    queue
        .sender
        .try_send(super::QueuedCommand {
            command: Command::UpdateProfile(request, query),
            ticket,
        })
        .map_err(Box::new)
}

fn image(pixels: &image::RgbaImage) -> slint::Image {
    slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(
        pixels.as_raw(),
        pixels.width(),
        pixels.height(),
    ))
}

pub(super) fn check_guards(ui: &NativeApp) -> Result<(), &'static str> {
    use zabor_native::critical_intents::CriticalAction;
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        4,
        4,
        image::Rgba([0, 255, 0, 255]),
    ))
    .write_to(&mut bytes, image::ImageFormat::Png)
    .map_err(|_| "Avatar guard fixture encoding failed")?;
    let source = Source::from_bytes(&bytes.into_inner(), &CancellationToken::new())
        .map_err(|_| "Avatar guard fixture decoding failed")?;
    let selection = || {
        source
            .select(Crop::default(), &CancellationToken::new())
            .map_err(|_| "Avatar guard fixture crop failed")
    };
    ui.set_authenticated(false);
    ui.set_auth_step(2);
    ui.set_login("avatar-fixture".into());
    ui.set_modal(0);
    ui.set_crop_active(true);
    let intents = CriticalIntents::default();
    let setup = RefCell::new(Some("original".to_string()));
    let mut state = AvatarUi::new();
    let query = Query::new(&intents, Context::Setup("avatar-fixture".into()));
    state.query = Some(Query::new(
        &intents,
        Context::Setup("avatar-fixture".into()),
    ));
    state.handle(
        Event::Cropped(query.clone(), Ok(selection()?)),
        ui,
        &intents,
        &setup,
    );
    if setup.borrow().as_deref() != Some("original") {
        return Err("Replaced avatar query published into the same context");
    }
    state.query = Some(query.clone());
    ui.set_login("replacement".into());
    state.handle(
        Event::Cropped(query.clone(), Ok(selection()?)),
        ui,
        &intents,
        &setup,
    );
    if setup.borrow().as_deref() != Some("original") {
        return Err("Avatar published after setup login changed");
    }
    ui.set_login("avatar-fixture".into());
    query.stop.cancel();
    state.handle(
        Event::Cropped(query.clone(), Ok(selection()?)),
        ui,
        &intents,
        &setup,
    );
    if setup.borrow().as_deref() != Some("original") {
        return Err("Cancelled avatar query published a crop");
    }
    let query = Query::new(&intents, Context::Setup("avatar-fixture".into()));
    state.query = Some(query.clone());
    intents.request(CriticalAction::Logout);
    state.handle(
        Event::Cropped(query, Ok(selection()?)),
        ui,
        &intents,
        &setup,
    );
    if setup.borrow().as_deref() != Some("original") {
        return Err("Avatar published after Logout");
    }
    let intents = CriticalIntents::default();
    let query = Query::new(&intents, Context::Setup("avatar-fixture".into()));
    state.query = Some(query.clone());
    state.handle(
        Event::Cropped(query, Ok(selection()?)),
        ui,
        &intents,
        &setup,
    );
    if !setup
        .borrow()
        .as_deref()
        .is_some_and(|wire| wire.starts_with("data:image/png;base64,"))
        || ui.get_crop_active()
    {
        return Err("Current setup crop was not committed");
    }
    let original = setup.borrow().clone();
    ui.set_authenticated(true);
    ui.set_user_id("avatar-owner".into());
    ui.set_profile_id("avatar-owner".into());
    ui.set_profile_editing(true);
    ui.set_modal(9);
    ui.set_crop_active(true);
    let query = Query::profile(&intents, "avatar-owner".into());
    state.query = Some(query.clone());
    state.handle(
        Event::Cropped(query, Ok(selection()?)),
        ui,
        &intents,
        &setup,
    );
    if state.draft.is_none() || *setup.borrow() != original || ui.get_crop_active() {
        return Err("Profile crop leaked into setup state");
    }
    ui.set_modal(0);
    ui.set_authenticated(false);
    ui.set_auth_step(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zabor_native::critical_intents::CriticalAction;

    #[tokio::test]
    async fn original_avatar_query_survives_leave_but_never_logout_or_same_context_replacement() {
        let intents = CriticalIntents::default();
        let query = Query::new(&intents, Context::Setup("fixture".into()));
        let replacement = Query::new(&intents, Context::Setup("fixture".into()));
        assert!(!query.same(&replacement));
        intents.request(CriticalAction::Leave);
        assert!(query.alive() && intents.is_current(query.ticket, IntentScope::Session));
        assert_eq!(intents.next().await, CriticalAction::Leave);
        intents.request(CriticalAction::Logout);
        assert!(!query.alive() && !replacement.alive());
        assert!(!Event::Loaded(query, None).alive());
    }

    #[tokio::test]
    async fn profile_queue_never_remints_original_query_ticket_after_leave_and_logout_rejects_it() {
        let intents = Arc::new(CriticalIntents::default());
        let query = Query::profile(&intents, "owner".into());
        let original = query.ticket;
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        let queue = CommandQueue {
            sender,
            intents: intents.clone(),
        };
        intents.request(CriticalAction::Leave);
        assert_ne!(intents.ticket(), original);
        let request = ProfileUpdate {
            owner: "owner".into(),
            name: "name".into(),
            avatar: None,
            color: "#C81E70".into(),
            about: "".into(),
        };
        assert!(queue_profile(&queue, request, query).is_ok());
        let queued = receiver.recv().await.unwrap();
        assert_eq!(queued.ticket, original);
        assert!(matches!(queued.command.scope(), IntentScope::Session));
        intents.request(CriticalAction::Logout);
        assert!(!intents.is_current(queued.ticket, queued.command.scope()));
    }

    #[tokio::test]
    async fn retained_avatar_job_panic_is_observed_as_failure_instead_of_successful_cleanup() {
        let mut state = AvatarUi::new();
        let job = tokio::spawn(async {
            panic!("synthetic avatar task failure");
        });
        tokio::time::timeout(Duration::from_secs(2), async {
            while !job.is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        state.jobs.push(job);
        state.reap();
        assert!(state.failed() && state.jobs.is_empty());
    }
}
