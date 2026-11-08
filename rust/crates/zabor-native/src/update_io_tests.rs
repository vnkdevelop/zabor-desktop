use super::*;
use crate::update_helper::tests::Fixture;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn server(
    body: Vec<u8>,
    delay: Duration,
    header_length: Option<usize>,
) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}/fixture.zip", listener.local_addr().unwrap());
    let job = tokio::spawn(async move {
        let (mut connection, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        let _ = connection.read(&mut request).await;
        tokio::time::sleep(delay).await;
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            header_length.unwrap_or(body.len())
        );
        let _ = connection.write_all(header.as_bytes()).await;
        let _ = connection.write_all(&body).await;
    });
    (address, job)
}

#[tokio::test]
async fn actual_loopback_download_is_operation_owned_and_exactly_hashed() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let io = UpdateIo::new(root.clone());
    let token = root.begin(2).unwrap();
    let (bundle, _, body) = fixture.package("3.8.0-rust", 2, 1, None);
    let expected = digest_test(&body);
    let (url, server) = server(body, Duration::ZERO, None).await;
    let archive = io
        .download(
            2,
            root.validate(bundle.clone(), 150).unwrap(),
            token.clone(),
            &url,
            Duration::from_secs(30),
        )
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(archive.sha256, expected);
    assert!(archive.path().is_file());
    io.stage(
        StageOptions {
            operation: 2,
            cancellation: token,
            budget: Duration::from_secs(30),
            now: 150,
            crash: CrashPoint::None,
        },
        root.validate(bundle, 150).unwrap(),
        archive.clone(),
    )
    .unwrap()
    .wait()
    .await
    .unwrap();
    drop(archive);
    server.await.unwrap();
    let reports = io.drain().await.unwrap();
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|report| report.result.is_ok()));
}

fn digest_test(bytes: &[u8]) -> String {
    crate::update_helper::digest(bytes)
}

#[tokio::test]
async fn cancellation_and_dropped_result_keep_original_join_observed() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let io = UpdateIo::new(root.clone());
    let token = root.begin(2).unwrap();
    let (bundle, _, body) = fixture.package("3.8.0-rust", 2, 1, None);
    let (url, server) = server(body, Duration::from_millis(150), None).await;
    let job = io
        .download(
            2,
            root.validate(bundle, 150).unwrap(),
            token,
            &url,
            Duration::from_secs(30),
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    drop(job);
    let reports = io.drain().await.unwrap();
    assert_eq!(reports.len(), 1);
    assert!(reports[0].result.is_err());
    server.await.unwrap();
    assert!(!root.path().read_dir().unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".part")));
}

#[tokio::test]
async fn stale_operation_rejects_late_response_and_does_not_publish_stage() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let io = UpdateIo::new(root.clone());
    let token = root.begin(2).unwrap();
    let (bundle, _, body) = fixture.package("3.8.0-rust", 2, 1, None);
    let (url, server) = server(body, Duration::from_millis(150), None).await;
    let job = io
        .download(
            2,
            root.validate(bundle, 150).unwrap(),
            token,
            &url,
            Duration::from_secs(30),
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    root.begin(3).unwrap();
    assert!(job.wait().await.is_err());
    assert_eq!(io.drain().await.unwrap().len(), 1);
    server.await.unwrap();
    assert!(root
        .current_executable()
        .unwrap()
        .ends_with("bootstrap/zabor-native.exe"));
}

#[tokio::test]
async fn wrong_length_digest_and_timeout_are_not_download_success() {
    for scenario in 0..3 {
        let fixture = Fixture::new();
        let root = fixture.root();
        let io = UpdateIo::new(root.clone());
        let token = root.begin(2).unwrap();
        let (bundle, _, mut body) = fixture.package("3.8.0-rust", 2, 1, None);
        let length = body.len();
        if scenario == 1 {
            body[50] ^= 1;
        }
        let (url, server) = server(
            body,
            if scenario == 2 {
                Duration::from_millis(200)
            } else {
                Duration::ZERO
            },
            if scenario == 0 {
                Some(length + 1)
            } else {
                None
            },
        )
        .await;
        let budget = if scenario == 2 {
            Duration::from_millis(75)
        } else {
            Duration::from_secs(30)
        };
        assert!(io
            .download(2, root.validate(bundle, 150).unwrap(), token, &url, budget)
            .unwrap()
            .wait()
            .await
            .is_err());
        assert_eq!(io.drain().await.unwrap().len(), 1);
        server.await.unwrap();
    }
}

#[tokio::test]
async fn sealed_owner_and_unsafe_urls_never_acquire_download_files() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let io = UpdateIo::new(root.clone());
    let token = root.begin(2).unwrap();
    let (bundle, _, _) = fixture.package("3.8.0-rust", 2, 1, None);
    let candidate = root.validate(bundle, 150).unwrap();
    for url in [
        "file:///C:/profile",
        "http://localhost/fixture",
        "http://127.0.0.1:1/fixture?override=x",
        "https://example.com/fixture",
        "https://github.com/vnkdevelop/zabor-desktop/releases/download/v3.8.0-rust/ZABOR-Native-Beta-3.8.0-rust-x64.zip",
    ] {
        assert!(io
            .download(
                2,
                candidate.clone(),
                token.clone(),
                url,
                Duration::from_secs(30)
            )
            .is_err());
    }
    io.drain().await.unwrap();
    assert!(io
        .download(
            2,
            candidate,
            token,
            "http://127.0.0.1:1/fixture",
            Duration::from_secs(30)
        )
        .is_err());
}

#[tokio::test]
async fn cancelled_waiter_and_cancelled_drain_preserve_original_blocking_join() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let io = UpdateIo::new(root.clone());
    let token = root.begin(2).unwrap();
    let (release, blocked) = std::sync::mpsc::channel::<()>();
    let (entered, started) = oneshot::channel();
    let job = io
        .register(2, token, async move {
            tokio::task::spawn_blocking(move || {
                let _ = entered.send(());
                blocked
                    .recv()
                    .map_err(|_| "fixture-owner-release-lost".to_string())
            })
            .await
            .map_err(|_| "fixture-owner-panicked".to_string())?
        })
        .unwrap();
    started.await.unwrap();
    job.cancel();
    assert!(tokio::time::timeout(Duration::from_millis(75), job.wait())
        .await
        .unwrap()
        .is_err());
    assert!(tokio::time::timeout(Duration::from_millis(50), io.drain())
        .await
        .is_err());
    assert_eq!(io.registry.jobs.lock().unwrap().len(), 1);
    release.send(()).unwrap();
    let reports = io.drain().await.unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].operation, 2);
    assert_eq!(io.drain().await.unwrap().len(), 1);
}

#[tokio::test]
async fn panic_outcome_is_retained_by_original_operation_after_drain_retry() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let io = UpdateIo::new(root.clone());
    let token = root.begin(2).unwrap();
    let job = io
        .register::<()>(2, token, async { panic!("LOCAL FAILURE INJECTION") })
        .unwrap();
    assert!(job.wait().await.is_err());
    let reports = io.drain().await.unwrap();
    assert_eq!(reports.len(), 1);
    assert!(reports[0].result.is_err());
    assert_eq!(io.drain().await.unwrap()[0].operation, 2);
}

#[tokio::test]
async fn owner_drain_future_is_send_for_registered_main_cleanup() {
    fn check_send<T: Send>(_: T) {}
    let fixture = Fixture::new();
    let io = UpdateIo::new(fixture.root());
    check_send(io.drain());
    assert!(io.drain().await.unwrap().is_empty());
}
