use super::*;
use crate::update::{NativeUpdateManifest, MANIFEST_SIGNATURE_DOMAIN};
use p256::ecdsa::{signature::Signer, SigningKey};
use std::io::Cursor;
use zip::write::SimpleFileOptions;

pub(crate) struct Fixture {
    pub root: Option<Arc<UpdateRoot>>,
    pub directory: tempfile::TempDir,
    pins: TrustPins,
    platform: NativePlatform,
    scope: String,
    manifest_key: SigningKey,
    acceptance_key: SigningKey,
}

fn key() -> SigningKey {
    loop {
        let mut bytes = [0; 32];
        getrandom::fill(&mut bytes).unwrap();
        if let Ok(key) = SigningKey::from_slice(&bytes) {
            return key;
        }
    }
}

fn sign(key: &SigningKey, domain: &[u8], raw: &[u8]) -> Vec<u8> {
    let mut message = domain.to_vec();
    message.extend_from_slice(raw);
    let signature: Signature = key.sign(&message);
    signature
        .normalize_s()
        .unwrap_or(signature)
        .to_bytes()
        .to_vec()
}

impl Fixture {
    pub fn new() -> Self {
        let parent = std::env::var_os("ZABOR_UPDATE_TEST_DIRECTORY")
            .expect("external temporary harness required");
        let directory = tempfile::Builder::new()
            .prefix("native-update-sandbox-")
            .tempdir_in(parent)
            .unwrap();
        let helper = PathBuf::from(
            std::env::var_os("ZABOR_UPDATE_TEST_HELPER").expect("native fixture helper required"),
        );
        let platform =
            NativePlatform::pinned(&helper, &digest(&std::fs::read(&helper).unwrap())).unwrap();
        let path = directory.path().join("installation");
        std::fs::create_dir(&path).unwrap();
        let path = plain_canonical(&path).unwrap();
        platform
            .run("isolate-empty-root", &[path.to_string_lossy().into_owned()])
            .unwrap();
        platform
            .run("root-access", &[path.to_string_lossy().into_owned()])
            .unwrap();
        let mut identity = vec![0; 32];
        getrandom::fill(&mut identity).unwrap();
        let scope = digest(&identity);
        std::fs::write(path.join("identity"), &identity).unwrap();
        std::fs::create_dir(path.join("versions")).unwrap();
        let manifest_key = key();
        let acceptance_key = key();
        let pins = TrustPins {
            manifest_key: manifest_key
                .verifying_key()
                .to_encoded_point(false)
                .as_bytes()
                .to_vec(),
            acceptance_key: acceptance_key
                .verifying_key()
                .to_encoded_point(false)
                .as_bytes()
                .to_vec(),
            publisher_certificate_sha256: digest(b"LOCAL CERTIFICATE FIXTURE; NEVER A RELEASE PIN"),
        };
        let mut fixture = Self {
            directory,
            root: None,
            pins,
            platform,
            scope,
            manifest_key,
            acceptance_key,
        };
        let (bundle, archive, _) = fixture.package("3.7.2-rust", 1, 1, None);
        let receipt = VersionReceipt {
            key: "bootstrap".into(),
            manifest_sha256: digest(&bundle.manifest),
            bundle,
        };
        let journal = Journal {
            format: 1,
            profile: NATIVE_BETA_PROFILE.into(),
            scope: fixture.scope.clone(),
            last_operation: 1,
            highest_sequence: 1,
            phase: DurablePhase::Idle,
            current: receipt.clone(),
            pending: None,
        };
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path.join("update.lock"))
            .unwrap();
        lease.try_lock_exclusive().unwrap();
        let root = Arc::new(UpdateRoot {
            path,
            identity,
            scope: fixture.scope.clone(),
            pins: fixture.pins.clone(),
            platform: fixture.platform.clone(),
            _lease: lease,
            journal: Mutex::new(journal.clone()),
            active: AtomicU64::new(1),
            current_cancellation: Mutex::new(CancellationToken::new()),
            cancelled_operation: AtomicU64::new(0),
            blocked: AtomicBool::new(false),
        });
        let baseline = root.retained(&receipt, 1).unwrap();
        let stage = tempfile::Builder::new()
            .prefix("bootstrap-stage-")
            .tempdir_in(root.path())
            .unwrap();
        root.extract(
            &baseline,
            &archive,
            stage.path(),
            1,
            &CancellationToken::new(),
            Instant::now() + Duration::from_secs(30),
        )
        .unwrap();
        std::fs::rename(stage.path(), root.path().join("versions/bootstrap")).unwrap();
        root.persist(&journal).unwrap();
        root.selector(&receipt).unwrap();
        fixture.root = Some(root);
        fixture
    }

    pub fn root(&self) -> Arc<UpdateRoot> {
        self.root.as_ref().unwrap().clone()
    }

    pub fn reload(&mut self) {
        let path = self.root.as_ref().unwrap().path.clone();
        drop(self.root.take());
        self.root = Some(
            UpdateRoot::load(
                &path,
                self.pins.clone(),
                self.platform.clone(),
                self.scope.clone(),
                true,
            )
            .unwrap(),
        );
    }

    pub fn package(
        &self,
        version: &str,
        sequence: u64,
        schema: u32,
        extra: Option<(&str, &[u8])>,
    ) -> (SignedBundle, PathBuf, Vec<u8>) {
        let payload =
            format!("MZ LOCAL NATIVE UPDATE FIXTURE {version}; NEVER EXECUTED").into_bytes();
        let text = b"LOCAL LICENSE FIXTURE ONLY".to_vec();
        let inventory =
            serde_json::to_vec(&serde_json::json!({"inventoryVersion":1,"complete":true,
            "packages":[{"name":"local-fixture","licenseExpression":"LOCAL-ONLY",
            "texts":[{"path":"licenses/LICENSE","sha256":digest(&text)}]}]}))
            .unwrap();
        let mut entries = vec![
            ("zabor-native.exe".to_owned(), payload),
            ("licenses/LICENSE".into(), text),
            ("licenses/inventory.json".into(), inventory.clone()),
        ];
        if let Some((name, bytes)) = extra {
            entries.push((name.into(), bytes.to_vec()));
        }
        let commit = "1234567890abcdef1234567890abcdef12345678";
        let files = entries.iter().map(|(path, data)| serde_json::json!({"path":path,"size":data.len(),"sha256":digest(data)})).collect::<Vec<_>>();
        let metadata = serde_json::to_vec(&serde_json::json!({"manifestVersion":1,"version":version,
            "channel":"native-beta","profile":NATIVE_BETA_PROFILE,"commit":commit,"platform":"win32",
            "architecture":"x64","dataSchema":schema,"localFixture":true,"neuralDsp":false,"files":files})).unwrap();
        entries.push(("native-package.json".into(), metadata));
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(&data).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        let archive = self
            .directory
            .path()
            .join(format!("fixture-{sequence}-{schema}.zip"));
        std::fs::write(&archive, &bytes).unwrap();
        let receipt = AcceptanceReceipt {
            receipt_version: 1,
            kind: "local-sandbox".into(),
            profile: NATIVE_BETA_PROFILE.into(),
            scope_id: self.scope.clone(),
            version: version.into(),
            commit: commit.into(),
            inventory_sha256: digest(&inventory),
            publisher_certificate_sha256: self.pins.publisher_certificate_sha256.clone(),
            data_schema: schema,
            complete_native_acceptance: false,
            windows_10_11_acceptance: false,
            two_device_acceptance: false,
            data_compatibility_acceptance: false,
            licenses_accepted: false,
        };
        let acceptance = serde_json::to_vec(&receipt).unwrap();
        let manifest = NativeUpdateManifest {
            manifest_version: 1,
            version: version.into(),
            channel: "native-beta".into(),
            profile: NATIVE_BETA_PROFILE.into(),
            platform: "win32".into(),
            architecture: "x64".into(),
            commit: commit.into(),
            sequence,
            issued_at: 100,
            expires_at: 300,
            data_schema: schema,
            minimum_data_schema: 1,
            maximum_data_schema: schema,
            package_name: format!("ZABOR-Native-Beta-{version}-x64.zip"),
            package_size: bytes.len() as u64,
            package_sha256: digest(&bytes),
            inventory_sha256: digest(&inventory),
            publisher_certificate_sha256: self.pins.publisher_certificate_sha256.clone(),
            acceptance_sha256: digest(&acceptance),
        };
        let manifest = serde_json::to_vec(&manifest).unwrap();
        let bundle = SignedBundle {
            signature: sign(&self.manifest_key, MANIFEST_SIGNATURE_DOMAIN, &manifest),
            manifest,
            acceptance_signature: sign(&self.acceptance_key, ACCEPTANCE_DOMAIN, &acceptance),
            acceptance,
        };
        (bundle, archive, bytes)
    }

    pub fn prepared(&self, crash: CrashPoint) -> CancellationToken {
        let root = self.root();
        let cancel = root.begin(2).unwrap();
        let (bundle, archive, _) = self.package("3.8.0-rust", 2, 1, None);
        let result = root.prepare(
            StageContext {
                operation: 2,
                cancellation: &cancel,
                deadline: Instant::now() + Duration::from_secs(30),
                now: 150,
                crash,
            },
            root.validate(bundle, 150).unwrap(),
            &archive,
        );
        if crash == CrashPoint::None {
            result.unwrap();
        } else {
            assert!(result.is_err());
        }
        cancel
    }
}

pub(crate) struct SandboxDrain;
impl DrainVerifier for SandboxDrain {
    fn verify(&self, _: u64, _: &str, sandbox: bool) -> Result<()> {
        if sandbox {
            Ok(())
        } else {
            Err("fixture-drain-cannot-authorize-live".into())
        }
    }
}

#[test]
fn protected_prepare_switch_health_and_retained_receipt_restart() {
    let mut fixture = Fixture::new();
    let cancel = fixture.prepared(CrashPoint::None);
    let root = fixture.root();
    let ack = root
        .activate(
            2,
            &cancel,
            Instant::now() + Duration::from_secs(30),
            150,
            &SandboxDrain,
            CrashPoint::None,
        )
        .unwrap();
    assert!(root
        .current_executable()
        .unwrap()
        .ends_with("bootstrap/zabor-native.exe"));
    assert!(root.healthy(&ack, &cancel, 150, CrashPoint::None).is_err());
    drop(root);
    fixture.reload();
    assert_eq!(fixture.root().highest_sequence().unwrap(), 2);
}

#[test]
fn health_commit_survives_restart_without_candidate_expiry_bypass() {
    let mut fixture = Fixture::new();
    let cancel = fixture.prepared(CrashPoint::None);
    let root = fixture.root();
    let ack = root
        .activate(
            2,
            &cancel,
            Instant::now() + Duration::from_secs(30),
            150,
            &SandboxDrain,
            CrashPoint::None,
        )
        .unwrap();
    root.healthy(&ack, &cancel, 150, CrashPoint::None).unwrap();
    assert_eq!(root.phase().unwrap(), DurablePhase::Committed);
    drop(root);
    fixture.reload();
    assert!(!fixture
        .root()
        .current_executable()
        .unwrap()
        .ends_with("bootstrap/zabor-native.exe"));
}

#[test]
fn all_switch_crash_boundaries_roll_back_and_keep_sequence_floor() {
    for point in [
        CrashPoint::Switching,
        CrashPoint::SelectorSwitched,
        CrashPoint::AwaitingHealth,
    ] {
        let mut fixture = Fixture::new();
        let cancel = fixture.prepared(CrashPoint::None);
        assert!(fixture
            .root()
            .activate(
                2,
                &cancel,
                Instant::now() + Duration::from_secs(30),
                150,
                &SandboxDrain,
                point
            )
            .is_err());
        fixture.reload();
        assert_eq!(fixture.root().phase().unwrap(), DurablePhase::RolledBack);
        assert_eq!(fixture.root().highest_sequence().unwrap(), 2);
        assert!(fixture
            .root()
            .current_executable()
            .unwrap()
            .ends_with("bootstrap/zabor-native.exe"));
        let (bundle, _, _) = fixture.package("3.8.0-rust", 2, 1, None);
        let root = fixture.root();
        let token = root.begin(3).unwrap();
        assert!(root.validate(bundle, 150).is_err());
        let (bundle, archive, _) = fixture.package("3.8.0-rust.1", 3, 1, None);
        root.prepare(
            StageContext {
                operation: 3,
                cancellation: &token,
                deadline: Instant::now() + Duration::from_secs(30),
                now: 150,
                crash: CrashPoint::None,
            },
            root.validate(bundle, 150).unwrap(),
            &archive,
        )
        .unwrap();
    }
}

#[test]
fn prepared_and_version_rename_failures_never_publish_target() {
    for point in [CrashPoint::VersionRenamed, CrashPoint::Prepared] {
        let mut fixture = Fixture::new();
        fixture.prepared(point);
        fixture.reload();
        assert!(fixture
            .root()
            .current_executable()
            .unwrap()
            .ends_with("bootstrap/zabor-native.exe"));
    }
}

#[test]
fn rollback_selector_interruption_is_idempotent_after_restart() {
    let mut fixture = Fixture::new();
    let cancel = fixture.prepared(CrashPoint::None);
    fixture
        .root()
        .activate(
            2,
            &cancel,
            Instant::now() + Duration::from_secs(30),
            150,
            &SandboxDrain,
            CrashPoint::None,
        )
        .unwrap();
    assert!(fixture
        .root()
        .recover(CrashPoint::RollbackSelector)
        .is_err());
    fixture.reload();
    assert_eq!(fixture.root().highest_sequence().unwrap(), 2);
    assert_eq!(fixture.root().phase().unwrap(), DurablePhase::RolledBack);
}

#[test]
fn stale_operation_cancel_missing_drain_and_bad_health_are_rejected() {
    let fixture = Fixture::new();
    let cancel = fixture.prepared(CrashPoint::None);
    let root = fixture.root();
    assert!(root.begin(2).is_err());
    assert!(root
        .activate(
            2,
            &cancel,
            Instant::now() + Duration::from_secs(30),
            150,
            &ClosedDrain,
            CrashPoint::None
        )
        .is_err());
    cancel.cancel();
    assert!(root
        .activate(
            2,
            &cancel,
            Instant::now() + Duration::from_secs(30),
            150,
            &SandboxDrain,
            CrashPoint::None
        )
        .is_err());
}

#[test]
fn schema_change_and_modified_archive_fail_before_prepared_journal() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let token = root.begin(2).unwrap();
    let (bundle, _, _) = fixture.package("3.8.0-rust", 2, 2, None);
    assert!(root.validate(bundle, 150).is_err());
    let (bundle, archive, _) = fixture.package("3.8.0-rust", 2, 1, None);
    std::fs::write(&archive, b"tampered").unwrap();
    assert!(root
        .prepare(
            StageContext {
                operation: 2,
                cancellation: &token,
                deadline: Instant::now() + Duration::from_secs(30),
                now: 150,
                crash: CrashPoint::None
            },
            root.validate(bundle, 150).unwrap(),
            &archive
        )
        .is_err());
}

#[test]
fn signed_fixture_acceptance_cannot_open_live_root_and_missing_keys_fail() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let path = root.path().to_owned();
    let pins = fixture.pins.clone();
    let platform = fixture.platform.clone();
    drop(root);
    assert!(UpdateRoot::open(&path, pins, platform).is_err());
    let (mut bundle, _, _) = fixture.package("3.8.0-rust", 2, 1, None);
    bundle.acceptance_signature.clear();
    assert!(fixture.root().validate(bundle, 150).is_err());
    let (mut bundle, _, _) = fixture.package("3.8.0-rust", 2, 1, None);
    bundle.signature.clear();
    assert!(fixture.root().validate(bundle, 150).is_err());
}

#[test]
fn fixture_signed_receipt_is_rejected_even_with_live_policy_scope() {
    let mut fixture = Fixture::new();
    let (bundle, _, _) = fixture.package("3.8.0-rust", 2, 1, None);
    let release = VerifiedRelease::verify(
        &bundle.manifest,
        &bundle.signature,
        &fixture.pins.manifest_key,
        150,
        1,
    )
    .unwrap();
    let mut root = Arc::try_unwrap(fixture.root.take().unwrap()).ok().unwrap();
    root.scope.clear();
    assert_eq!(
        root.acceptance(&bundle, &release).unwrap_err(),
        "complete-native-acceptance-required"
    );
}

#[test]
fn ciphertext_corruption_and_cross_root_copy_are_not_recovered_as_trusted() {
    let mut fixture = Fixture::new();
    let path = fixture.root().path().to_owned();
    let mut cipher = std::fs::read(path.join("journal.bin")).unwrap();
    cipher[20] ^= 1;
    drop(fixture.root.take());
    std::fs::write(path.join("journal.bin"), cipher).unwrap();
    assert!(UpdateRoot::load(
        &path,
        fixture.pins.clone(),
        fixture.platform.clone(),
        fixture.scope.clone(),
        true
    )
    .is_err());
}

#[test]
fn traversal_windows_aliases_and_unlisted_runtime_files_fail_closed() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let token = root.begin(2).unwrap();
    for name in [
        "../escape",
        "CON.txt",
        "licenses/NUL",
        "extra.js",
        "Electron.exe",
    ] {
        let (bundle, archive, _) = fixture.package("3.8.0-rust", 2, 1, Some((name, b"unsafe")));
        assert!(
            root.prepare(
                StageContext {
                    operation: 2,
                    cancellation: &token,
                    deadline: Instant::now() + Duration::from_secs(30),
                    now: 150,
                    crash: CrashPoint::None
                },
                root.validate(bundle, 150).unwrap(),
                &archive
            )
            .is_err(),
            "{name}"
        );
    }
}

#[test]
fn native_signature_validator_rejects_unsigned_fixture() {
    let fixture = Fixture::new();
    let file = fixture.directory.path().join("unsigned.exe");
    std::fs::write(&file, b"MZ LOCAL UNSIGNED FIXTURE").unwrap();
    assert!(fixture
        .platform
        .run(
            "verify-pe",
            &[file.to_string_lossy().into_owned(), "1".repeat(64)]
        )
        .is_err());
    assert!(fixture.platform.clone().certify(&"1".repeat(64)).is_err());
}

#[test]
fn original_cancellation_cannot_be_replaced_by_a_fresh_token() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let cancel = fixture.prepared(CrashPoint::None);
    cancel.cancel();
    assert!(root
        .activate(
            2,
            &CancellationToken::new(),
            Instant::now() + Duration::from_secs(30),
            150,
            &SandboxDrain,
            CrashPoint::None
        )
        .is_err());
    assert!(root
        .check(
            2,
            &CancellationToken::new(),
            Instant::now() + Duration::from_secs(1)
        )
        .is_err());
}

#[test]
fn health_requires_original_nonce_hash_operation_and_deadline() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let token = fixture.prepared(CrashPoint::None);
    let ack = root
        .activate(
            2,
            &token,
            Instant::now() + Duration::from_secs(30),
            150,
            &SandboxDrain,
            CrashPoint::None,
        )
        .unwrap();
    assert!(!root
        .pending_executable(2, &token, 150)
        .unwrap()
        .ends_with("bootstrap/zabor-native.exe"));
    let mut wrong = HealthChallenge {
        operation: 3,
        manifest_sha256: ack.manifest_sha256.clone(),
        nonce: ack.nonce.clone(),
    };
    assert!(root.healthy(&wrong, &token, 150, CrashPoint::None).is_err());
    wrong.operation = 2;
    wrong.nonce[0] ^= 1;
    assert!(root.healthy(&wrong, &token, 150, CrashPoint::None).is_err());
    wrong.nonce = ack.nonce.clone();
    wrong.manifest_sha256 = "0".repeat(64);
    assert!(root.healthy(&wrong, &token, 150, CrashPoint::None).is_err());
    assert!(root.healthy(&ack, &token, 300, CrashPoint::None).is_err());
    root.healthy(&ack, &token, 150, CrashPoint::None).unwrap();
}

#[test]
fn modified_installed_tree_and_forged_manifest_do_not_activate() {
    for manifest_change in [false, true] {
        let fixture = Fixture::new();
        let root = fixture.root();
        let token = fixture.prepared(CrashPoint::None);
        let key = root
            .journal
            .lock()
            .unwrap()
            .pending
            .as_ref()
            .unwrap()
            .target
            .key
            .clone();
        let tree = root.path().join("versions").join(key);
        if manifest_change {
            std::fs::write(tree.join("native-package.json"), b"{}").unwrap();
        } else {
            std::fs::write(tree.join("licenses/extra.txt"), b"unlisted").unwrap();
        }
        assert!(root
            .activate(
                2,
                &token,
                Instant::now() + Duration::from_secs(30),
                150,
                &SandboxDrain,
                CrashPoint::None
            )
            .is_err());
    }
}

#[test]
fn actual_disk_replacement_error_requires_reopen_not_memory_only_retry() {
    let mut fixture = Fixture::new();
    let root = fixture.root();
    let path = root.path().to_owned();
    std::fs::rename(path.join("journal.bin"), path.join("saved-journal.bin")).unwrap();
    std::fs::create_dir(path.join("journal.bin")).unwrap();
    assert!(root.begin(2).is_err());
    assert!(root.begin(3).is_err());
    std::fs::remove_dir(path.join("journal.bin")).unwrap();
    std::fs::rename(path.join("saved-journal.bin"), path.join("journal.bin")).unwrap();
    drop(root);
    fixture.reload();
    assert_eq!(fixture.root().highest_sequence().unwrap(), 1);
    fixture.root().begin(2).unwrap();
}

#[test]
fn cross_root_ciphertext_and_stale_restart_operation_are_rejected() {
    let mut original = Fixture::new();
    original.prepared(CrashPoint::Prepared);
    original.reload();
    assert!(original
        .root()
        .check(
            2,
            &CancellationToken::new(),
            Instant::now() + Duration::from_secs(30)
        )
        .is_err());
    let mut other = Fixture::new();
    let root = other.root();
    let path = root.path().to_owned();
    drop(root);
    drop(other.root.take());
    std::fs::copy(
        original.root().path().join("journal.bin"),
        path.join("journal.bin"),
    )
    .unwrap();
    assert!(UpdateRoot::load(
        &path,
        other.pins.clone(),
        other.platform.clone(),
        other.scope.clone(),
        true
    )
    .is_err());
}

#[test]
fn actual_junction_and_archive_symlink_are_rejected_without_following() {
    use std::os::windows::process::CommandExt;
    let fixture = Fixture::new();
    let root = fixture.root();
    let target = fixture.directory.path().join("outside");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("sentinel"), b"unchanged").unwrap();
    let junction = root.path().join("junction");
    let command = format!(
        "mklink /J \"{}\" \"{}\"",
        junction.display(),
        target.display()
    );
    let result = Command::new("cmd.exe")
        .args(["/d", "/v:off", "/s", "/c"])
        .raw_arg(format!("\"{command}\""))
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(no_reparse(&junction).is_err());
    assert!(open_read(&junction.join("sentinel")).is_err());
    assert_eq!(
        std::fs::read(target.join("sentinel")).unwrap(),
        b"unchanged"
    );
    std::fs::remove_dir(&junction).unwrap();
    let token = root.begin(2).unwrap();
    let (mut bundle, archive, bytes) = fixture.package("3.8.0-rust", 2, 1, None);
    let mut source = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..source.len() {
        let mut entry = source.by_index(index).unwrap();
        let name = entry.name().unwrap().into_owned();
        if name == "licenses/LICENSE" {
            writer
                .add_symlink(&name, "../outside", SimpleFileOptions::default())
                .unwrap();
        } else {
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            writer
                .start_file(&name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(&data).unwrap();
        }
    }
    let changed = writer.finish().unwrap().into_inner();
    std::fs::write(&archive, &changed).unwrap();
    let mut manifest: NativeUpdateManifest = serde_json::from_slice(&bundle.manifest).unwrap();
    manifest.package_size = changed.len() as u64;
    manifest.package_sha256 = digest(&changed);
    bundle.manifest = serde_json::to_vec(&manifest).unwrap();
    bundle.signature = sign(
        &fixture.manifest_key,
        MANIFEST_SIGNATURE_DOMAIN,
        &bundle.manifest,
    );
    assert!(root
        .prepare(
            StageContext {
                operation: 2,
                cancellation: &token,
                deadline: Instant::now() + Duration::from_secs(30),
                now: 150,
                crash: CrashPoint::None
            },
            root.validate(bundle, 150).unwrap(),
            &archive
        )
        .is_err());
}

#[test]
#[ignore]
fn crash_worker() {
    let public = PathBuf::from(std::env::var_os("ZABOR_UPDATE_PUBLIC_TRUST").unwrap());
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(public).unwrap()).unwrap();
    assert_eq!(config["kind"], "local-sandbox");
    let scope = config["scope"].as_str().unwrap().to_owned();
    assert_eq!(scope.len(), 64);
    let pins = TrustPins {
        manifest_key: serde_json::from_value(config["manifestKey"].clone()).unwrap(),
        acceptance_key: serde_json::from_value(config["acceptanceKey"].clone()).unwrap(),
        publisher_certificate_sha256: config["publisher"].as_str().unwrap().into(),
    };
    let helper = PathBuf::from(config["helper"].as_str().unwrap());
    let platform = NativePlatform::pinned(&helper, config["helperHash"].as_str().unwrap()).unwrap();
    let path = PathBuf::from(config["root"].as_str().unwrap());
    let root = UpdateRoot::load(&path, pins, platform, scope, false).unwrap();
    let token = root.current_cancellation.lock().unwrap().clone();
    let point = match config["point"].as_str().unwrap() {
        "Switching" => CrashPoint::Switching,
        "SelectorSwitched" => CrashPoint::SelectorSwitched,
        "AwaitingHealth" => CrashPoint::AwaitingHealth,
        "Committed" => CrashPoint::Committed,
        _ => panic!("unknown crash point"),
    };
    if point == CrashPoint::Committed {
        let ack = root
            .activate(
                2,
                &token,
                Instant::now() + Duration::from_secs(30),
                150,
                &SandboxDrain,
                CrashPoint::None,
            )
            .unwrap();
        assert!(root.healthy(&ack, &token, 150, point).is_err());
    } else {
        assert!(root
            .activate(
                2,
                &token,
                Instant::now() + Duration::from_secs(30),
                150,
                &SandboxDrain,
                point
            )
            .is_err());
    }
    std::process::exit(77);
}

#[test]
fn abrupt_helper_process_exit_recovers_only_acknowledged_commits() {
    for point in [
        "Switching",
        "SelectorSwitched",
        "AwaitingHealth",
        "Committed",
    ] {
        let mut fixture = Fixture::new();
        fixture.prepared(CrashPoint::None);
        let root = fixture.root();
        let path = root.path().to_owned();
        drop(root);
        let public = fixture.directory.path().join("public-test-trust.json");
        std::fs::write(&public,serde_json::to_vec(&serde_json::json!({"kind":"local-sandbox","point":point,
            "root":path,"scope":fixture.scope,"manifestKey":fixture.pins.manifest_key,
            "acceptanceKey":fixture.pins.acceptance_key,"publisher":fixture.pins.publisher_certificate_sha256,
            "helper":fixture.platform.executable,"helperHash":fixture.platform.sha256})).unwrap()).unwrap();
        drop(fixture.root.take());
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "update_helper::tests::crash_worker", "--ignored"])
            .env("ZABOR_UPDATE_PUBLIC_TRUST", &public)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(77));
        fixture.root = Some(
            UpdateRoot::load(
                &path,
                fixture.pins.clone(),
                fixture.platform.clone(),
                fixture.scope.clone(),
                true,
            )
            .unwrap(),
        );
        assert_eq!(fixture.root().highest_sequence().unwrap(), 2);
        let old = fixture
            .root()
            .current_executable()
            .unwrap()
            .ends_with("bootstrap/zabor-native.exe");
        assert_eq!(old, point != "Committed");
    }
}
