use crate::audio_mix::AudioControls;
use crate::audio_settings::AudioSettings;
use crate::hub::{HubConnection, HubError};
use crate::profile::{Profile, ProfileError};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::Duration;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

const LOCAL_SYNC_KEY: &str = "nativeAudioSync";
const DESKTOP_KEY: &str = "nativeDesktop";
const MAX_SETTINGS_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DesktopPreferences {
    pub open_at_login: bool,
    pub minimize_to_tray: bool,
}

impl Default for DesktopPreferences {
    fn default() -> Self {
        Self {
            open_at_login: false,
            minimize_to_tray: true,
        }
    }
}

impl DesktopPreferences {
    fn from_json(value: &Value) -> Self {
        Self {
            open_at_login: value["openAtLogin"].as_bool().unwrap_or(false),
            minimize_to_tray: value["minimizeToTray"].as_bool().unwrap_or(true),
        }
    }

    fn to_json(self) -> Value {
        json!({"openAtLogin": self.open_at_login, "minimizeToTray": self.minimize_to_tray})
    }

    fn edited(self, key: &str, value: bool) -> Option<Self> {
        match key {
            "openAtLogin" => Some(Self {
                open_at_login: value,
                ..self
            }),
            "minimizeToTray" => Some(Self {
                minimize_to_tray: value,
                ..self
            }),
            _ => None,
        }
    }
}

struct State {
    settings: AudioSettings,
    desktop: DesktopPreferences,
    owner: Option<String>,
    editable: bool,
    dirty: bool,
    revision: u64,
    serial: u64,
    epoch: u64,
    session: Option<HubConnection>,
    session_stop: CancellationToken,
}

pub struct AudioPreferences {
    state: Mutex<State>,
    changed: watch::Sender<u64>,
    pub controls: Arc<AudioControls>,
}

struct SaveTicket {
    hub: HubConnection,
    stop: CancellationToken,
    revision: u64,
    epoch: u64,
    payload: String,
}

pub struct AudioSessionRestore<'a> {
    preferences: &'a AudioPreferences,
    state: Option<MutexGuard<'a, State>>,
    controls: Option<MutexGuard<'a, AudioSettings>>,
    next: State,
    next_controls: AudioSettings,
    input_changed: bool,
    output_changed: bool,
    controls_changed: bool,
    applied: bool,
}

impl AudioSessionRestore<'_> {
    pub fn apply(&mut self) {
        if self.applied {
            return;
        }
        if self.controls_changed {
            if self.input_changed {
                self.preferences
                    .controls
                    .input_device_epoch
                    .fetch_add(1, Ordering::AcqRel);
                self.preferences.controls.capture_restarting();
            }
            if self.output_changed {
                self.preferences
                    .controls
                    .output_device_epoch
                    .fetch_add(1, Ordering::AcqRel);
                self.preferences
                    .controls
                    .playback_epoch
                    .fetch_add(1, Ordering::AcqRel);
            }
            std::mem::swap(
                &mut **self.controls.as_mut().unwrap(),
                &mut self.next_controls,
            );
            self.preferences
                .controls
                .settings_revision
                .fetch_add(1, Ordering::Release);
        }
        std::mem::swap(&mut **self.state.as_mut().unwrap(), &mut self.next);
        self.applied = true;
    }
}

impl Drop for AudioSessionRestore<'_> {
    fn drop(&mut self) {
        if self.applied {
            self.preferences
                .changed
                .send_replace(self.state.as_ref().unwrap().serial);
        }
        drop(self.controls.take());
        drop(self.state.take());
        if self.applied {
            self.next.session_stop.cancel();
        }
    }
}

pub struct AudioSessionLogout<'a> {
    preferences: &'a AudioPreferences,
    state: Option<MutexGuard<'a, State>>,
    session: Option<HubConnection>,
    stop: CancellationToken,
    applied: bool,
}

impl AudioSessionLogout<'_> {
    pub fn apply(&mut self) {
        if self.applied {
            return;
        }
        let state = self.state.as_mut().unwrap();
        std::mem::swap(&mut state.session, &mut self.session);
        state.epoch = state.epoch.wrapping_add(1);
        state.dirty = false;
        state.editable = false;
        state.serial = state.serial.wrapping_add(1);
        self.applied = true;
    }
}

impl Drop for AudioSessionLogout<'_> {
    fn drop(&mut self) {
        if self.applied {
            self.preferences
                .changed
                .send_replace(self.state.as_ref().unwrap().serial);
        }
        drop(self.state.take());
        if self.applied {
            self.stop.cancel();
        }
    }
}

impl AudioPreferences {
    pub fn new(local: &Value) -> Arc<Self> {
        let mut local = local.clone();
        let metadata = local
            .as_object_mut()
            .and_then(|raw| raw.remove(LOCAL_SYNC_KEY))
            .unwrap_or(Value::Null);
        let desktop = DesktopPreferences::from_json(
            &local
                .as_object_mut()
                .and_then(|raw| raw.remove(DESKTOP_KEY))
                .unwrap_or(Value::Null),
        );
        let owner = metadata["owner"]
            .as_str()
            .filter(|owner| !owner.is_empty() && owner.len() <= 4096)
            .map(str::to_owned);
        let settings = AudioSettings::from_json(&local);
        let controls = Arc::new(AudioControls::new(settings.clone()));
        let (changed, _) = watch::channel(0);
        Arc::new(Self {
            state: Mutex::new(State {
                settings,
                desktop,
                dirty: owner.is_some() && metadata["pending"] == true,
                owner,
                editable: false,
                revision: 0,
                serial: 0,
                epoch: 0,
                session: None,
                session_stop: CancellationToken::new(),
            }),
            changed,
            controls,
        })
    }

    pub fn settings(&self) -> Option<AudioSettings> {
        self.state.lock().ok().map(|state| state.settings.clone())
    }

    pub fn desktop(&self) -> DesktopPreferences {
        self.state
            .lock()
            .map(|state| state.desktop)
            .unwrap_or_default()
    }

    pub fn relay_only(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.settings.relay_only)
            .unwrap_or(true)
    }

    pub fn edit(&self, owner: &str, key: &str, value: Value) -> bool {
        self.update(owner, |settings| settings.edited(key, value))
    }

    pub fn peer_volume(&self, owner: &str, peer: &str, volume: f32) -> bool {
        self.update(owner, |settings| settings.with_peer_volume(peer, volume))
    }

    fn update(
        &self,
        owner: &str,
        edit: impl FnOnce(&AudioSettings) -> Option<AudioSettings>,
    ) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if !state.editable || state.owner.as_deref() != Some(owner) || owner.is_empty() {
            return false;
        }
        let Some(settings) = edit(&state.settings) else {
            return false;
        };
        self.apply(&mut state, settings, true)
    }

    pub fn language(&self, language: &str) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let Some(settings) = state.settings.edited("language", json!(language)) else {
            return false;
        };
        let dirty = state.editable && state.owner.is_some();
        self.apply(&mut state, settings, dirty)
    }

    pub fn desktop_setting(&self, key: &str, value: bool) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let Some(desktop) = state.desktop.edited(key, value) else {
            return false;
        };
        if desktop == state.desktop {
            return true;
        }
        state.desktop = desktop;
        self.notify(&mut state);
        true
    }

    fn apply(&self, state: &mut State, settings: AudioSettings, dirty: bool) -> bool {
        if settings == state.settings {
            return true;
        }
        if serde_json::to_vec(&settings)
            .ok()
            .is_none_or(|bytes| bytes.len() > MAX_SETTINGS_BYTES)
            || !self.controls.apply_settings(settings.clone())
        {
            return false;
        }
        state.settings = settings;
        state.dirty |= dirty;
        state.revision = state.revision.wrapping_add(1);
        self.notify(state);
        true
    }

    fn notify(&self, state: &mut State) {
        state.serial = state.serial.wrapping_add(1);
        self.changed.send_replace(state.serial);
    }

    pub fn begin_session(&self, owner: &str, hub: HubConnection, server: Option<&Value>) -> bool {
        if owner.is_empty() || owner.len() > 4096 {
            return false;
        }
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        state.session_stop.cancel();
        state.session_stop = CancellationToken::new();
        state.epoch = state.epoch.wrapping_add(1);
        let same_owner = state.owner.as_deref() == Some(owner);
        let Some(settings) = Self::session_settings(&state, owner, server) else {
            return false;
        };
        state.dirty &= same_owner;
        state.owner = Some(owner.into());
        state.editable = true;
        state.session = Some(hub);
        if !self.apply(&mut state, settings, false) {
            state.session = None;
            state.session_stop.cancel();
            return false;
        }
        self.notify(&mut state);
        true
    }

    fn session_settings(
        state: &State,
        owner: &str,
        server: Option<&Value>,
    ) -> Option<AudioSettings> {
        let same_owner = state.owner.as_deref() == Some(owner);
        let mut local = state.settings.clone();
        if state.owner.is_some() && !same_owner {
            local.user_volumes.clear();
        }
        let local = serde_json::to_value(&local).ok()?;
        let server = server.map(|server| {
            let mut raw = server.as_object().cloned().unwrap_or_default();
            raw.remove(LOCAL_SYNC_KEY);
            Value::Object(raw)
        });
        let settings = if same_owner && state.dirty {
            AudioSettings::restore(
                &local,
                server
                    .as_ref()
                    .map(|server| {
                        let mut raw = server.as_object().cloned().unwrap_or_default();
                        for (key, value) in local.as_object().into_iter().flatten() {
                            raw.insert(key.clone(), value.clone());
                        }
                        Value::Object(raw)
                    })
                    .as_ref(),
            )
        } else {
            AudioSettings::restore(&local, server.as_ref())
        };
        Some(settings)
    }

    pub fn try_prepare_session(
        &self,
        owner: &str,
        hub: HubConnection,
        server: Option<&Value>,
    ) -> Result<Option<AudioSessionRestore<'_>>, &'static str> {
        if owner.is_empty() || owner.len() > 4096 {
            return Err("invalid audio session owner");
        }
        let state = match self.state.try_lock() {
            Ok(state) => state,
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Poisoned(_)) => return Err("audio preferences unavailable"),
        };
        let controls = match self.controls.settings.try_lock() {
            Ok(controls) => controls,
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Poisoned(_)) => return Err("audio controls unavailable"),
        };
        let settings =
            Self::session_settings(&state, owner, server).ok_or("audio settings unavailable")?;
        if settings != state.settings
            && serde_json::to_vec(&settings)
                .ok()
                .is_none_or(|bytes| bytes.len() > MAX_SETTINGS_BYTES)
        {
            return Err("audio settings too large");
        }
        let changed = settings != state.settings;
        let next = State {
            settings: settings.clone(),
            desktop: state.desktop,
            owner: Some(owner.into()),
            editable: true,
            dirty: state.dirty && state.owner.as_deref() == Some(owner),
            revision: state.revision.wrapping_add(u64::from(changed)),
            serial: state.serial.wrapping_add(1 + u64::from(changed)),
            epoch: state.epoch.wrapping_add(1),
            session: Some(hub),
            session_stop: CancellationToken::new(),
        };
        Ok(Some(AudioSessionRestore {
            preferences: self,
            input_changed: controls.selected_input != settings.selected_input,
            output_changed: controls.selected_output != settings.selected_output,
            controls_changed: changed && *controls != settings,
            state: Some(state),
            controls: Some(controls),
            next,
            next_controls: settings,
            applied: false,
        }))
    }

    pub fn disconnect(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.session_stop.cancel();
            state.session = None;
            state.epoch = state.epoch.wrapping_add(1);
            self.notify(&mut state);
        }
    }

    pub fn logout(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.session_stop.cancel();
            state.session = None;
            state.epoch = state.epoch.wrapping_add(1);
            state.dirty = false;
            state.editable = false;
            self.notify(&mut state);
        }
    }

    pub fn try_prepare_logout(&self) -> Result<Option<AudioSessionLogout<'_>>, &'static str> {
        let state = match self.state.try_lock() {
            Ok(state) => state,
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Poisoned(_)) => return Err("audio preferences unavailable"),
        };
        Ok(Some(AudioSessionLogout {
            preferences: self,
            stop: state.session_stop.clone(),
            state: Some(state),
            session: None,
            applied: false,
        }))
    }

    fn ticket(&self) -> Option<SaveTicket> {
        let state = self.state.lock().ok()?;
        if !state.dirty {
            return None;
        }
        let hub = state.session.as_ref().filter(|hub| hub.is_connected())?;
        let payload = serde_json::to_string(&state.settings).ok()?;
        (payload.len() <= MAX_SETTINGS_BYTES).then(|| SaveTicket {
            hub: hub.clone(),
            stop: state.session_stop.clone(),
            revision: state.revision,
            epoch: state.epoch,
            payload,
        })
    }

    fn acknowledge(&self, ticket: &SaveTicket) {
        if let Ok(mut state) = self.state.lock() {
            if state.epoch == ticket.epoch && state.revision == ticket.revision {
                state.dirty = false;
                self.notify(&mut state);
            }
        }
    }

    fn local_snapshot(&self) -> Option<(u64, Value)> {
        let state = self.state.lock().ok()?;
        let mut raw = serde_json::to_value(&state.settings).ok()?;
        raw[LOCAL_SYNC_KEY] = json!({"owner":state.owner,"pending":state.dirty});
        raw[DESKTOP_KEY] = state.desktop.to_json();
        Some((state.serial, raw))
    }

    pub async fn run_local_checked(
        self: Arc<Self>,
        profile: Option<Arc<Profile>>,
        stop: CancellationToken,
        failed: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<(), ProfileError> {
        let incomplete = Arc::new(AtomicBool::new(false));
        let observed = incomplete.clone();
        self.clone()
            .run_local(
                profile,
                stop,
                Arc::new(move || {
                    observed.store(true, Ordering::Release);
                    failed();
                }),
            )
            .await;
        if incomplete.load(Ordering::Acquire) || self.local_snapshot().is_none() {
            Err(ProfileError::Storage)
        } else {
            Ok(())
        }
    }

    pub async fn run_local(
        self: Arc<Self>,
        profile: Option<Arc<Profile>>,
        stop: CancellationToken,
        failed: Arc<dyn Fn() + Send + Sync>,
    ) {
        let Some(profile) = profile else {
            return;
        };
        let mut changed = self.changed.subscribe();
        let mut saved = Some(0);
        let mut attempted = Some(0);
        loop {
            changed.borrow_and_update();
            if !stop.is_cancelled() {
                tokio::select! {
                    _ = stop.cancelled() => {},
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {},
                    result = changed.changed() => { if result.is_err() { break; } continue; },
                }
            }
            if let Some((serial, settings)) = self.local_snapshot() {
                if saved != Some(serial) && (attempted != Some(serial) || stop.is_cancelled()) {
                    let profile = profile.clone();
                    if matches!(
                        tokio::task::spawn_blocking(move || profile.write_settings(&settings))
                            .await,
                        Ok(Ok(()))
                    ) {
                        saved = Some(serial);
                    } else {
                        failed();
                    }
                    attempted = Some(serial);
                    if self
                        .local_snapshot()
                        .is_some_and(|(latest, _)| latest != serial)
                    {
                        continue;
                    }
                }
            }
            if stop.is_cancelled() {
                break;
            }
            tokio::select! {
                _ = stop.cancelled() => {},
                result = changed.changed() => { if result.is_err() { break; } },
            }
        }
    }

    pub async fn run_remote(
        self: Arc<Self>,
        stop: CancellationToken,
        failed: Arc<dyn Fn() + Send + Sync>,
    ) {
        let mut changed = self.changed.subscribe();
        loop {
            changed.borrow_and_update();
            if stop.is_cancelled() {
                break;
            }
            if let Some(ticket) = self.ticket() {
                tokio::select! {
                    _ = stop.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_millis(500)) => {},
                    result = changed.changed() => { if result.is_err() { break; } continue; },
                }
                let result = tokio::select! {
                    biased;
                    _ = stop.cancelled() => break,
                    _ = ticket.stop.cancelled() => None,
                    result = save_audio_settings(&ticket.hub, &ticket.payload) => Some(result),
                };
                match result {
                    Some(Ok(true)) => {
                        self.acknowledge(&ticket);
                        continue;
                    }
                    Some(_) if !ticket.stop.is_cancelled() => failed(),
                    _ => {}
                }
            }
            tokio::select! {
                _ = stop.cancelled() => break,
                result = changed.changed() => { if result.is_err() { break; } },
            }
        }
    }
}

pub async fn save_audio_settings(hub: &HubConnection, payload: &str) -> Result<bool, HubError> {
    if payload.len() > MAX_SETTINGS_BYTES
        || !serde_json::from_str::<Value>(payload).is_ok_and(|value| value.is_object())
    {
        return Err(HubError::Protocol);
    }
    let result = hub
        .invoke(
            "SaveAudioSettings",
            vec![json!(payload)],
            Duration::from_secs(20),
        )
        .await?;
    match result {
        Value::Null | Value::Bool(true) => Ok(true),
        Value::Bool(false) => Ok(false),
        _ => Err(HubError::Protocol),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_preferences_stay_local_and_never_enter_the_server_payload() {
        let preferences = AudioPreferences::new(&json!({
            "inputVolume": 150,
            "nativeDesktop": {"openAtLogin": true, "minimizeToTray": false},
            "nativeAudioSync": {"owner": "owner", "pending": false}
        }));
        assert_eq!(
            preferences.desktop(),
            DesktopPreferences {
                open_at_login: true,
                minimize_to_tray: false,
            }
        );
        assert!(preferences.settings().is_some_and(|settings| settings
            .extra
            .get(DESKTOP_KEY)
            .is_none()
            && settings.extra.get(LOCAL_SYNC_KEY).is_none()));
        assert!(!preferences.desktop_setting("inputVolume", true));
    }

    #[test]
    fn desktop_preferences_round_trip_through_the_local_settings_file() {
        let preferences = AudioPreferences::new(
            &json!({"nativeDesktop": {"openAtLogin": false, "minimizeToTray": true}}),
        );
        assert!(preferences.desktop_setting("openAtLogin", true));
        assert!(preferences.desktop_setting("minimizeToTray", false));
        assert_eq!(
            preferences.desktop(),
            DesktopPreferences {
                open_at_login: true,
                minimize_to_tray: false,
            }
        );
        let (_, snapshot) = preferences.local_snapshot().unwrap();
        assert_eq!(snapshot[DESKTOP_KEY]["openAtLogin"], true);
        assert_eq!(snapshot[DESKTOP_KEY]["minimizeToTray"], false);
        assert_eq!(snapshot[LOCAL_SYNC_KEY]["pending"], false);
        assert_eq!(
            DesktopPreferences::from_json(&snapshot[DESKTOP_KEY]),
            preferences.desktop()
        );
        assert!(preferences.ticket().is_none());
    }

    #[test]
    fn desktop_preferences_default_to_visible_window_and_no_autostart() {
        assert_eq!(
            DesktopPreferences::default(),
            DesktopPreferences {
                open_at_login: false,
                minimize_to_tray: true,
            }
        );
        assert_eq!(
            DesktopPreferences::from_json(&Value::Null),
            DesktopPreferences::default()
        );
    }
}
