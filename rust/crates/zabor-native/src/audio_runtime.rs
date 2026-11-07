use crate::audio_mix::{AudioControls, AudioMixer};
use crate::media::MediaError;
use crate::mic_test::{MicTest, MicTestFailure, PlaybackTicket, IDLE_RELEASE, PREPARATION_TIMEOUT};
use crate::microphone::{MicProcessor, MicSubmission, MonitoredMicFrame};
use futures_util::future::BoxFuture;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::Instant;
use tokio::sync::{watch, Notify};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct VoiceState {
    next: u64,
    reserved: u64,
    active: u64,
}

pub struct AudioDemand {
    controls: Arc<AudioControls>,
    voice: Mutex<VoiceState>,
    active: AtomicU64,
}

impl AudioDemand {
    pub fn new(controls: Arc<AudioControls>) -> Self {
        Self {
            controls,
            voice: Mutex::new(VoiceState::default()),
            active: AtomicU64::new(0),
        }
    }

    pub fn reserve(&self) -> Option<u64> {
        let mut state = self.voice.lock().ok()?;
        if state.reserved != 0 {
            return None;
        }
        state.next = state.next.checked_add(1)?;
        state.reserved = state.next;
        Some(state.reserved)
    }

    pub fn activate(&self, generation: u64) -> bool {
        let Ok(mut state) = self.voice.lock() else {
            return false;
        };
        if generation == 0 || state.reserved != generation {
            return false;
        }
        if state.active != generation {
            self.controls.capture_restarting();
            state.active = generation;
            self.active.store(generation, Ordering::Release);
        }
        true
    }

    pub fn revoke(&self, generation: u64) {
        if let Ok(mut state) = self.voice.lock() {
            if generation != 0 && state.reserved == generation {
                state.reserved = 0;
                state.active = 0;
                self.active.store(0, Ordering::Release);
                self.controls.capture_restarting();
            }
        }
    }

    pub fn reset(&self) {
        if let Ok(mut state) = self.voice.lock() {
            state.reserved = 0;
            state.active = 0;
            self.active.store(0, Ordering::Release);
            self.controls.capture_restarting();
        }
    }

    pub fn active(&self) -> u64 {
        self.active.load(Ordering::Acquire)
    }

    pub fn has_voice(&self) -> bool {
        self.voice.lock().is_ok_and(|state| state.reserved != 0)
    }

    pub fn is_reserved(&self, generation: u64) -> bool {
        generation != 0
            && self
                .voice
                .lock()
                .is_ok_and(|state| state.reserved == generation)
    }

    pub fn with_voice<T>(&self, generation: u64, submit: impl FnOnce() -> T) -> Option<T> {
        let state = self.voice.lock().ok()?;
        if generation == 0 || state.active != generation || state.reserved != generation {
            return None;
        }
        Some(submit())
    }
}

pub trait SharedAudioDevice: Send + Sync + 'static {
    fn failed(&self) -> bool;
    fn cancel(&self);
}

type Ready<D> = Option<Result<Arc<D>, ()>>;
type Factory<D> = dyn Fn(
        Arc<AudioDemand>,
        Arc<MicTest>,
        CancellationToken,
    ) -> BoxFuture<'static, Result<D, MediaError>>
    + Send
    + Sync;

struct Pending<D> {
    stop: CancellationToken,
    ready: watch::Sender<Ready<D>>,
}

struct RuntimeState<D> {
    device: Option<Arc<D>>,
    pending: Option<Arc<Pending<D>>>,
    idle_since: Option<Instant>,
    failed: bool,
}

pub struct SharedAudio<D: SharedAudioDevice> {
    pub demand: Arc<AudioDemand>,
    pub mic_test: Arc<MicTest>,
    stop: CancellationToken,
    state: Mutex<RuntimeState<D>>,
    factory: Arc<Factory<D>>,
    changed: Notify,
    monitor: AtomicU64,
}

impl<D: SharedAudioDevice> SharedAudio<D> {
    pub fn new(
        controls: Arc<AudioControls>,
        stop: CancellationToken,
        factory: impl Fn(
                Arc<AudioDemand>,
                Arc<MicTest>,
                CancellationToken,
            ) -> BoxFuture<'static, Result<D, MediaError>>
            + Send
            + Sync
            + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            demand: Arc::new(AudioDemand::new(controls)),
            mic_test: Arc::new(MicTest::default()),
            stop,
            state: Mutex::new(RuntimeState {
                device: None,
                pending: None,
                idle_since: None,
                failed: false,
            }),
            factory: Arc::new(factory),
            changed: Notify::new(),
            monitor: AtomicU64::new(0),
        })
    }

    fn has_demand(&self) -> bool {
        self.demand.has_voice() || self.mic_test.has_demand()
    }

    fn retire(device: Arc<D>) {
        device.cancel();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn_blocking(move || drop(device));
        }
    }

    fn start(self: &Arc<Self>, retry: bool) -> Result<watch::Receiver<Ready<D>>, MediaError> {
        if self.stop.is_cancelled() || !self.has_demand() {
            return Err(MediaError::Microphone);
        }
        let mut state = self.state.lock().map_err(|_| MediaError::Microphone)?;
        state.idle_since = None;
        if let Some(device) = state.device.as_ref().filter(|device| !device.failed()) {
            return Ok(watch::channel(Some(Ok(device.clone()))).1);
        }
        if let Some(pending) = &state.pending {
            if pending.stop.is_cancelled() {
                return Err(MediaError::Microphone);
            }
            return Ok(pending.ready.subscribe());
        }
        if state.failed && !retry {
            return Err(MediaError::Microphone);
        }
        if let Some(device) = state.device.take() {
            Self::retire(device);
        }
        state.failed = false;
        let (ready, receiver) = watch::channel(None);
        let pending = Arc::new(Pending {
            stop: self.stop.child_token(),
            ready,
        });
        state.pending = Some(pending.clone());
        let shared = Arc::downgrade(self);
        let factory = self.factory.clone();
        let demand = self.demand.clone();
        let mic_test = self.mic_test.clone();
        tokio::spawn(async move {
            let future = factory(demand, mic_test, pending.stop.clone());
            tokio::pin!(future);
            let result = tokio::select! {
                biased;
                _ = pending.stop.cancelled() => {
                    pending.ready.send_replace(Some(Err(())));
                    future.await
                },
                _ = tokio::time::sleep(PREPARATION_TIMEOUT) => {
                    pending.stop.cancel();
                    pending.ready.send_replace(Some(Err(())));
                    future.await
                },
                result = &mut future => result,
            };
            let device = result.ok().map(Arc::new);
            if let Some(shared) = shared.upgrade() {
                let wanted = !pending.stop.is_cancelled() && shared.has_demand();
                if let Ok(mut state) = shared.state.lock() {
                    if state
                        .pending
                        .as_ref()
                        .is_some_and(|run| Arc::ptr_eq(run, &pending))
                    {
                        state.pending = None;
                        if wanted {
                            state.device = device.clone();
                            state.failed = device.is_none();
                            pending.ready.send_replace(Some(device.clone().ok_or(())));
                            if device.is_none() {
                                pending.stop.cancel();
                                if let Some(token) = shared.mic_test.capture_token() {
                                    shared.mic_test.fail(token, MicTestFailure::Processor);
                                }
                            }
                            shared.changed.notify_one();
                            return;
                        }
                    }
                }
            }
            pending.stop.cancel();
            pending.ready.send_replace(Some(Err(())));
            if let Some(device) = device {
                Self::retire(device);
            }
        });
        Ok(receiver)
    }

    async fn ready(self: &Arc<Self>) -> Result<Arc<D>, MediaError> {
        let mut receiver = self.start(true)?;
        loop {
            if let Some(result) = receiver.borrow_and_update().clone() {
                return result.map_err(|_| MediaError::Microphone);
            }
            tokio::select! {
                biased;
                _ = self.stop.cancelled() => return Err(MediaError::Microphone),
                result = receiver.changed() => result.map_err(|_| MediaError::Microphone)?,
            }
        }
    }

    pub async fn voice(self: &Arc<Self>) -> Result<VoiceLease<D>, MediaError> {
        let generation = self.demand.reserve().ok_or(MediaError::Microphone)?;
        let mut lease = VoiceLease {
            shared: self.clone(),
            generation,
            device: None,
        };
        self.changed.notify_one();
        lease.device = Some(self.ready().await?);
        if !self.demand.is_reserved(generation) || self.stop.is_cancelled() {
            return Err(MediaError::Microphone);
        }
        Ok(lease)
    }

    pub fn monitor(self: &Arc<Self>, visible: bool) -> MonitorLease<D> {
        let generation = self.monitor.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        self.mic_test.dispose();
        self.mic_test.configure(true, visible);
        self.changed.notify_one();
        MonitorLease {
            shared: self.clone(),
            generation,
        }
    }

    pub fn maintain(&self, now: Instant) {
        self.mic_test.snapshot(now);
        let wanted = self.has_demand() && !self.stop.is_cancelled();
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if !wanted {
            if let Some(pending) = &state.pending {
                pending.stop.cancel();
                pending.ready.send_replace(Some(Err(())));
            }
            let idle = state.idle_since.get_or_insert(now);
            if self.stop.is_cancelled() || now.saturating_duration_since(*idle) >= IDLE_RELEASE {
                let device = state.device.take();
                state.failed = false;
                drop(state);
                if let Some(device) = device {
                    Self::retire(device);
                }
            }
        } else {
            state.idle_since = None;
            if state.device.as_ref().is_some_and(|device| device.failed()) {
                let device = state.device.take();
                state.failed = true;
                drop(state);
                if let Some(token) = self.mic_test.capture_token() {
                    self.mic_test.fail(token, MicTestFailure::Processor);
                }
                if let Some(device) = device {
                    Self::retire(device);
                }
            }
        }
    }

    pub async fn run(self: Arc<Self>) {
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(50));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                _ = self.stop.cancelled() => break,
                _ = self.changed.notified() => {},
                _ = tick.tick() => {},
            }
            self.maintain(Instant::now());
            if self.has_demand() {
                let _ = self.start(false);
            }
        }
        self.reset();
    }

    pub fn reset(&self) {
        self.demand.reset();
        self.monitor.fetch_add(1, Ordering::AcqRel);
        self.mic_test.dispose();
        if let Ok(mut state) = self.state.lock() {
            if let Some(pending) = &state.pending {
                pending.stop.cancel();
                pending.ready.send_replace(Some(Err(())));
            }
            let device = state.device.take();
            state.failed = false;
            state.idle_since = None;
            drop(state);
            if let Some(device) = device {
                Self::retire(device);
            }
        }
        self.changed.notify_one();
    }
}

impl<D: SharedAudioDevice> Drop for SharedAudio<D> {
    fn drop(&mut self) {
        self.stop.cancel();
        self.reset();
    }
}

pub struct VoiceLease<D: SharedAudioDevice> {
    shared: Arc<SharedAudio<D>>,
    generation: u64,
    device: Option<Arc<D>>,
}

impl<D: SharedAudioDevice> VoiceLease<D> {
    pub fn device(&self) -> Option<&Arc<D>> {
        self.device.as_ref()
    }

    pub fn activate(&self) -> bool {
        !self.shared.stop.is_cancelled()
            && self.device.as_ref().is_some_and(|device| !device.failed())
            && self.shared.demand.activate(self.generation)
    }

    pub fn failed(&self) -> bool {
        self.shared.stop.is_cancelled()
            || !self.shared.demand.is_reserved(self.generation)
            || self.device.as_ref().is_none_or(|device| device.failed())
    }
}

impl<D: SharedAudioDevice> Drop for VoiceLease<D> {
    fn drop(&mut self) {
        self.shared.demand.revoke(self.generation);
        self.shared.maintain(Instant::now());
        self.shared.changed.notify_one();
    }
}

pub struct MonitorLease<D: SharedAudioDevice> {
    shared: Arc<SharedAudio<D>>,
    generation: u64,
}

impl<D: SharedAudioDevice> MonitorLease<D> {
    pub fn is_current(&self) -> bool {
        !self.shared.stop.is_cancelled()
            && self.shared.monitor.load(Ordering::Acquire) == self.generation
    }

    pub fn configure(&self, visible: bool) {
        if self.shared.monitor.load(Ordering::Acquire) == self.generation {
            self.shared.mic_test.configure(true, visible);
            self.shared.maintain(Instant::now());
            self.shared.changed.notify_one();
        }
    }

    pub async fn ready(&self) -> Result<Arc<D>, MediaError> {
        if self.shared.monitor.load(Ordering::Acquire) != self.generation {
            return Err(MediaError::Microphone);
        }
        let device = self.shared.ready().await?;
        if self.shared.monitor.load(Ordering::Acquire) != self.generation {
            return Err(MediaError::Microphone);
        }
        Ok(device)
    }
}

impl<D: SharedAudioDevice> Drop for MonitorLease<D> {
    fn drop(&mut self) {
        if self.shared.monitor.load(Ordering::Acquire) == self.generation {
            self.shared.mic_test.dispose();
            self.shared.maintain(Instant::now());
            self.shared.changed.notify_one();
        }
    }
}

pub struct AudioFlow {
    pub controls: Arc<AudioControls>,
    pub demand: Arc<AudioDemand>,
    pub mic_test: Arc<MicTest>,
    pub mixer: Arc<AudioMixer>,
    pub output_session: AtomicU64,
    pub capture_voice: AtomicU64,
    pub stop: CancellationToken,
}

pub struct CaptureInput<'a> {
    pub samples: &'a [f32],
    pub channels: usize,
    pub gap: bool,
}

impl AudioFlow {
    pub fn capture(
        &self,
        processor: &mut MicProcessor,
        submit: &MicSubmission,
        input: CaptureInput<'_>,
        now: Instant,
    ) -> Result<(), MediaError> {
        if self.stop.is_cancelled() {
            return Ok(());
        }
        let voice = self.demand.active();
        let token = self.mic_test.capture_token();
        if voice == 0 && token.is_none() {
            return Ok(());
        }
        let route = self.controls.input_route_epoch.load(Ordering::Acquire);
        let changed = self.capture_voice.swap(voice, Ordering::AcqRel) != voice;
        let started = Instant::now();
        let mut frame = processor.process_shared(
            input.samples,
            input.channels,
            input.gap || changed,
            token.is_some(),
            voice != 0,
        );
        if self.stop.is_cancelled() {
            return Ok(());
        }
        if self.controls.processor_failed.load(Ordering::Acquire) {
            if let Some(token) = token {
                self.mic_test.fail(token, MicTestFailure::Processor);
            }
        } else if let (Some(token), Some(local)) = (token, frame.local) {
            if frame.epoch == self.controls.capture_epoch.load(Ordering::Acquire)
                && route == self.controls.input_route_epoch.load(Ordering::Acquire)
                && route == self.controls.input_device_epoch.load(Ordering::Acquire)
            {
                let now = now + started.elapsed();
                self.mic_test.prepared(token, now);
                self.mic_test
                    .capture(token, &local, frame.local_metrics.db, now);
            }
        }
        self.submit_frame(submit, &mut frame, voice)
    }

    fn submit_frame(
        &self,
        submit: &MicSubmission,
        frame: &mut MonitoredMicFrame,
        voice: u64,
    ) -> Result<(), MediaError> {
        self.demand
            .with_voice(voice, || {
                submit.submit(&mut frame.pcm, frame.epoch)?;
                let allowed = !self.stop.is_cancelled()
                    && !self.controls.effectively_muted()
                    && !self.controls.processor_failed.load(Ordering::Acquire)
                    && self.controls.capture_epoch.load(Ordering::Acquire) == frame.epoch
                    && self.controls.input_device_epoch.load(Ordering::Acquire)
                        == self.controls.input_route_epoch.load(Ordering::Acquire);
                self.controls.mic_db.store(
                    if allowed {
                        frame.network_metrics.db
                    } else {
                        -100.0
                    }
                    .to_bits(),
                    Ordering::Release,
                );
                self.controls
                    .speaking
                    .store(allowed && frame.network_metrics.speaking, Ordering::Release);
                if self.controls.effectively_muted()
                    || self.controls.capture_epoch.load(Ordering::Acquire) != frame.epoch
                {
                    self.controls
                        .mic_db
                        .store((-100.0_f32).to_bits(), Ordering::Release);
                    self.controls.speaking.store(false, Ordering::Release);
                }
                Ok(())
            })
            .unwrap_or(Ok(()))
    }

    pub fn render(
        &self,
        remote: &mut [f32],
        local: &mut [f32],
        samples: &mut [f32],
        gain: f32,
    ) -> Option<PlaybackTicket> {
        samples.fill(0.0);
        if remote.len() != samples.len() || local.len() != samples.len() || self.stop.is_cancelled()
        {
            return None;
        }
        let route = self.output_session.load(Ordering::Acquire);
        let selected = self.controls.output_route_epoch.load(Ordering::Acquire);
        if selected != self.controls.output_device_epoch.load(Ordering::Acquire) {
            return None;
        }
        let voice = self.demand.active();
        let epoch = self.controls.playback_epoch.load(Ordering::Acquire);
        self.mixer.render_pcm(remote);
        let ticket = self.mic_test.render(local, route, gain);
        if voice == 0
            || self.demand.active() != voice
            || self.controls.effectively_deafened()
            || self.controls.playback_epoch.load(Ordering::Acquire) != epoch
        {
            remote.fill(0.0);
        }
        let ticket = ticket.filter(|ticket| self.mic_test.validate_playback(*ticket, route));
        if ticket.is_none() {
            local.fill(0.0);
        }
        for ((sample, remote), local) in samples.iter_mut().zip(remote).zip(local) {
            *sample = (*remote + *local).clamp(-1.0, 1.0);
        }
        if self.stop.is_cancelled()
            || self.output_session.load(Ordering::Acquire) != route
            || self.controls.output_route_epoch.load(Ordering::Acquire) != selected
            || self.controls.output_device_epoch.load(Ordering::Acquire) != selected
        {
            samples.fill(0.0);
            return None;
        }
        ticket
    }

    pub fn played(&self, samples: &[f32], delay: u32, route: u64, ticket: Option<PlaybackTicket>) {
        if self.stop.is_cancelled()
            || self.output_session.load(Ordering::Acquire) != route
            || self.controls.output_route_epoch.load(Ordering::Acquire)
                != self.controls.output_device_epoch.load(Ordering::Acquire)
        {
            return;
        }
        if let Some(ticket) = ticket {
            self.mic_test.played(ticket, route);
        }
        self.mixer.played(samples);
        self.controls
            .render_delay_ms
            .store(delay, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_settings::AudioSettings;
    use crate::microphone::audio_source;
    use std::time::Duration;

    fn flow() -> AudioFlow {
        let controls = Arc::new(AudioControls::new(AudioSettings {
            noise_suppression: false,
            echo_cancellation: false,
            ..AudioSettings::default()
        }));
        AudioFlow {
            demand: Arc::new(AudioDemand::new(controls.clone())),
            mixer: Arc::new(AudioMixer::new(controls.clone())),
            mic_test: Arc::new(MicTest::default()),
            controls,
            output_session: AtomicU64::new(0),
            capture_voice: AtomicU64::new(0),
            stop: CancellationToken::new(),
        }
    }

    #[test]
    fn shared_dsp_cannot_publish_network_metrics_before_current_voice_commit() {
        let flow = flow();
        let voice = flow.demand.reserve().unwrap();
        flow.demand.activate(voice);
        let mut processor = MicProcessor::new(flow.controls.clone(), flow.mixer.clone()).unwrap();
        let mut frame = processor.process_shared(&[0.2; 480], 1, false, true, true);
        assert!(frame.network_metrics.db > -20.0);
        assert_eq!(
            f32::from_bits(flow.controls.mic_db.load(Ordering::Acquire)),
            -100.0
        );
        assert!(!flow.controls.speaking.load(Ordering::Acquire));
        frame.network_metrics.speaking = true;
        let submit = MicSubmission::new(audio_source(), flow.controls.clone(), flow.stop.clone());
        flow.submit_frame(&submit, &mut frame, voice).unwrap();
        assert!(f32::from_bits(flow.controls.mic_db.load(Ordering::Acquire)) > -20.0);
        assert!(flow.controls.speaking.load(Ordering::Acquire));
    }

    #[test]
    fn old_voice_frame_cannot_publish_into_a_new_generation_even_with_a_current_capture_epoch() {
        let flow = flow();
        let old = flow.demand.reserve().unwrap();
        flow.demand.activate(old);
        let mut processor = MicProcessor::new(flow.controls.clone(), flow.mixer.clone()).unwrap();
        let mut frame = processor.process_shared(&[0.2; 480], 1, false, true, true);
        frame.network_metrics.speaking = true;
        flow.demand.revoke(old);
        let current = flow.demand.reserve().unwrap();
        flow.demand.activate(current);
        frame.epoch = flow.controls.capture_epoch.load(Ordering::Acquire);
        let submit = MicSubmission::new(audio_source(), flow.controls.clone(), flow.stop.clone());
        flow.submit_frame(&submit, &mut frame, old).unwrap();
        assert_eq!(
            f32::from_bits(flow.controls.mic_db.load(Ordering::Acquire)),
            -100.0
        );
        assert!(!flow.controls.speaking.load(Ordering::Acquire));
        assert_ne!(old, current);
    }

    #[test]
    fn voice_revocation_waits_for_the_short_final_submission_gate_not_the_dsp_thread() {
        let flow = flow();
        let voice = flow.demand.reserve().unwrap();
        flow.demand.activate(voice);
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (revoked_tx, revoked_rx) = std::sync::mpsc::channel();
        let demand = &flow.demand;
        std::thread::scope(|scope| {
            scope.spawn(move || {
                assert!(demand
                    .with_voice(voice, || {
                        entered_tx.send(()).unwrap();
                        release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                    })
                    .is_some());
            });
            entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            scope.spawn(|| {
                flow.demand.revoke(voice);
                revoked_tx.send(()).unwrap();
            });
            assert_eq!(
                revoked_rx.recv_timeout(Duration::from_millis(30)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            );
            release_tx.send(()).unwrap();
            revoked_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        });
        assert_eq!(flow.demand.active(), 0);
        assert!(flow
            .demand
            .with_voice(voice, || panic!("revoked voice submitted"))
            .is_none());
    }
}
