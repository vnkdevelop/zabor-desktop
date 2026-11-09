use super::*;
use crate::update_helper::tests::Fixture;
use crate::update_helper::{CrashPoint, DurablePhase, NativePlatform, StageContext};
use std::path::PathBuf;

async fn evidence(host: &UpdateHost, root: &Arc<UpdateRoot>) -> (UpdateHost, AppDrainEvidence) {
    let owner = host.app_drain_owner();
    let mut jobs = Vec::new();
    for kind in [
        ResourceKind::Session,
        ResourceKind::Media,
        ResourceKind::Input,
        ResourceKind::Files,
        ResourceKind::Profile,
    ] {
        jobs.push(owner.register_resource(kind, |_| async { Ok(()) }).unwrap());
    }
    for job in jobs {
        job.wait().await.unwrap();
    }
    let evidence = owner.attest_app_drain(root).await.unwrap();
    (owner, evidence)
}

fn stage(fixture: &Fixture, mode: u32) -> CancellationToken {
    let root = fixture.root();
    let payload =
        PathBuf::from(std::env::var_os(format!("ZABOR_HOST_TEST_TARGET_{mode}")).unwrap());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (bundle, archive, _) = fixture.package_with_payload(
        "3.8.0-rust",
        2,
        1,
        None,
        std::fs::read(payload).unwrap(),
        now,
    );
    let candidate = root.validate(bundle, now).unwrap();
    let original = root.begin(2).unwrap();
    root.prepare(
        StageContext {
            operation: 2,
            cancellation: &original,
            deadline: Instant::now() + Duration::from_secs(30),
            now,
            crash: CrashPoint::None,
        },
        candidate,
        &archive,
    )
    .unwrap();
    original
}

#[tokio::test]
async fn admission_is_registered_before_acquisition_and_cancel_does_not_observe_exit() {
    let host = UpdateHost::new(2, Duration::from_secs(30)).unwrap();
    let (entered, start) = oneshot::channel();
    let (release, held) = oneshot::channel();
    let job = host
        .register_resource(ResourceKind::Session, move |_| async move {
            entered.send(()).unwrap();
            held.await.unwrap();
            Ok(())
        })
        .unwrap();
    start.await.unwrap();
    assert_eq!(host.registry.state.lock().unwrap().entries.len(), 1);
    job.cancel();
    assert!(job.wait().await.is_err());
    let mut drain = Box::pin(host.drain());
    assert!(tokio::time::timeout(Duration::from_millis(30), &mut drain)
        .await
        .is_err());
    release.send(()).unwrap();
    let reports = drain.await.unwrap();
    assert_eq!(reports.len(), 1);
    assert!(reports[0].result.is_ok());
    assert!(host
        .register_resource(ResourceKind::Files, |_| async { Ok(()) })
        .is_err());
}

#[tokio::test]
async fn cancelled_drain_returns_original_join_and_panics_are_retained() {
    let host = UpdateHost::new(2, Duration::from_secs(30)).unwrap();
    let (release, held) = oneshot::channel();
    let job = host
        .register_resource(ResourceKind::Session, move |_| async move {
            held.await.unwrap();
            Ok(())
        })
        .unwrap();
    let mut drain = Box::pin(host.drain());
    assert!(tokio::time::timeout(Duration::from_millis(30), &mut drain)
        .await
        .is_err());
    drop(drain);
    assert_eq!(host.registry.state.lock().unwrap().entries.len(), 1);
    release.send(()).unwrap();
    assert_eq!(host.drain().await.unwrap().len(), 1);
    drop(job);
    let host = UpdateHost::new(3, Duration::from_secs(30)).unwrap();
    let job = host
        .register_resource(ResourceKind::Session, |_| async {
            panic!("synthetic owner panic")
        })
        .unwrap();
    assert!(job.wait().await.is_err());
    assert!(host.drain().await.unwrap()[0].result.is_err());
}

#[tokio::test]
async fn missing_or_failed_resource_joins_never_attest_app_drain() {
    let fixture = Fixture::new();
    let host = UpdateHost::new(2, Duration::from_secs(30)).unwrap();
    assert!(host.attest_app_drain(&fixture.root()).await.is_err());
    let host = UpdateHost::new(2, Duration::from_secs(30)).unwrap();
    let job = host
        .register_resource(ResourceKind::Session, |_| async {
            Err("synthetic final-save failure".into())
        })
        .unwrap();
    assert!(job.wait().await.is_err());
    assert!(host.attest_app_drain(&fixture.root()).await.is_err());
}

#[tokio::test]
async fn native_health_commits_but_only_drain_observes_original_helper_target_and_children() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let original = stage(&fixture, 0);
    let host = UpdateHost::new(2, Duration::from_secs(30)).unwrap();
    let (_proof_owner, proof) = evidence(&host, &root).await;
    let mut job = host.activate(root.clone(), original, proof).unwrap();
    let ready = tokio::time::timeout(Duration::from_secs(20), job.ready())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ready.owner.pid, u64::from(std::process::id()));
    assert!(ready.owner.created != 0 && ready.helper.created != 0 && ready.target.created != 0);
    assert_ne!(ready.helper.pid, ready.target.pid);
    assert!(!ready.children_observed);
    assert_eq!(root.phase().unwrap(), DurablePhase::Committed);
    job.cancel();
    assert!(job.wait().await.is_err());
    let reports = host.drain().await.unwrap();
    assert_eq!(reports.len(), 1);
    assert!(reports[0].result.is_ok());
    assert_eq!(root.phase().unwrap(), DurablePhase::Committed);
}

#[tokio::test]
async fn wrong_nonce_early_exit_and_timeout_reap_children_then_recover_previous_selector() {
    for mode in [1, 2, 3] {
        let fixture = Fixture::new();
        let root = fixture.root();
        let baseline = root.current_executable().unwrap();
        let original = stage(&fixture, mode);
        let budget = if mode == 2 {
            Duration::from_secs(5)
        } else {
            Duration::from_secs(20)
        };
        let host = UpdateHost::new(2, budget).unwrap();
        let (_proof_owner, proof) = evidence(&host, &root).await;
        let mut job = host.activate(root.clone(), original, proof).unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(20), job.ready())
            .await
            .unwrap()
            .is_err());
        drop(job);
        let reports = host.drain().await.unwrap();
        assert_eq!(reports.len(), 1);
        assert!(reports[0].result.is_err(), "mode={mode}");
        assert_ne!(
            reports[0].result.as_ref().unwrap_err(),
            "original-helper-or-child-joins-not-observed",
            "mode={mode}"
        );
        assert_eq!(
            root.phase().unwrap(),
            DurablePhase::RolledBack,
            "mode={mode}; report={:?}",
            reports[0]
        );
        assert_eq!(root.current_executable().unwrap(), baseline);
        assert_eq!(root.highest_sequence().unwrap(), 2);
    }
}

#[tokio::test]
async fn app_drain_nonce_cannot_be_reminted_for_another_host() {
    let fixture = Fixture::new();
    let host = UpdateHost::new(2, Duration::from_secs(30)).unwrap();
    let (_proof_owner, proof) = evidence(&host, &fixture.root()).await;
    let other = UpdateHost::new(2, Duration::from_secs(30)).unwrap();
    assert!(other
        .activate(fixture.root(), CancellationToken::new(), proof)
        .is_err());
}

#[tokio::test]
async fn unsigned_baseline_and_bootstrap_are_rejected_without_mutating_root() {
    let fixture = Fixture::new();
    let path = fixture.directory.path().join("empty-baseline");
    std::fs::create_dir(&path).unwrap();
    let helper = PathBuf::from(std::env::var_os("ZABOR_UPDATE_TEST_HELPER").unwrap());
    let platform = NativePlatform::pinned(
        &helper,
        &crate::update_helper::digest(&std::fs::read(&helper).unwrap()),
    )
    .unwrap();
    let pins = crate::update_helper::TrustPins {
        manifest_key: vec![],
        acceptance_key: vec![],
        publisher_certificate_sha256: String::new(),
    };
    let (bundle, archive, _) = fixture.package("3.8.0-rust", 2, 1, None);
    let host = UpdateHost::new(2, Duration::from_secs(30)).unwrap();
    let job = host
        .establish_baseline(
            path.clone(),
            pins.clone(),
            platform.clone(),
            bundle,
            archive,
            0,
        )
        .unwrap();
    assert!(job.wait().await.is_err());
    assert!(std::fs::read_dir(&path).unwrap().next().is_none());
    assert!(host.drain().await.unwrap()[0].result.is_err());
    let host = UpdateHost::new(3, Duration::from_secs(30)).unwrap();
    assert!(host
        .bootstrap(path, pins, platform)
        .unwrap()
        .wait()
        .await
        .is_err());
    assert!(host.drain().await.unwrap()[0].result.is_err());
}

#[test]
fn private_health_bootstrap_rejects_truncation_names_profile_and_pid() {
    for bytes in [vec![], vec![0; 264], vec![0; 400]] {
        assert!(process::HealthBootstrap::read(&mut std::io::Cursor::new(bytes)).is_err());
    }
}
