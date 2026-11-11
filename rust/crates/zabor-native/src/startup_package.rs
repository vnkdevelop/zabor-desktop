use super::*;

pub struct HeldStartupPackage {
    image: HeldExecutable,
    root_sha256: String,
    manifest_sha256: String,
    pending: Option<(u64, Pending)>,
}

impl HeldStartupPackage {
    pub fn executable(&self) -> &Path {
        &self.image.path
    }

    pub fn root_sha256(&self) -> &str {
        &self.root_sha256
    }

    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }

    pub(crate) fn check_health(
        &self,
        operation: u64,
        nonce: &[u8],
        deadline_ms: u64,
    ) -> Result<()> {
        let (expected_operation, pending) = self.pending.as_ref().ok_or("startup-not-pending")?;
        if operation != *expected_operation
            || nonce != pending.challenge.as_slice()
            || deadline_ms > pending.health_deadline.saturating_mul(1000)
        {
            return Err("startup-health-journal-binding-rejected".into());
        }
        Ok(())
    }

    pub fn open_pending(
        path: &Path,
        pins: TrustPins,
        platform: NativePlatform,
        executing: &Path,
    ) -> Result<Self> {
        if !platform.signer_verified {
            return Err("signed-native-helper-required".into());
        }
        Self::read_pending(path, pins, platform, executing, String::new())
    }

    fn read_pending(
        path: &Path,
        pins: TrustPins,
        platform: NativePlatform,
        executing: &Path,
        scope: String,
    ) -> Result<Self> {
        let image = HeldExecutable::open(executing)?;
        let path = plain_canonical(path)?;
        platform.run("root-access", &[path.to_string_lossy().into_owned()])?;
        let identity = read_limited(&path.join("identity"), 32)?;
        if identity.len() != 32 {
            return Err("invalid-root-identity".into());
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(0x0020_0000).share_mode(3);
        }
        no_reparse(&path.join("update.lock"))?;
        let lease = options
            .open(path.join("update.lock"))
            .map_err(|_| "lease-open-failed")?;
        let cipher = read_limited(&path.join("journal.bin"), STATE_LIMIT)?;
        let plain = Zeroizing::new(
            windows_dpapi::decrypt_data(
                &cipher,
                windows_dpapi::Scope::User,
                Some(&UpdateRoot::entropy(&path, &identity)),
            )
            .map_err(|_| "protected-journal-invalid")?,
        );
        let journal: Journal =
            serde_json::from_slice(&plain).map_err(|_| "journal-schema-invalid")?;
        if journal.format != 1 || journal.profile != NATIVE_BETA_PROFILE || journal.scope != scope {
            return Err("journal-root-or-scope-mismatch".into());
        }
        if journal.phase != DurablePhase::AwaitingHealth || journal.pending.is_none() {
            return Err("startup-pending-health-required".into());
        }
        let root = UpdateRoot {
            path,
            identity,
            scope,
            pins,
            platform,
            _lease: lease,
            active: AtomicU64::new(0),
            current_cancellation: Mutex::new(CancellationToken::new()),
            cancelled_operation: AtomicU64::new(0),
            blocked: AtomicBool::new(false),
            journal: Mutex::new(journal),
        };
        root.validate_current()?;
        let state = root.journal.lock().map_err(|_| "journal-poisoned")?;
        let pending = state
            .pending
            .as_ref()
            .ok_or("pending-installation-missing")?;
        root.hold_startup_package(
            image,
            &pending.target,
            state.highest_sequence,
            Some((state.last_operation, pending.clone())),
        )
    }
}

impl UpdateRoot {
    pub fn hold_current_startup_package(&self, executing: &Path) -> Result<HeldStartupPackage> {
        let image = HeldExecutable::open(executing)?;
        let state = self.journal.lock().map_err(|_| "journal-poisoned")?;
        if state.pending.is_some() {
            return Err("startup-recovery-before-profile-required".into());
        }
        self.hold_startup_package(image, &state.current, state.highest_sequence, None)
    }

    fn hold_startup_package(
        &self,
        mut image: HeldExecutable,
        receipt: &VersionReceipt,
        sequence: u64,
        pending: Option<(u64, Pending)>,
    ) -> Result<HeldStartupPackage> {
        let release = self.retained(receipt, sequence)?;
        let tree = self.path.join("versions").join(&receipt.key);
        if plain_canonical(&image.path)? != plain_canonical(&tree.join("zabor-native.exe"))? {
            return Err("startup-executing-selector-mismatch".into());
        }
        let mut identity = open_read(&self.path.join("identity"))?;
        let (size, hash) = hash_reader(&mut identity, 32)?;
        if size != 32 || hash != digest(&self.identity) {
            return Err("startup-held-root-identity-changed".into());
        }
        image.proofs.push(identity);
        image.proofs.push(open_read(&tree.join("package.zip"))?);
        image
            .proofs
            .push(open_read(&tree.join("native-package.json"))?);
        let raw = read_limited(&tree.join("native-package.json"), 4 * 1024 * 1024)?;
        let package: PackageManifest =
            serde_json::from_slice(&raw).map_err(|_| "package-manifest-invalid")?;
        self.check_package(&release, &package)?;
        for file in &package.files {
            let held = HeldExecutable::open(&tree.join(&file.path))?;
            image.proofs.push(held.image);
            image._namespace.extend(held._namespace);
        }
        self.verify_selector(receipt)?;
        let verified = self.verify_tree_package(receipt, sequence)?;
        let executable = verified
            .files
            .iter()
            .find(|file| file.path.eq_ignore_ascii_case("zabor-native.exe"))
            .ok_or("startup-executable-pin-missing")?;
        image.verify(executable.size, &executable.sha256, true)?;
        if self.scope.is_empty() {
            let helper = verified
                .files
                .iter()
                .find(|file| {
                    file.path
                        .eq_ignore_ascii_case("zabor-native-update-helper.exe")
                })
                .ok_or("startup-helper-pin-missing")?;
            if helper.sha256 != self.platform.sha256 {
                return Err("startup-certified-helper-package-mismatch".into());
            }
            let held = self.platform.host_executable(false)?;
            image.proofs.push(held.image);
            image._namespace.extend(held._namespace);
        }
        Ok(HeldStartupPackage {
            image,
            root_sha256: digest(&Self::entropy(&self.path, &self.identity)),
            manifest_sha256: receipt.manifest_sha256.clone(),
            pending,
        })
    }
}

#[cfg(test)]
#[path = "startup_package_tests.rs"]
mod tests;
