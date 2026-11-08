use crate::update_helper::{CrashPoint, Result, StageContext, UpdateRoot, ValidatedRelease};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub struct StagedArchive {
    root: Arc<UpdateRoot>,
    operation: u64,
    path: tempfile::TempPath,
    pub length: u64,
    pub sha256: String,
}

impl StagedArchive {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Clone, Debug)]
pub struct JobReport {
    pub operation: u64,
    pub result: Result<()>,
}

pub struct StageOptions {
    pub operation: u64,
    pub cancellation: CancellationToken,
    pub budget: Duration,
    pub now: u64,
    pub crash: CrashPoint,
}

struct Registry {
    jobs: Mutex<Vec<(u64, JoinHandle<JobReport>)>>,
    cancellations: Mutex<Vec<CancellationToken>>,
    sealed: Mutex<bool>,
    completed: Mutex<Vec<JobReport>>,
    draining: Mutex<bool>,
}

struct DrainBatch {
    registry: Arc<Registry>,
    jobs: Vec<(u64, JoinHandle<JobReport>)>,
}

impl Drop for DrainBatch {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.registry.jobs.lock() {
            jobs.append(&mut self.jobs);
        }
        if let Ok(mut draining) = self.registry.draining.lock() {
            *draining = false;
        }
    }
}

pub struct UpdateIo {
    root: Arc<UpdateRoot>,
    registry: Arc<Registry>,
}

#[must_use]
pub struct RegisteredJob<T> {
    operation: u64,
    root: Arc<UpdateRoot>,
    cancellation: CancellationToken,
    result: oneshot::Receiver<Result<T>>,
    completed: bool,
}

impl<T> RegisteredJob<T> {
    pub fn cancel(&self) {
        self.cancellation.cancel();
        self.root.cancel_operation(self.operation);
    }

    pub async fn wait(mut self) -> Result<T> {
        let result = tokio::select! {
            biased;
            _=self.cancellation.cancelled()=>return Err("operation-cancelled".into()),
            result=&mut self.result=>result.map_err(|_|"update-worker-result-lost")?,
        };
        self.root.check(
            self.operation,
            &self.cancellation,
            Instant::now() + Duration::from_secs(30),
        )?;
        self.completed = result.is_ok();
        result
    }
}

impl<T> Drop for RegisteredJob<T> {
    fn drop(&mut self) {
        if !self.completed {
            self.cancellation.cancel();
            self.root.cancel_operation(self.operation);
        }
    }
}

impl UpdateIo {
    pub fn new(root: Arc<UpdateRoot>) -> Self {
        Self {
            root,
            registry: Arc::new(Registry {
                jobs: Mutex::new(Vec::new()),
                cancellations: Mutex::new(Vec::new()),
                sealed: Mutex::new(false),
                completed: Mutex::new(Vec::new()),
                draining: Mutex::new(false),
            }),
        }
    }

    fn register<T: Send + 'static>(
        &self,
        operation: u64,
        cancellation: CancellationToken,
        work: impl std::future::Future<Output = Result<T>> + Send + 'static,
    ) -> Result<RegisteredJob<T>> {
        let sealed = self
            .registry
            .sealed
            .lock()
            .map_err(|_| "update-registry-poisoned")?;
        if *sealed {
            return Err("update-owner-sealed".into());
        }
        let (start, started) = oneshot::channel();
        let (send, result) = oneshot::channel();
        let retained = self.registry.clone();
        let join = tokio::spawn(async move {
            let output = match started.await {
                Ok(()) => work.await,
                Err(_) => Err("update-not-admitted".into()),
            };
            let status = output.as_ref().map(|_| ()).map_err(Clone::clone);
            let _ = send.send(output);
            drop(retained);
            JobReport {
                operation,
                result: status,
            }
        });
        self.registry
            .jobs
            .lock()
            .map_err(|_| "update-registry-poisoned")?
            .push((operation, join));
        self.registry
            .cancellations
            .lock()
            .map_err(|_| "update-registry-poisoned")?
            .push(cancellation.clone());
        start.send(()).map_err(|_| "update-admission-failed")?;
        Ok(RegisteredJob {
            operation,
            root: self.root.clone(),
            cancellation,
            result,
            completed: false,
        })
    }

    pub fn download(
        &self,
        operation: u64,
        candidate: ValidatedRelease,
        cancellation: CancellationToken,
        url: &str,
        budget: Duration,
    ) -> Result<RegisteredJob<Arc<StagedArchive>>> {
        if budget.is_zero() || budget > Duration::from_secs(600) {
            return Err("download-budget-invalid".into());
        }
        let address = reqwest::Url::parse(url).map_err(|_| "invalid-update-url")?;
        let manifest = candidate.release().manifest();
        let native_url = !self.root.sandbox_scoped()
            && address.scheme() == "https"
            && address.host_str() == Some("github.com")
            && address.path()
                == format!(
                    "/vnkdevelop/zabor-desktop/releases/download/v{}/{}",
                    manifest.version, manifest.package_name
                );
        let fixture_url = self.root.sandbox_scoped()
            && address.scheme() == "http"
            && address.host_str() == Some("127.0.0.1")
            && address.port().is_some();
        if (!native_url && !fixture_url)
            || !address.username().is_empty()
            || address.password().is_some()
            || address.query().is_some()
            || address.fragment().is_some()
        {
            return Err("update-url-policy-rejected".into());
        }
        let root = self.root.clone();
        let cancel = cancellation.clone();
        let expected_size = manifest.package_size;
        let expected_sha256 = manifest.package_sha256.clone();
        self.register(operation, cancellation, async move {
            let deadline = Instant::now() + budget;
            root.check(operation, &cancel, deadline)?;
            let owned = root.clone();
            let token = cancel.clone();
            let mut file = tokio::task::spawn_blocking(move || {
                owned.check(operation, &token, deadline)?;
                tempfile::Builder::new()
                    .prefix(&format!("download-{operation}-"))
                    .suffix(".part")
                    .tempfile_in(owned.path())
                    .map_err(|_| "download-stage-open-failed".to_string())
            })
            .await
            .map_err(|_| "download-stage-owner-failed")??;
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .connect_timeout(budget.min(Duration::from_secs(5)))
                .timeout(budget)
                .build()
                .map_err(|_| "native-http-client-failed")?;
            let mut response = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err("operation-cancelled".into()),
                value = client.get(address).send() => value.map_err(|_| "update-request-failed")?,
            };
            if response.status() != reqwest::StatusCode::OK
                || response
                    .content_length()
                    .is_some_and(|length| length != expected_size)
            {
                return Err("update-response-length-or-status".into());
            }
            let mut length = 0_u64;
            let mut hash = Sha256::new();
            loop {
                root.check(operation, &cancel, deadline)?;
                let chunk = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Err("operation-cancelled".into()),
                    value = response.chunk() => value.map_err(|_| "update-response-read-failed")?,
                };
                let Some(chunk) = chunk else {
                    break;
                };
                if chunk.len() > 1024 * 1024 {
                    return Err("update-chunk-too-large".into());
                }
                length = length
                    .checked_add(chunk.len() as u64)
                    .ok_or("download-length-overflow")?;
                if length > expected_size {
                    return Err("download-exceeds-signed-length".into());
                }
                hash.update(&chunk);
                let owned = root.clone();
                let token = cancel.clone();
                file = tokio::task::spawn_blocking(move || {
                    owned.check(operation, &token, deadline)?;
                    file.write_all(&chunk)
                        .map_err(|_| "download-disk-write-failed")?;
                    owned.check(operation, &token, deadline)?;
                    Ok::<_, String>(file)
                })
                .await
                .map_err(|_| "download-disk-owner-failed")??;
            }
            let sha256 = format!("{:x}", hash.finalize());
            if length != expected_size || sha256 != expected_sha256 {
                return Err("download-digest-or-length-mismatch".into());
            }
            let owned = root.clone();
            let token = cancel.clone();
            let path = tokio::task::spawn_blocking(move || {
                owned.check(operation, &token, deadline)?;
                file.as_file()
                    .sync_all()
                    .map_err(|_| "download-flush-failed")?;
                owned.check(operation, &token, deadline)?;
                Ok::<_, String>(file.into_temp_path())
            })
            .await
            .map_err(|_| "download-final-owner-failed")??;
            root.check(operation, &cancel, deadline)?;
            Ok(Arc::new(StagedArchive {
                root,
                operation,
                path,
                length,
                sha256,
            }))
        })
    }

    pub fn stage(
        &self,
        options: StageOptions,
        candidate: ValidatedRelease,
        archive: Arc<StagedArchive>,
    ) -> Result<RegisteredJob<()>> {
        let StageOptions {
            operation,
            cancellation,
            budget,
            now,
            crash,
        } = options;
        if operation != archive.operation
            || !Arc::ptr_eq(&archive.root, &self.root)
            || archive.sha256 != candidate.release().manifest().package_sha256
            || archive.length != candidate.release().manifest().package_size
            || budget.is_zero()
            || budget > Duration::from_secs(600)
        {
            return Err("stage-original-owner-mismatch".into());
        }
        let root = self.root.clone();
        let token = cancellation.clone();
        self.register(operation, cancellation, async move {
            tokio::task::spawn_blocking(move || {
                root.prepare(
                    StageContext {
                        operation,
                        cancellation: &token,
                        deadline: Instant::now() + budget,
                        now,
                        crash,
                    },
                    candidate,
                    archive.path(),
                )
            })
            .await
            .map_err(|_| "stage-native-owner-failed")?
        })
    }

    pub async fn drain(&self) -> Result<Vec<JobReport>> {
        *self
            .registry
            .sealed
            .lock()
            .map_err(|_| "update-registry-poisoned")? = true;
        for cancellation in self
            .registry
            .cancellations
            .lock()
            .map_err(|_| "update-registry-poisoned")?
            .iter()
        {
            cancellation.cancel();
        }
        let jobs = {
            let mut draining = self
                .registry
                .draining
                .lock()
                .map_err(|_| "update-registry-poisoned")?;
            if *draining {
                return Err("update-drain-already-observed".into());
            }
            *draining = true;
            std::mem::take(
                &mut *self
                    .registry
                    .jobs
                    .lock()
                    .map_err(|_| "update-registry-poisoned")?,
            )
        };
        let mut batch = DrainBatch {
            registry: self.registry.clone(),
            jobs,
        };
        while let Some((operation, join)) = batch.jobs.last_mut() {
            let original_operation = *operation;
            let result = join.await.unwrap_or_else(|_| JobReport {
                operation: original_operation,
                result: Err("original-update-owner-panicked".into()),
            });
            batch.jobs.pop();
            self.registry
                .completed
                .lock()
                .map_err(|_| "update-registry-poisoned")?
                .push(result);
        }
        Ok(self
            .registry
            .completed
            .lock()
            .map_err(|_| "update-registry-poisoned")?
            .clone())
    }
}

impl Drop for UpdateIo {
    fn drop(&mut self) {
        if let Ok(tokens) = self.registry.cancellations.lock() {
            for token in tokens.iter() {
                token.cancel();
            }
        }
    }
}

#[cfg(test)]
#[path = "update_io_tests.rs"]
mod tests;
