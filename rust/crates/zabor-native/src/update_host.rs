use crate::update_helper::{DrainVerifier, Result, UpdateRoot};
use std::collections::BTreeSet;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

#[path = "update_host_process.rs"]
pub mod process;

#[path = "update_bootstrap.rs"]
pub mod bootstrap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResourceKind {
    Session,
    Media,
    Input,
    Files,
    Profile,
}

#[derive(Clone, Debug)]
pub struct OwnerReport {
    pub operation: u64,
    pub result: Result<()>,
}

struct Entry {
    kind: Option<ResourceKind>,
    join: JoinHandle<OwnerReport>,
}

struct State {
    sealed: bool,
    observing: bool,
    entries: Vec<Entry>,
    reports: Vec<(Option<ResourceKind>, OwnerReport)>,
}

struct Registry {
    operation: u64,
    nonce: Zeroizing<Vec<u8>>,
    deadline: Instant,
    cancellation: CancellationToken,
    state: Mutex<State>,
}

struct Batch {
    registry: Arc<Registry>,
    entries: Vec<Entry>,
}

impl Drop for Batch {
    fn drop(&mut self) {
        if let Ok(mut state) = self.registry.state.lock() {
            state.entries.append(&mut self.entries);
            state.observing = false;
        }
    }
}

#[must_use]
pub struct HostJob<T> {
    result: oneshot::Receiver<Result<T>>,
    cancellation: CancellationToken,
    completed: bool,
}

impl<T> HostJob<T> {
    pub async fn wait(mut self) -> Result<T> {
        let result = tokio::select! {
            biased;
            _ = self.cancellation.cancelled() => Err("host-waiter-cancelled".into()),
            result = &mut self.result => result.map_err(|_| "host-result-lost")?,
        };
        self.completed = result.is_ok();
        result
    }

    pub fn cancel(&self) {
        self.cancellation.cancel();
    }
}

impl<T> Drop for HostJob<T> {
    fn drop(&mut self) {
        if !self.completed {
            self.cancellation.cancel();
        }
    }
}

pub struct UpdateHost {
    registry: Arc<Registry>,
}

impl UpdateHost {
    pub fn new(operation: u64, budget: Duration) -> Result<Self> {
        if operation == 0 || budget.is_zero() || budget > Duration::from_secs(600) {
            return Err("host-operation-or-budget-invalid".into());
        }
        let mut nonce = Zeroizing::new(vec![0; 32]);
        getrandom::fill(&mut nonce).map_err(|_| "host-owner-random-failed")?;
        Ok(Self {
            registry: Arc::new(Registry {
                operation,
                nonce,
                deadline: Instant::now() + budget,
                cancellation: CancellationToken::new(),
                state: Mutex::new(State {
                    sealed: false,
                    observing: false,
                    entries: Vec::new(),
                    reports: Vec::new(),
                }),
            }),
        })
    }

    pub fn operation(&self) -> u64 {
        self.registry.operation
    }

    pub fn cancellation(&self) -> CancellationToken {
        self.registry.cancellation.clone()
    }

    pub fn app_drain_owner(&self) -> Self {
        Self {
            registry: Arc::new(Registry {
                operation: self.operation(),
                nonce: self.registry.nonce.clone(),
                deadline: self.deadline(),
                cancellation: CancellationToken::new(),
                state: Mutex::new(State {
                    sealed: false,
                    observing: false,
                    entries: Vec::new(),
                    reports: Vec::new(),
                }),
            }),
        }
    }

    pub(crate) fn deadline(&self) -> Instant {
        self.registry.deadline
    }

    pub(crate) fn nonce(&self) -> &[u8] {
        &self.registry.nonce
    }

    fn register<T: Send + 'static, F: Future<Output = Result<T>> + Send + 'static>(
        &self,
        kind: Option<ResourceKind>,
        build: impl FnOnce(CancellationToken, Instant) -> F + Send + 'static,
    ) -> Result<HostJob<T>> {
        let (start, barrier) = oneshot::channel();
        let (send, result) = oneshot::channel();
        let registry = self.registry.clone();
        let mut state = registry
            .state
            .lock()
            .map_err(|_| "host-registry-poisoned")?;
        if state.sealed
            || registry.cancellation.is_cancelled()
            || Instant::now() >= registry.deadline
        {
            drop(state);
            return Err("host-admission-closed".into());
        }
        let retained = registry.clone();
        let join = tokio::spawn(async move {
            let output = if barrier.await.is_ok() {
                build(retained.cancellation.clone(), retained.deadline).await
            } else {
                Err("host-not-admitted".into())
            };
            let report = OwnerReport {
                operation: retained.operation,
                result: output.as_ref().map(|_| ()).map_err(Clone::clone),
            };
            let _ = send.send(output);
            report
        });
        state.entries.push(Entry { kind, join });
        drop(state);
        start.send(()).map_err(|_| "host-admission-failed")?;
        Ok(HostJob {
            result,
            cancellation: registry.cancellation.clone(),
            completed: false,
        })
    }

    pub(crate) fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce(CancellationToken, Instant) -> Result<T> + Send + 'static,
    ) -> Result<HostJob<T>> {
        self.register(None, move |cancel, deadline| async move {
            tokio::task::spawn_blocking(move || work(cancel, deadline))
                .await
                .map_err(|_| "original-host-blocking-owner-panicked")?
        })
    }

    pub fn register_resource<F: Future<Output = Result<()>> + Send + 'static>(
        &self,
        kind: ResourceKind,
        shutdown: impl FnOnce(CancellationToken) -> F + Send + 'static,
    ) -> Result<HostJob<()>> {
        self.register(Some(kind), move |cancel, _| shutdown(cancel))
    }

    async fn observe(&self, cancel: bool) -> Result<Vec<(Option<ResourceKind>, OwnerReport)>> {
        if cancel {
            self.registry.cancellation.cancel();
        }
        let entries = {
            let mut state = self
                .registry
                .state
                .lock()
                .map_err(|_| "host-registry-poisoned")?;
            state.sealed = true;
            if state.observing {
                return Err("host-drain-already-owned".into());
            }
            state.observing = true;
            std::mem::take(&mut state.entries)
        };
        let mut batch = Batch {
            registry: self.registry.clone(),
            entries,
        };
        while let Some(entry) = batch.entries.last_mut() {
            let kind = entry.kind;
            let report = (&mut entry.join).await.unwrap_or_else(|_| OwnerReport {
                operation: self.operation(),
                result: Err("original-host-owner-panicked-or-aborted".into()),
            });
            batch.entries.pop();
            self.registry
                .state
                .lock()
                .map_err(|_| "host-registry-poisoned")?
                .reports
                .push((kind, report));
        }
        let reports = self
            .registry
            .state
            .lock()
            .map_err(|_| "host-registry-poisoned")?
            .reports
            .clone();
        Ok(reports)
    }

    pub async fn drain(&self) -> Result<Vec<OwnerReport>> {
        Ok(self
            .observe(true)
            .await?
            .into_iter()
            .map(|(_, report)| report)
            .collect())
    }

    pub async fn attest_app_drain(&self, root: &Arc<UpdateRoot>) -> Result<AppDrainEvidence> {
        let identity = root.host_identity()?;
        if !identity.sandbox {
            return Err("primary-original-app-drain-binding-required".into());
        }
        let reports = self.observe(false).await?;
        let required = [
            ResourceKind::Session,
            ResourceKind::Media,
            ResourceKind::Input,
            ResourceKind::Files,
            ResourceKind::Profile,
        ];
        let kinds = reports
            .iter()
            .filter_map(|(kind, report)| {
                if report.operation == self.operation() && report.result.is_ok() {
                    *kind
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();
        if self.registry.cancellation.is_cancelled()
            || reports.iter().any(|(_, report)| report.result.is_err())
            || required.iter().any(|kind| !kinds.contains(kind))
        {
            return Err("real-app-resource-joins-not-attested".into());
        }
        Ok(AppDrainEvidence {
            registry: self.registry.clone(),
            root: root.clone(),
            manifest_sha256: identity.current_manifest_sha256,
            consumed: AtomicBool::new(false),
        })
    }
}

impl Drop for UpdateHost {
    fn drop(&mut self) {
        self.registry.cancellation.cancel();
    }
}

pub struct AppDrainEvidence {
    registry: Arc<Registry>,
    root: Arc<UpdateRoot>,
    manifest_sha256: String,
    consumed: AtomicBool,
}

impl AppDrainEvidence {
    pub(crate) fn belongs_to(&self, host: &UpdateHost, root: &Arc<UpdateRoot>) -> bool {
        self.registry.operation == host.operation()
            && Arc::ptr_eq(&self.root, root)
            && self.registry.nonce.as_slice() == host.nonce()
    }
}

impl DrainVerifier for AppDrainEvidence {
    fn verify(&self, operation: u64, previous_manifest_sha256: &str, sandbox: bool) -> Result<()> {
        if !sandbox
            || !self.root.sandbox_scoped()
            || operation != self.registry.operation
            || previous_manifest_sha256 != self.manifest_sha256
            || self.registry.cancellation.is_cancelled()
            || Instant::now() >= self.registry.deadline
            || !self
                .registry
                .state
                .lock()
                .map_err(|_| "host-registry-poisoned")?
                .sealed
            || self.consumed.swap(true, Ordering::AcqRel)
        {
            return Err("original-app-drain-evidence-rejected".into());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "update_host_tests.rs"]
mod tests;
