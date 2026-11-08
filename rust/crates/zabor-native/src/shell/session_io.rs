use std::future::Future;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use zabor_native::auth::Credentials;
use zabor_native::critical_intents::{CriticalIntents, IntentScope, IntentTicket};
use zabor_native::profile::{PreparedSession, Profile, ProfileError, SavedSession};

pub(super) struct SessionIo {
    profile: Option<Arc<Profile>>,
    intents: Arc<CriticalIntents>,
    stop: CancellationToken,
    jobs: Mutex<Vec<JoinHandle<()>>>,
    failed: Arc<AtomicBool>,
}

struct JobFailure(Arc<AtomicBool>);

impl Drop for JobFailure {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.store(true, Ordering::Release);
        }
    }
}

impl SessionIo {
    pub(super) fn new(
        profile: Option<Arc<Profile>>,
        intents: Arc<CriticalIntents>,
        stop: CancellationToken,
    ) -> Arc<Self> {
        Arc::new(Self {
            profile,
            intents,
            stop,
            jobs: Mutex::new(Vec::new()),
            failed: Arc::new(AtomicBool::new(false)),
        })
    }

    fn dispatch<T: Send + 'static>(
        &self,
        record_failure: bool,
        operation: impl FnOnce() -> Result<T, ProfileError> + Send + 'static,
    ) -> oneshot::Receiver<Result<T, ProfileError>> {
        let (sender, receiver) = oneshot::channel();
        let failed = self.failed.clone();
        let job = tokio::task::spawn_blocking(move || {
            let _failure = JobFailure(failed.clone());
            let result = operation();
            if record_failure && result.is_err() {
                failed.store(true, Ordering::Release);
            }
            let _ = sender.send(result);
        });
        let mut jobs = self.jobs.lock().unwrap_or_else(|error| error.into_inner());
        jobs.retain(|job| !job.is_finished());
        jobs.push(job);
        receiver
    }

    pub(super) async fn load(&self) -> Result<Option<SavedSession>, ProfileError> {
        let Some(profile) = self.profile.clone() else {
            return Ok(None);
        };
        self.dispatch(false, move || profile.load_session())
            .await
            .map_err(|_| ProfileError::Storage)?
    }

    pub(super) async fn prepare(
        &self,
        owner: String,
        credentials: Credentials,
    ) -> Result<PreparedSession, ProfileError> {
        let Some(profile) = self.profile.clone() else {
            return Err(ProfileError::Storage);
        };
        self.dispatch(false, move || profile.prepare_session(&owner, &credentials))
            .await
            .map_err(|_| ProfileError::Storage)?
    }

    pub(super) async fn save(
        &self,
        ticket: IntentTicket,
        prepared: PreparedSession,
    ) -> Result<bool, ProfileError> {
        let Some(profile) = self.profile.clone() else {
            return Err(ProfileError::Storage);
        };
        let intents = self.intents.clone();
        let stop = self.stop.clone();
        let owner_stop = intents.session_token(ticket);
        let operation_stop = owner_stop.child_token();
        let _cancel = operation_stop.clone().drop_guard();
        self.dispatch(true, move || {
            profile.commit_session_if(
                prepared,
                || {
                    intents
                        .commit(ticket, IntentScope::Session, || {
                            !stop.is_cancelled() && !operation_stop.is_cancelled()
                        })
                        .unwrap_or(false)
                },
                || owner_stop.is_cancelled(),
            )
        })
        .await
        .map_err(|_| ProfileError::Storage)?
    }

    pub(super) fn clear(
        &self,
        ticket: IntentTicket,
    ) -> impl Future<Output = Result<bool, ProfileError>> + Send + 'static {
        let receiver = self.profile.clone().map(|profile| {
            let intents = self.intents.clone();
            self.dispatch(true, move || {
                profile.clear_session_if(|| {
                    intents
                        .commit(ticket, IntentScope::Session, || true)
                        .unwrap_or(false)
                })
            })
        });
        async move {
            match receiver {
                Some(receiver) => receiver.await.map_err(|_| ProfileError::Storage)?,
                None => Ok(true),
            }
        }
    }

    pub(super) async fn finish(&self) -> Result<(), ProfileError> {
        let jobs = {
            let mut jobs = self.jobs.lock().unwrap_or_else(|error| error.into_inner());
            std::mem::take(&mut *jobs)
        };
        for job in jobs {
            if job.await.is_err() {
                self.failed.store(true, Ordering::Release);
            }
        }
        if self.failed.load(Ordering::Acquire) {
            Err(ProfileError::Storage)
        } else {
            Ok(())
        }
    }
}
