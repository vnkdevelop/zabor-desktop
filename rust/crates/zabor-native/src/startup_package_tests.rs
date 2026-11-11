use super::*;
use crate::update_helper::tests::{Fixture, SandboxDrain};

#[test]
fn startup_read_only_pending_validation_preserves_original_exclusive_owner_and_selector() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let cancel = fixture.prepared(CrashPoint::None);
    let challenge = root
        .activate(
            2,
            &cancel,
            Instant::now() + Duration::from_secs(30),
            150,
            &SandboxDrain,
            CrashPoint::None,
        )
        .unwrap();
    let image = root.pending_executable(2, &cancel, 150).unwrap();
    let selector = std::fs::read(root.path.join("current.bin")).unwrap();
    let journal = std::fs::read(root.path.join("journal.bin")).unwrap();
    let held = HeldStartupPackage::read_pending(
        &root.path,
        root.pins.clone(),
        root.platform.clone(),
        &image,
        root.scope.clone(),
    )
    .unwrap();
    assert_eq!(held.manifest_sha256(), challenge.manifest_sha256);
    held.check_health(2, &challenge.nonce, 270000).unwrap();
    assert!(held.check_health(3, &challenge.nonce, 270000).is_err());
    assert!(held.check_health(2, &[0; 32], 270000).is_err());
    assert!(held.check_health(2, &challenge.nonce, 270001).is_err());
    assert_eq!(
        std::fs::read(root.path.join("current.bin")).unwrap(),
        selector
    );
    assert_eq!(
        std::fs::read(root.path.join("journal.bin")).unwrap(),
        journal
    );
    assert_eq!(root.phase().unwrap(), DurablePhase::AwaitingHealth);
    assert!(UpdateRoot::load(
        &root.path,
        root.pins.clone(),
        root.platform.clone(),
        root.scope.clone(),
        false
    )
    .is_err());
    assert!(std::fs::write(&image, b"UNTRUSTED").is_err());
    assert!(std::fs::rename(
        image.parent().unwrap(),
        image.parent().unwrap().with_extension("moved")
    )
    .is_err());
    assert!(std::fs::write(
        image.parent().unwrap().join("licenses/inventory.json"),
        b"CHANGED"
    )
    .is_err());
    root.healthy(&challenge, &cancel, 150, CrashPoint::None)
        .unwrap();
    assert_eq!(root.phase().unwrap(), DurablePhase::Committed);
    drop(held);
}

#[test]
fn startup_held_current_requires_executing_selected_bytes_and_keeps_all_proofs_immutable() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let image = root.current_executable().unwrap();
    let wrong = fixture.directory.path().join("copied.exe");
    std::fs::copy(&image, &wrong).unwrap();
    assert!(root.hold_current_startup_package(&wrong).is_err());
    let held = root.hold_current_startup_package(&image).unwrap();
    assert!(held.check_health(1, &[1; 32], 1).is_err());
    for path in [
        image.clone(),
        root.path.join("identity"),
        image.parent().unwrap().join("package.zip"),
        image.parent().unwrap().join("native-package.json"),
    ] {
        assert!(std::fs::write(path, b"CHANGED").is_err());
    }
    drop(held);
    std::fs::write(&image, b"UNVERIFIED").unwrap();
    assert!(root.hold_current_startup_package(&image).is_err());
}

#[test]
fn startup_health_cannot_use_unsigned_keys_wrong_scope_or_baseline_without_pending() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let image = root.current_executable().unwrap();
    assert!(HeldStartupPackage::read_pending(
        &root.path,
        root.pins.clone(),
        root.platform.clone(),
        &image,
        root.scope.clone()
    )
    .is_err());
    let cancel = fixture.prepared(CrashPoint::None);
    root.activate(
        2,
        &cancel,
        Instant::now() + Duration::from_secs(30),
        150,
        &SandboxDrain,
        CrashPoint::None,
    )
    .unwrap();
    let image = root.pending_executable(2, &cancel, 150).unwrap();
    let mut untrusted = root.pins.clone();
    untrusted.manifest_key = vec![1; 65];
    assert!(HeldStartupPackage::read_pending(
        &root.path,
        untrusted,
        root.platform.clone(),
        &image,
        root.scope.clone()
    )
    .is_err());
    assert!(HeldStartupPackage::read_pending(
        &root.path,
        root.pins.clone(),
        root.platform.clone(),
        &image,
        String::new()
    )
    .is_err());
    assert!(HeldStartupPackage::open_pending(
        &root.path,
        root.pins.clone(),
        root.platform.clone(),
        &image
    )
    .is_err());
    assert_eq!(root.phase().unwrap(), DurablePhase::AwaitingHealth);
}
