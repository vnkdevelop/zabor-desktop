use super::*;
use sha2::{Digest, Sha256};
use std::fs;

fn fixture(script: &str) -> (tempfile::TempDir, ExportRequest) {
    let temp = std::env::var_os("ZABOR_TEST_TEMP").expect("approved fixture directory required");
    let root = tempfile::Builder::new()
        .prefix("zabor-electron-copy-")
        .tempdir_in(temp)
        .unwrap();
    fs::write(
        root.path().join(".native-copy-snapshot"),
        b"zabor-electron-copy-v1",
    )
    .unwrap();
    let helper = root.path().join("fixture.cjs");
    fs::write(&helper, script).unwrap();
    let runtime =
        TrustedFile::offline_signed(Path::new("C:\\Program Files\\nodejs\\node.exe")).unwrap();
    let helper = TrustedFile::pinned(&helper, Sha256::digest(script.as_bytes()).into()).unwrap();
    let request = ExportRequest::synthetic_fixture(runtime, helper, root.path()).unwrap();
    (root, request)
}

#[test]
fn original_owner_and_private_pipe_are_joined_before_success() {
    let (root, request) = fixture("process.stdout.write(JSON.stringify({fixture:true}));");
    let registry = OwnerRegistry::new();
    let operation = registry.prepare().unwrap();
    let id = operation.id();
    operation.start(request, Limits::default()).unwrap();
    let result = operation
        .finish()
        .unwrap_or_else(|error| panic!("{error:?}: {:?}", registry.drain().unwrap()));
    assert_eq!(result.observation.operation, id);
    assert!(result.observation.owner_joined && result.observation.reader_joined);
    assert!(
        result.observation.assigned
            && result.observation.root_reaped
            && result.observation.job_empty
            && result.observation.pipe_eof
            && result.observation.handles_closed
    );
    assert!(&*result.bytes == br#"{"fixture":true}"#);
    registry.drain().unwrap();
    drop(result);
    root.close().unwrap();
}

#[test]
fn cancelled_before_start_never_acquires_a_process() {
    let (root, request) = fixture("process.stdout.write('{}');");
    let registry = OwnerRegistry::new();
    let operation = registry.prepare().unwrap();
    operation.cancel();
    assert_eq!(
        operation.start(request, Limits::default()),
        Err(Failure::Cancelled)
    );
    assert!(operation.observation().unwrap().pid.is_none());
    drop(operation);
    registry.drain().unwrap();
    root.close().unwrap();
}

#[test]
fn assignment_failure_reaps_the_suspended_root_and_closes_every_owned_handle() {
    let (root, mut request) = fixture("setInterval(()=>{},100);");
    request.fail_assignment = true;
    let registry = OwnerRegistry::new();
    let operation = registry.prepare().unwrap();
    let cancellation = operation.cancellation();
    operation.start(request, Limits::default()).unwrap();
    assert!(matches!(operation.finish(), Err(Failure::Process)));
    let observation = cancellation.observation().unwrap();
    assert!(observation.pid.is_some() && observation.job_created && observation.owner_joined);
    assert!(!observation.assigned && !observation.reader_started);
    assert!(observation.root_reaped && observation.job_empty && observation.handles_closed);
    assert!(observation.exit_code.is_some());
    registry.drain().unwrap();
    root.close().unwrap();
}

#[test]
fn chunked_output_at_the_exact_bound_never_reallocates_the_private_buffer() {
    let (root, request) =
        fixture("process.stdout.write(JSON.stringify({fixture:'x'.repeat(128*1024)}));");
    let registry = OwnerRegistry::new();
    let operation = registry.prepare().unwrap();
    let max_stdout = 128 * 1024 + 14;
    operation
        .start(
            request,
            Limits {
                max_stdout,
                timeout: Duration::from_secs(10),
            },
        )
        .unwrap();
    let result = operation.finish().unwrap();
    assert_eq!(result.bytes.len(), max_stdout);
    assert_eq!(result.bytes.capacity(), max_stdout);
    assert!(
        result.observation.owner_joined
            && result.observation.reader_joined
            && result.observation.pipe_eof
            && result.observation.handles_closed
    );
    drop(result);
    registry.drain().unwrap();
    root.close().unwrap();
}

#[test]
fn incomplete_oversize_and_failed_root_outputs_are_not_accepted() {
    for (script, max, expected) in [
        ("process.stdout.write('{');", 4096, Failure::Incomplete),
        (
            "process.stdout.write('x'.repeat(65536));setInterval(()=>{},100);",
            1024,
            Failure::Oversize,
        ),
        (
            "process.stdout.write('{}');process.exitCode=7;",
            4096,
            Failure::Process,
        ),
    ] {
        let (root, request) = fixture(script);
        let registry = OwnerRegistry::new();
        let operation = registry.prepare().unwrap();
        operation
            .start(
                request,
                Limits {
                    max_stdout: max,
                    timeout: Duration::from_secs(10),
                },
            )
            .unwrap();
        assert!(matches!(operation.finish(), Err(error) if error == expected));
        let observations = registry.drain().unwrap();
        assert!(
            observations[0].owner_joined
                && observations[0].reader_joined
                && observations[0].root_reaped
                && observations[0].job_empty
        );
        root.close().unwrap();
    }
}

#[test]
fn timeout_kills_and_reaps_only_the_registered_job() {
    let (root, request) = fixture("setInterval(()=>{},100);");
    let registry = OwnerRegistry::new();
    let operation = registry.prepare().unwrap();
    operation
        .start(
            request,
            Limits {
                max_stdout: 1024,
                timeout: Duration::from_millis(150),
            },
        )
        .unwrap();
    assert!(matches!(operation.finish(), Err(Failure::Timeout)));
    let observation = &registry.drain().unwrap()[0];
    assert!(
        observation.owner_joined
            && observation.root_reaped
            && observation.job_empty
            && observation.reader_joined
    );
    root.close().unwrap();
}

#[test]
fn dropping_the_waiter_does_not_detach_original_owner_or_grandchildren() {
    let script = "const {spawn}=require('node:child_process');spawn(process.execPath,['-e',\"require('node:child_process').spawn(process.execPath,['-e','setInterval(()=>{},100)'],{stdio:'inherit'});setInterval(()=>{},100);\"],{stdio:'inherit'});setInterval(()=>{},100);";
    let (root, request) = fixture(script);
    let registry = OwnerRegistry::new();
    let operation = registry.prepare().unwrap();
    let id = operation.id();
    operation.start(request, Limits::default()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while operation.observation().unwrap().active_process_peak < 3
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(operation.observation().unwrap().active_process_peak >= 3);
    drop(operation);
    let observation = &registry.drain().unwrap()[0];
    assert_eq!(observation.operation, id);
    assert!(
        observation.owner_joined
            && observation.root_reaped
            && observation.job_empty
            && observation.reader_joined
    );
    assert!(observation.active_process_peak >= 3);
    root.close().unwrap();
}

#[test]
fn native_owner_unwind_keeps_reader_and_process_retirement_observed() {
    let (root, mut request) = fixture("setInterval(()=>{},100);");
    request.panic_after_resume = true;
    let registry = OwnerRegistry::new();
    let operation = registry.prepare().unwrap();
    let cancellation = operation.cancellation();
    operation.start(request, Limits::default()).unwrap();
    assert!(matches!(operation.finish(), Err(Failure::Owner)));
    let observation = cancellation.observation().unwrap();
    assert!(
        observation.owner_joined
            && observation.reader_joined
            && observation.root_reaped
            && observation.job_empty
            && observation.handles_closed
    );
    registry.drain().unwrap();
    root.close().unwrap();
}

#[test]
fn late_cancellation_discards_completed_output_under_the_original_operation() {
    let (root, request) = fixture("process.stdout.write('{}');");
    let registry = OwnerRegistry::new();
    let operation = registry.prepare().unwrap();
    let cancellation = operation.cancellation();
    operation.start(request, Limits::default()).unwrap();
    loop {
        if operation
            .entry
            .join
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .is_finished()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    cancellation.cancel();
    assert!(matches!(operation.finish(), Err(Failure::Cancelled)));
    let observation = cancellation.observation().unwrap();
    assert_eq!(observation.operation, cancellation.id());
    assert!(
        observation.owner_joined
            && observation.reader_joined
            && observation.root_reaped
            && observation.job_empty
    );
    root.close().unwrap();
}

#[test]
fn poisoned_node_environment_does_not_reach_the_helper() {
    let script = "const blocked=['NODE_OPTIONS','NODE_PATH','ELECTRON_RUN_AS_NODE'];process.stdout.write(JSON.stringify({clean:blocked.every(name=>process.env[name]===undefined),isolated:process.env.APPDATA===process.argv[2]}));";
    let (root, request) = fixture(script);
    let registry = OwnerRegistry::new();
    let operation = registry.prepare().unwrap();
    operation.start(request, Limits::default()).unwrap();
    let result = operation.finish().unwrap();
    assert!(&*result.bytes == br#"{"clean":true,"isolated":true}"#);
    root.close().unwrap();
}

#[test]
fn seal_prevents_new_operations_and_releases_registered_owners() {
    let registry = OwnerRegistry::new();
    let operation = registry.prepare().unwrap();
    registry.seal();
    assert!(matches!(registry.prepare(), Err(Failure::Sealed)));
    assert!(operation.observation().unwrap().pid.is_none());
    drop(operation);
    registry.drain().unwrap();
}

#[test]
fn capacity_bounds_outstanding_owners_without_exhausting_cancelled_history() {
    let registry = OwnerRegistry::new();
    let mut operations = Vec::new();
    for _ in 0..128 {
        operations.push(registry.prepare().unwrap());
    }
    assert!(matches!(registry.prepare(), Err(Failure::Owner)));
    let retained = operations[0].cancellation();
    let original = retained.id();
    drop(operations);
    let next = registry.prepare().unwrap();
    assert!(next.id() > original);
    assert_eq!(retained.observation().unwrap().operation, original);
    assert!(retained.observation().unwrap().pid.is_none());
    drop(next);
    registry.drain().unwrap();
}

#[test]
fn capacity_joins_the_finished_original_cancelled_owner_before_reclamation() {
    let registry = OwnerRegistry::new();
    let live: Vec<_> = (0..127).map(|_| registry.prepare().unwrap()).collect();
    let cancelled = registry.prepare().unwrap();
    let token = cancelled.cancellation();
    let id = cancelled.id();
    cancelled.entry.started.store(true, Ordering::Release);
    *cancelled.entry.join.lock().unwrap() = Some(std::thread::spawn(move || ActorResult {
        bytes: Ok(Zeroizing::new(b"synthetic-private-result".to_vec())),
        observation: Observation {
            operation: id,
            owner_started: true,
            handles_closed: true,
            ..Observation::default()
        },
    }));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !cancelled
        .entry
        .join
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .is_finished()
    {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    drop(cancelled);
    assert!(!token.observation().unwrap().owner_joined);
    let next = registry.prepare().unwrap();
    assert!(next.id() > id);
    let observation = token.observation().unwrap();
    assert!(observation.owner_joined && observation.handles_closed);
    assert_eq!(observation.failure, Some(Failure::Cancelled));
    drop(live);
    drop(next);
    registry.drain().unwrap();
}
