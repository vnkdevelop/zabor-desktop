use super::{queue_control_command, Command, CommandQueue, DisplayedFrame, NativeApp};
use crate::RemoteControlPillWindow;
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, Model};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::runtime::Handle;
use zabor_native::audio_runtime::SharedAudio;
use zabor_native::critical_intents::{CriticalIntents, IntentScope};
use zabor_native::native_audio::NativeAudio;
use zabor_native::remote_viewer::desktop::{
    CursorCompletion, CursorState, HostBinding, HostPill, ViewerBinding, ViewerDesktop,
    ViewerRequest,
};
use zabor_native::voice::{ControlTarget, RemoteControlView, VoiceSession};

pub(super) struct Update {
    pub state: RemoteControlView,
    pub targets: Vec<ControlTarget>,
    pub original: Option<ControlTarget>,
    pub host: Option<HostBinding>,
}

impl Update {
    pub(super) fn from_session(session: &VoiceSession, original: Option<ControlTarget>) -> Self {
        Self {
            state: session.remote_control_view(),
            targets: session.control_targets(),
            original,
            host: session.host_control_binding(),
        }
    }
}

fn same_request(
    original: Option<&Arc<ViewerRequest>>,
    incoming: Option<&Arc<ViewerRequest>>,
) -> bool {
    original
        .zip(incoming)
        .is_some_and(|(a, b)| Arc::ptr_eq(a, b))
}

struct Viewer {
    target: ControlTarget,
    adapter: Option<ViewerDesktop>,
    ready: bool,
    confined: bool,
}

struct Host {
    target: ControlTarget,
    authorized: Arc<AtomicBool>,
    consent: bool,
    view: RemoteControlView,
    pending: Option<(HostBinding, RemoteControlPillWindow)>,
    pill: Option<HostPill>,
}

pub(super) struct Desktop {
    audio: Arc<SharedAudio<NativeAudio>>,
    handle: Handle,
    intents: Arc<CriticalIntents>,
    generation: Arc<AtomicU64>,
    commands: CommandQueue,
    preview: bool,
    focused: bool,
    viewer: Option<Viewer>,
    host: Option<Host>,
    bounds: BTreeMap<String, [f64; 4]>,
    completions: Vec<CursorCompletion>,
    failed_hosts: Vec<HostPill>,
    failed: bool,
}

impl Desktop {
    pub(super) fn new(
        audio: Arc<SharedAudio<NativeAudio>>,
        handle: Handle,
        intents: Arc<CriticalIntents>,
        generation: Arc<AtomicU64>,
        commands: CommandQueue,
        preview: bool,
    ) -> Self {
        Self {
            audio,
            handle,
            intents,
            generation,
            commands,
            preview,
            focused: false,
            viewer: None,
            host: None,
            bounds: BTreeMap::new(),
            completions: Vec::new(),
            failed_hosts: Vec::new(),
            failed: false,
        }
    }

    pub(super) fn target(&self, peer: &str) -> Option<ControlTarget> {
        self.viewer
            .as_ref()
            .filter(|viewer| viewer.target.peer == peer)
            .map(|viewer| viewer.target.clone())
    }

    pub(super) fn start(&mut self, ui: &NativeApp, target: ControlTarget) -> bool {
        self.release();
        if self.preview || target.request().is_none() || target.is_retired() {
            return false;
        }
        self.focused = ui
            .window()
            .with_winit_window(|window| window.has_focus())
            .unwrap_or(false);
        self.viewer = Some(Viewer {
            target,
            adapter: None,
            ready: false,
            confined: true,
        });
        ui.set_control_confined(true);
        self.ensure_viewer(ui);
        self.viewer.is_some()
    }

    fn ensure_viewer(&mut self, ui: &NativeApp) {
        let Some(viewer) = self
            .viewer
            .as_mut()
            .filter(|viewer| viewer.adapter.is_none())
        else {
            return;
        };
        let Some(window) = super::native_window_handle(ui) else {
            return;
        };
        let Some(request) = viewer.target.request() else {
            return;
        };
        let Some(ticket) = viewer.target.input_ticket() else {
            return;
        };
        let Some(voice) = viewer.target.voice() else {
            return;
        };
        let _entered = self.handle.enter();
        match ViewerDesktop::new(
            &self.audio,
            ViewerBinding {
                intents: self.intents.clone(),
                ticket,
                target: viewer.target.clone(),
                generation: self.generation.clone(),
                voice,
                request: (*request).clone(),
            },
            window,
        ) {
            Ok(mut adapter) => {
                if let Some(bounds) = self.bounds.get(&viewer.target.peer) {
                    adapter.relayout(*bounds, f64::from(ui.window().scale_factor()));
                }
                viewer.adapter = Some(adapter);
            }
            Err(_) => {
                drop(_entered);
                self.release();
            }
        }
    }

    pub(super) fn accepts(&self, incoming: Option<&ControlTarget>) -> bool {
        match (&self.viewer, incoming) {
            (Some(viewer), Some(target)) => {
                same_request(viewer.target.request().as_ref(), target.request().as_ref())
                    && viewer.target.same_request(target)
            }
            (None, None) => true,
            _ => false,
        }
    }

    pub(super) fn update(&mut self, ui: &NativeApp, update: &Update) -> bool {
        if !self.accepts(update.original.as_ref()) {
            return false;
        }
        self.ensure_viewer(ui);
        if let Some(viewer) = &mut self.viewer {
            ui.set_control_confined(viewer.confined);
            let current = update.targets.iter().find(|target| {
                target.lifetime.is_same(&viewer.target.lifetime)
                    && target.channel.is_same(&viewer.target.channel)
            });
            if let (Some(adapter), Some(request)) = (&mut viewer.adapter, viewer.target.request()) {
                adapter.update_control(
                    &update.state,
                    current,
                    self.focused && ui.get_focused_video() == viewer.target.peer.as_str(),
                    ui.get_modal() != 0,
                    request.as_ref(),
                );
            }
        }
        self.retire_failed_viewer();
        true
    }

    pub(super) fn decoded(&mut self, ui: &NativeApp, peer: &str, frame: &DisplayedFrame) {
        let Some(viewer) = self
            .viewer
            .as_mut()
            .filter(|viewer| viewer.target.peer == peer)
        else {
            return;
        };
        let Some(source) = &frame.source else {
            self.release();
            ui.set_control_ready(false);
            return;
        };
        if let Some(adapter) = &mut viewer.adapter {
            let _entered = self.handle.enter();
            viewer.ready = adapter
                .decoded_frame(
                    frame.stamp.clone(),
                    frame.ticket,
                    frame.generation,
                    frame.source_epoch,
                    source.as_ref(),
                )
                .unwrap_or(false);
        }
        self.retire_failed_viewer();
    }

    fn retire_failed_viewer(&mut self) {
        if self.viewer.as_ref().is_some_and(|viewer| {
            viewer.target.is_retired()
                || viewer
                    .adapter
                    .as_ref()
                    .is_some_and(|adapter| !adapter.is_alive())
        }) {
            self.release();
        }
    }

    pub(super) fn ready(&self) -> bool {
        self.viewer.as_ref().is_some_and(|viewer| {
            viewer.ready
                && !viewer.target.is_retired()
                && viewer.adapter.as_ref().is_some_and(ViewerDesktop::is_alive)
        })
    }

    pub(super) fn layout(&mut self, ui: &NativeApp, peer: String, bounds: [f64; 4]) {
        self.bounds.insert(peer.clone(), bounds);
        if let Some(adapter) = self
            .viewer
            .as_mut()
            .filter(|viewer| viewer.target.peer == peer)
            .and_then(|viewer| viewer.adapter.as_mut())
        {
            adapter.relayout(bounds, f64::from(ui.window().scale_factor()));
        }
        self.retire_failed_viewer();
    }

    pub(super) fn rescale(&mut self, scale: f64) {
        if let Some(viewer) = &mut self.viewer {
            if let (Some(adapter), Some(bounds)) =
                (&mut viewer.adapter, self.bounds.get(&viewer.target.peer))
            {
                adapter.relayout(*bounds, scale);
            }
        }
        self.retire_failed_viewer();
    }

    pub(super) fn focus(&mut self, focused: bool) {
        self.focused = focused;
        if !focused {
            self.release();
        }
    }

    pub(super) fn confinement(&mut self, ui: &NativeApp, peer: &str, confined: bool) {
        if let Some(viewer) = self
            .viewer
            .as_mut()
            .filter(|viewer| viewer.target.peer == peer)
        {
            viewer.confined = confined;
            if let Some(adapter) = &viewer.adapter {
                adapter.set_confined(confined);
            }
            ui.set_control_confined(confined);
            if let Ok(strings) = zabor_native::strings::Strings::new(ui.get_language().as_str()) {
                ui.set_error(
                    strings
                        .text(if confined {
                            "stream.cursorCaptured"
                        } else {
                            "stream.cursorReleased"
                        })
                        .into(),
                );
            }
        }
    }

    pub(super) fn activity(&self, target: &ControlTarget) {
        if self.accepts(Some(target)) {
            if let Some(adapter) = self
                .viewer
                .as_ref()
                .and_then(|viewer| viewer.adapter.as_ref())
            {
                adapter.admitted_activity();
            }
        }
    }

    pub(super) fn release(&mut self) {
        if let Some(viewer) = self.viewer.take() {
            if let Some(adapter) = &viewer.adapter {
                adapter.release();
                if let Some(completion) = adapter.completion() {
                    self.completions.push(completion);
                }
            }
            viewer.target.send_stop();
            queue_control_command(
                &self.commands,
                &self.handle,
                Command::ReleaseControlRequest(viewer.target),
            );
        }
    }

    pub(super) fn revoke_host(&mut self) {
        let target = self.host.as_ref().map(|host| host.target.clone());
        self.finish_host();
        if let Some(target) = target {
            queue_control_command(
                &self.commands,
                &self.handle,
                Command::RevokeControlTarget(target),
            );
        }
    }

    pub(super) fn update_host(
        &mut self,
        ui: &NativeApp,
        binding: Option<HostBinding>,
        view: &RemoteControlView,
    ) {
        if self.preview {
            return;
        }
        if self.host.as_ref().is_some_and(|host| {
            !host.authorized.load(Ordering::Acquire)
                || binding
                    .as_ref()
                    .is_some_and(|binding| !Arc::ptr_eq(&host.authorized, &binding.authorized))
        }) {
            self.finish_host();
        }
        if let Some(host) = &mut self.host {
            host.view = view.clone();
            return;
        }
        let Some(binding) = binding.filter(|binding| binding.authorized.load(Ordering::Acquire))
        else {
            return;
        };
        let Ok(pill) = RemoteControlPillWindow::new() else {
            binding.authorized.store(false, Ordering::Release);
            return;
        };
        let name = ui
            .get_voice_users()
            .iter()
            .find(|user| user.id == binding.target.peer.as_str())
            .map(|user| user.name.to_lowercase())
            .unwrap_or_default();
        pill.set_controller_name(name.into());
        self.host = Some(Host {
            target: binding.target.clone(),
            authorized: binding.authorized.clone(),
            consent: binding.consent,
            view: view.clone(),
            pending: Some((binding, pill)),
            pill: None,
        });
    }

    fn finish_host(&mut self) {
        if let Some(mut host) = self.host.take() {
            host.authorized.store(false, Ordering::Release);
            if let Some(pill) = host.pill.take() {
                self.completions.push(pill.completion());
                if pill.finish().is_err() {
                    self.failed = true;
                    self.failed_hosts.push(pill);
                }
            }
        }
    }

    pub(super) fn maintain(&mut self, ui: &NativeApp) {
        if self.viewer.as_ref().is_some_and(|viewer| {
            !ui.get_voice_active()
                || ui.get_modal() != 0
                || !self.focused
                || ui.get_focused_video() != viewer.target.peer.as_str()
                || viewer.target.is_retired()
                || viewer
                    .target
                    .input_ticket()
                    .is_none_or(|ticket| !self.intents.is_current(ticket, IntentScope::Voice))
                || viewer
                    .adapter
                    .as_ref()
                    .is_some_and(|adapter| !adapter.is_alive())
        }) {
            self.release();
        }
        self.ensure_viewer(ui);
        self.rescale(f64::from(ui.window().scale_factor()));
        ui.set_control_ready(ui.get_control_ready() && self.ready());
        if let Some(host) = &mut self.host {
            if !host.authorized.load(Ordering::Acquire) {
                self.finish_host();
                return;
            }
            if let Some((_, pill)) = &host.pending {
                use slint::winit_030::winit::raw_window_handle::{
                    HasWindowHandle, RawWindowHandle,
                };
                let hidden = pill
                    .window()
                    .with_winit_window(|window| {
                        if window.is_visible() != Some(false) {
                            return None;
                        }
                        match window.window_handle().ok()?.as_raw() {
                            RawWindowHandle::Win32(handle) => u64::try_from(handle.hwnd.get()).ok(),
                            _ => None,
                        }
                    })
                    .flatten();
                if hidden.is_some() {
                    let (binding, pill) = host.pending.take().unwrap();
                    let _entered = self.handle.enter();
                    match HostPill::attach(
                        &self.audio,
                        binding,
                        pill,
                        &host.view,
                        Some(&host.target),
                    ) {
                        Ok(pill) => host.pill = Some(pill),
                        Err(_) => host.authorized.store(false, Ordering::Release),
                    }
                }
            }
            if let Some(pill) = &host.pill {
                pill.update_control(&host.view, Some(&host.target), host.consent);
            }
        }
        self.failed |= self
            .completions
            .iter()
            .any(|completion| completion.state() == CursorState::Failed);
    }

    pub(super) fn remove_video(&mut self, peer: &str) {
        if self
            .viewer
            .as_ref()
            .is_some_and(|viewer| viewer.target.peer == peer)
        {
            self.release();
        }
        self.bounds.remove(peer);
    }

    pub(super) fn close(&mut self) {
        self.release();
        self.finish_host();
        self.failed_hosts.clear();
        self.bounds.clear();
    }

    #[allow(dead_code)]
    pub(super) fn failed(&self) -> bool {
        self.failed
            || self
                .completions
                .iter()
                .any(|completion| completion.state() == CursorState::Failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_request_clone_is_retained_and_replacement_cannot_accept_original_ui_results() {
        let original = Arc::new(ViewerRequest::default());
        let queued = original.clone();
        let delayed = queued.clone();
        let replacement = Arc::new(ViewerRequest::default());
        assert!(same_request(Some(&original), Some(&queued)));
        assert!(same_request(Some(&original), Some(&delayed)));
        assert!(!same_request(Some(&replacement), Some(&delayed)));
        assert!(!same_request(Some(&replacement), None));
    }
}
