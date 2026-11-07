use crate::audio_runtime::{MonitorLease, SharedAudio, SharedAudioDevice};
use crate::mic_test::{MicTestFailure, MicTestIssue, MicTestSnapshot, MicTestStage};
use std::sync::{Arc, Weak};
use std::time::Instant;
use tokio::runtime::Handle;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

struct Preparation<D> {
    result: oneshot::Receiver<Result<Weak<D>, ()>>,
    task: JoinHandle<()>,
}

impl<D> Drop for Preparation<D> {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub struct MicTestUi<D: SharedAudioDevice> {
    shared: Arc<SharedAudio<D>>,
    owner: String,
    revoked_owner: String,
    lease: Option<Arc<MonitorLease<D>>>,
    preparation: Option<Preparation<D>>,
    device: Weak<D>,
    visible: bool,
    failed: bool,
}

impl<D: SharedAudioDevice> MicTestUi<D> {
    pub fn new(shared: Arc<SharedAudio<D>>) -> Self {
        Self {
            shared,
            owner: String::new(),
            revoked_owner: String::new(),
            lease: None,
            preparation: None,
            device: Weak::new(),
            visible: false,
            failed: false,
        }
    }

    pub fn close(&mut self) {
        if self.lease.as_ref().is_some_and(|lease| lease.is_current()) {
            self.shared.mic_test.configure(false, false);
        }
        self.preparation = None;
        self.lease = None;
        self.device = Weak::new();
        self.owner.clear();
        self.revoked_owner.clear();
        self.visible = false;
        self.failed = false;
    }

    pub fn synchronize(
        &mut self,
        owner: &str,
        open: bool,
        active: bool,
        enabled: bool,
        runtime: &Handle,
    ) {
        if !open || !enabled || owner.is_empty() {
            self.close();
            return;
        }
        if self.lease.as_ref().is_some_and(|lease| !lease.is_current()) {
            let revoked = self.owner.clone();
            self.close();
            self.revoked_owner = revoked;
        }
        if self.revoked_owner == owner {
            return;
        }
        if self.owner != owner {
            self.close();
        }
        let visible = active;
        let entering = visible && !self.visible;
        if self.lease.is_none() {
            self.owner = owner.into();
            self.lease = Some(Arc::new(self.shared.monitor(visible)));
        } else if visible != self.visible {
            if let Some(lease) = &self.lease {
                lease.configure(visible);
            }
        }
        self.visible = visible;
        if !visible
            && !matches!(
                self.shared.mic_test.snapshot(Instant::now()).stage,
                MicTestStage::Arming | MicTestStage::Recording
            )
        {
            self.preparation = None;
        }
        if entering {
            self.prepare(runtime);
        }
    }

    fn prepare(&mut self, runtime: &Handle) {
        if self.preparation.is_some() {
            return;
        }
        if self.device.upgrade().is_some_and(|device| !device.failed()) {
            self.failed = false;
            return;
        }
        let Some(lease) = self.lease.clone().filter(|lease| lease.is_current()) else {
            return;
        };
        self.failed = false;
        let (send, result) = oneshot::channel();
        let task = runtime.spawn(async move {
            let ready = lease
                .ready()
                .await
                .map(|device| Arc::downgrade(&device))
                .map_err(|_| ());
            let _ = send.send(ready);
        });
        self.preparation = Some(Preparation { result, task });
    }

    pub fn record(&mut self, now: Instant, runtime: &Handle) -> bool {
        if !self.visible || !self.lease.as_ref().is_some_and(|lease| lease.is_current()) {
            return false;
        }
        let stage = self.shared.mic_test.snapshot(now).stage;
        if matches!(stage, MicTestStage::Arming | MicTestStage::Recording) {
            return false;
        }
        if self.shared.mic_test.begin(now).is_none() {
            return false;
        }
        self.prepare(runtime);
        true
    }

    pub fn toggle_playback(&mut self) -> bool {
        if !self.visible || !self.lease.as_ref().is_some_and(|lease| lease.is_current()) {
            return false;
        }
        if self.shared.mic_test.snapshot(Instant::now()).playing {
            self.shared.mic_test.pause();
            true
        } else {
            !self.failed && self.shared.mic_test.play(None)
        }
    }

    pub fn seek(&self, seconds: f64) -> bool {
        self.visible
            && self.lease.as_ref().is_some_and(|lease| lease.is_current())
            && self.shared.mic_test.seek(seconds)
    }

    pub fn snapshot(&mut self, now: Instant) -> MicTestSnapshot {
        if self.lease.is_none() {
            return MicTestSnapshot::default();
        }
        if self.lease.as_ref().is_some_and(|lease| !lease.is_current()) {
            let revoked = self.owner.clone();
            self.close();
            self.revoked_owner = revoked;
            return MicTestSnapshot::default();
        }
        if let Some(preparation) = &mut self.preparation {
            match preparation.result.try_recv() {
                Ok(result) => {
                    self.preparation = None;
                    match result {
                        Ok(device) => self.device = device,
                        Err(()) => self.failed = true,
                    }
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    self.preparation = None;
                    self.failed = true;
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
            }
        }
        if self.device.upgrade().is_some_and(|device| device.failed()) {
            self.failed = true;
        }
        if self.failed {
            self.shared.mic_test.pause();
            if let Some(token) = self.shared.mic_test.capture_token() {
                self.shared.mic_test.fail(token, MicTestFailure::Processor);
            }
        }
        let mut snapshot = self.shared.mic_test.snapshot(now);
        if self.failed {
            snapshot.meter_db = -100.0;
            if snapshot.issue.is_none() {
                snapshot.issue = Some(MicTestIssue::Failed);
            }
        }
        snapshot
    }
}

impl<D: SharedAudioDevice> Drop for MicTestUi<D> {
    fn drop(&mut self) {
        self.close();
    }
}
