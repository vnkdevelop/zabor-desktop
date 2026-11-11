use super::*;
use crate::update_helper::tests::Fixture;

fn local(fixture: &Fixture, host: Arc<UpdateHost>) -> (ShellAdmission, Arc<Profile>) {
    let root = fixture.root();
    let execution = root
        .hold_current_startup_package(&root.current_executable().unwrap())
        .unwrap();
    let profile = Arc::new(Profile::open(&fixture.directory.path().join("local-profile")).unwrap());
    (
        ShellAdmission {
            host,
            execution: Arc::new(execution),
            health: None,
        },
        profile,
    )
}

#[tokio::test]
async fn startup_readiness_waits_for_original_worker_and_observes_late_failure() {
    let fixture = Fixture::new();
    let host = Arc::new(UpdateHost::new(1, Duration::from_secs(30)).unwrap());
    let (admission, profile) = local(&fixture, host.clone());
    let (send, receive) = oneshot::channel();
    let job = admission.local_ui_ready(profile, receive).unwrap();
    send.send(Err("original-worker-late-settings-failure".into()))
        .unwrap();
    assert_eq!(
        job.wait().await.unwrap_err(),
        "original-worker-late-settings-failure"
    );
    let reports = host.drain().await.unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports[0].result.as_ref().unwrap_err(),
        "original-worker-late-settings-failure"
    );
}

#[tokio::test]
async fn startup_readiness_missing_original_worker_is_not_success_and_cancellation_is_observed() {
    let fixture = Fixture::new();
    for cancelled in [false, true] {
        let host = Arc::new(UpdateHost::new(1, Duration::from_secs(30)).unwrap());
        let (admission, profile) = local(&fixture, host.clone());
        let (send, receive) = oneshot::channel();
        let job = admission.local_ui_ready(profile, receive).unwrap();
        if cancelled {
            job.cancel();
        } else {
            drop(send);
        }
        assert!(job.wait().await.is_err());
        let reports = host.drain().await.unwrap();
        assert_eq!(reports.len(), 1);
        assert!(reports[0].result.is_err());
    }
}

#[tokio::test]
async fn startup_local_profile_and_settings_are_read_only_after_original_worker_initialization() {
    let fixture = Fixture::new();
    let host = Arc::new(UpdateHost::new(1, Duration::from_secs(30)).unwrap());
    let (admission, profile) = local(&fixture, host.clone());
    let (send, receive) = oneshot::channel();
    let job = admission.local_ui_ready(profile.clone(), receive).unwrap();
    profile
        .write_settings(&serde_json::json!({"language":"en"}))
        .unwrap();
    send.send(Ok(())).unwrap();
    job.wait().await.unwrap();
    let reports = host.drain().await.unwrap();
    assert_eq!(reports.len(), 1);
    assert!(reports[0].result.is_ok());
    assert!(!admission.is_health_target());
}

#[tokio::test]
async fn startup_cancelled_waiter_still_retains_and_observes_original_blocking_exit() {
    let host = Arc::new(UpdateHost::new(1, Duration::from_secs(30)).unwrap());
    let (started, ready) = oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let job = host
        .blocking(move |cancel, deadline| {
            started.send(()).unwrap();
            released.recv().unwrap();
            gate(&cancel, deadline)?;
            Err::<(), _>("late-original-owner-failure".into())
        })
        .unwrap();
    ready.await.unwrap();
    job.cancel();
    assert!(job.wait().await.is_err());
    release.send(()).unwrap();
    let reports = host.drain().await.unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports[0].result.as_ref().unwrap_err(),
        "startup-original-owner-cancelled-or-expired"
    );
}

#[test]
fn startup_config_requires_real_independent_keys_hashes_and_absolute_paths() {
    assert!(hex("AA").is_err());
    assert!(hex("a").is_err());
    assert_eq!(hex("01af").unwrap(), [1, 175]);
    let invalid = StartupConfig {
        root: "relative".into(),
        helper: "helper.exe".into(),
        helper_sha256: "0".repeat(64),
        pins: TrustPins {
            manifest_key: Vec::new(),
            acceptance_key: Vec::new(),
            publisher_certificate_sha256: "0".repeat(64),
        },
    };
    assert!(invalid.validate().is_err());
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(gate(&cancel, Instant::now() + Duration::from_secs(1)).is_err());
    assert!(gate(&CancellationToken::new(), Instant::now()).is_err());
}
