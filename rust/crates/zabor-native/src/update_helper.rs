use crate::release::NATIVE_BETA_PROFILE;
use crate::update::{VerifiedRelease, MAX_PACKAGE_BYTES};
use fs2::FileExt;
use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

pub type Result<T> = std::result::Result<T, String>;
const STATE_LIMIT: usize = 1024 * 1024;
const ACCEPTANCE_DOMAIN: &[u8] = b"ZABOR-NATIVE-ACCEPTANCE-V1\0";

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn safe_path(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 240
        && name.split('/').all(|part| {
            let base = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with('.')
                && part
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
                && !matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                && !(base.len() == 4
                    && (base.starts_with("COM") || base.starts_with("LPT"))
                    && (b'1'..=b'9').contains(&base.as_bytes()[3]))
        })
}

pub fn no_reparse(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        if matches!(component, Component::ParentDir | Component::CurDir) {
            return Err("unsafe-path-component".into());
        }
        current.push(component);
        let metadata = std::fs::symlink_metadata(&current).map_err(|_| "path-unavailable")?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("reparse-path-rejected".into());
            }
        }
        if metadata.file_type().is_symlink() {
            return Err("symlink-path-rejected".into());
        }
    }
    Ok(())
}

pub fn open_read(path: &Path) -> Result<File> {
    no_reparse(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000).share_mode(1);
    }
    let file = options.open(path).map_err(|_| "file-open-failed")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if file
            .metadata()
            .map_err(|_| "file-stat-failed")?
            .file_attributes()
            & 0x400
            != 0
        {
            return Err("reparse-file-rejected".into());
        }
    }
    if !file.metadata().map_err(|_| "file-stat-failed")?.is_file() {
        return Err("not-a-regular-file".into());
    }
    Ok(file)
}

pub(crate) struct HeldExecutable {
    pub path: PathBuf,
    image: File,
    _namespace: Vec<File>,
    proofs: Vec<File>,
}

impl HeldExecutable {
    fn open(path: &Path) -> Result<Self> {
        let parent = path.parent().ok_or("host-image-parent-missing")?;
        let mut namespace = Vec::new();
        let mut current = PathBuf::new();
        for component in parent.components() {
            if matches!(component, Component::ParentDir | Component::CurDir) {
                return Err("unsafe-path-component".into());
            }
            current.push(component);
            if matches!(component, Component::Prefix(_)) {
                continue;
            }
            let mut options = OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.custom_flags(0x0220_0000).share_mode(3);
            }
            let directory = options
                .open(&current)
                .map_err(|_| "host-namespace-lease-failed")?;
            let metadata = directory
                .metadata()
                .map_err(|_| "host-namespace-stat-failed")?;
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err("host-namespace-reparse-rejected".into());
                }
            }
            if !metadata.is_dir() {
                return Err("host-namespace-not-directory".into());
            }
            namespace.push(directory);
        }
        Ok(Self {
            path: path.to_owned(),
            image: open_read(path)?,
            _namespace: namespace,
            proofs: Vec::new(),
        })
    }

    fn verify(&mut self, size: u64, expected: &str, exact: bool) -> Result<()> {
        self.image
            .seek(SeekFrom::Start(0))
            .map_err(|_| "held-image-seek-failed")?;
        let (length, hash) = hash_reader(&mut self.image, size)?;
        if hash != expected || (exact && length != size) {
            return Err("held-executable-pin-mismatch".into());
        }
        Ok(())
    }
}

#[cfg(test)]
thread_local! {
    static HOST_VERIFICATION_HOOK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_host_verification_hook(hook: impl FnOnce() + 'static) {
    HOST_VERIFICATION_HOOK.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(hook));
    });
}

#[cfg(test)]
fn host_verification_hook() {
    let hook = HOST_VERIFICATION_HOOK.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

fn read_limited(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut file = open_read(path)?;
    if file.metadata().map_err(|_| "file-stat-failed")?.len() > limit as u64 {
        return Err("file-too-large".into());
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "file-read-failed")?;
    if bytes.len() > limit {
        return Err("file-too-large".into());
    }
    Ok(bytes)
}

#[derive(Clone)]
pub struct NativePlatform {
    executable: PathBuf,
    sha256: String,
    signer_verified: bool,
}

struct OwnedProcess(Child);

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

impl NativePlatform {
    pub(crate) fn host_executable(&self, sandbox: bool) -> Result<HeldExecutable> {
        if !sandbox && !self.signer_verified {
            return Err("certified-native-host-required".into());
        }
        let mut held = HeldExecutable::open(&self.executable)?;
        held.verify(16 * 1024 * 1024, &self.sha256, false)?;
        Ok(held)
    }

    pub fn pinned(executable: &Path, sha256: &str) -> Result<Self> {
        if sha256.len() != 64 || sha256 == "0".repeat(64) {
            return Err("native-helper-pin-missing".into());
        }
        let path = plain_canonical(executable)?;
        let bytes = read_limited(&path, 16 * 1024 * 1024)?;
        if digest(&bytes) != sha256 {
            return Err("native-helper-pin-mismatch".into());
        }
        Ok(Self {
            executable: path,
            sha256: sha256.into(),
            signer_verified: false,
        })
    }

    pub fn run(&self, verb: &str, arguments: &[String]) -> Result<()> {
        let held = open_read(&self.executable)?;
        let bytes = read_limited(&self.executable, 16 * 1024 * 1024)?;
        if digest(&bytes) != self.sha256 {
            return Err("native-helper-changed".into());
        }
        if arguments.iter().any(|part| part.contains(['\r', '\n'])) {
            return Err("unsupported-helper-argument".into());
        }
        let child = Command::new(&self.executable)
            .arg(verb)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "native-helper-start-failed")?;
        let mut child = OwnedProcess(child);
        let started = Instant::now();
        loop {
            match child
                .0
                .try_wait()
                .map_err(|_| "native-helper-observe-failed")?
            {
                Some(status) => {
                    drop(held);
                    return if status.success() {
                        Ok(())
                    } else {
                        Err("native-helper-failed-closed".into())
                    };
                }
                None if started.elapsed() >= Duration::from_secs(30) => {
                    let _ = child.0.kill();
                    let _ = child.0.wait();
                    return Err("native-helper-timeout".into());
                }
                None => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    }

    pub fn certify(mut self, publisher_certificate_sha256: &str) -> Result<Self> {
        self.run(
            "verify-pe",
            &[
                self.executable.to_string_lossy().into_owned(),
                publisher_certificate_sha256.into(),
            ],
        )?;
        self.signer_verified = true;
        Ok(self)
    }
}

fn plain_canonical(path: &Path) -> Result<PathBuf> {
    no_reparse(path)?;
    let canonical = path.canonicalize().map_err(|_| "canonical-path-failed")?;
    let text = canonical.to_string_lossy();
    Ok(PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text)))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignedBundle {
    pub manifest: Vec<u8>,
    pub signature: Vec<u8>,
    pub acceptance: Vec<u8>,
    pub acceptance_signature: Vec<u8>,
}

#[derive(Clone)]
pub struct TrustPins {
    pub manifest_key: Vec<u8>,
    pub acceptance_key: Vec<u8>,
    pub publisher_certificate_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptanceReceipt {
    pub receipt_version: u32,
    pub kind: String,
    pub profile: String,
    pub scope_id: String,
    pub version: String,
    pub commit: String,
    pub inventory_sha256: String,
    pub publisher_certificate_sha256: String,
    pub data_schema: u32,
    pub complete_native_acceptance: bool,
    pub windows_10_11_acceptance: bool,
    pub two_device_acceptance: bool,
    pub data_compatibility_acceptance: bool,
    pub licenses_accepted: bool,
}

#[derive(Clone)]
pub struct ValidatedRelease {
    release: VerifiedRelease,
    bundle: SignedBundle,
    scope: String,
}

impl ValidatedRelease {
    pub fn release(&self) -> &VerifiedRelease {
        &self.release
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VersionReceipt {
    key: String,
    manifest_sha256: String,
    bundle: SignedBundle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurablePhase {
    Idle,
    Prepared,
    Switching,
    AwaitingHealth,
    Committed,
    RolledBack,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pending {
    target: VersionReceipt,
    previous: VersionReceipt,
    challenge: Zeroizing<Vec<u8>>,
    health_deadline: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Journal {
    format: u32,
    profile: String,
    scope: String,
    last_operation: u64,
    highest_sequence: u64,
    phase: DurablePhase,
    current: VersionReceipt,
    pending: Option<Pending>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashPoint {
    None,
    VersionRenamed,
    Prepared,
    Switching,
    SelectorSwitched,
    AwaitingHealth,
    Committed,
    RollbackSelector,
}

pub struct UpdateRoot {
    path: PathBuf,
    identity: Vec<u8>,
    scope: String,
    pins: TrustPins,
    platform: NativePlatform,
    _lease: File,
    journal: Mutex<Journal>,
    active: AtomicU64,
    current_cancellation: Mutex<CancellationToken>,
    cancelled_operation: AtomicU64,
    blocked: AtomicBool,
}

pub struct HealthChallenge {
    pub operation: u64,
    pub manifest_sha256: String,
    pub nonce: Zeroizing<Vec<u8>>,
}

pub struct HostRootIdentity {
    pub root_sha256: String,
    pub current_manifest_sha256: String,
    pub sandbox: bool,
    pub health_deadline: Option<u64>,
}

pub struct StageContext<'a> {
    pub operation: u64,
    pub cancellation: &'a CancellationToken,
    pub deadline: Instant,
    pub now: u64,
    pub crash: CrashPoint,
}

impl UpdateRoot {
    pub fn open(path: &Path, pins: TrustPins, platform: NativePlatform) -> Result<Arc<Self>> {
        if !platform.signer_verified {
            return Err("signed-native-helper-required".into());
        }
        Self::load(path, pins, platform, String::new(), true)
    }

    pub fn initialize_accepted_baseline(
        path: &Path,
        pins: TrustPins,
        platform: NativePlatform,
        bundle: SignedBundle,
        archive: &Path,
        now: u64,
        deadline: Instant,
    ) -> Result<Arc<Self>> {
        Self::initialize_baseline_before(
            path,
            pins,
            platform,
            bundle,
            archive,
            StageContext {
                operation: 1,
                cancellation: &CancellationToken::new(),
                now,
                deadline,
                crash: CrashPoint::None,
            },
        )
    }

    pub(crate) fn initialize_baseline_before(
        path: &Path,
        pins: TrustPins,
        platform: NativePlatform,
        bundle: SignedBundle,
        archive: &Path,
        context: StageContext<'_>,
    ) -> Result<Arc<Self>> {
        let StageContext {
            now,
            cancellation,
            deadline,
            operation,
            ..
        } = context;
        if operation == 0
            || !platform.signer_verified
            || cancellation.is_cancelled()
            || Instant::now() >= deadline
        {
            return Err("certified-baseline-helper-and-deadline-required".into());
        }
        let release = VerifiedRelease::verify(
            &bundle.manifest,
            &bundle.signature,
            &pins.manifest_key,
            now,
            0,
        )
        .map_err(str::to_owned)?;
        Self::acceptance_for(&pins, "", &bundle, &release)?;
        let path = plain_canonical(path)?;
        platform.run("root-access", &[path.to_string_lossy().into_owned()])?;
        if std::fs::read_dir(&path)
            .map_err(|_| "baseline-root-read-failed")?
            .next()
            .is_some()
        {
            return Err("baseline-root-not-empty".into());
        }
        let receipt = VersionReceipt {
            key: format!("baseline-{}", release.manifest_sha256()),
            manifest_sha256: release.manifest_sha256().into(),
            bundle,
        };
        let journal = Journal {
            format: 1,
            profile: NATIVE_BETA_PROFILE.into(),
            scope: String::new(),
            last_operation: operation,
            highest_sequence: release.manifest().sequence,
            phase: DurablePhase::Idle,
            current: receipt.clone(),
            pending: None,
        };
        let mut identity = vec![0; 32];
        getrandom::fill(&mut identity).map_err(|_| "baseline-identity-random-failed")?;
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path.join("update.lock"))
            .map_err(|_| "baseline-root-must-be-uninitialized")?;
        lease
            .try_lock_exclusive()
            .map_err(|_| "baseline-root-owned")?;
        let root = Arc::new(Self {
            path,
            identity,
            scope: String::new(),
            pins,
            platform,
            _lease: lease,
            journal: Mutex::new(journal.clone()),
            active: AtomicU64::new(operation),
            current_cancellation: Mutex::new(CancellationToken::new()),
            cancelled_operation: AtomicU64::new(0),
            blocked: AtomicBool::new(false),
        });
        root.acceptance(&receipt.bundle, &release)?;
        root.platform
            .run("root-access", &[root.path.to_string_lossy().into_owned()])?;
        let existing = std::fs::read_dir(&root.path).map_err(|_| "baseline-root-read-failed")?;
        for item in existing {
            if item.map_err(|_| "baseline-root-read-failed")?.file_name() != "update.lock" {
                return Err("baseline-root-not-empty".into());
            }
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.path.join("identity"))
            .map_err(|_| "baseline-identity-write-failed")?;
        file.write_all(&root.identity)
            .map_err(|_| "baseline-identity-write-failed")?;
        file.sync_all()
            .map_err(|_| "baseline-identity-flush-failed")?;
        std::fs::create_dir(root.path.join("versions")).map_err(|_| "baseline-versions-failed")?;
        let stage = tempfile::Builder::new()
            .prefix("baseline-stage-")
            .tempdir_in(&root.path)
            .map_err(|_| "baseline-stage-failed")?;
        root.extract(
            &release,
            archive,
            stage.path(),
            operation,
            cancellation,
            deadline,
        )?;
        root.check(operation, cancellation, deadline)?;
        std::fs::rename(stage.path(), root.path.join("versions").join(&receipt.key))
            .map_err(|_| "baseline-version-rename-failed")?;
        root.selector(&receipt)?;
        root.check(operation, cancellation, deadline)?;
        root.persist(&journal)?;
        root.recover(CrashPoint::None)?;
        root.active.store(0, Ordering::Release);
        Ok(root)
    }

    pub fn host_identity(&self) -> Result<HostRootIdentity> {
        let state = self.journal.lock().map_err(|_| "journal-poisoned")?;
        Ok(HostRootIdentity {
            root_sha256: digest(&Self::entropy(&self.path, &self.identity)),
            current_manifest_sha256: state.current.manifest_sha256.clone(),
            sandbox: !self.scope.is_empty(),
            health_deadline: state
                .pending
                .as_ref()
                .map(|pending| pending.health_deadline),
        })
    }

    pub(crate) fn host_platform(&self) -> &NativePlatform {
        &self.platform
    }

    fn load(
        path: &Path,
        pins: TrustPins,
        platform: NativePlatform,
        scope: String,
        recover: bool,
    ) -> Result<Arc<Self>> {
        let path = plain_canonical(path)?;
        platform.run("root-access", &[path.to_string_lossy().into_owned()])?;
        let identity = read_limited(&path.join("identity"), 32)?;
        if identity.len() != 32 {
            return Err("invalid-root-identity".into());
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(0x0020_0000).share_mode(3);
        }
        let lease = options
            .open(path.join("update.lock"))
            .map_err(|_| "lease-open-failed")?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if lease
                .metadata()
                .map_err(|_| "lease-stat-failed")?
                .file_attributes()
                & 0x400
                != 0
            {
                return Err("reparse-lease-rejected".into());
            }
        }
        lease
            .try_lock_exclusive()
            .map_err(|_| "update-root-already-owned")?;
        let entropy = Self::entropy(&path, &identity);
        let cipher = read_limited(&path.join("journal.bin"), STATE_LIMIT)?;
        let plain = Zeroizing::new(
            windows_dpapi::decrypt_data(&cipher, windows_dpapi::Scope::User, Some(&entropy))
                .map_err(|_| "protected-journal-invalid")?,
        );
        let journal: Journal =
            serde_json::from_slice(&plain).map_err(|_| "journal-schema-invalid")?;
        if journal.format != 1 || journal.profile != NATIVE_BETA_PROFILE || journal.scope != scope {
            return Err("journal-root-or-scope-mismatch".into());
        }
        let root = Arc::new(Self {
            path,
            identity,
            scope,
            pins,
            platform,
            _lease: lease,
            active: AtomicU64::new(if recover { 0 } else { journal.last_operation }),
            current_cancellation: Mutex::new(CancellationToken::new()),
            cancelled_operation: AtomicU64::new(0),
            blocked: AtomicBool::new(false),
            journal: Mutex::new(journal),
        });
        root.validate_current()?;
        if recover {
            root.recover(CrashPoint::None)?;
        }
        Ok(root)
    }

    fn entropy(path: &Path, identity: &[u8]) -> Vec<u8> {
        let mut bytes = b"ZABOR-NATIVE-UPDATE-STATE-V1\0".to_vec();
        bytes.extend_from_slice(path.to_string_lossy().as_bytes());
        bytes.extend_from_slice(identity);
        Sha256::digest(bytes).to_vec()
    }

    fn protected_write<T: Serialize>(&self, name: &str, value: &T) -> Result<()> {
        self.platform
            .run("root-access", &[self.path.to_string_lossy().into_owned()])?;
        let plain =
            Zeroizing::new(serde_json::to_vec(value).map_err(|_| "journal-serialize-failed")?);
        if plain.len() > STATE_LIMIT - 4096 {
            return Err("journal-size-limit".into());
        }
        let cipher = windows_dpapi::encrypt_data(
            &plain,
            windows_dpapi::Scope::User,
            Some(&Self::entropy(&self.path, &self.identity)),
        )
        .map_err(|_| "journal-protection-failed")?;
        let temporary = tempfile::Builder::new()
            .prefix("update-")
            .suffix(".tmp")
            .tempfile_in(&self.path)
            .map_err(|_| "journal-stage-open-failed")?;
        let mut file = temporary.as_file();
        file.write_all(&cipher)
            .map_err(|_| "journal-write-failed")?;
        file.sync_all().map_err(|_| "journal-flush-failed")?;
        let temporary = temporary.into_temp_path();
        let replacement = self.platform.run(
            "replace",
            &[
                self.path.to_string_lossy().into_owned(),
                temporary
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                name.into(),
            ],
        );
        if replacement.is_err() {
            self.blocked.store(true, Ordering::Release);
        }
        replacement?;
        Ok(())
    }

    fn persist(&self, journal: &Journal) -> Result<()> {
        self.protected_write("journal.bin", journal)
    }
    fn selector(&self, receipt: &VersionReceipt) -> Result<()> {
        self.protected_write("current.bin", receipt)
    }

    pub fn validate(&self, bundle: SignedBundle, now: u64) -> Result<ValidatedRelease> {
        if self.blocked.load(Ordering::Acquire) {
            return Err("root-reopen-required".into());
        }
        let sequence = self
            .journal
            .lock()
            .map_err(|_| "journal-poisoned")?
            .highest_sequence;
        let release = VerifiedRelease::verify(
            &bundle.manifest,
            &bundle.signature,
            &self.pins.manifest_key,
            now,
            sequence,
        )
        .map_err(str::to_owned)?;
        self.acceptance(&bundle, &release)?;
        let state = self.journal.lock().map_err(|_| "journal-poisoned")?;
        let current = self.retained(&state.current, state.highest_sequence)?;
        if !release.permits_upgrade(
            &current.manifest().version,
            current.manifest().data_schema,
            state.highest_sequence,
        ) || release.manifest().data_schema != current.manifest().data_schema
        {
            return Err("candidate-replay-downgrade-or-schema-change".into());
        }
        Ok(ValidatedRelease {
            release,
            bundle,
            scope: self.scope.clone(),
        })
    }

    fn acceptance(&self, bundle: &SignedBundle, release: &VerifiedRelease) -> Result<()> {
        Self::acceptance_for(&self.pins, &self.scope, bundle, release)
    }

    fn acceptance_for(
        pins: &TrustPins,
        scope: &str,
        bundle: &SignedBundle,
        release: &VerifiedRelease,
    ) -> Result<()> {
        if bundle.acceptance.is_empty()
            || bundle.acceptance.len() > 32768
            || digest(&bundle.acceptance) != release.manifest().acceptance_sha256
            || pins.publisher_certificate_sha256.len() != 64
            || pins.publisher_certificate_sha256 == "0".repeat(64)
            || pins.publisher_certificate_sha256 != release.manifest().publisher_certificate_sha256
        {
            return Err("acceptance-or-publisher-pin-missing".into());
        }
        let key = VerifyingKey::from_sec1_bytes(&pins.acceptance_key)
            .map_err(|_| "acceptance-key-missing")?;
        let signature = Signature::from_slice(&bundle.acceptance_signature)
            .map_err(|_| "acceptance-signature-invalid")?;
        if signature.normalize_s().is_some() {
            return Err("acceptance-signature-high-s".into());
        }
        let mut message = ACCEPTANCE_DOMAIN.to_vec();
        message.extend_from_slice(&bundle.acceptance);
        key.verify(&message, &signature)
            .map_err(|_| "acceptance-signature-untrusted")?;
        let receipt: AcceptanceReceipt =
            serde_json::from_slice(&bundle.acceptance).map_err(|_| "acceptance-json-invalid")?;
        let manifest = release.manifest();
        if receipt.receipt_version != 1
            || receipt.profile != NATIVE_BETA_PROFILE
            || receipt.version != manifest.version
            || receipt.commit != manifest.commit
            || receipt.inventory_sha256 != manifest.inventory_sha256
            || receipt.data_schema != manifest.data_schema
            || receipt.publisher_certificate_sha256 != pins.publisher_certificate_sha256
        {
            return Err("acceptance-release-mismatch".into());
        }
        let complete = receipt.complete_native_acceptance
            && receipt.windows_10_11_acceptance
            && receipt.two_device_acceptance
            && receipt.data_compatibility_acceptance
            && receipt.licenses_accepted;
        if scope.is_empty() {
            if receipt.kind != "native-release" || !receipt.scope_id.is_empty() || !complete {
                return Err("complete-native-acceptance-required".into());
            }
        } else if receipt.kind != "local-sandbox"
            || receipt.scope_id != scope
            || complete
            || receipt.complete_native_acceptance
            || receipt.windows_10_11_acceptance
            || receipt.two_device_acceptance
            || receipt.data_compatibility_acceptance
            || receipt.licenses_accepted
        {
            return Err("sandbox-receipt-cannot-authorize-live-installation".into());
        }
        Ok(())
    }

    fn retained(&self, receipt: &VersionReceipt, sequence: u64) -> Result<VerifiedRelease> {
        if !safe_path(&receipt.key) || receipt.key.contains('/') {
            return Err("unsafe-version-key".into());
        }
        let release = VerifiedRelease::verify_retained(
            &receipt.bundle.manifest,
            &receipt.bundle.signature,
            &self.pins.manifest_key,
            &receipt.manifest_sha256,
            sequence,
        )
        .map_err(str::to_owned)?;
        self.acceptance(&receipt.bundle, &release)?;
        Ok(release)
    }

    fn validate_current(&self) -> Result<()> {
        let state = self.journal.lock().map_err(|_| "journal-poisoned")?;
        self.retained(&state.current, state.highest_sequence)?;
        if let Some(pending) = &state.pending {
            let target = self.retained(&pending.target, state.highest_sequence)?;
            let previous = self.retained(&pending.previous, state.highest_sequence)?;
            if target.manifest().data_schema != previous.manifest().data_schema
                || pending.challenge.len() != 32
            {
                return Err("unsafe-recovery-schema-or-challenge".into());
            }
        }
        Ok(())
    }

    pub fn begin(&self, operation: u64) -> Result<CancellationToken> {
        if self.blocked.load(Ordering::Acquire) {
            return Err("root-reopen-required".into());
        }
        self.recover(CrashPoint::None)?;
        let mut state = self.journal.lock().map_err(|_| "journal-poisoned")?;
        if operation == 0 || operation <= state.last_operation {
            return Err("stale-operation".into());
        }
        let mut next = state.clone();
        next.last_operation = operation;
        let mut cancellation = self
            .current_cancellation
            .lock()
            .map_err(|_| "cancellation-poisoned")?;
        cancellation.cancel();
        self.persist(&next)?;
        *state = next;
        self.active.store(operation, Ordering::Release);
        self.cancelled_operation.store(0, Ordering::Release);
        *cancellation = CancellationToken::new();
        Ok(cancellation.clone())
    }

    pub fn check(
        &self,
        operation: u64,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<()> {
        if self.blocked.load(Ordering::Acquire) {
            return Err("root-reopen-required".into());
        }
        if self.cancelled_operation.load(Ordering::Acquire) == operation
            || self
                .current_cancellation
                .try_lock()
                .map_err(|_| "operation-admission-busy")?
                .is_cancelled()
        {
            return Err("original-operation-cancelled".into());
        }
        if cancel.is_cancelled() {
            return Err("operation-cancelled".into());
        }
        if operation != self.active.load(Ordering::Acquire) {
            return Err("stale-operation".into());
        }
        if Instant::now() >= deadline {
            return Err("operation-deadline".into());
        }
        Ok(())
    }

    pub fn cancel_operation(&self, operation: u64) {
        if self.active.load(Ordering::Acquire) == operation {
            self.cancelled_operation.store(operation, Ordering::Release);
            if let Ok(token) = self.current_cancellation.try_lock() {
                token.cancel();
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn sandbox_scoped(&self) -> bool {
        !self.scope.is_empty()
    }
    pub fn phase(&self) -> Result<DurablePhase> {
        Ok(self.journal.lock().map_err(|_| "journal-poisoned")?.phase)
    }
    pub fn highest_sequence(&self) -> Result<u64> {
        Ok(self
            .journal
            .lock()
            .map_err(|_| "journal-poisoned")?
            .highest_sequence)
    }

    pub(crate) fn prepare(
        &self,
        context: StageContext<'_>,
        candidate: ValidatedRelease,
        archive: &Path,
    ) -> Result<()> {
        let StageContext {
            operation,
            cancellation: cancel,
            deadline,
            now,
            crash,
        } = context;
        self.check(operation, cancel, deadline)?;
        if candidate.scope != self.scope || !candidate.release.fresh_at(now) {
            return Err("candidate-scope-or-expiry".into());
        }
        let mut state = self.journal.lock().map_err(|_| "journal-poisoned")?;
        let previous = self.retained(&state.current, state.highest_sequence)?;
        if state.last_operation != operation
            || state.pending.is_some()
            || !candidate.release.permits_upgrade(
                &previous.manifest().version,
                previous.manifest().data_schema,
                state.highest_sequence,
            )
            || candidate.release.manifest().data_schema != previous.manifest().data_schema
        {
            return Err("unsafe-stage-or-schema-change".into());
        }
        let stage = tempfile::Builder::new()
            .prefix("version-stage-")
            .tempdir_in(&self.path)
            .map_err(|_| "version-stage-failed")?;
        self.extract(
            &candidate.release,
            archive,
            stage.path(),
            operation,
            cancel,
            deadline,
        )?;
        self.check(operation, cancel, deadline)?;
        let key = format!("v-{operation}-{}", candidate.release.manifest_sha256());
        let versions = self.path.join("versions");
        no_reparse(&versions)?;
        let destination = versions.join(&key);
        if destination.exists() {
            return Err("version-already-exists".into());
        }
        std::fs::rename(stage.path(), &destination).map_err(|_| "version-rename-failed")?;
        hit(crash, CrashPoint::VersionRenamed)?;
        self.check(operation, cancel, deadline)?;
        let target = VersionReceipt {
            key,
            manifest_sha256: candidate.release.manifest_sha256().into(),
            bundle: candidate.bundle,
        };
        let mut challenge = vec![0; 32];
        getrandom::fill(&mut challenge).map_err(|_| "challenge-random-failed")?;
        let mut next = state.clone();
        next.highest_sequence = candidate.release.manifest().sequence;
        next.phase = DurablePhase::Prepared;
        next.pending = Some(Pending {
            target,
            previous: state.current.clone(),
            challenge: Zeroizing::new(challenge),
            health_deadline: now.checked_add(120).ok_or("health-clock-overflow")?,
        });
        self.persist(&next)?;
        *state = next;
        hit(crash, CrashPoint::Prepared)
    }

    pub fn activate(
        &self,
        operation: u64,
        cancel: &CancellationToken,
        deadline: Instant,
        now: u64,
        drain: &dyn DrainVerifier,
        crash: CrashPoint,
    ) -> Result<HealthChallenge> {
        let started = Instant::now();
        self.activate_guarded(
            StageContext {
                operation,
                cancellation: cancel,
                deadline,
                now,
                crash,
            },
            drain,
            || {
                now.checked_add(started.elapsed().as_secs())
                    .ok_or_else(|| "activation-clock-overflow".into())
            },
        )
    }

    pub(crate) fn activate_guarded(
        &self,
        context: StageContext<'_>,
        drain: &dyn DrainVerifier,
        gate: impl Fn() -> Result<u64>,
    ) -> Result<HealthChallenge> {
        let StageContext {
            operation,
            cancellation: cancel,
            deadline,
            now,
            crash,
        } = context;
        self.check(operation, cancel, deadline)?;
        gate()?;
        let mut state = self.journal.lock().map_err(|_| "journal-poisoned")?;
        if state.last_operation != operation || state.phase != DurablePhase::Prepared {
            return Err("not-original-prepared-operation".into());
        }
        let pending = state
            .pending
            .clone()
            .ok_or("pending-installation-missing")?;
        let target = self.retained(&pending.target, state.highest_sequence)?;
        if !target.fresh_at(now) || now >= pending.health_deadline {
            return Err("activation-expired".into());
        }
        drain.verify(
            operation,
            &state.current.manifest_sha256,
            !self.scope.is_empty(),
        )?;
        self.verify_tree(&pending.target, state.highest_sequence)?;
        #[cfg(test)]
        host_verification_hook();
        let mut next = state.clone();
        next.phase = DurablePhase::Switching;
        self.check(operation, cancel, deadline)?;
        let checked_now = gate()?;
        if !target.fresh_at(checked_now) || checked_now >= pending.health_deadline {
            return Err("activation-expired".into());
        }
        self.persist(&next)?;
        *state = next;
        hit(crash, CrashPoint::Switching)?;
        self.check(operation, cancel, deadline)?;
        if gate()? >= pending.health_deadline {
            return Err("activation-expired".into());
        }
        self.selector(&pending.target)?;
        hit(crash, CrashPoint::SelectorSwitched)?;
        let mut next = state.clone();
        next.phase = DurablePhase::AwaitingHealth;
        self.persist(&next)?;
        *state = next;
        hit(crash, CrashPoint::AwaitingHealth)?;
        Ok(HealthChallenge {
            operation,
            manifest_sha256: pending.target.manifest_sha256,
            nonce: pending.challenge,
        })
    }

    pub fn healthy(
        &self,
        acknowledgment: &HealthChallenge,
        cancel: &CancellationToken,
        now: u64,
        crash: CrashPoint,
    ) -> Result<()> {
        self.healthy_before(
            acknowledgment,
            cancel,
            now,
            Instant::now() + Duration::from_secs(30),
            crash,
        )
    }

    pub fn healthy_before(
        &self,
        acknowledgment: &HealthChallenge,
        cancel: &CancellationToken,
        now: u64,
        deadline: Instant,
        crash: CrashPoint,
    ) -> Result<()> {
        let started = Instant::now();
        self.healthy_guarded(
            acknowledgment,
            StageContext {
                operation: acknowledgment.operation,
                cancellation: cancel,
                deadline,
                now,
                crash,
            },
            || {
                now.checked_add(started.elapsed().as_secs())
                    .ok_or_else(|| "health-clock-overflow".into())
            },
        )
    }

    pub(crate) fn healthy_guarded(
        &self,
        acknowledgment: &HealthChallenge,
        context: StageContext<'_>,
        gate: impl Fn() -> Result<u64>,
    ) -> Result<()> {
        let StageContext {
            operation,
            cancellation: cancel,
            now,
            deadline,
            crash,
        } = context;
        if operation != acknowledgment.operation {
            return Err("health-operation-mismatch".into());
        }
        self.check(acknowledgment.operation, cancel, deadline)?;
        gate()?;
        let mut state = self.journal.lock().map_err(|_| "journal-poisoned")?;
        let pending = state
            .pending
            .clone()
            .ok_or("pending-installation-missing")?;
        if state.phase != DurablePhase::AwaitingHealth
            || state.last_operation != acknowledgment.operation
            || acknowledgment.manifest_sha256 != pending.target.manifest_sha256
            || acknowledgment.nonce.as_slice() != pending.challenge.as_slice()
            || now >= pending.health_deadline
        {
            return Err("health-acknowledgment-rejected".into());
        }
        self.verify_selector(&pending.target)?;
        self.verify_tree(&pending.target, state.highest_sequence)?;
        #[cfg(test)]
        host_verification_hook();
        let mut next = state.clone();
        next.current = pending.target;
        next.pending = None;
        next.phase = DurablePhase::Committed;
        self.check(acknowledgment.operation, cancel, deadline)?;
        if gate()? >= pending.health_deadline {
            return Err("health-acknowledgment-rejected".into());
        }
        self.persist(&next)?;
        *state = next;
        hit(crash, CrashPoint::Committed)
    }

    pub fn recover(&self, crash: CrashPoint) -> Result<()> {
        if self.blocked.load(Ordering::Acquire) {
            return Err("root-reopen-required".into());
        }
        let mut state = self.journal.lock().map_err(|_| "journal-poisoned")?;
        if let Some(pending) = state.pending.clone() {
            self.verify_tree(&pending.previous, state.highest_sequence)?;
            self.selector(&pending.previous)?;
            hit(crash, CrashPoint::RollbackSelector)?;
            let mut next = state.clone();
            next.current = pending.previous;
            next.pending = None;
            next.phase = DurablePhase::RolledBack;
            self.persist(&next)?;
            *state = next;
        }
        self.verify_selector(&state.current)?;
        self.verify_tree(&state.current, state.highest_sequence)
    }

    fn verify_selector(&self, expected: &VersionReceipt) -> Result<()> {
        let cipher = read_limited(&self.path.join("current.bin"), STATE_LIMIT)?;
        let plain = Zeroizing::new(
            windows_dpapi::decrypt_data(
                &cipher,
                windows_dpapi::Scope::User,
                Some(&Self::entropy(&self.path, &self.identity)),
            )
            .map_err(|_| "selector-protection-invalid")?,
        );
        let actual: VersionReceipt =
            serde_json::from_slice(&plain).map_err(|_| "selector-json-invalid")?;
        if actual.key != expected.key
            || actual.manifest_sha256 != expected.manifest_sha256
            || digest(&actual.bundle.manifest) != expected.manifest_sha256
        {
            return Err("selector-receipt-mismatch".into());
        }
        Ok(())
    }

    pub fn current_executable(&self) -> Result<PathBuf> {
        self.recover(CrashPoint::None)?;
        let state = self.journal.lock().map_err(|_| "journal-poisoned")?;
        Ok(self
            .path
            .join("versions")
            .join(&state.current.key)
            .join("zabor-native.exe"))
    }

    pub fn pending_executable(
        &self,
        operation: u64,
        cancel: &CancellationToken,
        now: u64,
    ) -> Result<PathBuf> {
        self.check(operation, cancel, Instant::now() + Duration::from_secs(30))?;
        let state = self.journal.lock().map_err(|_| "journal-poisoned")?;
        if state.phase != DurablePhase::AwaitingHealth || state.last_operation != operation {
            return Err("no-original-pending-launch".into());
        }
        let pending = state
            .pending
            .as_ref()
            .ok_or("pending-installation-missing")?;
        if now >= pending.health_deadline {
            return Err("pending-launch-expired".into());
        }
        self.verify_selector(&pending.target)?;
        self.verify_tree(&pending.target, state.highest_sequence)?;
        Ok(self
            .path
            .join("versions")
            .join(&pending.target.key)
            .join("zabor-native.exe"))
    }

    pub(crate) fn pending_host_executable(
        &self,
        context: StageContext<'_>,
    ) -> Result<HeldExecutable> {
        self.check(context.operation, context.cancellation, context.deadline)?;
        let state = self.journal.lock().map_err(|_| "journal-poisoned")?;
        if state.phase != DurablePhase::AwaitingHealth || state.last_operation != context.operation
        {
            return Err("no-original-pending-launch".into());
        }
        let pending = state
            .pending
            .as_ref()
            .ok_or("pending-installation-missing")?;
        if context.now >= pending.health_deadline {
            return Err("pending-launch-expired".into());
        }
        let tree = self.path.join("versions").join(&pending.target.key);
        let mut held = HeldExecutable::open(&tree.join("zabor-native.exe"))?;
        held.proofs.push(open_read(&tree.join("package.zip"))?);
        held.proofs
            .push(open_read(&tree.join("native-package.json"))?);
        self.verify_selector(&pending.target)?;
        let package = self.verify_tree_package(&pending.target, state.highest_sequence)?;
        let image = package
            .files
            .iter()
            .find(|item| item.path.eq_ignore_ascii_case("zabor-native.exe"))
            .ok_or("pending-executable-pin-missing")?;
        held.verify(image.size, &image.sha256, true)?;
        self.check(context.operation, context.cancellation, context.deadline)?;
        Ok(held)
    }

    fn verify_tree(&self, receipt: &VersionReceipt, sequence: u64) -> Result<()> {
        self.verify_tree_package(receipt, sequence).map(|_| ())
    }

    fn verify_tree_package(
        &self,
        receipt: &VersionReceipt,
        sequence: u64,
    ) -> Result<PackageManifest> {
        let release = self.retained(receipt, sequence)?;
        let tree = self.path.join("versions").join(&receipt.key);
        no_reparse(&tree)?;
        let archive = tree.join("package.zip");
        self.hash_archive(&release, &archive)?;
        let metadata = read_limited(&tree.join("native-package.json"), 4 * 1024 * 1024)?;
        let mut zip =
            zip::ZipArchive::new(open_read(&archive)?).map_err(|_| "retained-zip-invalid")?;
        let mut original_metadata = Vec::new();
        zip.by_name("native-package.json")
            .map_err(|_| "retained-manifest-missing")?
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut original_metadata)
            .map_err(|_| "retained-manifest-read-failed")?;
        if metadata != original_metadata {
            return Err("version-manifest-changed".into());
        }
        let package: PackageManifest =
            serde_json::from_slice(&metadata).map_err(|_| "package-manifest-invalid")?;
        self.check_package(&release, &package)?;
        for item in &package.files {
            let (length, hash) = hash_file(&tree.join(&item.path), item.size)?;
            if length != item.size || hash != item.sha256 {
                return Err("version-file-changed".into());
            }
        }
        let expected = package
            .files
            .iter()
            .map(|item| item.path.to_ascii_lowercase())
            .chain(["package.zip".into(), "native-package.json".into()])
            .collect::<BTreeSet<_>>();
        if tree_files(&tree)? != expected {
            return Err("version-file-set-changed".into());
        }
        self.check_inventory(&release, &package, &tree)?;
        if self.scope.is_empty() {
            self.platform.run(
                "verify-pe",
                &[
                    tree.join("zabor-native.exe").to_string_lossy().into_owned(),
                    self.pins.publisher_certificate_sha256.clone(),
                ],
            )?;
            self.platform.run(
                "verify-pe",
                &[
                    tree.join("zabor-native-update-helper.exe")
                        .to_string_lossy()
                        .into_owned(),
                    self.pins.publisher_certificate_sha256.clone(),
                ],
            )?;
        }
        Ok(package)
    }

    fn hash_archive(&self, release: &VerifiedRelease, path: &Path) -> Result<()> {
        let mut file = open_read(path)?;
        if file.metadata().map_err(|_| "archive-stat-failed")?.len()
            != release.manifest().package_size
        {
            return Err("archive-length-mismatch".into());
        }
        let mut hash = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let count = file.read(&mut buffer).map_err(|_| "archive-read-failed")?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        if format!("{:x}", hash.finalize()) != release.manifest().package_sha256 {
            return Err("archive-digest-mismatch".into());
        }
        Ok(())
    }

    fn extract(
        &self,
        release: &VerifiedRelease,
        archive: &Path,
        stage: &Path,
        operation: u64,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<()> {
        self.hash_archive(release, archive)?;
        let source = open_read(archive)?;
        let mut zip = zip::ZipArchive::new(source).map_err(|_| "invalid-native-zip")?;
        if zip.len() > 20000 {
            return Err("too-many-zip-files".into());
        }
        let mut names = BTreeSet::new();
        let mut expanded = 0_u64;
        for index in 0..zip.len() {
            let file = zip.by_index(index).map_err(|_| "zip-entry-invalid")?;
            let name = file.name().map_err(|_| "zip-name-invalid")?;
            expanded = expanded
                .checked_add(file.size())
                .ok_or("zip-size-overflow")?;
            if !safe_path(&name)
                || file.is_dir()
                || file.is_symlink()
                || file.encrypted()
                || expanded > MAX_PACKAGE_BYTES
                || !names.insert(name.to_ascii_lowercase())
            {
                return Err("unsafe-native-zip-entry".into());
            }
        }
        let mut raw = Vec::new();
        zip.by_name("native-package.json")
            .map_err(|_| "package-manifest-missing")?
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut raw)
            .map_err(|_| "package-manifest-read-failed")?;
        if raw.len() > 4 * 1024 * 1024 {
            return Err("package-manifest-too-large".into());
        }
        let package: PackageManifest =
            serde_json::from_slice(&raw).map_err(|_| "package-manifest-invalid")?;
        self.check_package(release, &package)?;
        let expected = package
            .files
            .iter()
            .map(|item| item.path.to_ascii_lowercase())
            .chain(std::iter::once("native-package.json".into()))
            .collect::<BTreeSet<_>>();
        if names != expected || expected.len() != package.files.len() + 1 {
            return Err("zip-file-set-mismatch".into());
        }
        for item in &package.files {
            self.check(operation, cancel, deadline)?;
            let destination = stage.join(&item.path);
            std::fs::create_dir_all(destination.parent().unwrap())
                .map_err(|_| "payload-directory-failed")?;
            no_reparse(destination.parent().unwrap())?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)
                .map_err(|_| "payload-open-failed")?;
            let mut input = zip.by_name(&item.path).map_err(|_| "payload-missing")?;
            if input.size() != item.size {
                return Err("payload-size-mismatch".into());
            }
            let mut hash = Sha256::new();
            let mut written = 0_u64;
            let mut buffer = [0; 65536];
            loop {
                self.check(operation, cancel, deadline)?;
                let count = input.read(&mut buffer).map_err(|_| "payload-read-failed")?;
                if count == 0 {
                    break;
                }
                written += count as u64;
                if written > item.size {
                    return Err("payload-size-overflow".into());
                }
                output
                    .write_all(&buffer[..count])
                    .map_err(|_| "payload-write-failed")?;
                hash.update(&buffer[..count]);
            }
            output.sync_all().map_err(|_| "payload-flush-failed")?;
            if written != item.size || format!("{:x}", hash.finalize()) != item.sha256 {
                return Err("payload-digest-mismatch".into());
            }
        }
        std::fs::write(stage.join("native-package.json"), raw)
            .map_err(|_| "manifest-save-failed")?;
        self.check_inventory(release, &package, stage)?;
        if self.scope.is_empty() {
            self.platform.run(
                "verify-pe",
                &[
                    stage
                        .join("zabor-native.exe")
                        .to_string_lossy()
                        .into_owned(),
                    self.pins.publisher_certificate_sha256.clone(),
                ],
            )?;
            self.platform.run(
                "verify-pe",
                &[
                    stage
                        .join("zabor-native-update-helper.exe")
                        .to_string_lossy()
                        .into_owned(),
                    self.pins.publisher_certificate_sha256.clone(),
                ],
            )?;
        }
        std::fs::copy(archive, stage.join("package.zip"))
            .map_err(|_| "archive-retention-failed")?;
        self.hash_archive(release, &stage.join("package.zip"))?;
        for name in ["package.zip", "native-package.json"] {
            OpenOptions::new()
                .write(true)
                .open(stage.join(name))
                .map_err(|_| "retained-file-open-failed")?
                .sync_all()
                .map_err(|_| "retained-file-flush-failed")?;
        }
        Ok(())
    }

    fn check_inventory(
        &self,
        release: &VerifiedRelease,
        package: &PackageManifest,
        stage: &Path,
    ) -> Result<()> {
        let raw = read_limited(&stage.join("licenses/inventory.json"), 16 * 1024 * 1024)?;
        if digest(&raw) != release.manifest().inventory_sha256 {
            return Err("inventory-digest-mismatch".into());
        }
        let inventory: serde_json::Value =
            serde_json::from_slice(&raw).map_err(|_| "inventory-invalid")?;
        let components = inventory["packages"]
            .as_array()
            .ok_or("inventory-packages-missing")?;
        if inventory["complete"] != true || components.is_empty() {
            return Err("complete-inventory-required".into());
        }
        for component in components {
            let texts = component["texts"]
                .as_array()
                .ok_or("license-texts-missing")?;
            if texts.is_empty()
                || component["licenseExpression"]
                    .as_str()
                    .is_none_or(|license| license.is_empty() || license == "UNKNOWN")
            {
                return Err("license-provenance-incomplete".into());
            }
            for text in texts {
                let name = text["path"].as_str().ok_or("license-path-invalid")?;
                let hash = text["sha256"].as_str().ok_or("license-digest-invalid")?;
                if !package
                    .files
                    .iter()
                    .any(|file| file.path == name && file.sha256 == hash)
                {
                    return Err("license-text-digest-mismatch".into());
                }
            }
        }
        Ok(())
    }

    fn check_package(&self, release: &VerifiedRelease, package: &PackageManifest) -> Result<()> {
        if package.manifest_version != 1
            || package.version != release.manifest().version
            || package.commit != release.manifest().commit
            || package.profile != NATIVE_BETA_PROFILE
            || package.channel != "native-beta"
            || package.platform != "win32"
            || package.architecture != "x64"
            || package.data_schema != release.manifest().data_schema
            || package.files.len() > 20000
            || (self.scope.is_empty() && (package.local_fixture || !package.neural_dsp))
        {
            return Err("native-package-policy-rejected".into());
        }
        let mut names = BTreeSet::new();
        for item in &package.files {
            let name = item.path.to_ascii_lowercase();
            let binary = matches!(
                name.as_str(),
                "zabor-native.exe" | "zabor-native-update-helper.exe"
            ) || package.neural_dsp
                && matches!(
                    name.as_str(),
                    "onnxruntime.dll" | "onnxruntime_providers_shared.dll"
                );
            if !safe_path(&item.path)
                || !names.insert(name.clone())
                || item.size > MAX_PACKAGE_BYTES
                || (!binary && (name.ends_with(".exe") || name.ends_with(".dll")))
                || (!binary && name != "build-provenance.json" && !name.starts_with("licenses/"))
                || [
                    ".py", ".js", ".cjs", ".mjs", ".ps1", ".bat", ".cmd", ".wasm", ".lib", ".asar",
                ]
                .iter()
                .any(|suffix| name.ends_with(suffix))
            {
                return Err("unsafe-payload-composition".into());
            }
            if name == "onnxruntime.dll"
                && item.sha256 != "cb2fa518b18356617f6f9f460c012501da2819ec7e5c1d80bd8f5de20492db0c"
                || name == "onnxruntime_providers_shared.dll"
                    && item.sha256
                        != "8cea20be64259950cda8643af0ed42c2a424400fc3bb9bb29f9a35cee6b3419f"
            {
                return Err("pinned-onnx-api31-required".into());
            }
        }
        if !names.contains("zabor-native.exe")
            || self.scope.is_empty() && !names.contains("zabor-native-update-helper.exe")
            || !names.contains("licenses/inventory.json")
            || package.neural_dsp
                && (!names.contains("onnxruntime.dll")
                    || !names.contains("onnxruntime_providers_shared.dll"))
        {
            return Err("native-payload-missing".into());
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackageManifest {
    manifest_version: u32,
    version: String,
    commit: String,
    profile: String,
    channel: String,
    platform: String,
    architecture: String,
    data_schema: u32,
    local_fixture: bool,
    neural_dsp: bool,
    files: Vec<PackageFile>,
}

#[derive(Deserialize)]
struct PackageFile {
    path: String,
    size: u64,
    sha256: String,
}

fn hash_file(path: &Path, limit: u64) -> Result<(u64, String)> {
    let mut file = open_read(path)?;
    hash_reader(&mut file, limit)
}

fn hash_reader(file: &mut File, limit: u64) -> Result<(u64, String)> {
    let mut hash = Sha256::new();
    let mut length = 0_u64;
    let mut buffer = [0; 65536];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "file-hash-read-failed")?;
        if count == 0 {
            break;
        }
        length = length
            .checked_add(count as u64)
            .ok_or("file-size-overflow")?;
        if length > limit {
            return Err("file-exceeds-declared-length".into());
        }
        hash.update(&buffer[..count]);
    }
    Ok((length, format!("{:x}", hash.finalize())))
}

fn tree_files(root: &Path) -> Result<BTreeSet<String>> {
    let mut pending = vec![root.to_owned()];
    let mut names = BTreeSet::new();
    while let Some(directory) = pending.pop() {
        no_reparse(&directory)?;
        for entry in std::fs::read_dir(directory).map_err(|_| "version-directory-read-failed")? {
            let path = entry.map_err(|_| "version-entry-read-failed")?.path();
            no_reparse(&path)?;
            let name = path
                .strip_prefix(root)
                .map_err(|_| "version-path-escape")?
                .to_string_lossy()
                .replace('\\', "/");
            if !safe_path(&name) {
                return Err("version-path-rejected".into());
            }
            if path.is_dir() {
                pending.push(path);
            } else if !names.insert(name.to_ascii_lowercase()) {
                return Err("version-path-collision".into());
            }
            if names.len() + pending.len() > 20002 {
                return Err("version-entry-limit".into());
            }
        }
    }
    Ok(names)
}

pub trait DrainVerifier: Send + Sync {
    fn verify(&self, operation: u64, previous_manifest_sha256: &str, sandbox: bool) -> Result<()>;
}

pub struct ClosedDrain;
impl DrainVerifier for ClosedDrain {
    fn verify(&self, _: u64, _: &str, _: bool) -> Result<()> {
        Err("actual-app-drain-not-attested".into())
    }
}

fn hit(selected: CrashPoint, point: CrashPoint) -> Result<()> {
    if selected == point {
        Err(format!("injected-crash-{point:?}"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "update_helper_tests.rs"]
pub(crate) mod tests;
