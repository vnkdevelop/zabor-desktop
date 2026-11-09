use crate::audio_mix::{AudioControls, AudioMixer};
use crate::media::MediaError;
use crate::mic_test::{
    CaptureToken, MicTest, MicTestDispose, MicTestFailure, PlaybackTicket, IDLE_RELEASE,
    PREPARATION_TIMEOUT,
};
use crate::microphone::{MicProcessor, MicSubmission, MonitoredMicFrame};
use futures_util::future::BoxFuture;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Arc, Mutex, MutexGuard, TryLockError,
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
        let _ = self.revoke_if_current(generation);
    }

    fn revoke_if_current(&self, generation: u64) -> Option<bool> {
        let mut state = self.voice.lock().ok()?;
        if generation == 0 || state.reserved != generation {
            return Some(false);
        }
        state.reserved = 0;
        state.active = 0;
        self.active.store(0, Ordering::Release);
        self.controls.capture_restarting();
        Some(true)
    }

    fn clear_network_status(&self, generation: u64) {
        let state = self.voice.lock();
        if generation != 0
            && state.as_ref().map_or_else(
                |_| self.active() == generation,
                |state| state.reserved == generation,
            )
        {
            self.controls.speaking.store(false, Ordering::Release);
            self.controls
                .mic_db
                .store((-100.0_f32).to_bits(), Ordering::Release);
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

#[derive(Debug, thiserror::Error)]
pub enum AudioCleanupError {
    #[error("native audio shutdown completion is unsupported")]
    Unsupported,
    #[error("native audio cleanup did not complete successfully")]
    Incomplete,
}

pub trait SharedAudioDevice: Send + Sync + 'static {
    fn failed(&self) -> bool;
    fn cancel(&self);

    fn runtime_failed(&self) -> bool {
        self.failed()
    }

    fn retry_processor(&self) -> Option<watch::Receiver<Option<bool>>> {
        None
    }

    fn processor_recovering(&self) -> bool {
        false
    }

    fn finish_shutdown(&self) -> Result<(), AudioCleanupError> {
        Err(AudioCleanupError::Unsupported)
    }
}

#[derive(Default)]
pub(crate) struct AudioCleanup {
    pending: AtomicUsize,
    failed: AtomicBool,
    changed: Notify,
    sealed: Mutex<bool>,
}

struct AudioCleanupJob {
    cleanup: Arc<AudioCleanup>,
    completed: bool,
}

pub(crate) struct AudioCleanupTask {
    job: Option<AudioCleanupJob>,
}

pub(crate) struct AudioCleanupOwner<T: Send + 'static> {
    value: Option<T>,
    retirement: Option<AudioCleanupTask>,
    runtime: tokio::runtime::Handle,
}

impl<T: Send + 'static> std::ops::Deref for AudioCleanupOwner<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.value.as_ref().unwrap()
    }
}

impl<T: Send + 'static> std::ops::DerefMut for AudioCleanupOwner<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.value.as_mut().unwrap()
    }
}

impl<T: Send + 'static> Drop for AudioCleanupOwner<T> {
    fn drop(&mut self) {
        let _runtime = self.runtime.enter();
        let value = self.value.take().unwrap();
        self.retirement
            .take()
            .unwrap()
            .spawn_blocking_result(move || {
                drop(value);
                Ok(())
            });
    }
}

impl AudioCleanup {
    fn register(self: &Arc<Self>) -> AudioCleanupJob {
        self.pending.fetch_add(1, Ordering::AcqRel);
        AudioCleanupJob {
            cleanup: self.clone(),
            completed: false,
        }
    }

    fn try_register(self: &Arc<Self>) -> Result<AudioCleanupJob, AudioCleanupError> {
        let sealed = self.sealed.lock().map_err(|_| {
            self.failed.store(true, Ordering::Release);
            AudioCleanupError::Incomplete
        })?;
        if *sealed {
            return Err(AudioCleanupError::Incomplete);
        }
        Ok(self.register())
    }

    pub(crate) fn seal(&self) -> Result<(), AudioCleanupError> {
        let mut sealed = self.sealed.lock().map_err(|_| {
            self.failed.store(true, Ordering::Release);
            AudioCleanupError::Incomplete
        })?;
        *sealed = true;
        Ok(())
    }

    pub(crate) fn prepare_task(self: &Arc<Self>) -> Result<AudioCleanupTask, AudioCleanupError> {
        Ok(AudioCleanupTask {
            job: Some(self.try_register()?),
        })
    }

    pub(crate) async fn wait(&self) -> Result<(), AudioCleanupError> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.pending.load(Ordering::Acquire) == 0 {
                return if self.failed.load(Ordering::Acquire) {
                    Err(AudioCleanupError::Incomplete)
                } else {
                    Ok(())
                };
            }
            changed.await;
        }
    }

    #[cfg(test)]
    pub(crate) fn spawn_task(
        self: &Arc<Self>,
        stop: CancellationToken,
        future: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> Result<tokio::task::AbortHandle, AudioCleanupError> {
        Ok(self.prepare_task()?.spawn(stop, future))
    }

    fn retire<D: SharedAudioDevice>(self: &Arc<Self>, device: Arc<D>) {
        let job = match self.try_register() {
            Ok(job) => job,
            Err(_) => {
                self.failed.store(true, Ordering::Release);
                device.cancel();
                let _ = device.finish_shutdown();
                drop(device);
                return;
            }
        };
        Self::retire_registered(device, job);
    }

    fn retire_registered<D: SharedAudioDevice>(device: Arc<D>, job: AudioCleanupJob) {
        device.cancel();
        let finish = move || {
            let result = device.finish_shutdown();
            drop(device);
            job.complete(result);
        };
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn_blocking(finish);
        } else {
            finish();
        }
    }
}

impl AudioCleanupTask {
    pub(crate) fn hold<T: Send + 'static>(self, value: T) -> AudioCleanupOwner<T> {
        AudioCleanupOwner {
            value: Some(value),
            retirement: Some(self),
            runtime: tokio::runtime::Handle::current(),
        }
    }

    pub(crate) fn spawn_blocking_result(
        mut self,
        work: impl FnOnce() -> Result<(), AudioCleanupError> + Send + 'static,
    ) {
        let job = self.job.take().unwrap();
        let task = tokio::task::spawn_blocking(work);
        tokio::spawn(async move {
            let result = task.await.unwrap_or(Err(AudioCleanupError::Incomplete));
            job.complete(result);
        });
    }

    pub(crate) fn spawn(
        mut self,
        stop: CancellationToken,
        future: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> tokio::task::AbortHandle {
        let job = self.job.take().unwrap();
        let task = tokio::spawn(async move {
            future.await;
        });
        let abort = task.abort_handle();
        tokio::spawn(async move {
            let result = match task.await {
                Ok(()) => Ok(()),
                Err(error) if error.is_cancelled() && stop.is_cancelled() => Ok(()),
                Err(_) => Err(AudioCleanupError::Incomplete),
            };
            drop(stop);
            job.complete(result);
        });
        abort
    }
}

impl Drop for AudioCleanupTask {
    fn drop(&mut self) {
        if let Some(job) = self.job.take() {
            job.complete(if std::thread::panicking() {
                Err(AudioCleanupError::Incomplete)
            } else {
                Ok(())
            });
        }
    }
}

impl AudioCleanupJob {
    fn retire<D: SharedAudioDevice>(&self, device: Arc<D>) {
        AudioCleanup::retire_registered(device, self.cleanup.register());
    }

    fn complete(mut self, result: Result<(), AudioCleanupError>) {
        if result.is_err() {
            self.cleanup.failed.store(true, Ordering::Release);
        }
        self.completed = true;
    }
}

impl Drop for AudioCleanupJob {
    fn drop(&mut self) {
        if !self.completed {
            self.cleanup.failed.store(true, Ordering::Release);
        }
        self.cleanup.pending.fetch_sub(1, Ordering::AcqRel);
        self.cleanup.changed.notify_waiters();
    }
}

pub struct ProcessorRetry {
    stop: CancellationToken,
    requested: AtomicBool,
    pending: Mutex<Option<watch::Sender<Option<bool>>>>,
}

impl ProcessorRetry {
    pub fn new(stop: CancellationToken) -> Self {
        Self {
            stop,
            requested: AtomicBool::new(false),
            pending: Mutex::new(None),
        }
    }

    pub fn request(&self) -> Option<watch::Receiver<Option<bool>>> {
        let mut pending = self.pending.lock().ok()?;
        if self.stop.is_cancelled() {
            return None;
        }
        if let Some(ready) = pending.as_ref().filter(|ready| ready.borrow().is_none()) {
            return Some(ready.subscribe());
        }
        let (ready, receiver) = watch::channel(None);
        *pending = Some(ready);
        self.requested.store(true, Ordering::Release);
        Some(receiver)
    }

    pub fn take(&self) -> Option<watch::Sender<Option<bool>>> {
        let pending = self.pending.lock().ok()?;
        if self.stop.is_cancelled() || !self.requested.swap(false, Ordering::AcqRel) {
            return None;
        }
        pending.clone()
    }

    pub fn recovering(&self) -> bool {
        !self.stop.is_cancelled()
            && self.pending.lock().is_ok_and(|pending| {
                pending
                    .as_ref()
                    .is_some_and(|ready| ready.borrow().is_none())
            })
    }

    pub fn cancel(&self) {
        self.stop.cancel();
        if let Ok(pending) = self.pending.lock() {
            if let Some(ready) = &*pending {
                ready.send_replace(Some(false));
            }
        }
    }
}

type Ready<D> = Option<Result<Arc<D>, ()>>;
type Prepared<D> = (
    watch::Receiver<Ready<D>>,
    Option<watch::Receiver<Option<bool>>>,
);
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
    revoked: AtomicBool,
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
    cleanup: Arc<AudioCleanup>,
}

pub struct AudioRuntimeReset<'a, D: SharedAudioDevice> {
    shared: &'a SharedAudio<D>,
    voice: Option<MutexGuard<'a, VoiceState>>,
    mic_test: Option<MicTestDispose<'a>>,
    state: Option<MutexGuard<'a, RuntimeState<D>>>,
    pending: Option<Arc<Pending<D>>>,
    device: Option<Arc<D>>,
    complete: bool,
    applied: bool,
}

impl<D: SharedAudioDevice> AudioRuntimeReset<'_, D> {
    pub fn apply(&mut self) {
        if self.applied {
            return;
        }
        if let Some(voice) = &mut self.voice {
            voice.reserved = 0;
            voice.active = 0;
            self.shared.demand.active.store(0, Ordering::Release);
            self.shared.demand.controls.capture_restarting();
        }
        self.shared.monitor.fetch_add(1, Ordering::AcqRel);
        if let Some(mic_test) = &mut self.mic_test {
            mic_test.apply();
        }
        if let Some(state) = &mut self.state {
            if let Some(pending) = &self.pending {
                pending.revoked.store(true, Ordering::Release);
            }
            std::mem::swap(&mut state.device, &mut self.device);
            state.failed = false;
            state.idle_since = None;
        }
        self.applied = true;
    }

    pub fn resources_complete(&self) -> bool {
        self.complete
    }

    pub fn release_resources(&mut self) {
        drop(self.state.take());
        drop(self.mic_test.take());
        drop(self.voice.take());
    }
}

impl<D: SharedAudioDevice> Drop for AudioRuntimeReset<'_, D> {
    fn drop(&mut self) {
        self.release_resources();
        if self.applied {
            if let Some(pending) = &self.pending {
                pending.stop.cancel();
                pending.ready.send_replace(Some(Err(())));
            }
            if let Some(device) = self.device.take() {
                self.shared.cleanup.retire(device);
            }
            self.shared.changed.notify_one();
        }
    }
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
            cleanup: Arc::new(AudioCleanup::default()),
        })
    }

    fn has_demand(&self) -> bool {
        self.demand.has_voice() || self.mic_test.has_demand()
    }

    pub async fn wait_for_cleanup(&self) -> Result<(), AudioCleanupError> {
        self.cleanup.wait().await
    }

    pub fn seal_cleanup(&self) -> Result<(), AudioCleanupError> {
        self.cleanup.seal()
    }

    pub(crate) fn cleanup_tracker(&self) -> Arc<AudioCleanup> {
        self.cleanup.clone()
    }

    fn start(self: &Arc<Self>, retry: bool) -> Result<Prepared<D>, MediaError> {
        if self.stop.is_cancelled() || !self.has_demand() {
            return Err(MediaError::Microphone);
        }
        let mut state = self.state.lock().map_err(|_| MediaError::Microphone)?;
        state.idle_since = None;
        if let Some(device) = state
            .device
            .as_ref()
            .filter(|device| !device.runtime_failed())
        {
            let recovery = if retry && (device.failed() || device.processor_recovering()) {
                Some(device.retry_processor().ok_or(MediaError::Microphone)?)
            } else {
                None
            };
            return Ok((watch::channel(Some(Ok(device.clone()))).1, recovery));
        }
        if let Some(pending) = &state.pending {
            if pending.revoked.load(Ordering::Acquire) || pending.stop.is_cancelled() {
                return Err(MediaError::Microphone);
            }
            return Ok((pending.ready.subscribe(), None));
        }
        if state.failed && !retry {
            return Err(MediaError::Microphone);
        }
        let factory_job = self
            .cleanup
            .try_register()
            .map_err(|_| MediaError::Microphone)?;
        let retired = state.device.take();
        state.failed = false;
        let (ready, receiver) = watch::channel(None);
        let pending = Arc::new(Pending {
            stop: self.stop.child_token(),
            ready,
            revoked: AtomicBool::new(false),
        });
        state.pending = Some(pending.clone());
        let shared = Arc::downgrade(self);
        let factory = self.factory.clone();
        let demand = self.demand.clone();
        let mic_test = self.mic_test.clone();
        drop(state);
        if let Some(device) = retired {
            factory_job.retire(device);
        }
        tokio::spawn(async move {
            let factory_cleanup = &factory_job;
            async move {
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
                    let publish = shared.state.lock().is_ok_and(|state| {
                        state
                            .pending
                            .as_ref()
                            .is_some_and(|run| Arc::ptr_eq(run, &pending))
                            && !pending.revoked.load(Ordering::Acquire)
                            && !pending.stop.is_cancelled()
                            && !shared.stop.is_cancelled()
                            && shared.has_demand()
                    });
                    let failure_token = if publish && device.is_none() {
                        shared.mic_test.capture_token()
                    } else {
                        None
                    };
                    let accepted = if publish {
                        pending.ready.send_replace(Some(device.clone().ok_or(())));
                        shared.state.lock().is_ok_and(|mut state| {
                            if state
                                .pending
                                .as_ref()
                                .is_some_and(|run| Arc::ptr_eq(run, &pending))
                                && !pending.revoked.load(Ordering::Acquire)
                                && !pending.stop.is_cancelled()
                                && !shared.stop.is_cancelled()
                                && shared.has_demand()
                            {
                                state.device = device.clone();
                                state.failed = device.is_none();
                                if device.is_some() {
                                    state.pending = None;
                                }
                                true
                            } else {
                                false
                            }
                        })
                    } else {
                        false
                    };
                    if accepted && device.is_some() {
                        shared.changed.notify_one();
                        return;
                    }
                    if !accepted {
                        pending.revoked.store(true, Ordering::Release);
                    }
                    pending.stop.cancel();
                    if !accepted {
                        pending.ready.send_replace(Some(Err(())));
                    } else if let Some(token) = failure_token {
                        shared.mic_test.fail(token, MicTestFailure::Processor);
                    }
                    if let Some(device) = device {
                        factory_cleanup.retire(device);
                    }
                    if let Ok(mut state) = shared.state.lock() {
                        if state
                            .pending
                            .as_ref()
                            .is_some_and(|run| Arc::ptr_eq(run, &pending))
                        {
                            state.pending = None;
                        }
                    }
                    shared.changed.notify_one();
                    return;
                }
                pending.revoked.store(true, Ordering::Release);
                pending.stop.cancel();
                pending.ready.send_replace(Some(Err(())));
                if let Some(device) = device {
                    factory_cleanup.retire(device);
                }
            }
            .await;
            factory_job.complete(Ok(()));
        });
        Ok((receiver, None))
    }

    async fn ready(
        self: &Arc<Self>,
        receiver: Result<Prepared<D>, MediaError>,
    ) -> Result<Arc<D>, MediaError> {
        let (mut receiver, mut recovery) = receiver?;
        loop {
            let result = receiver.borrow_and_update().clone();
            if let Some(result) = result {
                let device = result.map_err(|_| MediaError::Microphone)?;
                if let Some(mut recovery) = recovery.take() {
                    let wait = async {
                        loop {
                            let result = *recovery.borrow_and_update();
                            if let Some(result) = result {
                                return if result {
                                    Ok(())
                                } else {
                                    Err(MediaError::Microphone)
                                };
                            }
                            recovery
                                .changed()
                                .await
                                .map_err(|_| MediaError::Microphone)?;
                        }
                    };
                    tokio::select! {
                        biased;
                        _ = self.stop.cancelled() => return Err(MediaError::Microphone),
                        result = tokio::time::timeout(PREPARATION_TIMEOUT, wait) => {
                            result.map_err(|_| MediaError::Microphone)??;
                        },
                    }
                }
                return if !self.stop.is_cancelled() && !device.failed() {
                    Ok(device)
                } else {
                    Err(MediaError::Microphone)
                };
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
        lease.device = Some(self.ready(self.start(true)).await?);
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
            let pending = state.pending.clone();
            if let Some(pending) = &pending {
                pending.revoked.store(true, Ordering::Release);
            }
            let idle = state.idle_since.get_or_insert(now);
            let device = if self.stop.is_cancelled()
                || now.saturating_duration_since(*idle) >= IDLE_RELEASE
            {
                let device = state.device.take();
                state.failed = false;
                device
            } else {
                None
            };
            drop(state);
            if let Some(pending) = pending {
                pending.stop.cancel();
                pending.ready.send_replace(Some(Err(())));
            }
            if let Some(device) = device {
                self.cleanup.retire(device);
            }
        } else {
            state.idle_since = None;
            if state
                .device
                .as_ref()
                .is_some_and(|device| device.runtime_failed())
            {
                let device = state.device.take();
                state.failed = true;
                drop(state);
                if let Some(token) = self.mic_test.capture_token() {
                    self.mic_test.fail(token, MicTestFailure::Processor);
                }
                if let Some(device) = device {
                    self.cleanup.retire(device);
                }
            } else if state
                .device
                .as_ref()
                .is_some_and(|device| device.failed() && !device.processor_recovering())
            {
                drop(state);
                if let Some(token) = self.mic_test.capture_token() {
                    self.mic_test.fail(token, MicTestFailure::Processor);
                }
            }
        }
    }

    fn maintain_after_voice(&self, generation: u64, monitor: u64, now: Instant) -> bool {
        let mut state = self.state.lock().ok();
        let mut mic_test = self.mic_test.prepare_maintenance(now);
        let wanted = mic_test
            .as_ref()
            .is_some_and(|mic_test| mic_test.has_demand())
            && !self.stop.is_cancelled();
        let device = state.as_ref().and_then(|state| state.device.as_ref());
        let runtime_failed = wanted && device.is_some_and(|device| device.runtime_failed());
        let processor_failed = wanted
            && (runtime_failed
                || device.is_some_and(|device| device.failed() && !device.processor_recovering()));
        let voice = match self.demand.voice.lock() {
            Ok(voice) => voice,
            Err(error) => {
                drop(error);
                drop(mic_test);
                drop(state);
                self.maintain(now);
                return true;
            }
        };
        if generation == 0
            || voice.next != generation
            || voice.reserved != 0
            || self.monitor.load(Ordering::Acquire) != monitor
        {
            return false;
        }
        let wanted = wanted && !self.stop.is_cancelled();
        if let Some(mic_test) = &mut mic_test {
            mic_test.apply(wanted && state.is_some() && processor_failed);
        }
        let mut retired = None;
        let mut pending = None;
        if let Some(state) = &mut state {
            if !wanted {
                if let Some(start) = &state.pending {
                    start.revoked.store(true, Ordering::Release);
                    pending = Some(start.clone());
                }
                let idle = state.idle_since.get_or_insert(now);
                if self.stop.is_cancelled() || now.saturating_duration_since(*idle) >= IDLE_RELEASE
                {
                    retired = state.device.take();
                    state.failed = false;
                }
            } else {
                state.idle_since = None;
                if runtime_failed {
                    retired = state.device.take();
                    state.failed = true;
                }
            }
        }
        drop(voice);
        drop(state);
        drop(mic_test);
        if let Some(pending) = pending {
            pending.stop.cancel();
            pending.ready.send_replace(Some(Err(())));
        }
        if let Some(device) = retired {
            self.cleanup.retire(device);
        }
        true
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
            let pending = state.pending.clone();
            if let Some(pending) = &pending {
                pending.revoked.store(true, Ordering::Release);
            }
            let device = state.device.take();
            state.failed = false;
            state.idle_since = None;
            drop(state);
            if let Some(pending) = pending {
                pending.stop.cancel();
                pending.ready.send_replace(Some(Err(())));
            }
            if let Some(device) = device {
                self.cleanup.retire(device);
            }
        }
        self.changed.notify_one();
    }

    pub fn try_prepare_reset(&self) -> Result<Option<AudioRuntimeReset<'_, D>>, &'static str> {
        let voice = match self.demand.voice.try_lock() {
            Ok(voice) => Some(voice),
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Poisoned(_)) => None,
        };
        let mic_test = match self.mic_test.try_prepare_dispose() {
            Ok(None) => return Ok(None),
            Ok(mic_test) => mic_test,
            Err(_) => None,
        };
        let state = match self.state.try_lock() {
            Ok(state) => Some(state),
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Poisoned(_)) => None,
        };
        Ok(Some(AudioRuntimeReset {
            shared: self,
            complete: voice.is_some() && mic_test.is_some() && state.is_some(),
            voice,
            mic_test,
            pending: state.as_ref().and_then(|state| state.pending.clone()),
            state,
            device: None,
            applied: false,
        }))
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

    pub(crate) fn clear_network_status(&self) {
        self.shared.demand.clear_network_status(self.generation);
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
        let monitor = self.shared.monitor.load(Ordering::Acquire);
        let maintained = match self.shared.demand.revoke_if_current(self.generation) {
            Some(false) => false,
            Some(true) => {
                self.shared
                    .maintain_after_voice(self.generation, monitor, Instant::now())
            }
            None => {
                self.shared.maintain(Instant::now());
                true
            }
        };
        if maintained {
            self.shared.changed.notify_one();
        }
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

    pub fn failed(&self) -> bool {
        !self.is_current()
            || self.shared.state.lock().map_or(true, |state| {
                state.failed || state.device.as_ref().is_some_and(|device| device.failed())
            })
    }

    pub fn playback_available(&self) -> bool {
        self.is_current()
            && self.shared.state.lock().is_ok_and(|state| {
                state
                    .device
                    .as_ref()
                    .is_some_and(|device| !device.runtime_failed())
            })
    }

    pub fn ready(&self) -> BoxFuture<'static, Result<Arc<D>, MediaError>> {
        let receiver = if self.is_current() {
            self.shared.start(true)
        } else {
            Err(MediaError::Microphone)
        };
        let shared = self.shared.clone();
        let generation = self.generation;
        Box::pin(async move {
            let device = shared.ready(receiver).await?;
            if shared.stop.is_cancelled() || shared.monitor.load(Ordering::Acquire) != generation {
                return Err(MediaError::Microphone);
            }
            Ok(device)
        })
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
        if self.controls.processor_failed.load(Ordering::Acquire) {
            return Ok(());
        }
        let voice = self.demand.active();
        let token = self.mic_test.capture_token();
        if voice == 0 && token.is_none() {
            return Ok(());
        }
        let route = self.controls.input_route_epoch.load(Ordering::Acquire);
        let epoch = self.controls.capture_epoch.load(Ordering::Acquire);
        let changed = self.capture_voice.swap(voice, Ordering::AcqRel) != voice;
        let started = Instant::now();
        let mut frame = processor.process_shared(
            input.samples,
            input.channels,
            input.gap || changed,
            token.is_some(),
            voice != 0,
        );
        if frame.epoch != epoch
            || !self.publish_capture_health(voice, token, epoch, route, processor.failed())
        {
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

    fn publish_capture_health(
        &self,
        voice: u64,
        token: Option<CaptureToken>,
        epoch: u64,
        route: u64,
        failed: bool,
    ) -> bool {
        let Ok(state) = self.demand.voice.lock() else {
            return false;
        };
        if state.active != voice || (voice != 0 && state.reserved != voice) {
            return false;
        }
        let publish = || {
            if self.stop.is_cancelled()
                || self.controls.capture_epoch.load(Ordering::Acquire) != epoch
                || self.controls.input_route_epoch.load(Ordering::Acquire) != route
                || self.controls.input_device_epoch.load(Ordering::Acquire) != route
            {
                return false;
            }
            self.controls
                .processor_failed
                .store(failed, Ordering::Release);
            true
        };
        if voice != 0 {
            publish()
        } else {
            token
                .and_then(|token| self.mic_test.with_capture_token(token, publish))
                .unwrap_or(false)
        }
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
    use crate::critical_intents::{CriticalAction, CriticalIntents, IntentScope};
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
    fn shared_processor_success_and_failure_do_not_publish_before_original_capture_commit() {
        let flow = flow();
        let mut processor = MicProcessor::new(flow.controls.clone(), flow.mixer.clone()).unwrap();
        flow.controls.speaking.store(true, Ordering::Release);
        flow.controls
            .mic_db
            .store((-17.0_f32).to_bits(), Ordering::Release);
        flow.controls
            .processor_failed
            .store(true, Ordering::Release);
        let frame = processor.process_shared(&[0.2; 480], 1, false, true, true);
        assert!(frame.pcm.iter().any(|sample| *sample != 0));
        assert!(flow.controls.processor_failed.load(Ordering::Acquire));
        flow.controls
            .processor_failed
            .store(false, Ordering::Release);
        let frame = processor.process_shared(&[f32::NAN; 480], 1, false, true, true);
        assert_eq!(frame.pcm, [0; 480]);
        assert!(frame.local.is_none() && processor.failed());
        assert!(!flow.controls.processor_failed.load(Ordering::Acquire));
        assert!(flow.controls.speaking.load(Ordering::Acquire));
        assert_eq!(
            f32::from_bits(flow.controls.mic_db.load(Ordering::Acquire)),
            -17.0
        );
    }

    #[test]
    fn shared_capture_health_rejects_replaced_voice_epoch_and_original_monitor_token() {
        let flow = flow();
        let voice = flow.demand.reserve().unwrap();
        assert!(flow.demand.activate(voice));
        let old_epoch = flow.controls.capture_epoch.load(Ordering::Acquire);
        flow.demand.revoke(voice);
        let current = flow.demand.reserve().unwrap();
        assert!(flow.demand.activate(current));
        let epoch = flow.controls.capture_epoch.load(Ordering::Acquire);
        assert!(!flow.publish_capture_health(voice, None, epoch, 0, true));
        assert!(!flow.publish_capture_health(current, None, old_epoch, 0, true));
        assert!(!flow.controls.processor_failed.load(Ordering::Acquire));
        assert!(flow.publish_capture_health(current, None, epoch, 0, true));
        assert!(flow.controls.processor_failed.load(Ordering::Acquire));
        flow.demand.revoke(current);
        flow.controls
            .processor_failed
            .store(false, Ordering::Release);
        flow.mic_test.configure(true, true);
        let old = flow.mic_test.capture_token().unwrap();
        flow.mic_test.dispose();
        flow.mic_test.configure(true, true);
        let fresh = flow.mic_test.capture_token().unwrap();
        let epoch = flow.controls.capture_epoch.load(Ordering::Acquire);
        assert_ne!(old, fresh);
        assert!(!flow.publish_capture_health(0, Some(old), epoch, 0, true));
        assert!(!flow.controls.processor_failed.load(Ordering::Acquire));
        assert!(flow.publish_capture_health(0, Some(fresh), epoch, 0, true));
        assert!(flow.controls.processor_failed.load(Ordering::Acquire));
    }

    #[test]
    fn actual_shared_capture_reports_current_local_failure_without_voice_or_network_submission() {
        let flow = flow();
        let now = Instant::now();
        flow.mic_test.configure(true, true);
        let token = flow.mic_test.begin(now).unwrap();
        let mut processor = MicProcessor::new(flow.controls.clone(), flow.mixer.clone()).unwrap();
        let submit = MicSubmission::new(audio_source(), flow.controls.clone(), flow.stop.clone());
        flow.capture(
            &mut processor,
            &submit,
            CaptureInput {
                samples: &[f32::NAN; 480],
                channels: 1,
                gap: false,
            },
            now,
        )
        .unwrap();
        assert!(flow.controls.processor_failed.load(Ordering::Acquire));
        assert!(!flow.controls.speaking.load(Ordering::Acquire));
        assert_eq!(flow.demand.active(), 0);
        assert_eq!(
            flow.mic_test.snapshot(now).failure,
            Some(MicTestFailure::Processor)
        );
        flow.mic_test.dispose();
        flow.mic_test.configure(true, true);
        assert_ne!(flow.mic_test.capture_token(), Some(token));
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

    #[derive(Default)]
    struct BoundaryObservation {
        blocked: AtomicBool,
        cancels: AtomicUsize,
        joins: AtomicUsize,
        drops: AtomicUsize,
        wakes: AtomicUsize,
        resets: AtomicUsize,
    }

    impl BoundaryObservation {
        fn inspect(&self, shared: &std::sync::Weak<SharedAudio<BoundaryDevice>>) -> bool {
            let available = shared
                .upgrade()
                .is_none_or(|shared| shared.state.try_lock().is_ok());
            if !available {
                self.blocked.store(true, Ordering::Release);
            }
            available
        }
    }

    struct BoundaryDevice {
        shared: std::sync::Weak<SharedAudio<Self>>,
        observed: Arc<BoundaryObservation>,
        failed: bool,
    }

    impl SharedAudioDevice for BoundaryDevice {
        fn failed(&self) -> bool {
            self.failed
        }

        fn cancel(&self) {
            self.observed.inspect(&self.shared);
            self.observed.cancels.fetch_add(1, Ordering::AcqRel);
        }

        fn finish_shutdown(&self) -> Result<(), AudioCleanupError> {
            self.observed.inspect(&self.shared);
            self.observed.joins.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }
    }

    impl Drop for BoundaryDevice {
        fn drop(&mut self) {
            self.observed.inspect(&self.shared);
            self.observed.drops.fetch_add(1, Ordering::AcqRel);
        }
    }

    struct BoundaryWake {
        shared: std::sync::Weak<SharedAudio<BoundaryDevice>>,
        observed: Arc<BoundaryObservation>,
        reset: AtomicBool,
    }

    impl std::task::Wake for BoundaryWake {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.observed.wakes.fetch_add(1, Ordering::AcqRel);
            if self.observed.inspect(&self.shared) && self.reset.swap(false, Ordering::AcqRel) {
                if let Some(shared) = self.shared.upgrade() {
                    shared.reset();
                    self.observed.resets.fetch_add(1, Ordering::AcqRel);
                }
            }
        }
    }

    struct BoundaryFixture {
        shared: Arc<SharedAudio<BoundaryDevice>>,
        observed: Arc<BoundaryObservation>,
        entered: Arc<tokio::sync::Semaphore>,
        released: Arc<tokio::sync::Semaphore>,
    }

    fn boundary_runtime(fail: bool) -> BoundaryFixture {
        let observed = Arc::new(BoundaryObservation::default());
        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let released = Arc::new(tokio::sync::Semaphore::new(0));
        let owner = Arc::new(Mutex::new(std::sync::Weak::new()));
        let factory_owner = owner.clone();
        let factory_observed = observed.clone();
        let factory_entered = entered.clone();
        let factory_released = released.clone();
        let shared = SharedAudio::new(
            Arc::new(AudioControls::new(AudioSettings::default())),
            CancellationToken::new(),
            move |_, _, _| {
                let owner = factory_owner.clone();
                let observed = factory_observed.clone();
                let entered = factory_entered.clone();
                let released = factory_released.clone();
                Box::pin(async move {
                    entered.add_permits(1);
                    released.acquire().await.unwrap().forget();
                    if fail {
                        Err(MediaError::Microphone)
                    } else {
                        Ok(BoundaryDevice {
                            shared: owner.lock().unwrap().clone(),
                            observed,
                            failed: false,
                        })
                    }
                })
            },
        );
        *owner.lock().unwrap() = Arc::downgrade(&shared);
        BoundaryFixture {
            shared,
            observed,
            entered,
            released,
        }
    }

    #[tokio::test]
    async fn explicit_restart_releases_runtime_lock_before_original_device_cancel_join_and_drop() {
        let fixture = boundary_runtime(true);
        let shared = &fixture.shared;
        shared.demand.reserve().unwrap();
        shared.state.lock().unwrap().device = Some(Arc::new(BoundaryDevice {
            shared: Arc::downgrade(shared),
            observed: fixture.observed.clone(),
            failed: true,
        }));
        fixture.released.add_permits(1);
        let ready = shared.start(true);
        shared.seal_cleanup().unwrap();
        assert!(shared.ready(ready).await.is_err());
        tokio::time::timeout(Duration::from_secs(2), shared.wait_for_cleanup())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fixture.observed.cancels.load(Ordering::Acquire), 1);
        assert_eq!(fixture.observed.joins.load(Ordering::Acquire), 1);
        assert_eq!(fixture.observed.drops.load(Ordering::Acquire), 1);
        assert!(!fixture.observed.blocked.load(Ordering::Acquire));
        assert!(!shared.state.is_poisoned());
    }

    #[tokio::test]
    async fn pending_revocation_wakes_after_runtime_unlock_and_preserves_original_acquisition() {
        use std::future::Future;
        for mode in ["reset", "maintain"] {
            let fixture = boundary_runtime(false);
            let shared = &fixture.shared;
            let voice = shared.demand.reserve().unwrap();
            let mut ready = shared.start(true).unwrap().0;
            fixture.entered.acquire().await.unwrap().forget();
            let pending = shared.state.lock().unwrap().pending.clone().unwrap();
            let waker = std::task::Waker::from(Arc::new(BoundaryWake {
                shared: Arc::downgrade(shared),
                observed: fixture.observed.clone(),
                reset: AtomicBool::new(false),
            }));
            let mut context = std::task::Context::from_waker(&waker);
            let mut cancelled = Box::pin(pending.stop.cancelled());
            let mut changed = Box::pin(ready.changed());
            assert!(cancelled.as_mut().poll(&mut context).is_pending());
            assert!(changed.as_mut().poll(&mut context).is_pending());
            if mode == "reset" {
                shared.reset();
            } else {
                shared.demand.revoke(voice);
                shared.maintain(Instant::now());
            }
            drop(changed);
            drop(cancelled);
            let revoked = pending.revoked.load(Ordering::Acquire);
            assert!(pending.stop.is_cancelled());
            assert!(matches!(*ready.borrow(), Some(Err(()))));
            assert!(Arc::ptr_eq(
                shared.state.lock().unwrap().pending.as_ref().unwrap(),
                &pending
            ));
            assert_eq!(shared.cleanup.pending.load(Ordering::Acquire), 1);
            let current = shared.demand.reserve().unwrap();
            assert!(shared.demand.activate(current));
            let monitor = shared.monitor(true);
            let snapshot = shared.mic_test.snapshot(Instant::now());
            assert!(shared.start(true).is_err());
            shared.seal_cleanup().unwrap();
            fixture.released.add_permits(1);
            tokio::time::timeout(Duration::from_secs(2), shared.wait_for_cleanup())
                .await
                .unwrap()
                .unwrap();
            assert!(matches!(*ready.borrow(), Some(Err(()))));
            assert!(shared.state.lock().unwrap().pending.is_none());
            assert!(shared.state.lock().unwrap().device.is_none());
            assert_eq!(fixture.observed.cancels.load(Ordering::Acquire), 1);
            assert_eq!(fixture.observed.joins.load(Ordering::Acquire), 1);
            assert_eq!(fixture.observed.drops.load(Ordering::Acquire), 1);
            assert!(fixture.observed.wakes.load(Ordering::Acquire) >= 2);
            assert!(monitor.is_current());
            assert_eq!(shared.demand.active(), current);
            assert_eq!(shared.mic_test.snapshot(Instant::now()), snapshot);
            assert!(
                revoked,
                "Original acquisition was not quarantined in {mode}"
            );
            assert!(
                !fixture.observed.blocked.load(Ordering::Acquire),
                "Runtime lock remained held during a {mode} wake"
            );
        }
    }

    #[tokio::test]
    async fn factory_readiness_wakes_after_unlock_and_reentrant_reset_cannot_accept_late_device() {
        use std::future::Future;
        for fail in [true, false] {
            let fixture = boundary_runtime(fail);
            let shared = &fixture.shared;
            let monitor = shared.monitor(true);
            let mut ready = shared.start(true).unwrap().0;
            fixture.entered.acquire().await.unwrap().forget();
            let pending = shared.state.lock().unwrap().pending.clone().unwrap();
            let waker = std::task::Waker::from(Arc::new(BoundaryWake {
                shared: Arc::downgrade(shared),
                observed: fixture.observed.clone(),
                reset: AtomicBool::new(!fail),
            }));
            let mut context = std::task::Context::from_waker(&waker);
            let mut changed = Box::pin(ready.changed());
            assert!(changed.as_mut().poll(&mut context).is_pending());
            fixture.released.add_permits(1);
            tokio::time::timeout(Duration::from_secs(2), shared.wait_for_cleanup())
                .await
                .unwrap()
                .unwrap();
            drop(changed);
            assert!(fixture.observed.wakes.load(Ordering::Acquire) >= 1);
            assert!(!fixture.observed.blocked.load(Ordering::Acquire));
            assert_eq!(
                fixture.observed.resets.load(Ordering::Acquire),
                usize::from(!fail)
            );
            assert!(matches!(*ready.borrow(), Some(Err(()))));
            assert!(shared.state.lock().unwrap().pending.is_none());
            assert!(shared.state.lock().unwrap().device.is_none());
            assert_eq!(monitor.is_current(), fail);
            assert_eq!(pending.revoked.load(Ordering::Acquire), !fail);
            assert_eq!(
                fixture.observed.joins.load(Ordering::Acquire),
                usize::from(!fail)
            );
            assert_eq!(
                fixture.observed.drops.load(Ordering::Acquire),
                usize::from(!fail)
            );
            assert!(!shared.state.is_poisoned());
        }
    }

    struct ReplacementMonitorWake {
        shared: std::sync::Weak<SharedAudio<BoundaryDevice>>,
        replacement: Mutex<Option<(MonitorLease<BoundaryDevice>, crate::mic_test::CaptureToken)>>,
        armed: AtomicBool,
        now: Instant,
    }

    impl std::task::Wake for ReplacementMonitorWake {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            if self.armed.swap(false, Ordering::AcqRel) {
                let shared = self.shared.upgrade().unwrap();
                let monitor = shared.monitor(true);
                let token = shared.mic_test.begin(self.now).unwrap();
                *self.replacement.lock().unwrap() = Some((monitor, token));
            }
        }
    }

    async fn factory_failure_preserves_replacement_monitor(cancelled_wake: bool) {
        use std::future::Future;
        let fixture = boundary_runtime(true);
        let shared = &fixture.shared;
        let now = Instant::now();
        let original = shared.monitor(true);
        let original_token = shared.mic_test.begin(now).unwrap();
        let mut ready = shared.start(true).unwrap().0;
        fixture.entered.acquire().await.unwrap().forget();
        let pending = shared.state.lock().unwrap().pending.clone().unwrap();
        let wake = Arc::new(ReplacementMonitorWake {
            shared: Arc::downgrade(shared),
            replacement: Mutex::new(None),
            armed: AtomicBool::new(true),
            now,
        });
        let waker = std::task::Waker::from(wake.clone());
        let mut context = std::task::Context::from_waker(&waker);
        let mut changed = Box::pin(ready.changed());
        let mut cancelled = Box::pin(pending.stop.cancelled());
        if cancelled_wake {
            assert!(cancelled.as_mut().poll(&mut context).is_pending());
        } else {
            assert!(changed.as_mut().poll(&mut context).is_pending());
        }
        fixture.released.add_permits(1);
        tokio::time::timeout(Duration::from_secs(2), shared.wait_for_cleanup())
            .await
            .unwrap()
            .unwrap();
        drop(changed);
        drop(cancelled);
        let (replacement, token) = wake.replacement.lock().unwrap().take().unwrap();
        assert!(!original.is_current());
        assert!(replacement.is_current());
        assert_ne!(original_token, token);
        assert!(matches!(*ready.borrow(), Some(Err(()))));
        assert!(pending.stop.is_cancelled());
        let state = shared.state.lock().unwrap();
        assert!(state.pending.is_none());
        assert!(state.device.is_none());
        assert!(state.failed);
        drop(state);
        assert!(shared.start(false).is_err());
        assert_eq!(
            shared.mic_test.capture_token(),
            Some(token),
            "Original factory failure invalidated replacement capture token"
        );
        let snapshot = shared.mic_test.snapshot(now);
        assert_eq!(snapshot.stage, crate::mic_test::MicTestStage::Arming);
        assert_eq!(snapshot.failure, None);
        assert_eq!(snapshot.issue, None);
    }

    #[tokio::test]
    async fn factory_failure_readiness_wake_cannot_fail_replacement_monitor_token() {
        factory_failure_preserves_replacement_monitor(false).await;
    }

    #[tokio::test]
    async fn factory_failure_cancellation_wake_cannot_fail_replacement_monitor_token() {
        factory_failure_preserves_replacement_monitor(true).await;
    }

    struct TaskDropProbe {
        entered: std::sync::mpsc::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
        finished: Arc<AtomicBool>,
    }

    impl Drop for TaskDropProbe {
        fn drop(&mut self) {
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(3)).unwrap();
            self.finished.store(true, Ordering::Release);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_value_retirement_stays_registered_through_seal_abort_and_held_owner_drop() {
        for mode in ["normal", "panic", "requested-abort", "unexpected-abort"] {
            let cleanup = Arc::new(AudioCleanup::default());
            let prepared = cleanup.prepare_task().unwrap();
            let retirement = cleanup.prepare_task().unwrap();
            let stop = CancellationToken::new();
            let (entered, waiting) = std::sync::mpsc::channel();
            let (release, held) = std::sync::mpsc::channel();
            let finished = Arc::new(AtomicBool::new(false));
            let value = retirement.hold(TaskDropProbe {
                entered,
                release: held,
                finished: finished.clone(),
            });
            let (started, ready) = tokio::sync::oneshot::channel();
            let task = prepared.spawn(stop.clone(), async move {
                let _value = value;
                started.send(()).unwrap();
                match mode {
                    "normal" => {}
                    "panic" => panic!("Expected task with native retirement panic"),
                    _ => std::future::pending::<()>().await,
                }
            });
            ready.await.unwrap();
            cleanup.seal().unwrap();
            if mode == "requested-abort" {
                stop.cancel();
            }
            if mode.ends_with("abort") {
                task.abort();
            }
            tokio::task::spawn_blocking(move || {
                waiting.recv_timeout(std::time::Duration::from_secs(2))
            })
            .await
            .unwrap()
            .unwrap();
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(30), cleanup.wait())
                    .await
                    .is_err()
            );
            assert!(!finished.load(Ordering::Acquire));
            release.send(()).unwrap();
            let result = tokio::time::timeout(std::time::Duration::from_secs(2), cleanup.wait())
                .await
                .unwrap();
            assert_eq!(
                result.is_err(),
                matches!(mode, "panic" | "unexpected-abort")
            );
            assert!(finished.load(Ordering::Acquire));
            assert_eq!(cleanup.wait().await.is_err(), result.is_err());
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn registered_native_value_drop_panic_is_sticky_even_after_original_task_succeeds() {
        let cleanup = Arc::new(AudioCleanup::default());
        let prepared = cleanup.prepare_task().unwrap();
        let value = cleanup.prepare_task().unwrap().hold(TaskPanicDrop(true));
        let task = prepared.spawn(CancellationToken::new(), async move {
            let _value = value;
        });
        cleanup.seal().unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(2), cleanup.wait())
                .await
                .unwrap()
                .is_err()
        );
        assert!(task.is_finished());
        assert_eq!(cleanup.pending.load(Ordering::Acquire), 0);
        assert!(cleanup.wait().await.is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tracked_task_join_waits_for_started_capture_destruction_and_keeps_concurrent_waiters()
    {
        for abort_task in [false, true] {
            let cleanup = Arc::new(AudioCleanup::default());
            let stop = CancellationToken::new();
            let started = Arc::new(tokio::sync::Semaphore::new(0));
            let release = Arc::new(tokio::sync::Semaphore::new(0));
            let (drop_entered, drop_waiting) = std::sync::mpsc::channel();
            let (drop_release, drop_blocked) = std::sync::mpsc::channel();
            let finished = Arc::new(AtomicBool::new(false));
            let probe = TaskDropProbe {
                entered: drop_entered,
                release: drop_blocked,
                finished: finished.clone(),
            };
            let task_started = started.clone();
            let task_release = release.clone();
            let task = cleanup
                .spawn_task(stop.clone(), async move {
                    let _probe = probe;
                    task_started.add_permits(1);
                    task_release.acquire().await.unwrap().forget();
                })
                .unwrap();
            started.acquire().await.unwrap().forget();
            assert_eq!(cleanup.pending.load(Ordering::Acquire), 1);
            assert!(
                tokio::time::timeout(Duration::from_millis(30), cleanup.wait())
                    .await
                    .is_err()
            );
            let first_cleanup = cleanup.clone();
            let second_cleanup = cleanup.clone();
            let first = tokio::spawn(async move { first_cleanup.wait().await });
            let second = tokio::spawn(async move { second_cleanup.wait().await });
            if abort_task {
                stop.cancel();
                task.abort();
            } else {
                release.add_permits(1);
            }
            tokio::task::spawn_blocking(move || drop_waiting.recv_timeout(Duration::from_secs(1)))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(cleanup.pending.load(Ordering::Acquire), 1);
            assert!(!finished.load(Ordering::Acquire));
            assert!(!first.is_finished());
            assert!(!second.is_finished());
            drop_release.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(1), first)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            tokio::time::timeout(Duration::from_secs(1), second)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(finished.load(Ordering::Acquire));
            assert!(task.is_finished());
            assert_eq!(cleanup.pending.load(Ordering::Acquire), 0);
            assert!(cleanup.wait().await.is_ok());
            println!("audio task cleanup started=true abort={abort_task} held_capture_drop=true join_after_destruction=true abandoned_waiter_preserved=true concurrent_waiters=2 physical_audio=false");
        }
    }

    #[test]
    fn tracked_task_keeps_unpolled_captures_registered_and_reports_abandoned_runtime_observation() {
        for abandon_runtime in [false, true] {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let cleanup = Arc::new(AudioCleanup::default());
            let stop = CancellationToken::new();
            let polled = Arc::new(AtomicBool::new(false));
            let finished = Arc::new(AtomicBool::new(false));
            let (drop_entered, drop_waiting) = std::sync::mpsc::channel();
            let (drop_release, drop_blocked) = std::sync::mpsc::channel();
            let probe = TaskDropProbe {
                entered: drop_entered,
                release: drop_blocked,
                finished: finished.clone(),
            };
            let task_polled = polled.clone();
            let task = {
                let _enter = runtime.enter();
                cleanup
                    .spawn_task(stop.clone(), async move {
                        let _probe = probe;
                        task_polled.store(true, Ordering::Release);
                        std::future::pending::<()>().await;
                    })
                    .unwrap()
            };
            assert_eq!(cleanup.pending.load(Ordering::Acquire), 1);
            stop.cancel();
            task.abort();
            let inspected = cleanup.clone();
            std::thread::scope(|scope| {
                scope.spawn(move || {
                    drop_waiting.recv_timeout(Duration::from_secs(1)).unwrap();
                    if !abandon_runtime {
                        assert_eq!(inspected.pending.load(Ordering::Acquire), 1);
                    }
                    drop_release.send(()).unwrap();
                });
                if abandon_runtime {
                    drop(runtime);
                    assert!(cleanup.failed.load(Ordering::Acquire));
                } else {
                    runtime.block_on(async {
                        tokio::time::timeout(Duration::from_secs(1), cleanup.wait())
                            .await
                            .unwrap()
                            .unwrap();
                    });
                    drop(runtime);
                    assert!(!cleanup.failed.load(Ordering::Acquire));
                }
            });
            assert!(!polled.load(Ordering::Acquire));
            assert!(finished.load(Ordering::Acquire));
            assert_eq!(cleanup.pending.load(Ordering::Acquire), 0);
            assert_eq!(cleanup.failed.load(Ordering::Acquire), abandon_runtime);
            println!("audio task cleanup unpolled=true abandoned_runtime={abandon_runtime} captures_destroyed=true unobserved_join_not_accepted=true physical_audio=false");
        }
    }

    struct TaskPanicDrop(bool);

    impl Drop for TaskPanicDrop {
        fn drop(&mut self) {
            assert!(!self.0, "Expected tracked task capture destruction panic");
        }
    }

    #[tokio::test]
    async fn tracked_task_accepts_only_observed_completion_or_requested_abort_and_keeps_panic_failure(
    ) {
        for mode in [
            "complete",
            "expected-abort",
            "unsolicited-abort",
            "body-panic",
            "drop-panic",
            "cancel-drop-panic",
        ] {
            let cleanup = Arc::new(AudioCleanup::default());
            let stop = CancellationToken::new();
            let started = Arc::new(tokio::sync::Semaphore::new(0));
            let task_started = started.clone();
            let task = cleanup
                .spawn_task(stop.clone(), async move {
                    let _probe = TaskPanicDrop(mode == "drop-panic" || mode == "cancel-drop-panic");
                    task_started.add_permits(1);
                    if mode.ends_with("abort") || mode == "cancel-drop-panic" {
                        std::future::pending::<()>().await;
                    }
                    assert_ne!(mode, "body-panic", "Expected tracked playback task panic");
                })
                .unwrap();
            started.acquire().await.unwrap().forget();
            if mode == "expected-abort" || mode == "cancel-drop-panic" {
                stop.cancel();
            }
            if mode.ends_with("abort") || mode == "cancel-drop-panic" {
                task.abort();
            }
            let accepted = mode == "complete" || mode == "expected-abort";
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(1), cleanup.wait())
                    .await
                    .unwrap()
                    .is_ok(),
                accepted
            );
            assert_eq!(cleanup.wait().await.is_ok(), accepted);
            assert!(task.is_finished());
            assert_eq!(cleanup.pending.load(Ordering::Acquire), 0);
            println!("audio task cleanup result={mode} accepted={accepted} join_observed=true failure_sticky=true physical_audio=false");
        }
    }

    struct CleanupCheck {
        entered: std::sync::mpsc::Sender<()>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }

    impl CleanupCheck {
        fn wait(&self) {
            self.entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
        }
    }

    struct CleanupDevice {
        mode: &'static str,
        cancelled: AtomicBool,
        finish: Option<CleanupCheck>,
        destruction: Option<CleanupCheck>,
    }

    impl SharedAudioDevice for CleanupDevice {
        fn failed(&self) -> bool {
            false
        }

        fn cancel(&self) {
            self.cancelled.store(true, Ordering::Release);
            assert_ne!(
                self.mode, "cancel-panic",
                "Expected cleanup cancellation panic"
            );
        }

        fn finish_shutdown(&self) -> Result<(), AudioCleanupError> {
            if let Some(finish) = &self.finish {
                finish.wait();
            }
            assert_ne!(self.mode, "join-panic", "Expected cleanup join panic");
            match self.mode {
                "unsupported" => Err(AudioCleanupError::Unsupported),
                "join-error" => Err(AudioCleanupError::Incomplete),
                _ => Ok(()),
            }
        }
    }

    impl Drop for CleanupDevice {
        fn drop(&mut self) {
            if let Some(destruction) = &self.destruction {
                destruction.wait();
            }
            assert_ne!(
                self.mode, "drop-panic",
                "Expected cleanup destruction panic"
            );
        }
    }

    #[tokio::test]
    async fn registered_retirement_waits_for_join_and_owned_drop_and_survives_abandoned_waiters() {
        for live_arc in [false, true] {
            let cleanup = Arc::new(AudioCleanup::default());
            let (join_entered, join_waiting) = std::sync::mpsc::channel();
            let (join_release, join_blocked) = std::sync::mpsc::channel();
            let (drop_entered, drop_waiting) = std::sync::mpsc::channel();
            let (drop_release, drop_blocked) = std::sync::mpsc::channel();
            let device = Arc::new(CleanupDevice {
                mode: "ok",
                cancelled: AtomicBool::new(false),
                finish: Some(CleanupCheck {
                    entered: join_entered,
                    release: Mutex::new(join_blocked),
                }),
                destruction: Some(CleanupCheck {
                    entered: drop_entered,
                    release: Mutex::new(drop_blocked),
                }),
            });
            let retained = live_arc.then(|| device.clone());
            cleanup.retire(device);
            join_waiting.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(cleanup.pending.load(Ordering::Acquire), 1);
            assert!(
                tokio::time::timeout(Duration::from_millis(30), cleanup.wait())
                    .await
                    .is_err()
            );
            assert_eq!(cleanup.pending.load(Ordering::Acquire), 1);
            let first = cleanup.clone();
            let second = cleanup.clone();
            let first = tokio::spawn(async move { first.wait().await });
            let second = tokio::spawn(async move { second.wait().await });
            tokio::task::yield_now().await;
            join_release.send(()).unwrap();
            if live_arc {
                tokio::time::timeout(Duration::from_secs(1), cleanup.wait())
                    .await
                    .unwrap()
                    .unwrap();
                assert!(drop_waiting.try_recv().is_err());
                drop_release.send(()).unwrap();
                drop(retained);
                drop_waiting.recv_timeout(Duration::from_secs(1)).unwrap();
            } else {
                drop_waiting.recv_timeout(Duration::from_secs(1)).unwrap();
                assert!(
                    tokio::time::timeout(Duration::from_millis(30), cleanup.wait())
                        .await
                        .is_err()
                );
                assert_eq!(cleanup.pending.load(Ordering::Acquire), 1);
                assert!(!first.is_finished());
                assert!(!second.is_finished());
                drop_release.send(()).unwrap();
                tokio::time::timeout(Duration::from_secs(1), cleanup.wait())
                    .await
                    .unwrap()
                    .unwrap();
            }
            tokio::time::timeout(Duration::from_secs(1), first)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            tokio::time::timeout(Duration::from_secs(1), second)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(cleanup.pending.load(Ordering::Acquire), 0);
            assert!(cleanup.wait().await.is_ok());
            println!("audio cleanup live_arc={live_arc} registered_before_cancel=true held_join_waited=true owned_drop_waited=true abandoned_waiter_preserved=true concurrent_waiters=2 external_last_arc_tracked=false physical_audio=false");
        }
    }

    #[tokio::test]
    async fn registered_factory_keeps_cleanup_open_through_revoked_late_return_and_shared_drop() {
        for shared_dropped in [false, true] {
            let factory_entered = Arc::new(tokio::sync::Semaphore::new(0));
            let factory_release = Arc::new(tokio::sync::Semaphore::new(0));
            let entered = factory_entered.clone();
            let release = factory_release.clone();
            let (join_entered, join_waiting) = std::sync::mpsc::channel();
            let (join_release, join_blocked) = std::sync::mpsc::channel();
            let device = Arc::new(Mutex::new(Some(CleanupDevice {
                mode: "ok",
                cancelled: AtomicBool::new(false),
                finish: Some(CleanupCheck {
                    entered: join_entered,
                    release: Mutex::new(join_blocked),
                }),
                destruction: None,
            })));
            let shared = SharedAudio::new(
                Arc::new(AudioControls::new(AudioSettings::default())),
                CancellationToken::new(),
                move |_, _, _| {
                    let entered = entered.clone();
                    let release = release.clone();
                    let device = device.clone();
                    Box::pin(async move {
                        entered.add_permits(1);
                        release.acquire().await.unwrap().forget();
                        Ok(device.lock().unwrap().take().unwrap())
                    })
                },
            );
            let monitor = shared.monitor(true);
            let mut ready = shared.start(true).unwrap().0;
            let cleanup = shared.cleanup.clone();
            tokio::time::timeout(Duration::from_secs(1), factory_entered.acquire())
                .await
                .unwrap()
                .unwrap()
                .forget();
            shared.reset();
            assert!(matches!(*ready.borrow_and_update(), Some(Err(()))));
            assert_eq!(cleanup.pending.load(Ordering::Acquire), 1);
            assert!(shared.start(true).is_err());
            let generation = shared.demand.reserve().unwrap();
            assert!(shared.demand.activate(generation));
            let demand = shared.demand.clone();
            let current_monitor = shared.monitor(true);
            let weak = Arc::downgrade(&shared);
            let retained = if shared_dropped {
                drop(monitor);
                drop(current_monitor);
                drop(shared);
                assert!(weak.upgrade().is_none());
                None
            } else {
                Some((shared, monitor, current_monitor))
            };
            assert!(
                tokio::time::timeout(Duration::from_millis(30), cleanup.wait())
                    .await
                    .is_err()
            );
            factory_release.add_permits(1);
            tokio::task::spawn_blocking(move || join_waiting.recv_timeout(Duration::from_secs(1)))
                .await
                .unwrap()
                .unwrap();
            assert!(
                tokio::time::timeout(Duration::from_millis(30), cleanup.wait())
                    .await
                    .is_err()
            );
            assert!(matches!(*ready.borrow(), Some(Err(()))));
            if !shared_dropped {
                assert_eq!(demand.active(), generation);
                let (shared, _, monitor) = retained.as_ref().unwrap();
                assert!(monitor.is_current());
                assert!(shared.state.lock().unwrap().device.is_none());
            }
            join_release.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(1), cleanup.wait())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(cleanup.pending.load(Ordering::Acquire), 0);
            if !shared_dropped {
                assert_eq!(demand.active(), generation);
            }
            println!("audio cleanup shared_dropped={shared_dropped} factory_registered_before_spawn=true revoked_late_return=true no_registration_gap=true pending_quarantine_preserved=true held_join_waited=true physical_audio=false");
        }
    }

    #[tokio::test]
    async fn cleanup_records_unsupported_errors_panics_and_unstarted_jobs_without_losing_drain() {
        for mode in [
            "unsupported",
            "join-error",
            "cancel-panic",
            "join-panic",
            "drop-panic",
            "unstarted",
            "factory-panic",
        ] {
            let cleanup = Arc::new(AudioCleanup::default());
            if mode == "unstarted" {
                let job = cleanup.register();
                let task = tokio::spawn(async move {
                    let _job = job;
                    std::future::pending::<()>().await;
                });
                task.abort();
                assert!(task.await.is_err());
            } else if mode == "factory-panic" {
                let job = cleanup.register();
                let task = tokio::spawn(async move {
                    let _job = job;
                    panic!("Expected tracked audio factory panic");
                });
                assert!(task.await.is_err());
            } else {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    cleanup.retire(Arc::new(CleanupDevice {
                        mode,
                        cancelled: AtomicBool::new(false),
                        finish: None,
                        destruction: None,
                    }));
                }));
                assert_eq!(result.is_err(), mode == "cancel-panic");
            }
            assert!(matches!(
                tokio::time::timeout(Duration::from_secs(1), cleanup.wait())
                    .await
                    .unwrap(),
                Err(AudioCleanupError::Incomplete)
            ));
            assert!(matches!(
                cleanup.wait().await,
                Err(AudioCleanupError::Incomplete)
            ));
            assert_eq!(cleanup.pending.load(Ordering::Acquire), 0);
            println!("audio cleanup failure={mode} registered_job_drained=true failure_sticky=true unsupported_not_accepted=true physical_audio=false");
        }
    }

    #[test]
    fn cleanup_without_a_runtime_preserves_synchronous_completion() {
        let cleanup = Arc::new(AudioCleanup::default());
        assert!(tokio::runtime::Handle::try_current().is_err());
        cleanup.retire(Arc::new(CleanupDevice {
            mode: "ok",
            cancelled: AtomicBool::new(false),
            finish: None,
            destruction: None,
        }));
        assert_eq!(cleanup.pending.load(Ordering::Acquire), 0);
        assert!(!cleanup.failed.load(Ordering::Acquire));
        println!("audio cleanup runtime=false synchronous_fallback=true registered_job_drained=true physical_audio=false");
    }

    #[tokio::test]
    async fn sealed_cleanup_linearizes_root_admission_and_waits_for_prepared_tasks_after_seal() {
        let cleanup = Arc::new(AudioCleanup::default());
        let prepared = cleanup.prepare_task().unwrap();
        assert_eq!(cleanup.pending.load(Ordering::Acquire), 1);
        cleanup.seal().unwrap();
        cleanup.seal().unwrap();
        assert!(cleanup.prepare_task().is_err());
        assert!(cleanup.try_register().is_err());
        assert!(!cleanup.failed.load(Ordering::Acquire));
        assert!(
            tokio::time::timeout(Duration::from_millis(30), cleanup.wait())
                .await
                .is_err()
        );
        let stop = CancellationToken::new();
        let started = Arc::new(tokio::sync::Semaphore::new(0));
        let task_started = started.clone();
        let task = prepared.spawn(stop.clone(), async move {
            task_started.add_permits(1);
            std::future::pending::<()>().await;
        });
        started.acquire().await.unwrap().forget();
        assert!(
            tokio::time::timeout(Duration::from_millis(30), cleanup.wait())
                .await
                .is_err()
        );
        stop.cancel();
        task.abort();
        tokio::time::timeout(Duration::from_secs(1), cleanup.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(task.is_finished());
        assert!(cleanup.prepare_task().is_err());
        assert!(cleanup.wait().await.is_ok());
        for _ in 0..64 {
            let cleanup = Arc::new(AudioCleanup::default());
            let barrier = std::sync::Barrier::new(2);
            let admission = std::thread::scope(|scope| {
                let admitted = scope.spawn(|| {
                    barrier.wait();
                    cleanup.prepare_task()
                });
                barrier.wait();
                cleanup.seal().unwrap();
                admitted.join().unwrap()
            });
            assert!(cleanup.prepare_task().is_err());
            assert_eq!(
                cleanup.pending.load(Ordering::Acquire),
                usize::from(admission.is_ok())
            );
            if admission.is_ok() {
                assert!(
                    tokio::time::timeout(Duration::from_millis(1), cleanup.wait())
                        .await
                        .is_err()
                );
            }
            drop(admission);
            assert!(cleanup.wait().await.is_ok());
            assert_eq!(cleanup.pending.load(Ordering::Acquire), 0);
        }
        println!("audio admission sealed=true idempotent=true prepared_before_seal=true spawned_after_seal=true observed_join_waited=true canceled_waiter_preserved=true root_race_repeats=64 root_after_seal_rejected=true physical_audio=false");
    }

    #[tokio::test]
    async fn sealed_factory_keeps_descendant_retirement_registered_through_late_result_and_destruction(
    ) {
        for shared_dropped in [false, true] {
            let started = Arc::new(tokio::sync::Semaphore::new(0));
            let released = Arc::new(tokio::sync::Semaphore::new(0));
            let entered = started.clone();
            let release = released.clone();
            let (join_entered, join_waiting) = std::sync::mpsc::channel();
            let (join_release, join_blocked) = std::sync::mpsc::channel();
            let (drop_entered, drop_waiting) = std::sync::mpsc::channel();
            let (drop_release, drop_blocked) = std::sync::mpsc::channel();
            let device = Arc::new(Mutex::new(Some(CleanupDevice {
                mode: "ok",
                cancelled: AtomicBool::new(false),
                finish: Some(CleanupCheck {
                    entered: join_entered,
                    release: Mutex::new(join_blocked),
                }),
                destruction: Some(CleanupCheck {
                    entered: drop_entered,
                    release: Mutex::new(drop_blocked),
                }),
            })));
            let shared = SharedAudio::new(
                Arc::new(AudioControls::new(AudioSettings::default())),
                CancellationToken::new(),
                move |_, _, _| {
                    let started = entered.clone();
                    let released = release.clone();
                    let device = device.clone();
                    Box::pin(async move {
                        started.add_permits(1);
                        released.acquire().await.unwrap().forget();
                        Ok(device.lock().unwrap().take().unwrap())
                    })
                },
            );
            let monitor = shared.monitor(true);
            let ready = shared.start(true).unwrap().0;
            started.acquire().await.unwrap().forget();
            shared.stop.cancel();
            shared.reset();
            shared.seal_cleanup().unwrap();
            let cleanup = shared.cleanup.clone();
            assert!(cleanup.prepare_task().is_err());
            assert!(shared.start(true).is_err());
            assert_eq!(cleanup.pending.load(Ordering::Acquire), 1);
            assert!(matches!(*ready.borrow(), Some(Err(()))));
            drop(monitor);
            let retained = if shared_dropped {
                drop(shared);
                None
            } else {
                Some(shared)
            };
            assert!(
                tokio::time::timeout(Duration::from_millis(30), cleanup.wait())
                    .await
                    .is_err()
            );
            let first = cleanup.clone();
            let second = cleanup.clone();
            let first = tokio::spawn(async move { first.wait().await });
            let second = tokio::spawn(async move { second.wait().await });
            released.add_permits(1);
            tokio::task::spawn_blocking(move || join_waiting.recv_timeout(Duration::from_secs(1)))
                .await
                .unwrap()
                .unwrap();
            assert!(cleanup.pending.load(Ordering::Acquire) >= 1);
            assert!(!cleanup.failed.load(Ordering::Acquire));
            assert!(
                tokio::time::timeout(Duration::from_millis(30), cleanup.wait())
                    .await
                    .is_err()
            );
            assert!(!first.is_finished());
            assert!(!second.is_finished());
            join_release.send(()).unwrap();
            tokio::task::spawn_blocking(move || drop_waiting.recv_timeout(Duration::from_secs(1)))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(cleanup.pending.load(Ordering::Acquire), 1);
            assert!(!first.is_finished());
            assert!(!second.is_finished());
            drop_release.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(1), first)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            tokio::time::timeout(Duration::from_secs(1), second)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(cleanup.wait().await.is_ok());
            assert!(cleanup.try_register().is_err());
            assert_eq!(cleanup.pending.load(Ordering::Acquire), 0);
            drop(retained);
            assert!(cleanup.wait().await.is_ok());
            println!("audio admission factory=true shared_dropped={shared_dropped} sealed_before_late_result=true descendant_registered_before_parent_release=true owned_join_drop_waited=true concurrent_waiters=2 root_after_seal_rejected=true physical_audio=false");
        }
    }

    struct SealedRetirementDevice {
        owner: std::thread::ThreadId,
        calls: Arc<AtomicUsize>,
        mode: &'static str,
    }

    impl SharedAudioDevice for SealedRetirementDevice {
        fn failed(&self) -> bool {
            false
        }

        fn cancel(&self) {
            assert_eq!(std::thread::current().id(), self.owner);
            self.calls.fetch_add(1, Ordering::AcqRel);
        }

        fn finish_shutdown(&self) -> Result<(), AudioCleanupError> {
            assert_eq!(std::thread::current().id(), self.owner);
            self.calls.fetch_add(1, Ordering::AcqRel);
            assert_ne!(
                self.mode, "panic",
                "Expected sealed retirement shutdown panic"
            );
            if self.mode == "unsupported" {
                Err(AudioCleanupError::Unsupported)
            } else {
                Ok(())
            }
        }
    }

    impl Drop for SealedRetirementDevice {
        fn drop(&mut self) {
            assert_eq!(std::thread::current().id(), self.owner);
            self.calls.fetch_add(1, Ordering::AcqRel);
        }
    }

    #[tokio::test]
    async fn sealed_cleanup_rejects_new_retirement_without_deferring_owned_disposal_and_retains_poison(
    ) {
        for mode in ["ok", "unsupported", "panic"] {
            let cleanup = Arc::new(AudioCleanup::default());
            cleanup.seal().unwrap();
            let calls = Arc::new(AtomicUsize::new(0));
            let device = Arc::new(SealedRetirementDevice {
                owner: std::thread::current().id(),
                calls: calls.clone(),
                mode,
            });
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cleanup.retire(device)));
            assert_eq!(result.is_err(), mode == "panic");
            assert_eq!(calls.load(Ordering::Acquire), 3);
            assert_eq!(cleanup.pending.load(Ordering::Acquire), 0);
            assert!(cleanup.wait().await.is_err());
            assert!(cleanup.wait().await.is_err());
            assert!(cleanup.prepare_task().is_err());
            println!("audio admission retirement={mode} root_after_seal_rejected=true owned_disposal_same_thread=true no_deferred_work=true failure_sticky=true physical_audio=false");
        }
        let cleanup = Arc::new(AudioCleanup::default());
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _sealed = cleanup.sealed.lock().unwrap();
            panic!("Expected audio admission lock poison");
        }));
        assert!(cleanup.seal().is_err());
        assert!(cleanup.prepare_task().is_err());
        assert!(cleanup.wait().await.is_err());
        assert!(cleanup.sealed.is_poisoned());
        let clean = Arc::new(AudioCleanup::default());
        let prepared = clean.prepare_task().unwrap();
        clean.seal().unwrap();
        drop(prepared);
        assert!(clean.wait().await.is_ok());
        let broken = Arc::new(AudioCleanup::default());
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _prepared = broken.prepare_task().unwrap();
            broken.seal().unwrap();
            panic!("Expected admitted preparation panic");
        }));
        assert!(broken.wait().await.is_err());
        assert_eq!(broken.pending.load(Ordering::Acquire), 0);
        println!("audio admission poison_retained=true seal_failure_reported=true unused_preparation_drained=true preparation_panic_sticky=true physical_audio=false");
    }

    #[tokio::test]
    async fn supervisor_shutdown_preserves_registered_retirement_before_sealed_final_drain() {
        let shared = SharedAudio::new(
            Arc::new(AudioControls::new(AudioSettings::default())),
            CancellationToken::new(),
            |_, _, _| {
                Box::pin(async {
                    panic!("Supervisor shutdown fixture must not start physical audio")
                })
            },
        );
        let monitor = shared.monitor(true);
        let (entered, waiting) = std::sync::mpsc::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        shared.state.lock().unwrap().device = Some(Arc::new(CleanupDevice {
            mode: "ok",
            cancelled: AtomicBool::new(false),
            finish: Some(CleanupCheck {
                entered,
                release: Mutex::new(blocked),
            }),
            destruction: None,
        }));
        let supervisor = tokio::spawn(shared.clone().run());
        shared.stop.cancel();
        tokio::time::timeout(Duration::from_secs(1), supervisor)
            .await
            .unwrap()
            .unwrap();
        tokio::task::spawn_blocking(move || waiting.recv_timeout(Duration::from_secs(1)))
            .await
            .unwrap()
            .unwrap();
        shared.seal_cleanup().unwrap();
        assert!(shared.cleanup.prepare_task().is_err());
        assert!(!monitor.is_current());
        assert!(shared.state.lock().unwrap().device.is_none());
        assert_eq!(shared.cleanup.pending.load(Ordering::Acquire), 1);
        assert!(
            tokio::time::timeout(Duration::from_millis(30), shared.wait_for_cleanup())
                .await
                .is_err()
        );
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), shared.wait_for_cleanup())
            .await
            .unwrap()
            .unwrap();
        shared.seal_cleanup().unwrap();
        assert!(shared.wait_for_cleanup().await.is_ok());
        drop(monitor);
        assert!(shared.wait_for_cleanup().await.is_ok());
        println!("audio admission supervisor_joined=true retirement_before_seal=true final_drain_waited=true late_roots_rejected=true external_last_arc_bounds=false physical_audio=false");
    }

    struct ResetDevice {
        entered: std::sync::mpsc::Sender<()>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }

    impl SharedAudioDevice for ResetDevice {
        fn failed(&self) -> bool {
            false
        }

        fn cancel(&self) {
            self.entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
        }
    }

    fn reset_runtime() -> Arc<SharedAudio<ResetDevice>> {
        SharedAudio::new(
            Arc::new(AudioControls::new(AudioSettings::default())),
            CancellationToken::new(),
            |_, _, _| Box::pin(async { panic!("No device should start in a reset fixture") }),
        )
    }

    #[test]
    fn stale_voice_lease_clear_and_drop_do_not_enter_new_owner_resource_maintenance() {
        for resource in ["mic-test", "runtime"] {
            let shared = reset_runtime();
            let old_generation = shared.demand.reserve().unwrap();
            assert!(shared.demand.activate(old_generation));
            let old = VoiceLease {
                shared: shared.clone(),
                generation: old_generation,
                device: None,
            };
            shared.reset();
            let monitor = shared.monitor(true);
            let current = shared.demand.reserve().unwrap();
            assert!(shared.demand.activate(current));
            shared
                .demand
                .controls
                .speaking
                .store(true, Ordering::Release);
            shared
                .demand
                .controls
                .mic_db
                .store((-17.0_f32).to_bits(), Ordering::Release);
            let epoch = shared.demand.controls.capture_epoch.load(Ordering::Acquire);
            let snapshot = shared.mic_test.snapshot(Instant::now());
            let mic_test = (resource == "mic-test")
                .then(|| shared.mic_test.try_prepare_dispose().unwrap().unwrap());
            let runtime = (resource == "runtime").then(|| shared.state.lock().unwrap());
            let (done, finished) = std::sync::mpsc::channel();
            std::thread::scope(|scope| {
                scope.spawn(move || {
                    old.clear_network_status();
                    drop(old);
                    done.send(()).unwrap();
                });
                let result = finished.recv_timeout(Duration::from_millis(1500));
                drop(mic_test);
                drop(runtime);
                result.unwrap();
            });
            assert_eq!(shared.demand.active(), current);
            assert!(shared.demand.is_reserved(current));
            assert!(monitor.is_current());
            assert_eq!(shared.mic_test.snapshot(Instant::now()), snapshot);
            assert_eq!(
                shared.demand.controls.capture_epoch.load(Ordering::Acquire),
                epoch
            );
            assert!(shared.demand.controls.speaking.load(Ordering::Acquire));
            assert_eq!(
                f32::from_bits(shared.demand.controls.mic_db.load(Ordering::Acquire)),
                -17.0
            );
            println!("stale voice cleanup held={resource} current_generation=true controls_unchanged=true monitor_unchanged=true shared_maintenance_skipped=true physical_audio=false");
        }
    }

    #[test]
    fn current_voice_cleanup_preserves_status_order_monitor_and_poisoned_demand_behavior() {
        for poisoned in [false, true] {
            let shared = reset_runtime();
            let monitor = shared.monitor(true);
            let generation = shared.demand.reserve().unwrap();
            assert!(shared.demand.activate(generation));
            let lease = VoiceLease {
                shared: shared.clone(),
                generation,
                device: None,
            };
            let epoch = shared.demand.controls.capture_epoch.load(Ordering::Acquire);
            let snapshot = shared.mic_test.snapshot(Instant::now());
            shared
                .demand
                .controls
                .speaking
                .store(true, Ordering::Release);
            shared
                .demand
                .controls
                .mic_db
                .store((-17.0_f32).to_bits(), Ordering::Release);
            if poisoned {
                assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _held = shared.demand.voice.lock().unwrap();
                    panic!("Expected voice cleanup demand fixture poison");
                }))
                .is_err());
            }
            lease.clear_network_status();
            assert!(!shared.demand.controls.speaking.load(Ordering::Acquire));
            assert_eq!(
                f32::from_bits(shared.demand.controls.mic_db.load(Ordering::Acquire)),
                -100.0
            );
            assert_eq!(shared.demand.active(), generation);
            assert_eq!(
                shared.demand.controls.capture_epoch.load(Ordering::Acquire),
                epoch
            );
            drop(lease);
            assert_eq!(
                shared.demand.active(),
                if poisoned { generation } else { 0 }
            );
            assert_eq!(
                shared.demand.controls.capture_epoch.load(Ordering::Acquire),
                epoch + u64::from(!poisoned)
            );
            assert_eq!(shared.demand.voice.is_poisoned(), poisoned);
            assert!(monitor.is_current());
            assert_eq!(shared.mic_test.snapshot(Instant::now()), snapshot);
            println!("current voice cleanup poison={poisoned} status_before_revoke=true monitor_unchanged=true legacy_partial=true physical_audio=false");
        }
    }

    struct MaintenanceCheck {
        entered: std::sync::mpsc::Sender<()>,
        release: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    }

    struct MaintenanceDevice {
        failed: bool,
        runtime_failed: bool,
        check: Option<MaintenanceCheck>,
        cancelled: AtomicBool,
    }

    impl SharedAudioDevice for MaintenanceDevice {
        fn failed(&self) -> bool {
            self.failed
        }

        fn runtime_failed(&self) -> bool {
            if let Some(check) = &self.check {
                if let Some(release) = check.release.lock().unwrap().take() {
                    check.entered.send(()).unwrap();
                    release.recv_timeout(Duration::from_secs(3)).unwrap();
                }
            }
            self.runtime_failed
        }

        fn cancel(&self) {
            self.cancelled.store(true, Ordering::Release);
        }
    }

    #[test]
    fn post_revoke_maintenance_rejects_new_voice_and_monitor_after_resource_acquisition() {
        for resource in ["mic-test", "runtime"] {
            for replacement in ["voice", "monitor"] {
                let shared = reset_runtime();
                let old_monitor = shared.monitor(true);
                let monitor = shared.monitor.load(Ordering::Acquire);
                let generation = shared.demand.reserve().unwrap();
                assert!(shared.demand.activate(generation));
                assert_eq!(shared.demand.revoke_if_current(generation), Some(true));
                let current_voice = (replacement == "voice").then(|| {
                    let current = shared.demand.reserve().unwrap();
                    assert!(shared.demand.activate(current));
                    current
                });
                let current_monitor = (replacement == "monitor").then(|| shared.monitor(true));
                let now = Instant::now();
                let token = shared.mic_test.begin(now).unwrap();
                let snapshot = shared.mic_test.snapshot(now);
                let pending = Arc::new(Pending {
                    stop: CancellationToken::new(),
                    ready: watch::channel(None).0,
                    revoked: AtomicBool::new(false),
                });
                {
                    let mut state = shared.state.lock().unwrap();
                    state.pending = Some(pending.clone());
                    state.failed = true;
                    state.idle_since = Some(now);
                }
                let epoch = shared.demand.controls.capture_epoch.load(Ordering::Acquire);
                shared
                    .demand
                    .controls
                    .speaking
                    .store(true, Ordering::Release);
                shared
                    .demand
                    .controls
                    .mic_db
                    .store((-17.0_f32).to_bits(), Ordering::Release);
                let mic_test = (resource == "mic-test")
                    .then(|| shared.mic_test.try_prepare_dispose().unwrap().unwrap());
                let state = (resource == "runtime").then(|| shared.state.lock().unwrap());
                let (entered, waiting) = std::sync::mpsc::channel();
                let (done, finished) = std::sync::mpsc::channel();
                std::thread::scope(|scope| {
                    scope.spawn(|| {
                        entered.send(()).unwrap();
                        done.send(shared.maintain_after_voice(
                            generation,
                            monitor,
                            now + PREPARATION_TIMEOUT * 2,
                        ))
                        .unwrap();
                    });
                    waiting.recv_timeout(Duration::from_secs(1)).unwrap();
                    assert_eq!(
                        finished.recv_timeout(Duration::from_millis(30)),
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                    );
                    assert!(shared.demand.voice.try_lock().is_ok());
                    drop(mic_test);
                    drop(state);
                    assert!(!finished.recv_timeout(Duration::from_millis(1500)).unwrap());
                });
                assert_eq!(shared.demand.active(), current_voice.unwrap_or(0));
                assert_eq!(
                    shared.demand.controls.capture_epoch.load(Ordering::Acquire),
                    epoch
                );
                assert!(shared.demand.controls.speaking.load(Ordering::Acquire));
                assert_eq!(
                    f32::from_bits(shared.demand.controls.mic_db.load(Ordering::Acquire)),
                    -17.0
                );
                assert_eq!(shared.mic_test.capture_token(), Some(token));
                assert_eq!(shared.mic_test.snapshot(now), snapshot);
                assert!(!pending.revoked.load(Ordering::Acquire));
                assert!(!pending.stop.is_cancelled());
                assert!(pending.ready.borrow().is_none());
                let state = shared.state.lock().unwrap();
                assert!(state.failed);
                assert_eq!(state.idle_since, Some(now));
                assert!(Arc::ptr_eq(state.pending.as_ref().unwrap(), &pending));
                drop(state);
                assert_eq!(old_monitor.is_current(), replacement == "voice");
                assert!(current_monitor
                    .as_ref()
                    .is_none_or(MonitorLease::is_current));
                println!("voice maintenance held={resource} replacement={replacement} post_revoke=true original_generations=true rejected_after_resources=true fresh_state_unchanged=true physical_audio=false");
            }
        }
    }

    #[test]
    fn current_lease_drop_rechecks_ownership_after_native_health_without_holding_voice_gate() {
        let shared = SharedAudio::new(
            Arc::new(AudioControls::new(AudioSettings::default())),
            CancellationToken::new(),
            |_, _, _| Box::pin(async { panic!("No device should start in a maintenance fixture") }),
        );
        let monitor = shared.monitor(true);
        let now = Instant::now();
        let token = shared.mic_test.begin(now).unwrap();
        let snapshot = shared.mic_test.snapshot(now);
        let generation = shared.demand.reserve().unwrap();
        assert!(shared.demand.activate(generation));
        let old = VoiceLease {
            shared: shared.clone(),
            generation,
            device: None,
        };
        let (entered, waiting) = std::sync::mpsc::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let device = Arc::new(MaintenanceDevice {
            failed: true,
            runtime_failed: true,
            check: Some(MaintenanceCheck {
                entered,
                release: Mutex::new(Some(blocked)),
            }),
            cancelled: AtomicBool::new(false),
        });
        shared.state.lock().unwrap().device = Some(device.clone());
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(move || {
                drop(old);
                done.send(()).unwrap();
            });
            waiting.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(shared.demand.active(), 0);
            assert!(shared.demand.voice.try_lock().is_ok());
            let current = shared.demand.reserve().unwrap();
            assert!(shared.demand.activate(current));
            let epoch = shared.demand.controls.capture_epoch.load(Ordering::Acquire);
            shared
                .demand
                .controls
                .speaking
                .store(true, Ordering::Release);
            shared
                .demand
                .controls
                .mic_db
                .store((-17.0_f32).to_bits(), Ordering::Release);
            release.send(()).unwrap();
            finished.recv_timeout(Duration::from_millis(1500)).unwrap();
            assert_eq!(shared.demand.active(), current);
            assert_eq!(
                shared.demand.controls.capture_epoch.load(Ordering::Acquire),
                epoch
            );
            assert!(shared.demand.controls.speaking.load(Ordering::Acquire));
            assert_eq!(
                f32::from_bits(shared.demand.controls.mic_db.load(Ordering::Acquire)),
                -17.0
            );
            assert_eq!(shared.mic_test.capture_token(), Some(token));
            assert_eq!(shared.mic_test.snapshot(now), snapshot);
            assert!(monitor.is_current());
            let state = shared.state.lock().unwrap();
            assert!(Arc::ptr_eq(state.device.as_ref().unwrap(), &device));
            assert!(!state.failed);
            assert!(state.idle_since.is_none());
            assert!(!device.cancelled.load(Ordering::Acquire));
        });
        println!("voice maintenance held=native-health actual_lease_drop=true voice_gate_free=true fresh_voice_unchanged=true native_snapshot_rejected=true physical_audio=false");
    }

    #[test]
    fn accepted_voice_maintenance_quarantines_pending_and_retires_after_all_resource_release() {
        let shared = reset_runtime();
        let generation = shared.demand.reserve().unwrap();
        assert!(shared.demand.activate(generation));
        let lease = VoiceLease {
            shared: shared.clone(),
            generation,
            device: None,
        };
        let pending = Arc::new(Pending {
            stop: CancellationToken::new(),
            ready: watch::channel(None).0,
            revoked: AtomicBool::new(false),
        });
        let (entered, waiting) = std::sync::mpsc::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        {
            let mut state = shared.state.lock().unwrap();
            state.device = Some(Arc::new(ResetDevice {
                entered,
                release: Mutex::new(blocked),
            }));
            state.pending = Some(pending.clone());
            state.idle_since = Some(Instant::now() - IDLE_RELEASE * 2);
        }
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(move || {
                drop(lease);
                done.send(()).unwrap();
            });
            waiting.recv_timeout(Duration::from_secs(1)).unwrap();
            assert!(pending.revoked.load(Ordering::Acquire));
            assert!(pending.stop.is_cancelled());
            assert!(matches!(*pending.ready.borrow(), Some(Err(()))));
            assert!(shared.demand.voice.try_lock().is_ok());
            assert!(shared.state.try_lock().is_ok());
            assert!(shared.mic_test.try_prepare_dispose().unwrap().is_some());
            let monitor = shared.monitor(true);
            let current = shared.demand.reserve().unwrap();
            assert!(shared.demand.activate(current));
            assert!(shared.start(true).is_err());
            let epoch = shared.demand.controls.capture_epoch.load(Ordering::Acquire);
            let snapshot = shared.mic_test.snapshot(Instant::now());
            release.send(()).unwrap();
            finished.recv_timeout(Duration::from_millis(1500)).unwrap();
            assert!(monitor.is_current());
            assert_eq!(shared.demand.active(), current);
            assert_eq!(
                shared.demand.controls.capture_epoch.load(Ordering::Acquire),
                epoch
            );
            assert_eq!(shared.mic_test.snapshot(Instant::now()), snapshot);
            let state = shared.state.lock().unwrap();
            assert!(state.device.is_none());
            assert!(!state.failed);
            assert!(Arc::ptr_eq(state.pending.as_ref().unwrap(), &pending));
            drop(state);
        });
        println!("voice maintenance held=device-cancel actual_lease_drop=true pending_quarantined=true resource_locks_released=true fresh_leases_unchanged=true physical_audio=false");
    }

    #[test]
    fn prepared_voice_maintenance_preserves_legacy_idle_failure_and_partial_poison_results() {
        for resource in ["healthy", "mic-test", "runtime"] {
            for mode in ["idle", "timeout", "processor", "endpoint"] {
                let mut fixtures = Vec::new();
                let now = Instant::now();
                for _ in 0..2 {
                    let shared = SharedAudio::new(
                        Arc::new(AudioControls::new(AudioSettings::default())),
                        CancellationToken::new(),
                        |_, _, _| {
                            Box::pin(async { panic!("No device should start in a parity fixture") })
                        },
                    );
                    let monitor = (mode != "idle").then(|| shared.monitor(true));
                    if mode != "idle" {
                        shared.mic_test.begin(now).unwrap();
                    }
                    let generation = shared.demand.reserve().unwrap();
                    assert!(shared.demand.activate(generation));
                    let device = Arc::new(MaintenanceDevice {
                        failed: mode == "processor" || mode == "endpoint",
                        runtime_failed: mode == "endpoint",
                        check: None,
                        cancelled: AtomicBool::new(false),
                    });
                    {
                        let mut state = shared.state.lock().unwrap();
                        state.device = Some(device.clone());
                        state.idle_since = Some(now - IDLE_RELEASE * 2);
                    }
                    if resource != "healthy" {
                        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            if resource == "mic-test" {
                                let _held = shared.mic_test.try_prepare_dispose().unwrap().unwrap();
                                panic!("Expected maintenance MicTest fixture poison");
                            } else {
                                let _held = shared.state.lock().unwrap();
                                panic!("Expected maintenance runtime fixture poison");
                            }
                        }))
                        .is_err());
                    }
                    fixtures.push((shared, generation, device, monitor));
                }
                let time = if mode == "timeout" {
                    now + PREPARATION_TIMEOUT
                } else {
                    now
                };
                let (legacy, old, legacy_device, _) = &fixtures[0];
                let (prepared, current, prepared_device, _) = &fixtures[1];
                legacy.demand.revoke(*old);
                legacy.maintain(time);
                let monitor = prepared.monitor.load(Ordering::Acquire);
                assert_eq!(prepared.demand.revoke_if_current(*current), Some(true));
                assert!(prepared.maintain_after_voice(*current, monitor, time));
                assert_eq!(prepared.demand.active(), legacy.demand.active());
                assert_eq!(
                    prepared
                        .demand
                        .controls
                        .capture_epoch
                        .load(Ordering::Acquire),
                    legacy.demand.controls.capture_epoch.load(Ordering::Acquire)
                );
                assert_eq!(
                    prepared.mic_test.snapshot(time),
                    legacy.mic_test.snapshot(time)
                );
                assert_eq!(
                    prepared.mic_test.capture_token(),
                    legacy.mic_test.capture_token()
                );
                assert_eq!(
                    prepared_device.cancelled.load(Ordering::Acquire),
                    legacy_device.cancelled.load(Ordering::Acquire)
                );
                let before = legacy
                    .state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let after = prepared
                    .state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                assert_eq!(after.failed, before.failed);
                assert_eq!(after.idle_since, before.idle_since);
                assert_eq!(after.device.is_some(), before.device.is_some());
                assert_eq!(prepared.state.is_poisoned(), resource == "runtime");
                assert_eq!(
                    prepared.mic_test.try_prepare_dispose().is_err(),
                    resource == "mic-test"
                );
                drop(after);
                drop(before);
                println!("voice maintenance parity resource={resource} mode={mode} legacy_result=true poison_retained=true physical_audio=false");
            }
        }
    }

    #[test]
    fn reset_preparation_releases_partial_locks_and_revoked_or_discarded_plans_change_nothing() {
        let shared = reset_runtime();
        let monitor = shared.monitor(true);
        let generation = shared.demand.reserve().unwrap();
        assert!(shared.demand.activate(generation));
        let epoch = shared.demand.controls.capture_epoch.load(Ordering::Acquire);
        {
            let held = shared.demand.voice.lock().unwrap();
            assert!(shared.try_prepare_reset().unwrap().is_none());
            assert!(shared.state.try_lock().is_ok());
            drop(held);
        }
        {
            let held = shared.mic_test.try_prepare_dispose().unwrap().unwrap();
            assert!(shared.try_prepare_reset().unwrap().is_none());
            assert!(shared.demand.voice.try_lock().is_ok());
            drop(held);
        }
        {
            let held = shared.state.lock().unwrap();
            assert!(shared.try_prepare_reset().unwrap().is_none());
            assert!(shared.demand.voice.try_lock().is_ok());
            assert!(shared.mic_test.try_prepare_dispose().unwrap().is_some());
            drop(held);
        }
        for revoked in [false, true] {
            let intents = CriticalIntents::default();
            let ticket = intents.ticket();
            let mut prepared = shared.try_prepare_reset().unwrap().unwrap();
            if revoked {
                intents.request(CriticalAction::Logout);
                assert!(intents
                    .commit(ticket, IntentScope::Session, || prepared.apply())
                    .is_none());
            }
            drop(prepared);
            assert_eq!(shared.demand.active(), generation);
            assert!(shared.demand.is_reserved(generation));
            assert!(monitor.is_current());
            assert!(shared.mic_test.has_demand());
            assert_eq!(
                shared.demand.controls.capture_epoch.load(Ordering::Acquire),
                epoch
            );
        }
    }

    #[test]
    fn held_device_cancel_runs_after_intent_and_resource_release_and_cannot_reset_new_leases() {
        let shared = reset_runtime();
        let monitor = shared.monitor(true);
        let voice = shared.demand.reserve().unwrap();
        shared.demand.activate(voice);
        let (entered, waiting) = std::sync::mpsc::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        shared.state.lock().unwrap().device = Some(Arc::new(ResetDevice {
            entered,
            release: Mutex::new(blocked),
        }));
        let intents = Arc::new(CriticalIntents::default());
        let ticket = intents.ticket();
        let (requested, request_done) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let mut reset = shared.try_prepare_reset().unwrap().unwrap();
                assert!(intents
                    .commit(ticket, IntentScope::Session, || {
                        reset.apply();
                        reset.apply();
                    })
                    .is_some());
                drop(reset);
            });
            waiting.recv_timeout(Duration::from_secs(1)).unwrap();
            let current = shared.monitor(true);
            let generation = shared.demand.reserve().unwrap();
            assert!(shared.demand.activate(generation));
            assert!(!monitor.is_current());
            assert!(shared.state.try_lock().is_ok());
            scope.spawn(|| {
                intents.request(CriticalAction::Logout);
                requested.send(()).unwrap();
            });
            let prompt = request_done.recv_timeout(Duration::from_millis(1500));
            release.send(()).unwrap();
            prompt.unwrap();
            assert!(current.is_current());
            assert_eq!(shared.demand.active(), generation);
            assert!(shared.mic_test.has_demand());
        });
        println!("audio reset held=device-cancel original_ticket=true resource_locks_released=true priority_gate_free=true physical_audio=false");
    }

    #[test]
    fn poisoned_resource_preserves_legacy_partial_reset_without_clearing_poison_or_reporting_success(
    ) {
        for resource in ["voice", "mic-test", "runtime"] {
            let legacy = reset_runtime();
            let guarded = reset_runtime();
            let mut pending = Vec::new();
            let mut monitors = Vec::new();
            for shared in [&legacy, &guarded] {
                monitors.push(shared.monitor(true));
                let voice = shared.demand.reserve().unwrap();
                shared.demand.activate(voice);
                let next = Arc::new(Pending {
                    stop: CancellationToken::new(),
                    ready: watch::channel(None).0,
                    revoked: AtomicBool::new(false),
                });
                {
                    let mut state = shared.state.lock().unwrap();
                    state.pending = Some(next.clone());
                    state.failed = true;
                    state.idle_since = Some(Instant::now());
                }
                pending.push(next);
                assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if resource == "voice" {
                        let _held = shared.demand.voice.lock().unwrap();
                        panic!("Expected voice fixture poison");
                    } else if resource == "mic-test" {
                        let _held = shared.mic_test.try_prepare_dispose().unwrap().unwrap();
                        panic!("Expected MicTest fixture poison");
                    } else if resource == "runtime" {
                        let _held = shared.state.lock().unwrap();
                        panic!("Expected runtime fixture poison");
                    }
                }))
                .is_err());
            }
            legacy.reset();
            let mut reset = guarded.try_prepare_reset().unwrap().unwrap();
            assert!(!reset.resources_complete());
            reset.apply();
            reset.apply();
            drop(reset);
            assert_eq!(guarded.demand.active(), legacy.demand.active());
            assert_eq!(
                guarded
                    .demand
                    .controls
                    .capture_epoch
                    .load(Ordering::Acquire),
                legacy.demand.controls.capture_epoch.load(Ordering::Acquire)
            );
            assert_eq!(
                guarded.mic_test.snapshot(Instant::now()),
                legacy.mic_test.snapshot(Instant::now())
            );
            assert!(!monitors[0].is_current());
            assert!(!monitors[1].is_current());
            assert_eq!(
                pending[0].stop.is_cancelled(),
                pending[1].stop.is_cancelled()
            );
            for shared in [&legacy, &guarded] {
                assert_eq!(shared.demand.voice.is_poisoned(), resource == "voice");
                assert_eq!(shared.state.is_poisoned(), resource == "runtime");
                assert_eq!(
                    shared.mic_test.try_prepare_dispose().is_err(),
                    resource == "mic-test"
                );
                let state = shared
                    .state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                assert_eq!(state.failed, resource == "runtime");
                assert_eq!(state.idle_since.is_some(), resource == "runtime");
            }
            println!("audio reset poison={resource} legacy_partial=true poison_retained=true complete=false physical_audio=false");
        }
    }
}
