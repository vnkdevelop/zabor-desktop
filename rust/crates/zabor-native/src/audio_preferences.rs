use crate::audio_mix::AudioControls;
use crate::audio_settings::AudioSettings;
use crate::hub::{HubConnection, HubError};
use crate::profile::Profile;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

const LOCAL_SYNC_KEY: &str = "nativeAudioSync";
const MAX_SETTINGS_BYTES: usize = 1024 * 1024;

struct State {
    settings: AudioSettings,
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

impl AudioPreferences {
    pub fn new(local: &Value) -> Arc<Self> {
        let mut local = local.clone();
        let metadata = local
            .as_object_mut()
            .and_then(|raw| raw.remove(LOCAL_SYNC_KEY))
            .unwrap_or(Value::Null);
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
        let mut local = state.settings.clone();
        if state.owner.is_some() && !same_owner {
            local.user_volumes.clear();
        }
        let Ok(local) = serde_json::to_value(&local) else {
            return false;
        };
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
        Some((state.serial, raw))
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
