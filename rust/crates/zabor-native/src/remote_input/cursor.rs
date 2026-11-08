use crate::audio_runtime::{AudioCleanup, AudioCleanupError};
use crate::media::MediaError;
use std::sync::{mpsc, Arc};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use zabor_input::desktop::{CleanupObservation, CleanupState, ViewerCursorLease};
use zabor_input::InputError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorState {
    Starting,
    Ended,
    Failed,
}

#[derive(Clone)]
pub struct CursorCompletion {
    state: watch::Receiver<CursorState>,
    observation: CleanupObservation,
}

impl CursorCompletion {
    pub fn state(&self) -> CursorState {
        if self.state.has_changed().is_err() && *self.state.borrow() == CursorState::Starting {
            CursorState::Failed
        } else {
            *self.state.borrow()
        }
    }

    pub fn cleanup(&self) -> CleanupState {
        match (self.state(), self.observation.state()) {
            (CursorState::Failed, _) | (_, CleanupState::Failed) => CleanupState::Failed,
            (CursorState::Starting, _) => CleanupState::Pending,
            (_, cleanup) => cleanup,
        }
    }

    pub async fn wait(&mut self) -> Result<CleanupState, InputError> {
        while self.state() == CursorState::Starting {
            if self.state.changed().await.is_err() {
                return Err(InputError::Cleanup);
            }
        }
        if self.state() == CursorState::Failed
            || matches!(self.cleanup(), CleanupState::Pending | CleanupState::Failed)
        {
            Err(InputError::Cleanup)
        } else {
            Ok(self.cleanup())
        }
    }
}

pub(crate) struct CursorActor {
    stop: CancellationToken,
    lease: Arc<ViewerCursorLease>,
    completion: CursorCompletion,
}

struct CursorRetirement {
    stop: CancellationToken,
    lease: Arc<ViewerCursorLease>,
    status: watch::Sender<CursorState>,
    state: CursorState,
}

impl Drop for CursorRetirement {
    fn drop(&mut self) {
        self.lease.revoke();
        self.stop.cancel();
        self.status.send_replace(self.state);
    }
}

struct OnExit<F: FnOnce()>(Option<F>);

impl<F: FnOnce()> Drop for OnExit<F> {
    fn drop(&mut self) {
        if let Some(ended) = self.0.take() {
            ended();
        }
    }
}

pub(crate) fn register_surface(
    cleanup: Arc<AudioCleanup>,
) -> Result<(mpsc::Sender<Result<(), InputError>>, CursorCompletion), MediaError> {
    if tokio::runtime::Handle::try_current().is_err() {
        return Err(MediaError::Backend);
    }
    let prepared = cleanup.prepare_task().map_err(|_| MediaError::Backend)?;
    let observation = CleanupObservation::default();
    let worker_observation = observation.clone();
    let (sender, receiver) = mpsc::channel();
    let (status, state) = watch::channel(CursorState::Starting);
    prepared.spawn_blocking_result(move || {
        let failed = !matches!(receiver.recv(), Ok(Ok(())))
            || matches!(
                worker_observation.state(),
                CleanupState::Pending | CleanupState::Failed
            );
        status.send_replace(if failed {
            CursorState::Failed
        } else {
            CursorState::Ended
        });
        if failed {
            Err(AudioCleanupError::Incomplete)
        } else {
            Ok(())
        }
    });
    Ok((sender, CursorCompletion { state, observation }))
}

impl CursorCompletion {
    pub(crate) fn observation(&self) -> CleanupObservation {
        self.observation.clone()
    }
}

impl CursorActor {
    pub(crate) fn start(
        cleanup: Arc<AudioCleanup>,
        lease: Arc<ViewerCursorLease>,
        voice: CancellationToken,
        cancelled: impl Fn() -> bool + Send + 'static,
        ended: impl FnOnce() + Send + 'static,
    ) -> Result<Self, MediaError> {
        Self::start_with(cleanup, lease, voice, move |lease, stop, observation| {
            let _ended = OnExit(Some(ended));
            zabor_input::desktop::run_viewer_cursor(
                lease,
                || stop.is_cancelled() || cancelled(),
                observation,
            )
        })
    }

    pub(crate) fn start_with(
        cleanup: Arc<AudioCleanup>,
        lease: Arc<ViewerCursorLease>,
        voice: CancellationToken,
        run: impl FnOnce(
                Arc<ViewerCursorLease>,
                CancellationToken,
                CleanupObservation,
            ) -> Result<(), InputError>
            + Send
            + 'static,
    ) -> Result<Self, MediaError> {
        if tokio::runtime::Handle::try_current().is_err() {
            return Err(MediaError::Backend);
        }
        let prepared = cleanup.prepare_task().map_err(|_| MediaError::Backend)?;
        let stop = voice.child_token();
        let worker_stop = stop.clone();
        let worker_lease = lease.clone();
        let observation = CleanupObservation::default();
        let worker_observation = observation.clone();
        let (status, state) = watch::channel(CursorState::Starting);
        prepared.spawn_blocking_result(move || {
            let mut retirement = CursorRetirement {
                stop: worker_stop.clone(),
                lease: worker_lease.clone(),
                status,
                state: CursorState::Failed,
            };
            let result = run(
                worker_lease.clone(),
                worker_stop.clone(),
                worker_observation.clone(),
            );
            let failed = result == Err(InputError::Cleanup)
                || matches!(
                    worker_observation.state(),
                    CleanupState::Pending | CleanupState::Failed
                );
            retirement.state = if failed {
                CursorState::Failed
            } else {
                CursorState::Ended
            };
            drop(retirement);
            if failed {
                Err(AudioCleanupError::Incomplete)
            } else {
                Ok(())
            }
        });
        Ok(Self {
            stop,
            lease,
            completion: CursorCompletion { state, observation },
        })
    }

    pub(crate) fn completion(&self) -> CursorCompletion {
        self.completion.clone()
    }

    pub(crate) fn stop(&self) {
        self.lease.revoke();
        self.stop.cancel();
    }
}

impl Drop for CursorActor {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::Duration;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn original_cursor_join_remains_registered_after_drop_and_blocked_cleanup() {
        let cleanup = Arc::new(AudioCleanup::default());
        let voice = CancellationToken::new();
        let (release, blocked) = mpsc::sync_channel(1);
        let entered = Arc::new(AtomicBool::new(false));
        let worker_entered = entered.clone();
        let actor = CursorActor::start_with(
            cleanup.clone(),
            ViewerCursorLease::new(123),
            voice.clone(),
            move |_, stop, observation| {
                worker_entered.store(true, Ordering::Release);
                zabor_input::desktop::run_viewer_cursor(
                    ViewerCursorLease::new(0),
                    || true,
                    observation,
                )
                .unwrap();
                while !stop.is_cancelled() {
                    std::thread::park_timeout(Duration::from_millis(1));
                }
                blocked.recv().unwrap();
                Err(InputError::Cleanup)
            },
        )
        .unwrap();
        let mut completion = actor.completion();
        voice.cancel();
        drop(actor);
        assert!(
            tokio::time::timeout(Duration::from_millis(30), cleanup.wait())
                .await
                .is_err()
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(30), completion.wait())
                .await
                .is_err()
        );
        assert_eq!(completion.cleanup(), CleanupState::Pending);
        release.send(()).unwrap();
        assert!(matches!(
            cleanup.wait().await,
            Err(AudioCleanupError::Incomplete)
        ));
        assert_eq!(completion.wait().await, Err(InputError::Cleanup));
        assert_eq!(completion.cleanup(), CleanupState::Failed);
        assert!(entered.load(Ordering::Acquire));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cursor_unwind_is_a_failed_original_join_not_a_dropped_handle_success() {
        let cleanup = Arc::new(AudioCleanup::default());
        let actor = CursorActor::start_with(
            cleanup.clone(),
            ViewerCursorLease::new(123),
            CancellationToken::new(),
            |_, _, _| panic!("synthetic cursor unwind"),
        )
        .unwrap();
        let mut completion = actor.completion();
        drop(actor);
        assert!(matches!(
            cleanup.wait().await,
            Err(AudioCleanupError::Incomplete)
        ));
        assert_eq!(completion.wait().await, Err(InputError::Cleanup));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sealed_cleanup_refuses_acquisition_and_original_cancel_never_opens_native_cursor() {
        let cleanup = Arc::new(AudioCleanup::default());
        cleanup.seal().unwrap();
        let entered = Arc::new(AtomicBool::new(false));
        let worker_entered = entered.clone();
        assert!(CursorActor::start_with(
            cleanup.clone(),
            ViewerCursorLease::new(123),
            CancellationToken::new(),
            move |_, _, _| {
                worker_entered.store(true, Ordering::Release);
                Ok(())
            }
        )
        .is_err());
        assert!(!entered.load(Ordering::Acquire));
        cleanup.wait().await.unwrap();
        let cleanup = Arc::new(AudioCleanup::default());
        let voice = CancellationToken::new();
        voice.cancel();
        let lease = ViewerCursorLease::new(123);
        let actor = CursorActor::start_with(
            cleanup.clone(),
            lease.clone(),
            voice,
            |lease, stop, observation| {
                assert!(stop.is_cancelled());
                zabor_input::desktop::run_viewer_cursor(lease, || stop.is_cancelled(), observation)
            },
        )
        .unwrap();
        let mut completion = actor.completion();
        cleanup.wait().await.unwrap();
        assert_eq!(completion.wait().await, Ok(CleanupState::Restored));
        assert!(!lease.is_alive());
    }
}
