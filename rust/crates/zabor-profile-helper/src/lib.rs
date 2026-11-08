use serde::de::IgnoredAny;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use zeroize::Zeroizing;

#[cfg(windows)]
mod windows;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    UnsafePath,
    Untrusted,
    InvalidRequest,
    Sealed,
    AlreadyStarted,
    Cancelled,
    Timeout,
    Oversize,
    Incomplete,
    Process,
    Reader,
    Cleanup,
    Owner,
    Unsupported,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("profile helper operation failed")
    }
}

impl std::error::Error for Failure {}

#[derive(Clone, Debug, Default)]
pub struct Observation {
    pub operation: u64,
    pub pid: Option<u32>,
    pub owner_started: bool,
    pub owner_joined: bool,
    pub job_created: bool,
    pub assigned: bool,
    pub root_reaped: bool,
    pub exit_code: Option<u32>,
    pub job_empty: bool,
    pub active_process_peak: u32,
    pub reader_started: bool,
    pub reader_joined: bool,
    pub pipe_eof: bool,
    pub children_terminated: bool,
    pub handles_closed: bool,
    pub failure: Option<Failure>,
}

pub struct CompletedExport {
    pub bytes: Zeroizing<Vec<u8>>,
    pub observation: Observation,
}

pub struct TrustedFile {
    path: PathBuf,
    sha256: [u8; 32],
    _pin: std::fs::File,
}

impl TrustedFile {
    pub fn pinned(path: &Path, independently_trusted_sha256: [u8; 32]) -> Result<Self, Failure> {
        #[cfg(windows)]
        {
            windows::trusted(path, independently_trusted_sha256)
        }
        #[cfg(not(windows))]
        {
            let _ = (path, independently_trusted_sha256);
            Err(Failure::Unsupported)
        }
    }

    pub fn offline_signed(path: &Path) -> Result<Self, Failure> {
        #[cfg(windows)]
        {
            windows::signed(path)
        }
        #[cfg(not(windows))]
        {
            let _ = path;
            Err(Failure::Unsupported)
        }
    }

    pub fn sha256(&self) -> [u8; 32] {
        self.sha256
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub struct ExportRequest {
    runtime: TrustedFile,
    helper: TrustedFile,
    root: PathBuf,
    _root_pin: std::fs::File,
    fixture: bool,
    #[cfg(test)]
    panic_after_resume: bool,
    #[cfg(test)]
    fail_assignment: bool,
}

impl ExportRequest {
    pub fn copied_profile(
        runtime: TrustedFile,
        helper: TrustedFile,
        root: &Path,
    ) -> Result<Self, Failure> {
        #[cfg(windows)]
        {
            let (root, pin) = windows::snapshot(root)?;
            Ok(Self {
                runtime,
                helper,
                root,
                _root_pin: pin,
                fixture: false,
                #[cfg(test)]
                panic_after_resume: false,
                #[cfg(test)]
                fail_assignment: false,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = (runtime, helper, root);
            Err(Failure::Unsupported)
        }
    }

    #[cfg(any(test, feature = "test-source"))]
    pub fn synthetic_fixture(
        runtime: TrustedFile,
        helper: TrustedFile,
        root: &Path,
    ) -> Result<Self, Failure> {
        let mut request = Self::copied_profile(runtime, helper, root)?;
        request.fixture = true;
        Ok(request)
    }
}

#[derive(Clone, Copy)]
pub struct Limits {
    pub max_stdout: usize,
    pub timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_stdout: 64 * 1024 * 1024,
            timeout: Duration::from_secs(60),
        }
    }
}

struct ActorResult {
    bytes: Result<Zeroizing<Vec<u8>>, Failure>,
    observation: Observation,
}

struct Entry {
    id: u64,
    cancelled: AtomicBool,
    started: AtomicBool,
    open_handles: AtomicUsize,
    join: Mutex<Option<JoinHandle<ActorResult>>>,
    observation: Mutex<Observation>,
}

struct RegistryInner {
    sealed: AtomicBool,
    next: AtomicU64,
    entries: Mutex<BTreeMap<u64, Arc<Entry>>>,
}

#[derive(Clone)]
pub struct OwnerRegistry {
    inner: Arc<RegistryInner>,
}

impl Default for OwnerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl OwnerRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RegistryInner {
                sealed: AtomicBool::new(false),
                next: AtomicU64::new(1),
                entries: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    pub fn prepare(&self) -> Result<Operation, Failure> {
        let mut entries = self.inner.entries.lock().map_err(|_| Failure::Owner)?;
        if self.inner.sealed.load(Ordering::Acquire) {
            return Err(Failure::Sealed);
        }
        if entries.len() >= 128 {
            for entry in entries.values() {
                if entry.cancelled.load(Ordering::Acquire) {
                    if let Ok(Some(result)) = join(entry, true) {
                        drop(result.bytes);
                    }
                }
            }
            entries.retain(|_, entry| {
                let Ok(join) = entry.join.try_lock() else {
                    return true;
                };
                let Ok(observation) = entry.observation.try_lock() else {
                    return true;
                };
                !(join.is_none()
                    && entry.open_handles.load(Ordering::Acquire) == 0
                    && (observation.owner_joined || entry.cancelled.load(Ordering::Acquire)))
            });
            if entries.len() >= 128 {
                return Err(Failure::Owner);
            }
        }
        let id = self.inner.next.fetch_add(1, Ordering::Relaxed);
        if id == u64::MAX {
            return Err(Failure::Owner);
        }
        let entry = Arc::new(Entry {
            id,
            cancelled: AtomicBool::new(false),
            started: AtomicBool::new(false),
            open_handles: AtomicUsize::new(0),
            join: Mutex::new(None),
            observation: Mutex::new(Observation {
                operation: id,
                ..Observation::default()
            }),
        });
        entries.insert(id, entry.clone());
        Ok(Operation {
            registry: self.clone(),
            entry,
        })
    }

    pub fn seal(&self) {
        self.inner.sealed.store(true, Ordering::Release);
        if let Ok(entries) = self.inner.entries.lock() {
            for entry in entries.values() {
                entry.cancelled.store(true, Ordering::Release);
            }
        }
    }

    pub fn drain(&self) -> Result<Vec<Observation>, Failure> {
        self.seal();
        let entries: Vec<_> = self
            .inner
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .cloned()
            .collect();
        let mut observations = Vec::new();
        let mut failed = false;
        for entry in entries {
            match join(&entry, false) {
                Ok(Some(result)) => drop(result.bytes),
                Ok(None) => {}
                Err(_) => failed = true,
            }
            observations.push(
                entry
                    .observation
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone(),
            );
        }
        if failed {
            Err(Failure::Owner)
        } else {
            Ok(observations)
        }
    }
}

pub struct Operation {
    registry: OwnerRegistry,
    entry: Arc<Entry>,
}

#[derive(Clone)]
pub struct Cancellation {
    entry: Arc<Entry>,
}

impl Cancellation {
    pub fn id(&self) -> u64 {
        self.entry.id
    }
    pub fn cancel(&self) {
        self.entry.cancelled.store(true, Ordering::Release);
    }
    pub fn observation(&self) -> Result<Observation, Failure> {
        Ok(self
            .entry
            .observation
            .lock()
            .map_err(|_| Failure::Owner)?
            .clone())
    }
}

impl Operation {
    pub fn id(&self) -> u64 {
        self.entry.id
    }
    pub fn cancellation(&self) -> Cancellation {
        Cancellation {
            entry: self.entry.clone(),
        }
    }
    pub fn cancel(&self) {
        self.entry.cancelled.store(true, Ordering::Release);
    }
    pub fn observation(&self) -> Result<Observation, Failure> {
        Ok(self
            .entry
            .observation
            .lock()
            .map_err(|_| Failure::Owner)?
            .clone())
    }

    pub fn start(&self, request: ExportRequest, limits: Limits) -> Result<(), Failure> {
        if limits.max_stdout == 0
            || limits.max_stdout > 64 * 1024 * 1024
            || limits.timeout.is_zero()
            || limits.timeout > Duration::from_secs(300)
        {
            return Err(Failure::InvalidRequest);
        }
        let mut slot = self.entry.join.lock().map_err(|_| Failure::Owner)?;
        if self.registry.inner.sealed.load(Ordering::Acquire)
            || self.entry.cancelled.load(Ordering::Acquire)
        {
            return Err(Failure::Cancelled);
        }
        if self.entry.started.swap(true, Ordering::AcqRel) {
            return Err(Failure::AlreadyStarted);
        }
        let entry = self.entry.clone();
        let thread = std::thread::Builder::new()
            .name("profile-export-owner".into())
            .spawn(move || {
                #[cfg(windows)]
                {
                    windows::execute(request, limits, &entry)
                }
                #[cfg(not(windows))]
                {
                    let _ = (request, limits);
                    ActorResult {
                        bytes: Err(Failure::Unsupported),
                        observation: Observation {
                            operation: entry.id,
                            owner_started: true,
                            failure: Some(Failure::Unsupported),
                            ..Observation::default()
                        },
                    }
                }
            })
            .map_err(|_| Failure::Owner)?;
        *slot = Some(thread);
        Ok(())
    }

    pub fn finish(self) -> Result<CompletedExport, Failure> {
        let result = join(&self.entry, false)?;
        if self.entry.cancelled.load(Ordering::Acquire)
            || self.registry.inner.sealed.load(Ordering::Acquire)
        {
            return Err(Failure::Cancelled);
        }
        let result = result.ok_or(Failure::Owner)?;
        Ok(CompletedExport {
            bytes: result.bytes?,
            observation: result.observation,
        })
    }
}

impl Drop for Operation {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn join(entry: &Entry, only_finished: bool) -> Result<Option<ActorResult>, Failure> {
    let poisoned = entry.join.is_poisoned() || entry.observation.is_poisoned();
    let mut slot = if only_finished {
        match entry.join.try_lock() {
            Ok(slot) => slot,
            Err(std::sync::TryLockError::WouldBlock) => return Ok(None),
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        }
    } else {
        entry
            .join
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    };
    if only_finished && slot.as_ref().is_some_and(|thread| !thread.is_finished()) {
        return Ok(None);
    }
    let Some(thread) = slot.take() else {
        return Ok(None);
    };
    let mut result = match thread.join() {
        Ok(result) => result,
        Err(_) => {
            let mut observation = entry
                .observation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            observation.owner_joined = true;
            observation.failure = Some(Failure::Owner);
            return Err(Failure::Owner);
        }
    };
    result.observation.owner_joined = true;
    if entry.cancelled.load(Ordering::Acquire) {
        result.bytes = Err(Failure::Cancelled);
        result.observation.failure = Some(Failure::Cancelled);
    }
    if poisoned {
        result.bytes = Err(Failure::Owner);
        result.observation.failure = Some(Failure::Owner);
    }
    *entry
        .observation
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = result.observation.clone();
    Ok(Some(result))
}

impl Drop for RegistryInner {
    fn drop(&mut self) {
        let entries = self
            .entries
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for entry in entries.values() {
            entry.cancelled.store(true, Ordering::Release);
        }
        for entry in entries.values() {
            let _ = join(entry, false);
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope {
    version: u8,
    session: IgnoredAny,
    identity: Option<IgnoredAny>,
    app_settings: IgnoredAny,
    local_storage: IgnoredAny,
    messages: IgnoredAny,
}

fn validate(bytes: &[u8], fixture: bool) -> Result<(), Failure> {
    if bytes.is_empty() {
        return Err(Failure::Incomplete);
    }
    if fixture {
        serde_json::from_slice::<IgnoredAny>(bytes).map_err(|_| Failure::Incomplete)?;
    } else {
        let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_| Failure::Incomplete)?;
        if envelope.version != 1 {
            return Err(Failure::Incomplete);
        }
        let _ = (
            envelope.session,
            envelope.identity,
            envelope.app_settings,
            envelope.local_storage,
            envelope.messages,
        );
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests;
