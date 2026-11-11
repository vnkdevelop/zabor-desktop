use super::{AppDrainEvidence, HostJob, UpdateHost};
use crate::release::NATIVE_BETA_PROFILE;
use crate::update_helper::{digest, CrashPoint, HealthChallenge, Result, StageContext, UpdateRoot};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

const FRAME_SIZE: usize = 264;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: u64,
    pub created: u64,
}

#[derive(Clone, Debug)]
pub struct ProcessReceipt {
    pub operation: u64,
    pub owner: ProcessIdentity,
    pub helper: ProcessIdentity,
    pub target: ProcessIdentity,
    pub children_observed: bool,
}

pub struct ProcessJob {
    ready: oneshot::Receiver<Result<ProcessReceipt>>,
    owner: HostJob<ProcessReceipt>,
}

impl ProcessJob {
    pub async fn ready(&mut self) -> Result<ProcessReceipt> {
        tokio::select! {
            biased;
            _ = self.owner.cancellation.cancelled() => Err("host-ready-waiter-cancelled".into()),
            result = &mut self.ready => result.map_err(|_| "host-health-result-lost")?,
        }
    }

    pub fn cancel(&self) {
        self.owner.cancel();
    }

    pub async fn wait(self) -> Result<ProcessReceipt> {
        self.owner.wait().await
    }
}

fn now_ms() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "host-clock-invalid")?
        .as_millis()
        .try_into()
        .map_err(|_| "host-clock-overflow".into())
}

fn put(frame: &mut [u8], offset: usize, value: u64) {
    frame[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn number(frame: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(frame[offset..offset + 8].try_into().unwrap())
}

fn hash_bytes(hash: &str) -> Result<Vec<u8>> {
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("host-digest-invalid".into());
    }
    (0..32)
        .map(|index| {
            u8::from_str_radix(&hash[index * 2..index * 2 + 2], 16)
                .map_err(|_| "host-digest-invalid".into())
        })
        .collect()
}

fn identity(frame: &[u8], offset: usize) -> ProcessIdentity {
    ProcessIdentity {
        pid: number(frame, offset),
        created: number(frame, offset + 8),
    }
}

fn receipt(frame: &[u8], children_observed: bool) -> ProcessReceipt {
    ProcessReceipt {
        operation: number(frame, 8),
        owner: identity(frame, 16),
        helper: identity(frame, 32),
        target: identity(frame, 48),
        children_observed,
    }
}

struct OriginalProcess {
    child: Child,
    reader: Option<JoinHandle<()>>,
    stopped: bool,
}

impl OriginalProcess {
    fn stop(&mut self) {
        if !self.stopped {
            if let Some(input) = self.child.stdin.as_mut() {
                let _ = input.write_all(b"X");
                let _ = input.flush();
            }
            self.stopped = true;
        }
    }

    fn join(&mut self) -> Result<bool> {
        let success = self
            .child
            .wait()
            .map_err(|_| "original-helper-exit-not-observed")?
            .success();
        if let Some(reader) = self.reader.take() {
            reader
                .join()
                .map_err(|_| "original-helper-pipe-reader-panicked")?;
        }
        Ok(success)
    }
}

impl Drop for OriginalProcess {
    fn drop(&mut self) {
        self.stop();
        let _ = self.join();
    }
}

struct ProcessRequest {
    host_nonce: Zeroizing<Vec<u8>>,
    root: Arc<UpdateRoot>,
    operation: u64,
    original: CancellationToken,
    cancel: CancellationToken,
    deadline: Instant,
    drain: AppDrainEvidence,
    ready: oneshot::Sender<Result<ProcessReceipt>>,
}

struct HostGate<'a> {
    original: &'a CancellationToken,
    cancel: &'a CancellationToken,
    deadline: Instant,
    expires_ms: u64,
    clock: &'a dyn Fn() -> Result<u64>,
}

impl HostGate<'_> {
    fn check(&self) -> Result<u64> {
        if self.cancel.is_cancelled() || self.original.is_cancelled() {
            return Err("host-owner-cancelled".into());
        }
        let now = (self.clock)()?;
        if Instant::now() >= self.deadline || now >= self.expires_ms {
            return Err("host-health-deadline".into());
        }
        Ok(now / 1000)
    }

    fn context(&self, operation: u64) -> Result<StageContext<'_>> {
        Ok(StageContext {
            operation,
            cancellation: self.original,
            deadline: self.deadline,
            now: self.check()?,
            crash: CrashPoint::None,
        })
    }
}

fn commit_health(
    root: &UpdateRoot,
    challenge: &HealthChallenge,
    gate: &HostGate<'_>,
) -> Result<()> {
    root.healthy_guarded(challenge, gate.context(challenge.operation)?, || {
        gate.check()
    })
}

fn run(request: ProcessRequest) -> Result<ProcessReceipt> {
    let ProcessRequest {
        host_nonce,
        root,
        operation,
        original,
        cancel,
        deadline,
        drain,
        ready,
    } = request;
    root.check(operation, &original, deadline)?;
    if cancel.is_cancelled() {
        return Err("host-owner-cancelled".into());
    }
    let root_identity = root.host_identity()?;
    let remaining: u64 = deadline
        .saturating_duration_since(Instant::now())
        .as_millis()
        .try_into()
        .map_err(|_| "host-budget-overflow")?;
    let expires = now_ms()?
        .checked_add(remaining.min(120000))
        .ok_or("host-clock-overflow")?
        .min(
            root_identity
                .health_deadline
                .ok_or("host-health-deadline-missing")?
                .checked_mul(1000)
                .ok_or("host-clock-overflow")?,
        );
    let health_deadline =
        deadline.min(Instant::now() + Duration::from_millis(expires.saturating_sub(now_ms()?)));
    let gate = HostGate {
        original: &original,
        cancel: &cancel,
        deadline: health_deadline,
        expires_ms: expires,
        clock: &now_ms,
    };
    let challenge = root.activate_guarded(gate.context(operation)?, &drain, || gate.check())?;
    let held_helper = root
        .host_platform()
        .host_executable(root_identity.sandbox)?;
    let held_image = root.pending_host_executable(gate.context(operation)?)?;
    root.check(operation, &original, health_deadline)?;
    gate.check()?;
    let mut frame = Zeroizing::new(vec![0; FRAME_SIZE]);
    frame[..8].copy_from_slice(b"ZABHOST1");
    put(&mut frame, 8, operation);
    put(&mut frame, 16, u64::from(std::process::id()));
    put(&mut frame, 64, expires);
    if challenge.nonce.len() != 32 || host_nonce.len() != 32 {
        return Err("host-nonce-invalid".into());
    }
    frame[72..104].copy_from_slice(&challenge.nonce);
    frame[104..136].copy_from_slice(&hash_bytes(&root_identity.root_sha256)?);
    frame[136..168].copy_from_slice(&hash_bytes(&challenge.manifest_sha256)?);
    frame[168..200].copy_from_slice(&hash_bytes(&digest(NATIVE_BETA_PROFILE.as_bytes()))?);
    frame[200..232].copy_from_slice(&host_nonce);
    gate.check()?;
    let child = Command::new(&held_helper.path)
        .arg("host")
        .arg(&held_image.path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "original-native-host-spawn-failed")?;
    let helper_pid = u64::from(child.id());
    let mut owner = OriginalProcess {
        child,
        reader: None,
        stopped: false,
    };
    let mut output = owner.child.stdout.take().ok_or("host-event-pipe-missing")?;
    let (send, events) = mpsc::sync_channel(4);
    owner.reader = Some(std::thread::spawn(move || loop {
        let mut event = [0];
        if output.read_exact(&mut event).is_err() {
            break;
        }
        let mut bytes = Zeroizing::new(vec![0; FRAME_SIZE]);
        if output.read_exact(&mut bytes).is_err() {
            break;
        }
        if send.try_send((event[0], bytes)).is_err() {
            break;
        }
    }));
    owner
        .child
        .stdin
        .as_mut()
        .ok_or("host-control-pipe-missing")?
        .write_all(&frame)
        .map_err(|_| "host-private-startup-write-failed")?;
    let mut started = None;
    let mut healthy = false;
    let mut committed = false;
    let mut children_observed = false;
    let mut ready = Some(ready);
    let mut failure = None;
    loop {
        if cancel.is_cancelled()
            || original.is_cancelled()
            || (!committed && (Instant::now() >= health_deadline || now_ms()? >= expires))
        {
            failure.get_or_insert_with(|| "host-owner-cancelled-or-expired".to_owned());
            owner.stop();
        }
        match events.recv_timeout(Duration::from_millis(10)) {
            Ok((event, bytes)) => {
                let accepted = if event == b'S' && started.is_none() {
                    bytes[..24] == frame[..24]
                        && bytes[64..232] == frame[64..232]
                        && number(&bytes, 24) != 0
                        && number(&bytes, 32) == helper_pid
                        && number(&bytes, 40) != 0
                        && number(&bytes, 48) != 0
                        && number(&bytes, 56) != 0
                        && bytes[232..].iter().any(|byte| *byte != 0)
                } else {
                    started
                        .as_ref()
                        .is_some_and(|original: &Zeroizing<Vec<u8>>| {
                            original.as_slice() == bytes.as_slice()
                        })
                };
                if !accepted {
                    failure.get_or_insert_with(|| "host-original-identity-frame-rejected".into());
                    owner.stop();
                    continue;
                }
                match event {
                    b'S' if started.is_none() => started = Some(bytes),
                    b'H' if !healthy && !committed && failure.is_none() => {
                        let result = commit_health(&root, &challenge, &gate);
                        match result {
                            Ok(()) => {
                                healthy = true;
                                owner
                                    .child
                                    .stdin
                                    .as_mut()
                                    .ok_or("host-control-pipe-missing")?
                                    .write_all(b"C")
                                    .map_err(|_| "host-commit-control-failed")?;
                            }
                            Err(error) => {
                                failure = Some(error);
                                owner.stop();
                            }
                        }
                    }
                    b'A' if healthy && !committed && failure.is_none() => {
                        committed = true;
                        if let Some(ready) = ready.take() {
                            let _ = ready.send(Ok(receipt(&bytes, false)));
                        }
                    }
                    b'E' if !children_observed => children_observed = true,
                    _ => {
                        failure.get_or_insert_with(|| "host-event-order-rejected".into());
                        owner.stop();
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let exited = owner.join()?;
    drop((held_helper, held_image));
    if !children_observed || !exited {
        return Err("original-helper-or-child-joins-not-observed".into());
    }
    if !committed {
        root.recover(CrashPoint::None)?;
        return Err(failure.unwrap_or_else(|| "authenticated-target-health-missing".into()));
    }
    Ok(receipt(
        started.as_ref().ok_or("host-original-start-missing")?,
        true,
    ))
}

impl UpdateHost {
    pub fn activate(
        &self,
        root: Arc<UpdateRoot>,
        original: CancellationToken,
        drain: AppDrainEvidence,
    ) -> Result<ProcessJob> {
        if !drain.belongs_to(self, &root) {
            return Err("host-original-app-drain-mismatch".into());
        }
        let (send, ready) = oneshot::channel();
        let nonce = Zeroizing::new(self.nonce().to_vec());
        let operation = self.operation();
        let owner = self.register(None, move |cancel, deadline| async move {
            tokio::task::spawn_blocking(move || {
                run(ProcessRequest {
                    host_nonce: nonce,
                    root,
                    operation,
                    original,
                    cancel,
                    deadline,
                    drain,
                    ready: send,
                })
            })
            .await
            .map_err(|_| "original-host-process-owner-panicked")?
        })?;
        Ok(ProcessJob { ready, owner })
    }
}

pub struct HealthBootstrap {
    frame: Zeroizing<Vec<u8>>,
    pipe: String,
}

impl HealthBootstrap {
    pub fn read(input: &mut impl Read) -> Result<Self> {
        let mut frame = Zeroizing::new(vec![0; FRAME_SIZE]);
        input
            .read_exact(&mut frame)
            .map_err(|_| "private-health-bootstrap-missing")?;
        let mut length = [0; 2];
        input
            .read_exact(&mut length)
            .map_err(|_| "health-pipe-length-missing")?;
        let length = usize::from(u16::from_le_bytes(length));
        if !(1..=128).contains(&length) {
            return Err("health-pipe-name-invalid".into());
        }
        let mut bytes = Zeroizing::new(vec![0; length]);
        input
            .read_exact(&mut bytes)
            .map_err(|_| "health-pipe-name-missing")?;
        let pipe = String::from_utf8(bytes.to_vec()).map_err(|_| "health-pipe-name-invalid")?;
        let prefix = r"\\.\pipe\zabor-native-health-";
        let suffix = pipe
            .strip_prefix(prefix)
            .ok_or("health-pipe-name-invalid")?;
        if suffix.len() != 64
            || !suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || frame[..8] != *b"ZABHOST1"
            || number(&frame, 8) == 0
            || number(&frame, 48) != u64::from(std::process::id())
            || [16, 24, 32, 40, 56]
                .iter()
                .any(|offset| number(&frame, *offset) == 0)
            || frame[168..200] != hash_bytes(&digest(NATIVE_BETA_PROFILE.as_bytes()))?
            || frame[72..104].iter().all(|byte| *byte == 0)
            || frame[200..232].iter().all(|byte| *byte == 0)
            || now_ms()? >= number(&frame, 64)
        {
            return Err("private-health-bootstrap-identity-rejected".into());
        }
        let mut trailing = [0; 1];
        if input
            .read(&mut trailing)
            .map_err(|_| "private-health-bootstrap-read-failed")?
            != 0
        {
            return Err("private-health-bootstrap-trailing-data".into());
        }
        Ok(Self { frame, pipe })
    }

    pub fn validate_execution(
        &self,
        execution: &crate::update_helper::HeldStartupPackage,
    ) -> Result<()> {
        if self.frame[104..136] != hash_bytes(execution.root_sha256())?
            || self.frame[136..168] != hash_bytes(execution.manifest_sha256())?
            || now_ms()? >= number(&self.frame, 64)
        {
            return Err("health-bootstrap-root-or-manifest-rejected".into());
        }
        execution.check_health(
            number(&self.frame, 8),
            &self.frame[72..104],
            number(&self.frame, 64),
        )
    }

    pub fn acknowledge_execution(
        self,
        execution: &crate::update_helper::HeldStartupPackage,
    ) -> Result<()> {
        self.validate_execution(execution)?;
        self.acknowledge(execution.root_sha256(), execution.manifest_sha256())
    }

    pub fn acknowledge(
        self,
        expected_root_sha256: &str,
        expected_manifest_sha256: &str,
    ) -> Result<()> {
        if self.frame[104..136] != hash_bytes(expected_root_sha256)?
            || self.frame[136..168] != hash_bytes(expected_manifest_sha256)?
            || now_ms()? >= number(&self.frame, 64)
        {
            return Err("health-bootstrap-root-or-manifest-rejected".into());
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(0x0011_0000);
        }
        let mut pipe: File = options
            .open(Path::new(&self.pipe))
            .map_err(|_| "private-health-pipe-open-failed")?;
        let mut challenge = Zeroizing::new(vec![0; FRAME_SIZE]);
        pipe.read_exact(&mut challenge)
            .map_err(|_| "private-health-challenge-missing")?;
        if challenge.as_slice() != self.frame.as_slice() || now_ms()? >= number(&self.frame, 64) {
            return Err("private-health-server-identity-rejected".into());
        }
        pipe.write_all(&challenge)
            .map_err(|_| "private-health-ack-write-failed".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update_helper::set_host_verification_hook;
    use crate::update_helper::tests::{Fixture, SandboxDrain};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn envelope() -> Vec<u8> {
        let mut frame = vec![1; FRAME_SIZE];
        frame[..8].copy_from_slice(b"ZABHOST1");
        put(&mut frame, 8, 2);
        put(&mut frame, 48, u64::from(std::process::id()));
        put(&mut frame, 64, now_ms().unwrap() + 30000);
        frame[168..200]
            .copy_from_slice(&hash_bytes(&digest(NATIVE_BETA_PROFILE.as_bytes())).unwrap());
        let pipe = format!(r"\\.\pipe\zabor-native-health-{}", "a".repeat(64));
        frame.extend_from_slice(&(pipe.len() as u16).to_le_bytes());
        frame.extend_from_slice(pipe.as_bytes());
        frame
    }

    #[test]
    fn private_bootstrap_binds_profile_pid_nonce_deadline_and_expected_trust_before_connecting() {
        let valid = envelope();
        assert!(HealthBootstrap::read(&mut std::io::Cursor::new(valid.clone())).is_ok());
        for (offset, value) in [(48, 0), (64, 1), (8, 0)] {
            let mut invalid = valid.clone();
            put(&mut invalid, offset, value);
            assert!(HealthBootstrap::read(&mut std::io::Cursor::new(invalid)).is_err());
        }
        for range in [72..104, 168..200, 200..232] {
            let mut invalid = valid.clone();
            invalid[range].fill(0);
            assert!(HealthBootstrap::read(&mut std::io::Cursor::new(invalid)).is_err());
        }
        let bootstrap = HealthBootstrap::read(&mut std::io::Cursor::new(valid)).unwrap();
        assert_eq!(
            bootstrap
                .acknowledge(&"0".repeat(64), &"0".repeat(64))
                .unwrap_err(),
            "health-bootstrap-root-or-manifest-rejected"
        );
    }

    #[test]
    fn followup_execution_leases_reject_replacement_after_prior_verification() {
        let fixture = Fixture::new();
        let root = fixture.root();
        let original = fixture.prepared(CrashPoint::None);
        root.activate(
            2,
            &original,
            Instant::now() + Duration::from_secs(30),
            150,
            &SandboxDrain,
            CrashPoint::None,
        )
        .unwrap();
        let image = root.pending_executable(2, &original, 150).unwrap();
        let bytes = std::fs::read(&image).unwrap();
        let displaced = fixture.directory.path().join("verified-image.exe");
        std::fs::rename(&image, &displaced).unwrap();
        std::fs::write(&image, b"UNVERIFIED SAME-USER REPLACEMENT").unwrap();
        let context = || StageContext {
            operation: 2,
            cancellation: &original,
            deadline: Instant::now() + Duration::from_secs(30),
            now: 150,
            crash: CrashPoint::None,
        };
        assert!(root.pending_host_executable(context()).is_err());
        std::fs::write(&image, bytes).unwrap();
        let held_image = root.pending_host_executable(context()).unwrap();
        assert!(std::fs::write(&image, b"CHANGED").is_err());
        assert!(std::fs::rename(&image, fixture.directory.path().join("moved-image.exe")).is_err());
        assert!(std::fs::rename(
            image.parent().unwrap(),
            fixture.directory.path().join("moved-version")
        )
        .is_err());
        drop(held_image);
        let helper = fixture.directory.path().join("held-helper.exe");
        std::fs::copy(
            std::env::var_os("ZABOR_UPDATE_TEST_HELPER").unwrap(),
            &helper,
        )
        .unwrap();
        let bytes = std::fs::read(&helper).unwrap();
        let platform =
            crate::update_helper::NativePlatform::pinned(&helper, &digest(&bytes)).unwrap();
        std::fs::write(&helper, b"UNVERIFIED HELPER REPLACEMENT").unwrap();
        assert!(platform.host_executable(true).is_err());
        std::fs::write(&helper, bytes).unwrap();
        let held_helper = platform.host_executable(true).unwrap();
        assert!(std::fs::write(&helper, b"CHANGED").is_err());
        assert!(
            std::fs::rename(&helper, fixture.directory.path().join("moved-helper.exe")).is_err()
        );
        assert!(std::fs::rename(
            fixture.directory.path(),
            fixture.directory.path().with_extension("moved")
        )
        .is_err());
        drop(held_helper);
    }

    #[tokio::test]
    async fn followup_held_health_verification_rejects_both_owner_cancellations_and_expiry() {
        for mode in 0..4 {
            let fixture = Fixture::new();
            let root = fixture.root();
            let baseline = root.current_executable().unwrap();
            let original = fixture.prepared(CrashPoint::None);
            let challenge = root
                .activate(
                    2,
                    &original,
                    Instant::now() + Duration::from_secs(30),
                    150,
                    &SandboxDrain,
                    CrashPoint::None,
                )
                .unwrap();
            let host = UpdateHost::new(2, Duration::from_secs(600)).unwrap();
            let clock = Arc::new(AtomicU64::new(150000));
            let held_clock = clock.clone();
            let held_root = root.clone();
            let held_original = original.clone();
            let (entered, start) = mpsc::sync_channel(1);
            let (release, held) = mpsc::sync_channel(1);
            let job = host
                .blocking(move |cancel, deadline| {
                    set_host_verification_hook(move || {
                        entered.send(()).unwrap();
                        held.recv().unwrap();
                    });
                    let now = || Ok(held_clock.load(Ordering::Acquire));
                    let gate = HostGate {
                        original: &held_original,
                        cancel: &cancel,
                        deadline,
                        expires_ms: if mode == 3 { 300000 } else { 151000 },
                        clock: &now,
                    };
                    commit_health(&held_root, &challenge, &gate)
                })
                .unwrap();
            tokio::task::spawn_blocking(move || start.recv_timeout(Duration::from_secs(20)))
                .await
                .unwrap()
                .unwrap();
            match mode {
                0 => job.cancel(),
                1 => original.cancel(),
                2 => clock.store(151000, Ordering::Release),
                3 => clock.store(270000, Ordering::Release),
                _ => unreachable!(),
            }
            release.send(()).unwrap();
            assert!(job.wait().await.is_err());
            let reports = host.drain().await.unwrap();
            assert_eq!(reports.len(), 1);
            assert!(reports[0].result.is_err());
            assert_eq!(
                root.phase().unwrap(),
                crate::update_helper::DurablePhase::AwaitingHealth
            );
            root.recover(CrashPoint::None).unwrap();
            assert_eq!(root.current_executable().unwrap(), baseline);
            assert_eq!(
                root.phase().unwrap(),
                crate::update_helper::DurablePhase::RolledBack
            );
        }
    }

    #[tokio::test]
    async fn followup_held_activation_verification_rejects_host_cancellation() {
        let fixture = Fixture::new();
        let root = fixture.root();
        let baseline = root.current_executable().unwrap();
        let original = fixture.prepared(CrashPoint::None);
        let host = UpdateHost::new(2, Duration::from_secs(600)).unwrap();
        let held_root = root.clone();
        let (entered, start) = mpsc::sync_channel(1);
        let (release, held) = mpsc::sync_channel(1);
        let job = host
            .blocking(move |cancel, deadline| {
                set_host_verification_hook(move || {
                    entered.send(()).unwrap();
                    held.recv().unwrap();
                });
                let now = || Ok(150000);
                let gate = HostGate {
                    original: &original,
                    cancel: &cancel,
                    deadline,
                    expires_ms: 270000,
                    clock: &now,
                };
                held_root
                    .activate_guarded(gate.context(2)?, &SandboxDrain, || gate.check())
                    .map(|_| ())
            })
            .unwrap();
        tokio::task::spawn_blocking(move || start.recv_timeout(Duration::from_secs(20)))
            .await
            .unwrap()
            .unwrap();
        job.cancel();
        release.send(()).unwrap();
        assert!(job.wait().await.is_err());
        assert!(host.drain().await.unwrap()[0].result.is_err());
        assert_eq!(
            root.phase().unwrap(),
            crate::update_helper::DurablePhase::Prepared
        );
        root.recover(CrashPoint::None).unwrap();
        assert_eq!(root.current_executable().unwrap(), baseline);
    }

    #[tokio::test]
    async fn followup_queued_cancel_and_expired_suspended_acquisition_never_resume_target() {
        for expired in [false, true] {
            let parent = std::env::var_os("ZABOR_UPDATE_TEST_DIRECTORY").unwrap();
            let directory = tempfile::Builder::new()
                .prefix("native-host-acquisition-")
                .tempdir_in(parent)
                .unwrap();
            let marker = directory.path().join("resumed-marker");
            let checked_marker = marker.clone();
            let host = UpdateHost::new(2, Duration::from_secs(30)).unwrap();
            let job = host
                .blocking(move |_, _| {
                    let helper = std::env::var_os(if expired {
                        "ZABOR_HOST_TEST_ACQUISITION_HELPER"
                    } else {
                        "ZABOR_UPDATE_TEST_HELPER"
                    })
                    .unwrap();
                    let mut frame = vec![1; FRAME_SIZE];
                    frame[..8].copy_from_slice(b"ZABHOST1");
                    put(&mut frame, 8, 2);
                    put(&mut frame, 16, u64::from(std::process::id()));
                    frame[24..64].fill(0);
                    put(&mut frame, 64, now_ms()? + 5000);
                    let child = Command::new(helper)
                        .arg("host")
                        .arg(std::env::var_os("ZABOR_HOST_TEST_TARGET_4").unwrap())
                        .env("ZABOR_HOST_TEST_RESUME_MARKER", marker)
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn()
                        .map_err(|_| "synthetic-acquisition-host-spawn-failed")?;
                    let mut owner = OriginalProcess {
                        child,
                        reader: None,
                        stopped: false,
                    };
                    let input = owner.child.stdin.as_mut().unwrap();
                    if !expired {
                        frame.push(b'X');
                    }
                    input.write_all(&frame).unwrap();
                    let mut output = owner.child.stdout.take().unwrap();
                    let mut event = [0];
                    let mut started = vec![0; FRAME_SIZE];
                    output.read_exact(&mut event).unwrap();
                    output.read_exact(&mut started).unwrap();
                    assert_eq!(event, [b'S']);
                    assert_ne!(number(&started, 48), 0);
                    assert_ne!(number(&started, 56), 0);
                    if expired {
                        std::thread::sleep(Duration::from_millis(
                            number(&started, 64).saturating_sub(now_ms()?) + 1,
                        ));
                        owner.child.stdin.as_mut().unwrap().write_all(b"R").unwrap();
                    }
                    let mut exited = vec![0; FRAME_SIZE];
                    output.read_exact(&mut event).unwrap();
                    output.read_exact(&mut exited).unwrap();
                    assert_eq!(event, [b'E']);
                    assert_eq!(started, exited);
                    assert!(owner.join()?);
                    Ok(())
                })
                .unwrap();
            job.wait().await.unwrap();
            assert!(host.drain().await.unwrap()[0].result.is_ok());
            assert!(!checked_marker.exists());
        }
    }
}
