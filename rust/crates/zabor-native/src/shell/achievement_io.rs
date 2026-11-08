use super::app_ui::{AchievementResult, Event};
use super::UiEvent;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use zabor_native::achievement_store::HiddenStore;
use zabor_native::achievements::{AchievementTarget, HiddenCatalog};
use zabor_native::critical_intents::{CriticalIntents, IntentScope};
use zabor_native::profile::{Profile, ProfileError};

enum BlockingState {
    Pending(JoinHandle<Result<(), ProfileError>>),
    Done(bool),
}

type BlockingJob = Arc<tokio::sync::Mutex<BlockingState>>;

struct StoragePolicy {
    ack_timeout: Duration,
    metadata_ordered: bool,
}

pub(super) struct AchievementIo {
    profile: Option<Arc<Profile>>,
    intents: Arc<CriticalIntents>,
    stop: CancellationToken,
    next: AtomicU64,
    jobs: Mutex<BTreeMap<u64, BlockingJob>>,
    failed: AtomicBool,
    metadata_ordered: bool,
}

pub(super) struct Job {
    pub(super) id: u64,
    pub(super) receiver: oneshot::Receiver<UiEvent>,
}

impl AchievementIo {
    pub(super) fn new(
        profile: Option<Arc<Profile>>,
        intents: Arc<CriticalIntents>,
        stop: CancellationToken,
    ) -> Arc<Self> {
        Arc::new(Self {
            profile,
            intents,
            stop,
            next: AtomicU64::new(0),
            jobs: Mutex::new(BTreeMap::new()),
            failed: AtomicBool::new(false),
            metadata_ordered: false,
        })
    }

    #[cfg(test)]
    pub(super) fn unverified_transaction_fixture(
        profile: Option<Arc<Profile>>,
        intents: Arc<CriticalIntents>,
        stop: CancellationToken,
    ) -> Arc<Self> {
        let mut io = Self::new(profile, intents, stop);
        Arc::get_mut(&mut io).unwrap().metadata_ordered = true;
        io
    }

    pub(super) fn start(
        &self,
        result: AchievementResult,
        update: Arc<dyn Fn(UiEvent) + Send + Sync>,
    ) -> Job {
        self.start_timed(result, update, Duration::from_secs(20))
    }

    fn start_timed(
        &self,
        result: AchievementResult,
        update: Arc<dyn Fn(UiEvent) + Send + Sync>,
        ack_timeout: Duration,
    ) -> Job {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        let profile = self.profile.clone();
        let intents = self.intents.clone();
        let stop = self.stop.clone();
        let runtime = tokio::runtime::Handle::current();
        let policy = StoragePolicy {
            ack_timeout,
            metadata_ordered: self.metadata_ordered,
        };
        let job = tokio::task::spawn_blocking(move || {
            let event = persist(
                result,
                profile.as_deref(),
                &intents,
                &stop,
                &runtime,
                update,
                policy,
            );
            let cleanup = match &event {
                Event::AchievementsStored { result: Err(_), .. } => Err(ProfileError::Storage),
                Event::Achievements(result) if result.storage_failed => Err(ProfileError::Storage),
                _ => Ok(()),
            };
            drop(profile);
            let _ = sender.send(UiEvent::App(event));
            cleanup
        });
        self.jobs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(
                id,
                Arc::new(tokio::sync::Mutex::new(BlockingState::Pending(job))),
            );
        Job { id, receiver }
    }

    pub(super) async fn join(&self, id: u64) -> Result<(), ProfileError> {
        let job = self
            .jobs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&id)
            .cloned();
        let succeeded = if let Some(job) = job {
            let mut job = job.lock().await;
            let succeeded = match &mut *job {
                BlockingState::Pending(handle) => {
                    let succeeded = matches!(handle.await, Ok(Ok(())));
                    if !succeeded {
                        self.failed.store(true, Ordering::Release);
                    }
                    *job = BlockingState::Done(succeeded);
                    succeeded
                }
                BlockingState::Done(succeeded) => *succeeded,
            };
            drop(job);
            self.jobs
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&id);
            succeeded
        } else {
            !self.failed.load(Ordering::Acquire)
        };
        if succeeded {
            Ok(())
        } else {
            Err(ProfileError::Storage)
        }
    }

    pub(super) async fn finish(&self) -> Result<(), ProfileError> {
        let ids: Vec<_> = self
            .jobs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .keys()
            .copied()
            .collect();
        for id in ids {
            let _ = self.join(id).await;
        }
        if self.failed.load(Ordering::Acquire) {
            Err(ProfileError::Storage)
        } else {
            Ok(())
        }
    }
}

fn persist(
    mut result: AchievementResult,
    profile: Option<&Profile>,
    intents: &CriticalIntents,
    stop: &CancellationToken,
    runtime: &tokio::runtime::Handle,
    update: Arc<dyn Fn(UiEvent) + Send + Sync>,
    policy: StoragePolicy,
) -> Event {
    let query = result.query.clone();
    let current = || {
        intents
            .commit(query.ticket(), IntentScope::Session, || {
                !stop.is_cancelled() && query.alive()
            })
            .unwrap_or(false)
    };
    let Some(profile) =
        profile.filter(|_| matches!(query.target(), AchievementTarget::Own) && current())
    else {
        return Event::Achievements(Box::new(result));
    };
    let store = match HiddenStore::open(profile, query.owner()) {
        Ok(store) => store,
        Err(_) => {
            result.storage_failed = true;
            return Event::Achievements(Box::new(result));
        }
    };
    let catalog = match store.load() {
        Ok(catalog) => catalog,
        Err(_) => {
            result.storage_failed = true;
            return Event::Achievements(Box::new(result));
        }
    };
    let plan = match result
        .result
        .as_ref()
        .ok()
        .map(|data| catalog.prepare_response(&query, data))
    {
        Some(Ok(Some(plan))) => plan,
        Some(Err(_)) => {
            result.catalog = Some(catalog);
            result.storage_failed = true;
            return Event::Achievements(Box::new(result));
        }
        _ => {
            result.catalog = Some(catalog);
            return Event::Achievements(Box::new(result));
        }
    };
    if !policy.metadata_ordered {
        result.catalog = Some(catalog);
        result.storage_failed = true;
        return Event::Achievements(Box::new(result));
    }
    let prepared = match store.prepare(&plan) {
        Ok(prepared) => prepared,
        Err(_) => {
            result.catalog = Some(catalog);
            result.storage_failed = true;
            return Event::Achievements(Box::new(result));
        }
    };
    result.catalog = Some(catalog);
    let owner_stop = intents.session_token(query.ticket());
    let completed = store.replace_if(prepared, current, || {
        if !current() {
            return None;
        }
        let (acknowledge, receipt) = oneshot::channel::<Option<HiddenCatalog>>();
        update(
            UiEvent::App(Event::AchievementProposal {
                result: Box::new(result),
                plan,
                acknowledge,
            })
            .guarded(query.ticket(), IntentScope::Session),
        );
        let accepted = runtime.block_on(async {
            tokio::select! {
                biased;
                _ = stop.cancelled() => None,
                _ = owner_stop.cancelled() => None,
                _ = query.cancelled() => None,
                receipt = tokio::time::timeout(policy.ack_timeout, receipt) => receipt.ok().and_then(Result::ok).flatten(),
            }
        });
        if current() {
            accepted
        } else {
            None
        }
    });
    drop(store);
    Event::AchievementsStored {
        query,
        result: completed,
    }
}

#[cfg(test)]
#[path = "achievement_io_tests.rs"]
mod tests;
