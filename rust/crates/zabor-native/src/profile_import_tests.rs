use super::*;

struct Fixture {
    source: tempfile::TempDir,
    scratch: tempfile::TempDir,
    parent: tempfile::TempDir,
    owner: ImportOwner,
    data: Value,
    now: i64,
}

impl Fixture {
    fn new() -> Self {
        let source = tempfile::tempdir().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        let identity = ChatIdentity::generate();
        let owner = ImportOwner::verified("alice", "fixture", Some(identity.public_key().unwrap()))
            .unwrap();
        std::fs::write(
            source.path().join("session.enc"),
            b"fixture-ciphertext-not-a-live-secret",
        )
        .unwrap();
        std::fs::create_dir(source.path().join("chat-files")).unwrap();
        let now = RETENTION_MS * 3;
        let data = json!({"version":1,"session":{"userId":"alice","login":"fixture","password":"fixture-only","settings":{"language":"en","inputVolume":81,"unknownSecret":"must-not-copy"}},"identity":identity.export().unwrap().as_str(),"appSettings":{"openAtLogin":false,"minimizeToTray":true},"localStorage":{},"messages":[message("fresh", "alice", now), message("foreign", "other", now), message("expired", "alice", now - RETENTION_MS)]});
        Self {
            source,
            scratch,
            parent,
            owner,
            data,
            now,
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot::copy(self.source.path(), self.scratch.path()).unwrap()
    }

    fn prepare(&self, snapshot: &Snapshot) -> Result<PreparedImport, ImportError> {
        PreparedImport::prepare(
            snapshot,
            self.parent.path(),
            &self.owner,
            &Zeroizing::new(serde_json::to_vec(&self.data).unwrap()),
            self.now,
        )
    }
}

fn message(id: &str, owner: &str, now: i64) -> Value {
    json!({"id":id,"ownerId":owner,"conversationId":"bob","senderId":owner,"targetId":"bob","kind":"text","text":"fixture history","createdAt":now,"sequence":1,"delivery":"sent","file":null})
}

#[test]
fn copy_import_is_atomic_owner_isolated_and_reencrypts_current_user_secrets() {
    let mut fixture = Fixture::new();
    let peer = ChatIdentity::generate();
    fixture.data["localStorage"]["zabor_chat_key_alice_bob"] =
        json!(fingerprint(&peer.public_key().unwrap()).unwrap());
    fixture.data["localStorage"]["zabor_chat_key_other_bob"] =
        json!("invalid foreign key must not be trusted");
    let original = hashes(fixture.source.path(), false).unwrap();
    let snapshot = fixture.snapshot();
    let prepared = fixture.prepare(&snapshot).unwrap();
    assert!(!fixture.parent.path().join(BETA_PROFILE).exists());
    assert!(Profile::open(&fixture.parent.path().join(BETA_PROFILE)).is_err());
    assert!(prepared.publish(|| true).unwrap());
    let root = fixture.parent.path().join(BETA_PROFILE);
    let profile = Profile::open(&root).unwrap();
    let saved = profile.load_session().unwrap().unwrap();
    assert_eq!(saved.user_id, "alice");
    assert_eq!(saved.username, "fixture");
    assert_eq!(saved.password.as_str(), "fixture-only");
    assert_eq!(profile.read_settings().unwrap()["inputVolume"], 81);
    assert!(profile
        .read_settings()
        .unwrap()
        .get("unknownSecret")
        .is_none());
    assert!(std::fs::read(root.join("session.enc"))
        .unwrap()
        .starts_with(b"ZNS1"));
    assert!(!std::fs::read(root.join("session.enc"))
        .unwrap()
        .windows(12)
        .any(|bytes| bytes == b"fixture-only"));
    let restored = ChatIdentity::import(
        std::str::from_utf8(&profile.load_identity().unwrap().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        fingerprint(&restored.public_key().unwrap()).unwrap(),
        fingerprint(fixture.owner.public_key.as_ref().unwrap()).unwrap()
    );
    let store = ChatStore::open(&root.join("chat.sqlite3")).unwrap();
    assert_eq!(
        store
            .conversation("alice", "bob", fixture.now)
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .conversation("other", "bob", fixture.now)
        .unwrap()
        .is_empty());
    assert!(!store
        .accept_peer_key("alice", "bob", "changed-key")
        .unwrap());
    assert_eq!(hashes(fixture.source.path(), false).unwrap(), original);
    snapshot.close().unwrap();
    assert_eq!(
        std::fs::read_dir(fixture.scratch.path()).unwrap().count(),
        0
    );
}

#[test]
fn different_owner_login_and_global_identity_are_not_imported() {
    for variant in 0..4 {
        let mut fixture = Fixture::new();
        match variant {
            0 => fixture.data["session"]["userId"] = json!("other"),
            1 => fixture.data["session"]["login"] = json!("other-login"),
            2 => {
                fixture.data["identity"] =
                    json!(ChatIdentity::generate().export().unwrap().as_str())
            }
            _ => fixture.owner.public_key = None,
        }
        let snapshot = fixture.snapshot();
        assert!(matches!(
            fixture.prepare(&snapshot),
            Err(ImportError::Invalid)
        ));
        assert_eq!(std::fs::read_dir(fixture.parent.path()).unwrap().count(), 0);
    }
}

#[test]
fn extractor_failure_removes_its_snapshot_without_starting_a_beta_transaction() {
    let fixture = Fixture::new();
    let result = PreparedImport::copy(
        fixture.source.path(),
        fixture.scratch.path(),
        fixture.parent.path(),
        &fixture.owner,
        fixture.now,
        |snapshot| {
            assert!(snapshot.root().join(".native-copy-snapshot").exists());
            Err(ImportError::Export)
        },
    );
    assert!(matches!(result, Err(ImportError::Export)));
    assert_eq!(
        std::fs::read_dir(fixture.scratch.path()).unwrap().count(),
        0
    );
    assert_eq!(std::fs::read_dir(fixture.parent.path()).unwrap().count(), 0);
}

#[test]
fn destination_inside_the_electron_source_is_rejected_before_any_source_write() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let before = tree_entries(fixture.source.path()).unwrap();
    let bytes = hashes(fixture.source.path(), false).unwrap();
    let exported = Zeroizing::new(serde_json::to_vec(&fixture.data).unwrap());
    assert!(matches!(
        PreparedImport::prepare(
            &snapshot,
            fixture.source.path(),
            &fixture.owner,
            &exported,
            fixture.now
        ),
        Err(ImportError::Path)
    ));
    assert_eq!(tree_entries(fixture.source.path()).unwrap(), before);
    assert_eq!(hashes(fixture.source.path(), false).unwrap(), bytes);
}

#[test]
fn changed_closed_source_bytes_fail_admission_but_snapshot_cleanup_still_runs() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    std::fs::write(
        fixture.source.path().join("session.enc"),
        b"changed fixture ciphertext",
    )
    .unwrap();
    assert!(snapshot.verify_source_unchanged().is_err());
    assert!(fixture.prepare(&snapshot).is_err());
    assert!(!fixture.parent.path().join(JOURNAL).exists());
    assert!(snapshot.close().is_err());
    assert_eq!(
        std::fs::read_dir(fixture.scratch.path()).unwrap().count(),
        0
    );
}

#[test]
fn uncopied_source_changes_and_new_root_files_fail_the_complete_inventory_gate() {
    for changed in ["Preferences", "new-root-entry"] {
        let fixture = Fixture::new();
        std::fs::write(
            fixture.source.path().join("Preferences"),
            b"synthetic preferences",
        )
        .unwrap();
        let snapshot = fixture.snapshot();
        assert!(!snapshot.root().join("Preferences").exists());
        std::fs::write(fixture.source.path().join(changed), b"changed source bytes").unwrap();
        assert!(snapshot.verify_source_unchanged().is_err());
        assert!(fixture.prepare(&snapshot).is_err());
        assert!(!fixture.parent.path().join(JOURNAL).exists());
        assert!(snapshot.close().is_err());
        assert_eq!(
            std::fs::read_dir(fixture.scratch.path()).unwrap().count(),
            0
        );
    }
}

#[test]
fn wrong_settings_types_and_future_or_duplicate_messages_cannot_publish() {
    for variant in 0..3 {
        let mut fixture = Fixture::new();
        match variant {
            0 => fixture.data["session"]["settings"]["language"] = json!("invalid-language"),
            1 => fixture.data["messages"][0]["createdAt"] = json!(fixture.now + 1),
            _ => fixture.data["messages"]
                .as_array_mut()
                .unwrap()
                .push(message("fresh", "alice", fixture.now)),
        }
        let snapshot = fixture.snapshot();
        assert!(fixture.prepare(&snapshot).is_err());
        assert!(Profile::open(&fixture.parent.path().join(BETA_PROFILE)).is_err());
        assert!(!fixture.parent.path().join(BETA_PROFILE).exists());
        recover(fixture.parent.path(), RecoveryAction::AbortUnpublishedStage).unwrap();
    }
}

#[test]
fn absent_identity_is_allowed_only_when_the_verified_owner_expects_a_new_key() {
    let mut fixture = Fixture::new();
    fixture.data["identity"] = Value::Null;
    fixture.owner.public_key = None;
    let snapshot = fixture.snapshot();
    assert!(fixture
        .prepare(&snapshot)
        .unwrap()
        .publish(|| true)
        .unwrap());
    let profile = Profile::open(&fixture.parent.path().join(BETA_PROFILE)).unwrap();
    assert!(profile.load_identity().unwrap().is_none());
}

#[test]
fn existing_beta_and_active_profile_are_never_overwritten_or_rolled_back() {
    let fixture = Fixture::new();
    let root = fixture.parent.path().join(BETA_PROFILE);
    let profile = Profile::open(&root).unwrap();
    profile.write_settings(&json!({"language":"ru"})).unwrap();
    let snapshot = fixture.snapshot();
    assert!(matches!(
        fixture.prepare(&snapshot),
        Err(ImportError::Locked)
    ));
    assert!(matches!(
        rollback(fixture.parent.path(), &fixture.owner),
        Err(ImportError::Locked)
    ));
    drop(profile);
    assert!(matches!(
        fixture.prepare(&snapshot),
        Err(ImportError::Exists)
    ));
    assert!(rollback(fixture.parent.path(), &fixture.owner).is_err());
    assert_eq!(
        Profile::open(&root).unwrap().read_settings().unwrap()["language"],
        "ru"
    );
}

#[test]
fn rejected_publication_quarantines_only_staging_and_does_not_create_beta() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let original = hashes(fixture.source.path(), false).unwrap();
    assert!(!fixture
        .prepare(&snapshot)
        .unwrap()
        .publish(|| false)
        .unwrap());
    assert!(!fixture.parent.path().join(BETA_PROFILE).exists());
    assert!(!fixture.parent.path().join(JOURNAL).exists());
    assert!(fixture
        .parent
        .path()
        .join(ABORTED)
        .join("session.enc")
        .exists());
    assert_eq!(hashes(fixture.source.path(), false).unwrap(), original);
}

#[test]
fn revoked_post_rename_admission_never_leaves_a_usable_beta() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let checks = AtomicUsize::new(0);
    assert!(!fixture
        .prepare(&snapshot)
        .unwrap()
        .publish(|| checks.fetch_add(1, Ordering::SeqCst) == 0)
        .unwrap());
    assert_eq!(checks.load(Ordering::SeqCst), 2);
    assert!(!fixture.parent.path().join(BETA_PROFILE).exists());
    assert!(fixture.parent.path().join(ABORTED).join(RECEIPT).exists());
    assert!(!fixture.parent.path().join(JOURNAL).exists());
}

#[test]
fn failed_revocation_rollback_cannot_be_recovered_as_an_accepted_publication() {
    use std::os::windows::fs::OpenOptionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let checks = AtomicUsize::new(0);
    let held = Mutex::new(None);
    let result = fixture.prepare(&snapshot).unwrap().publish(|| {
        if checks.fetch_add(1, Ordering::SeqCst) == 0 {
            return true;
        }
        *held.lock().unwrap() = Some(
            OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(fixture.parent.path().join(BETA_PROFILE).join("session.enc"))
                .unwrap(),
        );
        false
    });
    assert!(result.is_err());
    assert!(Profile::open(&fixture.parent.path().join(BETA_PROFILE)).is_err());
    drop(held);
    assert!(recover(fixture.parent.path(), RecoveryAction::ConfirmPublished).is_err());
    recover(
        fixture.parent.path(),
        RecoveryAction::FinishAbortedPublication,
    )
    .unwrap();
    assert!(!fixture.parent.path().join(BETA_PROFILE).exists());
    assert!(fixture.parent.path().join(ABORTED).exists());
}

#[test]
fn dropped_preparation_requires_explicit_verified_recovery() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    drop(fixture.prepare(&snapshot).unwrap());
    assert!(matches!(
        fixture.prepare(&snapshot),
        Err(ImportError::Recovery)
    ));
    assert!(Profile::open(&fixture.parent.path().join(BETA_PROFILE)).is_err());
    recover(fixture.parent.path(), RecoveryAction::PublishVerifiedStage).unwrap();
    assert_eq!(
        Profile::open(&fixture.parent.path().join(BETA_PROFILE))
            .unwrap()
            .load_session()
            .unwrap()
            .unwrap()
            .user_id,
        "alice"
    );
}

#[test]
fn corrupted_or_unknown_journals_and_changed_staging_never_publish() {
    for variant in 0..4 {
        let fixture = Fixture::new();
        let snapshot = fixture.snapshot();
        drop(fixture.prepare(&snapshot).unwrap());
        match variant {
            0 => std::fs::write(fixture.parent.path().join(JOURNAL), b"not-json").unwrap(),
            1 => std::fs::write(
                fixture.parent.path().join(STAGE).join("settings.json"),
                b"{}",
            )
            .unwrap(),
            2 => std::fs::write(
                fixture.parent.path().join(STAGE).join("chat.sqlite3-wal"),
                b"unresolved",
            )
            .unwrap(),
            _ => std::fs::write(
                fixture
                    .parent
                    .path()
                    .join(STAGE)
                    .join(".session-commit.pending"),
                b"unresolved",
            )
            .unwrap(),
        }
        assert!(recover(fixture.parent.path(), RecoveryAction::PublishVerifiedStage).is_err());
        assert!(Profile::open(&fixture.parent.path().join(BETA_PROFILE)).is_err());
        assert!(!fixture.parent.path().join(BETA_PROFILE).exists());
    }
}

#[test]
fn crash_after_atomic_rename_requires_explicit_confirmation_before_open() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    drop(fixture.prepare(&snapshot).unwrap());
    std::fs::rename(
        fixture.parent.path().join(STAGE),
        fixture.parent.path().join(BETA_PROFILE),
    )
    .unwrap();
    assert!(Profile::open(&fixture.parent.path().join(BETA_PROFILE)).is_err());
    assert!(recover(fixture.parent.path(), RecoveryAction::PublishVerifiedStage).is_err());
    recover(fixture.parent.path(), RecoveryAction::ConfirmPublished).unwrap();
    assert!(Profile::open(&fixture.parent.path().join(BETA_PROFILE)).is_ok());
}

#[test]
fn rollback_preserves_post_import_beta_changes_without_touching_electron() {
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    let original = hashes(fixture.source.path(), false).unwrap();
    fixture
        .prepare(&snapshot)
        .unwrap()
        .publish(|| true)
        .unwrap();
    let root = fixture.parent.path().join(BETA_PROFILE);
    let profile = Profile::open(&root).unwrap();
    profile
        .write_settings(&json!({"language":"ru","newBetaData":true}))
        .unwrap();
    drop(profile);
    let beta = hashes(&root, false).unwrap();
    let restored = rollback(fixture.parent.path(), &fixture.owner).unwrap();
    assert_eq!(hashes(&restored, false).unwrap(), beta);
    assert!(!root.exists());
    assert_eq!(hashes(fixture.source.path(), false).unwrap(), original);
    assert!(rollback(fixture.parent.path(), &fixture.owner).is_err());
}

#[test]
fn rollback_sharing_failure_stays_blocked_until_explicit_recovery() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    let snapshot = fixture.snapshot();
    fixture
        .prepare(&snapshot)
        .unwrap()
        .publish(|| true)
        .unwrap();
    let root = fixture.parent.path().join(BETA_PROFILE);
    let locked = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(root.join("session.enc"))
        .unwrap();
    assert!(rollback(fixture.parent.path(), &fixture.owner).is_err());
    assert!(root.exists());
    assert!(Profile::open(&root).is_err());
    drop(locked);
    recover(fixture.parent.path(), RecoveryAction::FinishRollback).unwrap();
    assert!(!root.exists());
    assert!(fixture.parent.path().join(ROLLBACK).exists());
}

#[test]
fn invalid_message_or_attachment_fails_closed_and_can_only_abort_unpublished_stage() {
    for invalid in [
        json!("../session.enc"),
        json!("C:escape"),
        json!("file:stream"),
        json!("..\\escape"),
    ] {
        let mut fixture = Fixture::new();
        fixture.data["messages"][0]["kind"] = json!("file");
        fixture.data["messages"][0]["file"] = json!({"name":"file","size":0,"sha256":"0".repeat(64),"storedName":invalid,"transferState":"available"});
        let snapshot = fixture.snapshot();
        assert!(fixture.prepare(&snapshot).is_err());
        assert!(Profile::open(&fixture.parent.path().join(BETA_PROFILE)).is_err());
        recover(fixture.parent.path(), RecoveryAction::AbortUnpublishedStage).unwrap();
        assert!(!fixture.parent.path().join(BETA_PROFILE).exists());
    }
}

#[test]
fn attachments_are_copied_and_hashed_and_external_saved_paths_are_discarded() {
    let mut fixture = Fixture::new();
    let bytes = b"fixture local attachment";
    let hash = hex(&Sha256::digest(bytes));
    std::fs::write(
        fixture
            .source
            .path()
            .join("chat-files")
            .join("0123456789abcdef-attachment.txt"),
        bytes,
    )
    .unwrap();
    fixture.data["messages"][0]["kind"] = json!("file");
    fixture.data["messages"][0]["file"] = json!({"name":"attachment.txt","size":bytes.len(),"sha256":hash,"storedName":"0123456789abcdef-attachment.txt","savedPath":"C:\\never-read\\file","sourceAvailable":true,"transferState":"transferring","progress":42});
    let snapshot = fixture.snapshot();
    fixture
        .prepare(&snapshot)
        .unwrap()
        .publish(|| true)
        .unwrap();
    let root = fixture.parent.path().join(BETA_PROFILE);
    let store = ChatStore::open(&root.join("chat.sqlite3")).unwrap();
    let file = store
        .message("alice", "bob", "fresh", fixture.now)
        .unwrap()
        .unwrap()
        .file
        .unwrap();
    assert_eq!(file["savedPath"], Value::Null);
    assert_eq!(file["transferState"], "available");
    assert_eq!(
        std::fs::read(root.join("chat-files").join(format!("{hash}.bin"))).unwrap(),
        bytes
    );
    assert_eq!(
        std::fs::read(
            fixture
                .source
                .path()
                .join("chat-files")
                .join("0123456789abcdef-attachment.txt")
        )
        .unwrap(),
        bytes
    );
}

#[test]
fn imported_deletion_tombstones_do_not_resurrect_text_or_file_contents() {
    let mut fixture = Fixture::new();
    fixture.data["messages"]
        .as_array_mut()
        .unwrap()
        .push(message("deleted-file", "alice", fixture.now));
    fixture.data["messages"][3]["kind"] = json!("file");
    fixture.data["messages"][3]["file"] = json!({"name":"file","size":0,"sha256":"0".repeat(64),"storedName":null,"transferState":"missing"});
    fixture.data["localStorage"]["zabor_chat_deleted_incoming_alice_bob"] = json!("[{\"type\":\"message-delete\",\"messageId\":\"fresh\"},{\"type\":\"file-delete\",\"messageId\":\"deleted-file\"}]");
    let snapshot = fixture.snapshot();
    fixture
        .prepare(&snapshot)
        .unwrap()
        .publish(|| true)
        .unwrap();
    let store = ChatStore::open(
        &fixture
            .parent
            .path()
            .join(BETA_PROFILE)
            .join("chat.sqlite3"),
    )
    .unwrap();
    assert!(store
        .message("alice", "bob", "fresh", fixture.now)
        .unwrap()
        .is_none());
    assert_eq!(
        store
            .message("alice", "bob", "deleted-file", fixture.now)
            .unwrap()
            .unwrap()
            .file
            .unwrap()["transferState"],
        "deleted"
    );
    assert_eq!(
        store.deletions("alice", "bob", true).unwrap(),
        vec!["fresh"]
    );
    assert_eq!(
        store
            .file_deletions("alice", "bob", true, fixture.now)
            .unwrap(),
        vec!["deleted-file"]
    );
}

#[test]
fn source_sharing_and_directory_junctions_are_rejected_without_copying() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    let writer = OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(3)
        .open(fixture.source.path().join("session.enc"))
        .unwrap();
    assert!(matches!(
        Snapshot::copy(fixture.source.path(), fixture.scratch.path()),
        Err(ImportError::Locked)
    ));
    drop(writer);
    let outside = tempfile::tempdir().unwrap();
    let junction = fixture.source.path().join("IndexedDB");
    let status = std::process::Command::new("cmd.exe")
        .args(["/c", "mklink", "/J"])
        .arg(&junction)
        .arg(outside.path())
        .status()
        .unwrap();
    assert!(status.success());
    assert!(matches!(
        Snapshot::copy(fixture.source.path(), fixture.scratch.path()),
        Err(ImportError::Path)
    ));
    assert!(Profile::open(&junction).is_err());
    std::fs::remove_dir(junction).unwrap();
    assert_eq!(
        std::fs::read_dir(fixture.scratch.path()).unwrap().count(),
        0
    );
}

#[test]
fn beta_junction_and_path_escape_are_rejected_before_any_write() {
    let fixture = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    let junction = fixture.parent.path().join(BETA_PROFILE);
    let status = std::process::Command::new("cmd.exe")
        .args(["/c", "mklink", "/J"])
        .arg(&junction)
        .arg(outside.path())
        .status()
        .unwrap();
    assert!(status.success());
    let snapshot = fixture.snapshot();
    assert!(matches!(fixture.prepare(&snapshot), Err(ImportError::Path)));
    assert!(Profile::open(&junction).is_err());
    std::fs::remove_dir(junction).unwrap();
    assert!(check_path(&fixture.parent.path().join("..\\escape"), true).is_err());
    assert!(check_path(&fixture.parent.path().join("settings.json:stream"), true).is_err());
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}
