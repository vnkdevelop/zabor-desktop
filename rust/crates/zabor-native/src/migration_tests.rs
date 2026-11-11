use super::*;
use crate::chat_crypto::ChatIdentity;
#[cfg(all(windows, feature = "profile-import-test-source"))]
use crate::critical_intents::CriticalAction;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
#[cfg(all(windows, feature = "profile-import-test-source"))]
use sha2::{Digest, Sha256};
#[cfg(all(windows, feature = "profile-import-test-source"))]
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

fn args(server: &str, parent: &std::path::Path, action: &[&str]) -> Vec<String> {
    let mut args = vec![
        "--parent".into(),
        parent.display().to_string(),
        "--server".into(),
        server.into(),
        "--approved-server".into(),
        server.into(),
        "--confirmed-no-identity".into(),
    ];
    args.extend(action.iter().map(|value| (*value).into()));
    args
}

#[test]
fn approvals_and_secret_arguments_are_fail_closed_before_resources() {
    let root = tempfile::tempdir().unwrap();
    let valid = args(
        "http://127.0.0.1:1/hub",
        root.path(),
        &["--rollback-profile", "--consent", "rollback"],
    );
    assert!(MigrationCommand::parse(&valid, |_| None).is_ok());
    for extra in [
        vec!["--password", "fixture-only"],
        vec!["--username", "fixture"],
        vec!["--app"],
        vec!["--consent", "rollback"],
        vec!["--import-profile", "source"],
    ] {
        let mut invalid = valid.clone();
        invalid.extend(extra.into_iter().map(String::from));
        assert!(MigrationCommand::parse(&invalid, |_| None).is_err());
    }
    let mut invalid = valid.clone();
    invalid[5] = "https://example.invalid/hub".into();
    assert!(MigrationCommand::parse(&invalid, |_| None).is_err());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    assert!(is_command(&["--app".into(), "--recover-profile".into()]));
    assert!(is_command(&[
        "--app".into(),
        "--import-profile=source".into()
    ]));
    assert!(matches!(
        run_cli(&["--app".into(), "--import-profile=source".into()]),
        Err(MigrationError::Approval)
    ));
}

#[test]
fn public_key_requires_independent_explicit_input_and_never_accepts_private_jwk() {
    let root = tempfile::tempdir().unwrap();
    let mut arguments = args(
        "http://127.0.0.1:1/hub",
        root.path(),
        &[
            "--recover-profile",
            "publish-stage",
            "--consent",
            "publish-stage",
        ],
    );
    arguments[6] = "--confirmed-key".into();
    assert!(MigrationCommand::parse(&arguments, |_| None).is_err());
    let identity = ChatIdentity::generate();
    let public = identity.public_key().unwrap().to_string();
    assert!(MigrationCommand::parse(&arguments, |_| Some(Zeroizing::new(public.clone()))).is_ok());
    let mut private = identity.public_key().unwrap();
    private["d"] = json!("fixture-not-a-key");
    assert!(
        MigrationCommand::parse(&arguments, |_| Some(Zeroizing::new(private.to_string()))).is_err()
    );
    assert!(MigrationCommand::parse(
        &args(
            "http://127.0.0.1:1/hub",
            root.path(),
            &["--rollback-profile", "--consent", "rollback"]
        ),
        |_| Some(Zeroizing::new(public.clone()))
    )
    .is_err());
}

async fn server(user_id: &str, login_ok: bool) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/hub", listener.local_addr().unwrap());
    let user_id = user_id.to_owned();
    let task = tokio::spawn(async move {
        let (mut http, _) = listener.accept().await.unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            header.push(http.read_u8().await.unwrap());
            assert!(header.len() < 8192);
        }
        let body = json!({"connectionToken":"fixture","availableTransports":[{"transport":"WebSockets","transferFormats":["Text"]}]}).to_string();
        http.write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        drop(http);
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
        socket.next().await.unwrap().unwrap();
        socket.send(Message::text("{}\u{1e}")).await.unwrap();
        let mut decoder = crate::protocol::Decoder::default();
        let mut methods = Vec::new();
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            for request in decoder.push(text.as_bytes()).unwrap() {
                if request["type"] == 6 {
                    continue;
                }
                let target = request["target"].as_str().unwrap();
                methods.push(target.into());
                let result = match target {
                    "CheckUserExists" => json!(true),
                    "Login" if login_ok => {
                        json!({"id":user_id,"username":"fixture","displayName":"Fixture"})
                    }
                    "GetChannels" | "GetFriends" | "GetFriendRequests" | "GetChannelInvites" => {
                        json!([])
                    }
                    _ => Value::Null,
                };
                let response =
                    json!({"type":3,"invocationId":request["invocationId"],"result":result});
                if socket
                    .send(Message::text(crate::protocol::encode(&response).unwrap()))
                    .await
                    .is_err()
                {
                    return methods;
                }
            }
        }
        methods
    });
    (endpoint, task)
}

fn credentials() -> Credentials {
    Credentials {
        username: "fixture".into(),
        password: Zeroizing::new("fresh-fixture-only".into()),
    }
}

#[cfg(all(windows, feature = "profile-import-test-source"))]
fn record_retirement(name: &str, receipt: &MigrationRetirement) {
    let Some(directory) = std::env::var_os("ZABOR_IMPORT_RECEIPT_DIR") else {
        return;
    };
    let records: Vec<_> = receipt.registry().iter().map(|entry| json!({
        "operation":entry.operation,"pid":entry.pid,"ownerStarted":entry.owner_started,
        "ownerJoined":entry.owner_joined,"rootReaped":entry.root_reaped,"exitCode":entry.exit_code,
        "jobEmpty":entry.job_empty,"readerStarted":entry.reader_started,"readerJoined":entry.reader_joined,
        "pipeEof":entry.pipe_eof,"handlesClosed":entry.handles_closed,
        "failure":entry.failure.map(|failure| format!("{failure:?}"))
    })).collect();
    let bytes = serde_json::to_vec_pretty(
        &json!({"originalOperation":receipt.exporter().operation,"registry":records,
        "failure":receipt.failure().map(|failure| format!("{failure:?}"))}),
    )
    .unwrap();
    assert!(bytes.len() < 4096);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(PathBuf::from(directory).join(name))
        .unwrap();
    std::io::Write::write_all(&mut file, &bytes).unwrap();
    file.sync_all().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn supplied_owner_shape_is_not_authentication_and_missing_auth_never_touches_beta() {
    let root = tempfile::tempdir().unwrap();
    let (endpoint, server) = server("alice", false).await;
    let command = MigrationCommand::parse(
        &args(
            &endpoint,
            root.path(),
            &["--rollback-profile", "--consent", "rollback"],
        ),
        |_| None,
    )
    .unwrap();
    let intents = Arc::new(CriticalIntents::default());
    let mut operation =
        MigrationOperation::new(command, intents.clone(), intents.ticket()).unwrap();
    assert!(matches!(
        operation.execute(),
        Err(MigrationError::Authentication)
    ));
    assert!(matches!(
        operation.authenticate(credentials(), None).await,
        Err(MigrationError::Authentication)
    ));
    assert!(operation.execute().is_err());
    operation.close().await.unwrap();
    assert_eq!(server.await.unwrap(), ["CheckUserExists", "Login"]);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retired_authenticated_connection_cannot_authorize_a_later_storage_operation() {
    let root = tempfile::tempdir().unwrap();
    let (endpoint, server) = server("alice", true).await;
    let command = MigrationCommand::parse(
        &args(
            &endpoint,
            root.path(),
            &["--rollback-profile", "--consent", "rollback"],
        ),
        |_| None,
    )
    .unwrap();
    let intents = Arc::new(CriticalIntents::default());
    let mut operation =
        MigrationOperation::new(command, intents.clone(), intents.ticket()).unwrap();
    operation.authenticate(credentials(), None).await.unwrap();
    operation.hub.as_ref().unwrap().close();
    assert!(matches!(
        operation.execute(),
        Err(MigrationError::Cancelled)
    ));
    operation.close().await.unwrap();
    server.await.unwrap();
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropped_close_waiter_retains_the_original_monitor_join_in_its_operation() {
    let root = tempfile::tempdir().unwrap();
    let command = MigrationCommand::parse(
        &args(
            "http://127.0.0.1:1/hub",
            root.path(),
            &["--rollback-profile", "--consent", "rollback"],
        ),
        |_| None,
    )
    .unwrap();
    let intents = Arc::new(CriticalIntents::default());
    let mut operation =
        MigrationOperation::new(command, intents.clone(), intents.ticket()).unwrap();
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    operation.monitor = Some(tokio::spawn(async move {
        let _ = released.await;
    }));
    let mut closing = Box::pin(operation.close());
    assert!(futures_util::poll!(&mut closing).is_pending());
    drop(closing);
    assert!(operation
        .monitor
        .as_ref()
        .is_some_and(|monitor| !monitor.is_finished()));
    release.send(()).unwrap();
    operation.close().await.unwrap();
    assert!(operation.monitor.is_none());
}

#[cfg(all(windows, feature = "profile-import-test-source"))]
struct Fixture {
    root: tempfile::TempDir,
    source: PathBuf,
    scratch: PathBuf,
    parent: PathBuf,
    runtime: PathBuf,
    helper: PathBuf,
    public: Value,
}

#[cfg(all(windows, feature = "profile-import-test-source"))]
impl Fixture {
    fn new(mode: &[u8]) -> Self {
        let temp = std::env::var_os("ZABOR_TEST_TEMP").expect("approved fixture temp required");
        let root = tempfile::tempdir_in(&temp).unwrap();
        let source = root.path().join("source");
        let scratch = root.path().join("scratch");
        let parent = root.path().join("parent");
        for path in [&source, &scratch, &parent] {
            std::fs::create_dir(path).unwrap();
        }
        let runtime = PathBuf::from(
            std::env::var_os("ZABOR_IMPORT_MOCK_RUNTIME")
                .expect("compiled synthetic mock required"),
        );
        assert_eq!(runtime.parent(), Some(std::path::Path::new(&temp)));
        assert_eq!(runtime.file_name().unwrap(), "mock.exe");
        let helper = root.path().join("mock-helper.cjs");
        std::fs::write(&helper, b"synthetic-helper-v1").unwrap();
        let identity = ChatIdentity::generate();
        let public = identity.public_key().unwrap();
        let export = Zeroizing::new(json!({"version":1,"session":{"userId":"alice","login":"fixture","password":"old-fixture-only","settings":{}},
            "identity":identity.export().unwrap().as_str(),"appSettings":{},"localStorage":{},"messages":[]}).to_string());
        std::fs::write(source.join("session.json"), export.as_bytes()).unwrap();
        std::fs::write(source.join("Local State"), mode).unwrap();
        Self {
            root,
            source,
            scratch,
            parent,
            runtime,
            helper,
            public,
        }
    }

    fn command(&self, endpoint: &str) -> MigrationCommand {
        let mut command = args(
            endpoint,
            &self.parent,
            &["--import-profile", "unused", "--consent", "copy-import"],
        );
        command[6] = "--confirmed-key".into();
        command[8] = self.source.display().to_string();
        for (name, path) in [
            ("--scratch", &self.scratch),
            ("--runtime", &self.runtime),
            ("--helper", &self.helper),
            (
                "--approved-runtime-bundle",
                &self.runtime.parent().unwrap().to_path_buf(),
            ),
        ] {
            command.push(name.into());
            command.push(path.display().to_string());
        }
        MigrationCommand::parse(&command, |name| {
            Some(Zeroizing::new(match name {
                "ZABOR_PROFILE_EXPECTED_PUBLIC_KEY" => self.public.to_string(),
                "ZABOR_PROFILE_HELPER_TRUSTED_SHA256" => format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(&self.runtime).unwrap())
                ),
                "ZABOR_PROFILE_EXPORT_TRUSTED_SHA256" => {
                    format!("{:x}", Sha256::digest(b"synthetic-helper-v1"))
                }
                _ => panic!("unexpected input"),
            }))
        })
        .unwrap()
    }
}

#[cfg(all(windows, feature = "profile-import-test-source"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pinned_mock_copy_import_fresh_auth_reopen_and_owner_bound_rollback() {
    let fixture = Fixture::new(b"");
    let original = std::fs::read(fixture.source.join("session.json")).unwrap();
    let (endpoint, server) = server("alice", true).await;
    let intents = Arc::new(CriticalIntents::default());
    let mut operation = MigrationOperation::new(
        fixture.command(&endpoint),
        intents.clone(),
        intents.ticket(),
    )
    .unwrap();
    let cancellation = operation.cancellation();
    operation.authenticate(credentials(), None).await.unwrap();
    assert!(!fixture.parent.join(import::BETA_PROFILE).exists());
    operation.execute().unwrap();
    let observation = cancellation.helper.observation().unwrap();
    assert!(
        observation.owner_joined
            && observation.reader_joined
            && observation.root_reaped
            && observation.job_empty
            && observation.handles_closed
    );
    assert_eq!(std::fs::read_dir(&fixture.scratch).unwrap().count(), 0);
    assert!(std::fs::read(fixture.source.join("session.json")).unwrap() == original);
    let profile = Profile::open(&fixture.parent.join(import::BETA_PROFILE)).unwrap();
    let saved = profile.load_session().unwrap().unwrap();
    assert!(saved.password.as_str() == "fresh-fixture-only");
    drop(profile);
    operation.close().await.unwrap();
    let methods = server.await.unwrap();
    assert!(
        methods.iter().any(|method| method == "Login")
            && !methods.iter().any(|method| method == "Register")
    );
    let (endpoint, server) = self::server("alice", true).await;
    let command = MigrationCommand::parse(
        &args(
            &endpoint,
            &fixture.parent,
            &["--rollback-profile", "--consent", "rollback"],
        ),
        |_| None,
    )
    .unwrap();
    let mut rollback = MigrationOperation::new(command, intents.clone(), intents.ticket()).unwrap();
    rollback.authenticate(credentials(), None).await.unwrap();
    rollback.execute().unwrap();
    rollback.close().await.unwrap();
    server.await.unwrap();
    assert!(!fixture.parent.join(import::BETA_PROFILE).exists());
    assert!(fixture
        .parent
        .join(".zabor-desktop-native-beta.import-rollback")
        .is_dir());
    fixture.root.close().unwrap();
}

#[cfg(all(windows, feature = "profile-import-test-source"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_drains_original_process_and_copy_before_any_publication() {
    let fixture = Fixture::new(b"delay");
    let (endpoint, server) = server("alice", true).await;
    let intents = Arc::new(CriticalIntents::default());
    let mut operation = MigrationOperation::new(
        fixture.command(&endpoint),
        intents.clone(),
        intents.ticket(),
    )
    .unwrap();
    operation.authenticate(credentials(), None).await.unwrap();
    let cancellation = operation.cancellation();
    let cancel = cancellation.clone();
    let requester = tokio::spawn(async move {
        for _ in 0..500 {
            if cancel.helper.observation().unwrap().reader_started {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(cancel.helper.observation().unwrap().reader_started);
        cancel.cancel();
    });
    assert!(operation.execute().is_err());
    requester.await.unwrap();
    operation.close().await.unwrap();
    server.await.unwrap();
    let observation = cancellation.helper.observation().unwrap();
    assert!(
        observation.owner_joined
            && observation.root_reaped
            && observation.job_empty
            && observation.reader_joined
            && observation.handles_closed
    );
    assert_eq!(std::fs::read_dir(&fixture.scratch).unwrap().count(), 0);
    assert!(!fixture.parent.join(import::BETA_PROFILE).exists());
    fixture.root.close().unwrap();
}

#[cfg(all(windows, feature = "profile-import-test-source"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logout_and_wrong_authenticated_owner_never_publish_or_acquire_replacement_ticket() {
    for logout in [true, false] {
        let fixture = Fixture::new(b"");
        let (endpoint, server) = server(if logout { "alice" } else { "other" }, true).await;
        let intents = Arc::new(CriticalIntents::default());
        let ticket = intents.ticket();
        let mut operation =
            MigrationOperation::new(fixture.command(&endpoint), intents.clone(), ticket).unwrap();
        operation.authenticate(credentials(), None).await.unwrap();
        if logout {
            intents.request(CriticalAction::Logout);
        }
        assert!(operation.execute().is_err());
        operation.close().await.unwrap();
        server.await.unwrap();
        assert_eq!(operation.ticket, ticket);
        assert!(!fixture.parent.join(import::BETA_PROFILE).exists());
        assert_eq!(std::fs::read_dir(&fixture.scratch).unwrap().count(), 0);
        fixture.root.close().unwrap();
    }
}

#[cfg(all(windows, feature = "profile-import-test-source"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bad_runtime_pin_bundle_helper_failure_and_wrong_key_remain_fail_closed() {
    for variant in 0..4 {
        let fixture = Fixture::new(if variant == 2 { b"fail" } else { b"" });
        let (endpoint, server) = server("alice", true).await;
        let mut command = fixture.command(&endpoint);
        if let Action::Import(config) = &mut command.action {
            if variant == 0 {
                config.runtime_hash = [0; 32];
            }
            if variant == 1 {
                config.bundle = fixture.parent.clone();
            }
        }
        if variant == 3 {
            command.expected_key = Some(ChatIdentity::generate().public_key().unwrap());
        }
        let intents = Arc::new(CriticalIntents::default());
        let mut operation =
            MigrationOperation::new(command, intents.clone(), intents.ticket()).unwrap();
        let cancellation = operation.cancellation();
        operation.authenticate(credentials(), None).await.unwrap();
        assert!(operation.execute().is_err());
        operation.close().await.unwrap();
        server.await.unwrap();
        let observation = cancellation.helper.observation().unwrap();
        if variant < 2 {
            assert!(observation.pid.is_none());
        } else {
            assert!(
                observation.owner_joined
                    && observation.root_reaped
                    && observation.job_empty
                    && observation.reader_joined
                    && observation.handles_closed
            );
        }
        assert_eq!(std::fs::read_dir(&fixture.scratch).unwrap().count(), 0);
        assert!(!fixture.parent.join(import::BETA_PROFILE).exists());
        fixture.root.close().unwrap();
    }
}

#[cfg(all(windows, feature = "profile-import-test-source"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_recovery_route_authenticates_again_and_never_auto_opens_pending_beta() {
    for action in ["publish-stage", "confirm-published"] {
        let fixture = Fixture::new(b"");
        let snapshot = import::Snapshot::copy(&fixture.source, &fixture.scratch).unwrap();
        let owner =
            ImportOwner::verified("alice", "fixture", Some(fixture.public.clone())).unwrap();
        let export = Zeroizing::new(std::fs::read(snapshot.root().join("session.json")).unwrap());
        let now = i64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap();
        drop(PreparedImport::prepare(&snapshot, &fixture.parent, &owner, &export, now).unwrap());
        snapshot.close().unwrap();
        if action == "confirm-published" {
            std::fs::rename(
                fixture
                    .parent
                    .join(".zabor-desktop-native-beta.import-stage"),
                fixture.parent.join(import::BETA_PROFILE),
            )
            .unwrap();
        }
        assert!(Profile::open(&fixture.parent.join(import::BETA_PROFILE)).is_err());
        for user in ["other", "alice"] {
            let (endpoint, server) = server(user, true).await;
            let mut arguments = args(
                &endpoint,
                &fixture.parent,
                &["--recover-profile", action, "--consent", action],
            );
            arguments[6] = "--confirmed-key".into();
            let command = MigrationCommand::parse(&arguments, |_| {
                Some(Zeroizing::new(fixture.public.to_string()))
            })
            .unwrap();
            let intents = Arc::new(CriticalIntents::default());
            let mut operation =
                MigrationOperation::new(command, intents.clone(), intents.ticket()).unwrap();
            operation.authenticate(credentials(), None).await.unwrap();
            if user == "other" {
                assert!(operation.execute().is_err());
                assert!(fixture
                    .parent
                    .join(".zabor-desktop-native-beta.import.pending.json")
                    .exists());
                assert!(Profile::open(&fixture.parent.join(import::BETA_PROFILE)).is_err());
            } else {
                operation.execute().unwrap();
            }
            operation.close().await.unwrap();
            assert!(server.await.unwrap().iter().any(|method| method == "Login"));
        }
        let profile = Profile::open(&fixture.parent.join(import::BETA_PROFILE)).unwrap();
        assert!(profile.load_session().unwrap().unwrap().password.as_str() == "fresh-fixture-only");
        drop(profile);
        fixture.root.close().unwrap();
    }
}

#[cfg(all(windows, feature = "profile-import-test-source"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn original_registry_receipt_and_profile_gate_survive_until_same_ticket_installation() {
    let fixture = Fixture::new(b"");
    let (endpoint, server) = server("alice", true).await;
    let intents = Arc::new(CriticalIntents::default());
    let ticket = intents.ticket();
    let registry = OwnerRegistry::new();
    let earlier = registry.prepare().unwrap();
    let mut operation = MigrationOperation::with_registry(
        fixture.command(&endpoint),
        intents.clone(),
        ticket,
        registry.clone(),
    )
    .unwrap();
    let original_id = operation
        .cancellation()
        .exporter_observation()
        .unwrap()
        .operation;
    assert_ne!(original_id, earlier.id());
    operation.authenticate(credentials(), None).await.unwrap();
    let publication = operation.execute_for_reuse().unwrap().unwrap();
    let receipt = publication.retirement();
    assert_eq!(receipt.exporter().operation, original_id);
    assert!(receipt.exported());
    assert_eq!(receipt.registry().len(), 2);
    record_retirement("caller-registry-success.json", receipt);
    assert!(registry.prepare().is_err());
    assert!(Profile::open(&fixture.parent.join(import::BETA_PROFILE)).is_err());
    let (profile, installed_ticket) = publication
        .install(&operation, |profile, original| (profile, original))
        .unwrap();
    assert_eq!(installed_ticket, ticket);
    assert!(Profile::open(&fixture.parent.join(import::BETA_PROFILE)).is_err());
    assert!(profile.load_session().unwrap().unwrap().password.as_str() == "fresh-fixture-only");
    drop(profile);
    drop(earlier);
    operation.close().await.unwrap();
    server.await.unwrap();
    fixture.root.close().unwrap();
}

#[cfg(all(windows, feature = "profile-import-test-source"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn completed_publication_cannot_install_after_logout_or_into_a_replacement_operation() {
    for logout in [true, false] {
        let fixture = Fixture::new(b"");
        let (endpoint, server) = server("alice", true).await;
        let intents = Arc::new(CriticalIntents::default());
        let ticket = intents.ticket();
        let mut operation =
            MigrationOperation::new(fixture.command(&endpoint), intents.clone(), ticket).unwrap();
        operation.authenticate(credentials(), None).await.unwrap();
        let publication = operation.execute_for_reuse().unwrap().unwrap();
        let installed = std::sync::atomic::AtomicBool::new(false);
        if logout {
            intents.request(CriticalAction::Logout);
            assert!(publication
                .install(&operation, |_, _| installed
                    .store(true, std::sync::atomic::Ordering::SeqCst))
                .is_err());
        } else {
            let (replacement_endpoint, replacement_server) = self::server("alice", true).await;
            let mut replacement = MigrationOperation::new(
                fixture.command(&replacement_endpoint),
                intents.clone(),
                ticket,
            )
            .unwrap();
            replacement.authenticate(credentials(), None).await.unwrap();
            assert_eq!(
                publication.retirement().exporter().operation,
                replacement
                    .cancellation()
                    .exporter_observation()
                    .unwrap()
                    .operation
            );
            assert!(publication
                .install(&replacement, |_, _| installed
                    .store(true, std::sync::atomic::Ordering::SeqCst))
                .is_err());
            replacement.close().await.unwrap();
            replacement_server.await.unwrap();
        }
        assert!(!installed.load(std::sync::atomic::Ordering::SeqCst));
        operation.close().await.unwrap();
        server.await.unwrap();
        fixture.root.close().unwrap();
    }
}

#[cfg(all(windows, feature = "profile-import-test-source"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_export_retains_original_exit_receipt_without_empty_import_or_process_fallback() {
    let fixture = Fixture::new(b"fail");
    let (endpoint, server) = server("alice", true).await;
    let intents = Arc::new(CriticalIntents::default());
    let ticket = intents.ticket();
    let mut operation =
        MigrationOperation::new(fixture.command(&endpoint), intents.clone(), ticket).unwrap();
    operation.authenticate(credentials(), None).await.unwrap();
    assert!(operation.execute_for_reuse().is_err());
    let failed = operation.retirement().unwrap();
    assert!(failed.retired());
    assert!(!failed.exported());
    assert_eq!(failed.failure(), Some(Failure::Process));
    assert_eq!(failed.exporter().exit_code, Some(7));
    let original_pid = failed.exporter().pid;
    assert!(original_pid.is_some());
    assert!(operation.execute_for_reuse().is_err());
    operation.close().await.unwrap();
    let retained = operation.retirement().unwrap();
    assert_eq!(retained.exporter().pid, original_pid);
    assert_eq!(retained.exporter().exit_code, Some(7));
    assert_eq!(retained.failure(), Some(Failure::Process));
    record_retirement("failed-export-original.json", &retained);
    assert!(!fixture.parent.join(import::BETA_PROFILE).exists());
    assert_eq!(std::fs::read_dir(&fixture.scratch).unwrap().count(), 0);
    server.await.unwrap();
    fixture.root.close().unwrap();
}
