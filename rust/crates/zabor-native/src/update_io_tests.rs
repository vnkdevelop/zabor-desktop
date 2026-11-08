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

struct Reply {
    path: String,
    status: u16,
    locations: Vec<Vec<u8>>,
    body: Vec<u8>,
    length: Option<usize>,
    omit_length: bool,
    delay: Duration,
    cookie: bool,
}

impl Reply {
    fn redirect(path: &str, status: u16, location: &str) -> Self {
        Self {
            path: path.into(),
            status,
            locations: vec![location.as_bytes().to_vec()],
            body: b"redirect-body".to_vec(),
            length: None,
            omit_length: false,
            delay: Duration::ZERO,
            cookie: false,
        }
    }

    fn final_body(path: &str, body: Vec<u8>) -> Self {
        Self {
            path: path.into(),
            status: 200,
            locations: Vec::new(),
            body,
            length: None,
            omit_length: false,
            delay: Duration::ZERO,
            cookie: false,
        }
    }
}

struct Router {
    url: String,
    requests: tokio::sync::mpsc::UnboundedReceiver<String>,
    join: JoinHandle<Vec<String>>,
}

async fn router(build: impl FnOnce(std::net::SocketAddr) -> Vec<Reply>) -> Router {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let replies = build(address);
    let (send, requests) = tokio::sync::mpsc::unbounded_channel();
    let join = tokio::spawn(async move {
        let mut observed = Vec::new();
        for reply in replies {
            let accepted =
                tokio::time::timeout(Duration::from_millis(500), listener.accept()).await;
            let Ok(Ok((mut connection, _))) = accepted else {
                break;
            };
            let mut raw = Vec::new();
            let mut buffer = [0; 4096];
            while raw.len() < 16384 && !raw.windows(4).any(|part| part == b"\r\n\r\n") {
                let count = connection.read(&mut buffer).await.unwrap_or(0);
                if count == 0 {
                    break;
                }
                raw.extend_from_slice(&buffer[..count]);
            }
            let request = String::from_utf8_lossy(&raw).into_owned();
            assert!(request.starts_with(&format!("GET {} HTTP/1.1", reply.path)));
            let _ = send.send(request.clone());
            observed.push(request);
            tokio::time::sleep(reply.delay).await;
            let mut header =
                format!("HTTP/1.1 {} Fixture\r\nConnection: close\r\n", reply.status).into_bytes();
            if !reply.omit_length {
                header.extend_from_slice(
                    format!(
                        "Content-Length: {}\r\n",
                        reply.length.unwrap_or(reply.body.len())
                    )
                    .as_bytes(),
                );
            }
            for location in reply.locations {
                header.extend_from_slice(b"Location: ");
                header.extend_from_slice(&location);
                header.extend_from_slice(b"\r\n");
            }
            if reply.cookie {
                header.extend_from_slice(b"Set-Cookie: LOCAL_COOKIE=LOCAL_ONLY\r\n");
            }
            header.extend_from_slice(b"\r\n");
            let _ = connection.write_all(&header).await;
            let _ = connection.write_all(&reply.body).await;
        }
        observed
    });
    Router {
        url: format!("http://{address}/start"),
        requests,
        join,
    }
}

#[tokio::test]
async fn validated_redirects_download_exact_package_without_credentials_or_url_metadata() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let io = UpdateIo::new(root.clone());
    let token = root.begin(2).unwrap();
    let (bundle, _, body) = fixture.package("3.8.0-rust", 2, 1, None);
    let expected = digest_test(&body);
    let service = router(|address| {
        let mut first = Reply::redirect("/start", 302, "/middle");
        first.cookie = true;
        vec![
            first,
            Reply::redirect(
                "/middle",
                307,
                &format!("http://{address}/asset?sig=LOCAL_SAS_ONLY"),
            ),
            Reply::final_body("/asset?sig=LOCAL_SAS_ONLY", body),
        ]
    })
    .await;
    let archive = io
        .download(
            2,
            root.validate(bundle.clone(), 150).unwrap(),
            token.clone(),
            &service.url,
            Duration::from_secs(30),
        )
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(archive.sha256, expected);
    assert!(!archive.path().to_string_lossy().contains("LOCAL_SAS_ONLY"));
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
    let requests = service.join.await.unwrap();
    assert_eq!(requests.len(), 3);
    for request in requests {
        let lower = request.to_ascii_lowercase();
        for header in [
            "authorization:",
            "proxy-authorization:",
            "cookie:",
            "referer:",
        ] {
            assert!(!lower.contains(header));
        }
    }
    drop(archive);
    let reports = io.drain().await.unwrap();
    assert_eq!(reports.len(), 2);
    assert!(reports
        .iter()
        .all(|report| report.operation == 2 && report.result.is_ok()));
}

#[tokio::test]
async fn redirect_bodies_are_not_downloaded_or_counted_toward_signed_bytes() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let io = UpdateIo::new(root.clone());
    let token = root.begin(2).unwrap();
    let (bundle, _, body) = fixture.package("3.8.0-rust", 2, 1, None);
    let expected = digest_test(&body);
    let length = body.len() as u64;
    let service = router(|_| {
        let mut redirect = Reply::redirect("/start", 301, "/final");
        redirect.length = Some(2 * 1024 * 1024);
        vec![redirect, Reply::final_body("/final", body)]
    })
    .await;
    let archive = io
        .download(
            2,
            root.validate(bundle, 150).unwrap(),
            token,
            &service.url,
            Duration::from_secs(30),
        )
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(archive.sha256, expected);
    assert_eq!(archive.length, length);
    assert_eq!(service.join.await.unwrap().len(), 2);
    drop(archive);
    assert!(io.drain().await.unwrap()[0].result.is_ok());
}

#[tokio::test]
async fn invalid_missing_duplicate_null_oversized_and_foreign_locations_never_succeed() {
    let cases = vec![
        Vec::new(),
        vec![Vec::new()],
        vec![b"/final".to_vec(), b"/other".to_vec()],
        vec![vec![0]],
        vec![vec![0xff]],
        vec![vec![b'x'; url_policy::MAX_LOCATION_BYTES + 1]],
        vec![b"http://192.168.1.1/private?sig=LOCAL_SAS_ONLY".to_vec()],
        vec![b"https://release-assets.githubusercontent.com/asset?sig=LOCAL_SAS_ONLY".to_vec()],
        vec![b"http://user:pass@127.0.0.1:1/asset".to_vec()],
        vec![b"/final#fragment".to_vec()],
    ];
    for locations in cases {
        let fixture = Fixture::new();
        let root = fixture.root();
        let io = UpdateIo::new(root.clone());
        let token = root.begin(2).unwrap();
        let (bundle, _, _) = fixture.package("3.8.0-rust", 2, 1, None);
        let service = router(|_| {
            let mut reply = Reply::redirect("/start", 302, "/unused");
            reply.locations = locations;
            vec![reply]
        })
        .await;
        let result = io
            .download(
                2,
                root.validate(bundle, 150).unwrap(),
                token,
                &service.url,
                Duration::from_secs(30),
            )
            .unwrap()
            .wait()
            .await;
        let error = result.err().unwrap();
        assert!(!error.contains("LOCAL_SAS_ONLY"));
        assert!(!error.contains("Location"));
        assert_eq!(service.join.await.unwrap().len(), 1);
        let reports = io.drain().await.unwrap();
        assert_eq!(reports.len(), 1);
        assert!(reports[0].result.is_err());
        assert!(!root.path().read_dir().unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".part")));
    }
}

#[tokio::test]
async fn redirect_loop_and_hop_limit_do_not_remint_operation_or_download_success() {
    for looped in [true, false] {
        let fixture = Fixture::new();
        let root = fixture.root();
        let io = UpdateIo::new(root.clone());
        let token = root.begin(2).unwrap();
        let (bundle, _, _) = fixture.package("3.8.0-rust", 2, 1, None);
        let service = router(|_| {
            if looped {
                vec![Reply::redirect("/start", 302, "/start")]
            } else {
                (0..=url_policy::MAX_REDIRECTS)
                    .map(|index| {
                        Reply::redirect(
                            &if index == 0 {
                                "/start".into()
                            } else {
                                format!("/hop{index}")
                            },
                            302,
                            &format!("/hop{}", index + 1),
                        )
                    })
                    .collect()
            }
        })
        .await;
        assert!(io
            .download(
                2,
                root.validate(bundle, 150).unwrap(),
                token,
                &service.url,
                Duration::from_secs(30)
            )
            .unwrap()
            .wait()
            .await
            .is_err());
        let requests = service.join.await.unwrap();
        assert_eq!(
            requests.len(),
            if looped {
                1
            } else {
                url_policy::MAX_REDIRECTS + 1
            }
        );
        let reports = io.drain().await.unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].operation, 2);
        assert!(reports[0].result.is_err());
    }
}

#[tokio::test]
async fn cancellation_and_stale_original_operation_between_hops_never_request_final() {
    for stale in [false, true] {
        let fixture = Fixture::new();
        let root = fixture.root();
        let io = UpdateIo::new(root.clone());
        let token = root.begin(2).unwrap();
        let (bundle, _, body) = fixture.package("3.8.0-rust", 2, 1, None);
        let mut service = router(|_| {
            let mut middle = Reply::redirect("/middle", 302, "/final");
            middle.delay = Duration::from_millis(200);
            vec![
                Reply::redirect("/start", 302, "/middle"),
                middle,
                Reply::final_body("/final", body),
            ]
        })
        .await;
        let job = io
            .download(
                2,
                root.validate(bundle, 150).unwrap(),
                token,
                &service.url,
                Duration::from_secs(30),
            )
            .unwrap();
        for _ in 0..2 {
            service.requests.recv().await.unwrap();
        }
        if stale {
            root.begin(3).unwrap();
        } else {
            job.cancel();
        }
        assert!(job.wait().await.is_err());
        assert_eq!(service.join.await.unwrap().len(), 2);
        let reports = io.drain().await.unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].operation, 2);
        assert!(reports[0].result.is_err());
    }
}

#[tokio::test]
async fn overall_deadline_is_not_reset_for_each_redirect_or_stage() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let io = UpdateIo::new(root.clone());
    let token = root.begin(2).unwrap();
    let (bundle, _, body) = fixture.package("3.8.0-rust", 2, 1, None);
    let service = router(|_| {
        let mut first = Reply::redirect("/start", 302, "/middle");
        first.delay = Duration::from_millis(100);
        let mut middle = Reply::redirect("/middle", 302, "/final");
        middle.delay = Duration::from_millis(100);
        let mut final_body = Reply::final_body("/final", body);
        final_body.delay = Duration::from_millis(100);
        vec![first, middle, final_body]
    })
    .await;
    assert!(io
        .download(
            2,
            root.validate(bundle, 150).unwrap(),
            token,
            &service.url,
            Duration::from_millis(175)
        )
        .unwrap()
        .wait()
        .await
        .is_err());
    assert_eq!(service.join.await.unwrap().len(), 2);
    let reports = io.drain().await.unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].operation, 2);
    assert!(reports[0].result.is_err());
    let fixture = Fixture::new();
    let root = fixture.root();
    let io = UpdateIo::new(root.clone());
    let token = root.begin(2).unwrap();
    let (bundle, _, body) = fixture.package("3.8.0-rust", 2, 1, None);
    let service = router(|_| {
        vec![
            Reply::redirect("/start", 303, "/final"),
            Reply::final_body("/final", body),
        ]
    })
    .await;
    let archive = io
        .download(
            2,
            root.validate(bundle.clone(), 150).unwrap(),
            token.clone(),
            &service.url,
            Duration::from_secs(2),
        )
        .unwrap()
        .wait()
        .await
        .unwrap();
    service.join.await.unwrap();
    tokio::time::sleep_until(tokio::time::Instant::from_std(
        archive.deadline + Duration::from_millis(10),
    ))
    .await;
    assert!(io
        .stage(
            StageOptions {
                operation: 2,
                cancellation: token,
                budget: Duration::from_secs(30),
                now: 150,
                crash: CrashPoint::None
            },
            root.validate(bundle, 150).unwrap(),
            archive.clone()
        )
        .unwrap()
        .wait()
        .await
        .is_err());
    let reports = io.drain().await.unwrap();
    assert_eq!(reports.len(), 2);
    assert_eq!(
        reports
            .iter()
            .filter(|report| report.result.is_err())
            .count(),
        1
    );
    assert_eq!(
        root.phase().unwrap(),
        crate::update_helper::DurablePhase::Idle
    );
    drop(archive);
}

#[tokio::test]
async fn final_response_after_redirect_still_requires_exact_digest_length_and_200() {
    for scenario in 0..6 {
        let fixture = Fixture::new();
        let root = fixture.root();
        let io = UpdateIo::new(root.clone());
        let token = root.begin(2).unwrap();
        let (bundle, _, mut body) = fixture.package("3.8.0-rust", 2, 1, None);
        let signed_length = body.len();
        if scenario == 0 {
            body[50] ^= 1;
        }
        let service = router(|_| {
            let mut final_body = Reply::final_body("/asset?sig=LOCAL_SAS_ONLY", body);
            if scenario == 1 {
                final_body.length = Some(signed_length + 1);
            }
            if scenario == 2 {
                final_body.status = 403;
            }
            if scenario == 3 {
                final_body.body.truncate(signed_length - 1);
                final_body.omit_length = true;
            }
            if scenario == 4 {
                final_body.body.push(0);
                final_body.omit_length = true;
            }
            if scenario == 5 {
                final_body.length = Some(signed_length);
                final_body.body.truncate(signed_length - 1);
            }
            vec![
                Reply::redirect("/start", 308, "/asset?sig=LOCAL_SAS_ONLY"),
                final_body,
            ]
        })
        .await;
        let error = io
            .download(
                2,
                root.validate(bundle, 150).unwrap(),
                token,
                &service.url,
                Duration::from_secs(30),
            )
            .unwrap()
            .wait()
            .await
            .err()
            .unwrap();
        assert!(!error.contains("LOCAL_SAS_ONLY"));
        assert!(!error.contains("sig="));
        assert_eq!(service.join.await.unwrap().len(), 2);
        assert!(io.drain().await.unwrap()[0].result.is_err());
        assert_eq!(
            root.phase().unwrap(),
            crate::update_helper::DurablePhase::Idle
        );
        assert!(!root.path().read_dir().unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".part")));
    }
}
