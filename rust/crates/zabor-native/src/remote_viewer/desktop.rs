use crate::audio_runtime::{AudioCleanup, SharedAudio, SharedAudioDevice};
use crate::critical_intents::{CriticalIntents, IntentScope, IntentTicket};
use crate::media::{ChannelLease, MediaError, PeerLifetime};
use crate::remote_input::cursor::{register_surface, CursorActor};
use crate::video::VideoFrameStamp;
use crate::voice::{ControlTarget, RemoteControlView};
use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, PhysicalPosition, PhysicalSize, Timer, TimerMode};
use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use zabor_input::desktop::{host_pill_bounds, physical_viewport, HostSurface, ViewerCursorLease};
use zabor_input::{CaptureLease, InputError, Rect};

pub use crate::remote_input::cursor::{CursorCompletion, CursorState};

pub struct ViewerBinding {
    pub intents: Arc<CriticalIntents>,
    pub ticket: IntentTicket,
    pub target: ControlTarget,
    pub generation: Arc<AtomicU64>,
    pub voice: CancellationToken,
}

fn same_target(original: &ControlTarget, current: &ControlTarget) -> bool {
    original.peer == current.peer
        && original.lifetime.is_same(&current.lifetime)
        && original.channel.is_same(&current.channel)
}

fn frame_admitted(
    source_epoch: Option<u64>,
    current: bool,
    decoded_at: Instant,
    accepted_at: Instant,
    now: Instant,
) -> bool {
    source_epoch.is_none()
        && current
        && decoded_at >= accepted_at
        && decoded_at <= now
        && now.duration_since(decoded_at) <= Duration::from_millis(750)
}

pub struct ViewerDesktop {
    binding: ViewerBinding,
    cleanup: Arc<AudioCleanup>,
    voice: CancellationToken,
    lease: Arc<ViewerCursorLease>,
    accepted_at: Option<Instant>,
    generation: u64,
    viewport: Option<Rect>,
    frame: Arc<Mutex<Option<VideoFrameStamp>>>,
    permitted: Arc<AtomicBool>,
    released: AtomicBool,
    actor: Option<CursorActor>,
    affinity: PhantomData<Rc<()>>,
}

impl ViewerDesktop {
    pub fn new<D: SharedAudioDevice>(
        runtime: &SharedAudio<D>,
        binding: ViewerBinding,
        window: u64,
    ) -> Result<Self, MediaError> {
        let voice = binding.voice.child_token();
        if window == 0
            || voice.is_cancelled()
            || !binding.target.lifetime.is_alive()
            || !binding.target.channel.is_alive()
            || !binding
                .intents
                .is_current(binding.ticket, IntentScope::Voice)
        {
            return Err(MediaError::UnauthorizedPeer);
        }
        let generation = binding.generation.load(Ordering::Acquire);
        Ok(Self {
            binding,
            cleanup: runtime.cleanup_tracker(),
            voice,
            lease: ViewerCursorLease::new(window),
            accepted_at: None,
            generation,
            viewport: None,
            frame: Arc::new(Mutex::new(None)),
            permitted: Arc::new(AtomicBool::new(false)),
            released: AtomicBool::new(false),
            actor: None,
            affinity: PhantomData,
        })
    }

    pub fn update_control(
        &mut self,
        view: &RemoteControlView,
        current: Option<&ControlTarget>,
        focused: bool,
        modal: bool,
    ) -> bool {
        if !self.lease.is_alive()
            || self.voice.is_cancelled()
            || !focused
            || modal
            || view.revoked
            || view.viewer != self.binding.target.peer
            || !current.is_some_and(|target| same_target(&self.binding.target, target))
            || !view.permitted_peers.contains(&self.binding.target.peer)
            || !self
                .binding
                .intents
                .is_current(self.binding.ticket, IntentScope::Voice)
            || self.binding.generation.load(Ordering::Acquire) != self.generation
        {
            self.release();
            return false;
        }
        let Some(accepted_at) = view.viewer_accepted_at else {
            if self.accepted_at.is_some() {
                self.release();
            }
            return false;
        };
        if self
            .accepted_at
            .is_some_and(|original| original != accepted_at)
            || (!view.viewer_ready && self.accepted_at.is_some())
        {
            self.release();
            return false;
        }
        if !view.viewer_ready {
            return false;
        }
        self.lease.acknowledge_at(accepted_at);
        self.accepted_at = Some(accepted_at);
        self.permitted.store(true, Ordering::Release);
        self.lease.is_alive()
    }

    pub fn relayout(&mut self, bounds: [f64; 4], scale: f64) -> bool {
        let Some(viewport) = physical_viewport(bounds, scale) else {
            self.release();
            return false;
        };
        if !self.lease.is_alive() {
            return false;
        }
        self.viewport = Some(viewport);
        self.lease.relayout(viewport);
        true
    }

    pub fn update_focus(&self, focused: bool, modal: bool) -> bool {
        if !focused || modal {
            self.release();
        }
        self.is_alive()
    }

    pub fn is_alive(&self) -> bool {
        let current = self.lease.is_alive()
            && !self.voice.is_cancelled()
            && self.binding.target.lifetime.is_alive()
            && self.binding.target.channel.is_alive()
            && self
                .binding
                .intents
                .is_current(self.binding.ticket, IntentScope::Voice)
            && self.binding.generation.load(Ordering::Acquire) == self.generation;
        let frame = match self.frame.lock() {
            Ok(frame) => frame.clone(),
            Err(_) => {
                self.release();
                return false;
            }
        };
        if !current
            || frame.is_some_and(|frame| {
                !self
                    .accepted_at
                    .is_some_and(|at| frame.is_current_since(&self.binding.target.lifetime, at))
            })
        {
            self.release();
            return false;
        }
        true
    }

    pub fn decoded_frame(
        &mut self,
        frame: VideoFrameStamp,
        ticket: IntentTicket,
        generation: u64,
        source_epoch: Option<u64>,
    ) -> Result<bool, MediaError> {
        let (Some(accepted_at), Some(viewport)) = (self.accepted_at, self.viewport) else {
            return Ok(false);
        };
        if !self.lease.is_alive() || !self.permitted.load(Ordering::Acquire) {
            self.release();
            return Ok(false);
        }
        let current = ticket == self.binding.ticket
            && generation == self.generation
            && self.binding.generation.load(Ordering::Acquire) == generation
            && self.binding.intents.is_current(ticket, IntentScope::Voice)
            && frame.is_current_since(&self.binding.target.lifetime, accepted_at);
        if !frame_admitted(
            source_epoch,
            current,
            frame.decoded_at(),
            accepted_at,
            Instant::now(),
        ) {
            self.release();
            return Ok(false);
        }
        let mut latest = match self.frame.lock() {
            Ok(latest) => latest,
            Err(_) => {
                self.lease.revoke();
                return Err(MediaError::Backend);
            }
        };
        if latest
            .as_ref()
            .is_some_and(|previous| previous.decoded_at() > frame.decoded_at())
        {
            return Ok(false);
        }
        self.lease
            .decoded_p2p_frame(viewport, frame.pixels(), frame.decoded_at());
        let previous = latest.replace(frame);
        drop(latest);
        drop(previous);
        if self.actor.is_none() && self.lease.is_ready() {
            let binding = &self.binding;
            let target = binding.target.clone();
            let lifetime = target.lifetime.clone();
            let channel = target.channel.clone();
            let intents = binding.intents.clone();
            let ticket = binding.ticket;
            let generations = binding.generation.clone();
            let generation = self.generation;
            let permitted = self.permitted.clone();
            let frames = self.frame.clone();
            let actor = CursorActor::start(
                self.cleanup.clone(),
                self.lease.clone(),
                self.voice.clone(),
                move || {
                    let frame = frames.lock().ok().and_then(|frame| frame.clone());
                    !permitted.load(Ordering::Acquire)
                        || !lifetime.is_alive()
                        || !channel.is_alive()
                        || !intents.is_current(ticket, IntentScope::Voice)
                        || generations.load(Ordering::Acquire) != generation
                        || !frame.is_some_and(|frame| {
                            frame_admitted(
                                None,
                                frame.is_current_since(&lifetime, accepted_at),
                                frame.decoded_at(),
                                accepted_at,
                                Instant::now(),
                            )
                        })
                },
                move || target.send_stop(),
            );
            match actor {
                Ok(actor) => self.actor = Some(actor),
                Err(error) => {
                    self.release();
                    return Err(error);
                }
            }
        }
        Ok(self.lease.is_alive())
    }

    pub fn set_confined(&self, confined: bool) {
        if self.is_alive() {
            self.lease.set_confined(confined);
        }
    }

    pub fn admitted_activity(&self) {
        if self.is_alive() {
            self.lease.activity();
        }
    }

    pub fn completion(&self) -> Option<CursorCompletion> {
        self.actor.as_ref().map(CursorActor::completion)
    }

    pub fn release(&self) {
        self.permitted.store(false, Ordering::Release);
        self.lease.revoke();
        if let Some(actor) = &self.actor {
            actor.stop();
        } else if !self.released.swap(true, Ordering::AcqRel) {
            self.binding.target.send_stop();
        }
    }
}

impl Drop for ViewerDesktop {
    fn drop(&mut self) {
        self.release();
    }
}

pub struct HostBinding {
    pub intents: Arc<CriticalIntents>,
    pub ticket: IntentTicket,
    pub target: ControlTarget,
    pub capture: Arc<CaptureLease>,
    pub consent: bool,
    pub voice: CancellationToken,
    pub authorized: Arc<AtomicBool>,
}

trait PillBackend {
    fn size(&self) -> Option<(i32, i32)>;
    fn present(&mut self, bounds: Rect, authorized: &dyn Fn() -> bool) -> Result<(), InputError>;
    fn finish(&mut self) -> Result<(), InputError>;
}

struct SlintPill<C: ComponentHandle> {
    surface: HostSurface,
    pill: C,
}

impl<C: ComponentHandle> PillBackend for SlintPill<C> {
    fn size(&self) -> Option<(i32, i32)> {
        let rect = physical_viewport(
            [0.0, 0.0, 160.0, 32.0],
            f64::from(self.pill.window().scale_factor()),
        )?;
        Some((rect.width, rect.height))
    }

    fn present(&mut self, bounds: Rect, authorized: &dyn Fn() -> bool) -> Result<(), InputError> {
        if !authorized() {
            return Err(InputError::Unavailable);
        }
        self.surface.present(bounds, authorized)?;
        self.pill
            .window()
            .set_position(PhysicalPosition::new(bounds.left, bounds.top));
        self.pill
            .window()
            .set_size(PhysicalSize::new(bounds.width as u32, bounds.height as u32));
        self.pill.show().map_err(|_| InputError::Unavailable)?;
        self.surface.present(bounds, authorized)
    }

    fn finish(&mut self) -> Result<(), InputError> {
        let restored = self.surface.finish();
        let hidden = self.pill.hide();
        if restored.is_err() || hidden.is_err() {
            Err(InputError::Cleanup)
        } else {
            Ok(())
        }
    }
}

struct PillSurface {
    backend: Option<Box<dyn PillBackend>>,
    capture: Arc<CaptureLease>,
    authorized: Box<dyn Fn() -> bool>,
    done: Option<mpsc::Sender<Result<(), InputError>>>,
    retired: bool,
    failed: bool,
}

impl PillSurface {
    fn update(&mut self) -> Result<(), InputError> {
        if self.retired {
            return Err(InputError::Unavailable);
        }
        let bounds = self
            .capture
            .snapshot()
            .and_then(|capture| host_pill_bounds(capture, self.backend.as_ref()?.size()?));
        let Some(bounds) = bounds.filter(|_| (self.authorized)()) else {
            self.finish()?;
            return Err(InputError::Unavailable);
        };
        let result = self
            .backend
            .as_mut()
            .ok_or(InputError::Unavailable)?
            .present(bounds, &self.authorized);
        if result.is_err() {
            self.finish()?;
        }
        result
    }

    fn finish(&mut self) -> Result<(), InputError> {
        self.retired = true;
        let restored = self
            .backend
            .as_mut()
            .map_or(Ok(()), |backend| backend.finish());
        self.failed |= restored.is_err();
        if restored.is_ok() {
            drop(self.backend.take());
            self.complete();
        }
        if self.failed {
            Err(InputError::Cleanup)
        } else {
            Ok(())
        }
    }

    fn complete(&mut self) {
        if let Some(done) = self.done.take() {
            let _ = done.send(if self.failed {
                Err(InputError::Cleanup)
            } else {
                Ok(())
            });
        }
    }
}

impl Drop for PillSurface {
    fn drop(&mut self) {
        let _ = self.finish();
        drop(self.backend.take());
        self.complete();
    }
}

pub struct HostPill {
    peer: String,
    lifetime: PeerLifetime,
    channel: ChannelLease,
    permitted: Arc<AtomicBool>,
    state: Rc<RefCell<PillSurface>>,
    completion: CursorCompletion,
    timer: Timer,
}

impl HostPill {
    pub fn attach<D: SharedAudioDevice, C: ComponentHandle + 'static>(
        runtime: &SharedAudio<D>,
        binding: HostBinding,
        pill: C,
        view: &RemoteControlView,
        current: Option<&ControlTarget>,
    ) -> Result<Self, MediaError> {
        let voice = binding.voice.child_token();
        if !binding.consent
            || voice.is_cancelled()
            || !binding.target.lifetime.is_alive()
            || !binding.target.channel.is_alive()
            || binding.capture.snapshot().is_none()
            || !binding
                .intents
                .is_current(binding.ticket, IntentScope::Voice)
            || !binding.authorized.load(Ordering::Acquire)
            || view.revoked
            || view.controller != binding.target.peer
            || !current.is_some_and(|target| same_target(&binding.target, target))
        {
            return Err(MediaError::UnauthorizedPeer);
        }
        let window = pill
            .window()
            .with_winit_window(|window| match window.window_handle().ok()?.as_raw() {
                RawWindowHandle::Win32(handle) => u64::try_from(handle.hwnd.get()).ok(),
                _ => None,
            })
            .flatten()
            .ok_or(MediaError::Backend)?;
        let (done, completion) = register_surface(runtime.cleanup_tracker())?;
        let surface = match HostSurface::attach_hidden(
            window,
            binding.capture.clone(),
            completion.observation(),
        ) {
            Ok(surface) => surface,
            Err(error) => {
                let _ = done.send(if error == InputError::Cleanup {
                    Err(error)
                } else {
                    Ok(())
                });
                return Err(MediaError::Backend);
            }
        };
        let permitted = Arc::new(AtomicBool::new(true));
        let permission = permitted.clone();
        let lifetime = binding.target.lifetime.clone();
        let channel = binding.target.channel.clone();
        let intents = binding.intents.clone();
        let ticket = binding.ticket;
        let authorized = binding.authorized.clone();
        let state = Rc::new(RefCell::new(PillSurface {
            backend: Some(Box::new(SlintPill { surface, pill })),
            capture: binding.capture.clone(),
            authorized: Box::new(move || {
                permission.load(Ordering::Acquire)
                    && !voice.is_cancelled()
                    && authorized.load(Ordering::Acquire)
                    && lifetime.is_alive()
                    && channel.is_alive()
                    && intents.is_current(ticket, IntentScope::Voice)
            }),
            done: Some(done),
            retired: false,
            failed: false,
        }));
        state
            .borrow_mut()
            .update()
            .map_err(|_| MediaError::Backend)?;
        let timer = Timer::default();
        let weak = Rc::downgrade(&state);
        timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
            if let Some(state) = weak.upgrade() {
                let _ = state.borrow_mut().update();
            }
        });
        Ok(Self {
            peer: binding.target.peer.clone(),
            lifetime: binding.target.lifetime.clone(),
            channel: binding.target.channel.clone(),
            permitted,
            state,
            completion,
            timer,
        })
    }

    pub fn update_control(
        &self,
        view: &RemoteControlView,
        current: Option<&ControlTarget>,
        consent: bool,
    ) -> bool {
        let accepted = self.permitted.load(Ordering::Acquire)
            && consent
            && !view.revoked
            && view.controller == self.peer
            && current.is_some_and(|target| {
                target.peer == self.peer
                    && self.lifetime.is_same(&target.lifetime)
                    && self.channel.is_same(&target.channel)
            })
            && !self.state.borrow().retired
            && (self.state.borrow().authorized)();
        if !accepted {
            let _ = self.finish();
        }
        accepted
    }

    pub fn completion(&self) -> CursorCompletion {
        self.completion.clone()
    }

    pub fn finish(&self) -> Result<(), InputError> {
        self.permitted.store(false, Ordering::Release);
        self.timer.stop();
        self.state.borrow_mut().finish()
    }
}

impl Drop for HostPill {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[cfg(test)]
mod tests;
