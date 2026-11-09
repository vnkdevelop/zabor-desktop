use crate::chat::{ChatEngine, OutboundPacket};
use crate::chat_transport::ChatTransport;
use crate::files::{self, FileBroker, FileError, FileInfo, FileTransfer, StagedFile};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

const MAX_FILE_JOBS: usize = 32;

pub enum FileEvent {
    Staged {
        peer: String,
        file: Result<StagedFile, FileError>,
    },
    Sent {
        peer: String,
        id: String,
        job: u64,
        success: bool,
    },
    Saved {
        peer: String,
        id: String,
        stored_name: String,
        path: Result<PathBuf, FileError>,
    },
    Revealed {
        peer: String,
        id: String,
        success: bool,
    },
}

pub struct FileJobs {
    root: PathBuf,
    stop: CancellationToken,
    authority: Arc<dyn Fn() -> bool + Send + Sync + 'static>,
    jobs: JoinSet<TrackedFileEvent>,
    operations: Vec<Arc<FileOperation>>,
    failed: bool,
    sending: BTreeMap<(String, String), (u64, CancellationToken)>,
    pending: BTreeMap<(String, String), Instant>,
    targets: BTreeMap<(String, String), PathBuf>,
    next_job: u64,
}

struct FileOperation {
    peer: String,
    id: Option<String>,
    saving: bool,
    stop: CancellationToken,
}

struct TrackedFileEvent {
    event: FileEvent,
    operation: Arc<FileOperation>,
}

impl FileJobs {
    pub fn new(root: &Path) -> Self {
        Self::with_stop(root, CancellationToken::new())
    }

    pub fn with_stop(root: &Path, stop: CancellationToken) -> Self {
        Self::with_stop_and_authority(root, stop, Arc::new(|| true))
    }

    pub fn with_stop_and_authority(
        root: &Path,
        stop: CancellationToken,
        authority: Arc<dyn Fn() -> bool + Send + Sync + 'static>,
    ) -> Self {
        Self {
            root: root.into(),
            stop,
            authority,
            jobs: JoinSet::new(),
            operations: Vec::new(),
            failed: false,
            sending: BTreeMap::new(),
            pending: BTreeMap::new(),
            targets: BTreeMap::new(),
            next_job: 1,
        }
    }

    pub fn stage(&mut self, peer: String, source: PathBuf) -> Result<(), FileError> {
        let operation = self.operation(&peer, None, false)?;
        let root = self.root.clone();
        self.jobs.spawn_blocking(move || TrackedFileEvent {
            event: FileEvent::Staged {
                peer,
                file: FileBroker::new(&root)
                    .and_then(|broker| broker.stage_cancellable(&source, &operation.stop)),
            },
            operation,
        });
        Ok(())
    }

    fn operation(
        &mut self,
        peer: &str,
        id: Option<&str>,
        saving: bool,
    ) -> Result<Arc<FileOperation>, FileError> {
        if self.stop.is_cancelled() || self.jobs.len() >= MAX_FILE_JOBS {
            return Err(FileError);
        }
        self.operations
            .retain(|operation| Arc::strong_count(operation) > 1);
        let operation = Arc::new(FileOperation {
            peer: peer.into(),
            id: id.map(String::from),
            saving,
            stop: self.stop.child_token(),
        });
        self.operations.push(operation.clone());
        Ok(operation)
    }

    pub fn can_stage(&self, count: usize) -> bool {
        !self.stop.is_cancelled()
            && count > 0
            && count <= 20
            && self.jobs.len().saturating_add(count) <= MAX_FILE_JOBS
    }

    pub fn reveal(&mut self, peer: String, id: String, path: PathBuf) -> Result<(), FileError> {
        let operation = self.operation(&peer, Some(&id), false)?;
        self.jobs.spawn_blocking(move || TrackedFileEvent {
            event: FileEvent::Revealed {
                peer,
                id,
                success: !operation.stop.is_cancelled() && files::reveal_saved(&path).is_ok(),
            },
            operation,
        });
        Ok(())
    }

    pub fn save(
        &mut self,
        peer: String,
        id: String,
        source: std::fs::File,
        info: FileInfo,
        target: PathBuf,
    ) -> Result<(), FileError> {
        let operation = self.operation(&peer, Some(&id), true)?;
        for previous in &self.operations {
            if previous.saving
                && previous.peer == peer
                && previous.id.as_deref() == Some(&id)
                && !Arc::ptr_eq(previous, &operation)
            {
                previous.stop.cancel();
            }
        }
        let authority = self.authority.clone();
        self.jobs.spawn_blocking(move || TrackedFileEvent {
            event: FileEvent::Saved {
                peer,
                id,
                stored_name: info.stored_name,
                path: files::save_verified_if(
                    source,
                    &target,
                    info.size,
                    &info.sha256,
                    &operation.stop,
                    || authority(),
                ),
            },
            operation,
        });
        Ok(())
    }

    pub fn set_target(&mut self, peer: &str, id: &str, target: PathBuf) {
        if !self.stop.is_cancelled() {
            self.targets.insert((peer.into(), id.into()), target);
        }
    }

    pub fn finish_incoming(
        &mut self,
        engine: &ChatEngine,
        peer: &str,
        id: &str,
        now: i64,
    ) -> Result<(), FileError> {
        if self.stop.is_cancelled() {
            return Err(FileError);
        }
        if let Some(target) = self.targets.remove(&(peer.into(), id.into())) {
            let (source, info) = engine.cached_file(peer, id, now).map_err(|_| FileError)?;
            self.save(peer.into(), id.into(), source, info, target)?;
        }
        Ok(())
    }

    pub fn cancel(&mut self, peer: &str, id: &str) {
        for operation in &self.operations {
            if operation.peer == peer && operation.id.as_deref() == Some(id) {
                operation.stop.cancel();
            }
        }
        let key = (peer.into(), id.into());
        self.pending.remove(&key);
        self.targets.remove(&key);
        if let Some((_, stop)) = self.sending.remove(&key) {
            stop.cancel();
        }
    }

    pub fn disconnect(&mut self, peer: &str) {
        for operation in &self.operations {
            if operation.peer == peer {
                operation.stop.cancel();
            }
        }
        self.pending.retain(|(id, _), _| peer != id);
        self.targets.retain(|(id, _), _| peer != id);
        self.sending.retain(|(id, _), (_, stop)| {
            if peer == id {
                stop.cancel();
                false
            } else {
                true
            }
        });
    }

    pub fn pump(
        &mut self,
        engine: &mut ChatEngine,
        transport: Option<&ChatTransport>,
        now: i64,
    ) -> Vec<OutboundPacket> {
        let mut packets = Vec::new();
        if self.stop.is_cancelled() {
            return packets;
        }
        self.operations
            .retain(|operation| Arc::strong_count(operation) > 1);
        for (peer, id) in engine.take_file_cancels() {
            self.cancel(&peer, &id);
        }
        for operation in &self.operations {
            if !engine.is_friend(&operation.peer) {
                operation.stop.cancel();
            }
        }
        self.sending.retain(|(peer, _), (_, stop)| {
            if engine.is_friend(peer) {
                true
            } else {
                stop.cancel();
                false
            }
        });
        self.targets
            .retain(|(peer, id), _| engine.is_friend(peer) && engine.is_downloading(peer, id));
        for (peer, id) in engine.take_file_requests() {
            let key = (peer, id);
            if self.sending.contains_key(&key) || self.pending.contains_key(&key) {
                continue;
            }
            if self.pending.len() >= 8 {
                if let Ok(packet) = engine.unavailable(&key.0, &key.1) {
                    packets.push(packet);
                }
            } else {
                self.pending.insert(key, Instant::now());
            }
        }
        let pending: Vec<_> = self.pending.keys().cloned().collect();
        for (peer, id) in pending {
            let key = (peer.clone(), id.clone());
            if !engine.is_friend(&peer) {
                self.cancel(&peer, &id);
                continue;
            }
            let channel = transport.and_then(|transport| transport.file_channel(&peer));
            if channel.is_none() || self.sending.len() >= 8 || self.jobs.len() >= MAX_FILE_JOBS {
                if self
                    .pending
                    .get(&key)
                    .is_some_and(|started| started.elapsed() >= Duration::from_secs(30))
                {
                    self.pending.remove(&key);
                    if let Ok(packet) = engine.unavailable(&peer, &id) {
                        packets.push(packet);
                    }
                }
                continue;
            }
            self.pending.remove(&key);
            let Ok((input, size, sha256)) = engine.outgoing_file(&peer, &id, now) else {
                if let Ok(packet) = engine.unavailable(&peer, &id) {
                    packets.push(packet);
                }
                continue;
            };
            let Some(next) = self.next_job.checked_add(1) else {
                if let Ok(packet) = engine.unavailable(&peer, &id) {
                    packets.push(packet);
                }
                continue;
            };
            let job = self.next_job;
            self.next_job = next;
            let stop = self.stop.child_token();
            self.sending.insert(key, (job, stop.clone()));
            let operation = Arc::new(FileOperation {
                peer: peer.clone(),
                id: Some(id.clone()),
                saving: false,
                stop: stop.clone(),
            });
            self.operations.push(operation.clone());
            let file = FileTransfer {
                owner: engine.owner().into(),
                peer: peer.clone(),
                message: id.clone(),
                size,
                sha256,
            };
            let channel = channel.unwrap();
            self.jobs.spawn(async move {
                let success =
                    files::send_file(tokio::fs::File::from_std(input), channel, file, stop)
                        .await
                        .is_ok();
                TrackedFileEvent {
                    event: FileEvent::Sent {
                        peer,
                        id,
                        job,
                        success,
                    },
                    operation,
                }
            });
        }
        packets
    }

    pub fn has_jobs(&self) -> bool {
        !self.jobs.is_empty()
    }
    pub fn needs_tick(&self) -> bool {
        !self.pending.is_empty() || !self.targets.is_empty()
    }

    pub async fn next_event(&mut self) -> Option<FileEvent> {
        loop {
            let event = tokio::select! {
                biased;
                _ = self.stop.cancelled() => return None,
                event = self.jobs.join_next() => event?,
            };
            let Ok(tracked) = event else {
                self.failed = true;
                continue;
            };
            if self.stop.is_cancelled() {
                return None;
            }
            let event = tracked.event;
            if let FileEvent::Sent { peer, id, job, .. } = &event {
                let key = (peer.clone(), id.clone());
                if self
                    .sending
                    .get(&key)
                    .is_none_or(|(current, _)| current != job)
                {
                    continue;
                }
                self.sending.remove(&key);
            }
            if tracked.operation.stop.is_cancelled() {
                continue;
            }
            return Some(event);
        }
    }

    pub async fn finish(&mut self) -> Result<(), FileError> {
        self.stop.cancel();
        for operation in &self.operations {
            operation.stop.cancel();
        }
        while let Some(result) = self.jobs.join_next().await {
            if result.is_err() {
                self.failed = true;
            }
        }
        self.operations.clear();
        self.sending.clear();
        self.pending.clear();
        self.targets.clear();
        if self.failed {
            Err(FileError)
        } else {
            Ok(())
        }
    }
}

impl Drop for FileJobs {
    fn drop(&mut self) {
        self.stop.cancel();
        self.jobs.abort_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::critical_intents::{CriticalAction, CriticalIntents};
    use tokio::sync::Notify;

    #[tokio::test]
    async fn logout_wakes_stalled_file_event_wait_and_drops_uncommitted_staging() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("original.bin");
            let cache = root.path().join("cache");
            std::fs::write(&source, b"original data").unwrap();
            let staged = FileBroker::new(&cache)
                .unwrap()
                .stage_cancellable(&source, &CancellationToken::new())
                .unwrap();
            let intents = CriticalIntents::default();
            let mut jobs = FileJobs::with_stop(&cache, intents.session_token(intents.ticket()));
            let started = std::sync::Arc::new(Notify::new());
            let started_job = started.clone();
            let operation = jobs.operation("peer", None, false).unwrap();
            jobs.jobs.spawn(async move {
                let file = staged;
                started_job.notify_one();
                std::future::pending::<()>().await;
                TrackedFileEvent {
                    event: FileEvent::Staged {
                        peer: "peer".into(),
                        file: Ok(file),
                    },
                    operation,
                }
            });
            started.notified().await;
            intents.request(CriticalAction::Leave);
            assert!(!jobs.stop.is_cancelled());
            intents.request(CriticalAction::Logout);
            assert!(jobs.stop.is_cancelled());
            assert!(jobs.next_event().await.is_none());
            jobs.jobs.shutdown().await;
            assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
            assert_eq!(std::fs::read(&source).unwrap(), b"original data");
            assert!(!jobs.can_stage(1));
            assert!(jobs.stage("peer".into(), source).is_err());
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn disconnected_staging_drops_uncommitted_cache_without_returning_an_old_peer_event() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.bin");
        let cache = root.path().join("cache");
        std::fs::write(&source, b"preserved original").unwrap();
        let mut jobs = FileJobs::new(&cache);
        jobs.stage("peer".into(), source.clone()).unwrap();
        jobs.disconnect("peer");
        assert!(jobs.next_event().await.is_none());
        jobs.finish().await.unwrap();
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
        assert_eq!(std::fs::read(source).unwrap(), b"preserved original");
    }

    #[tokio::test]
    async fn cancelled_event_wait_and_drain_retain_original_blocking_join_and_staged_cleanup() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("source.bin");
            let cache = root.path().join("cache");
            std::fs::write(&source, b"preserved original").unwrap();
            let staged = FileBroker::new(&cache)
                .unwrap()
                .stage_cancellable(&source, &CancellationToken::new())
                .unwrap();
            let mut jobs = FileJobs::new(&cache);
            let operation = jobs.operation("peer", Some("message"), false).unwrap();
            let (started, entered) = tokio::sync::oneshot::channel();
            let (release, blocked) = std::sync::mpsc::channel();
            jobs.jobs.spawn_blocking(move || {
                started.send(()).unwrap();
                blocked.recv().unwrap();
                TrackedFileEvent {
                    event: FileEvent::Staged {
                        peer: "peer".into(),
                        file: Ok(staged),
                    },
                    operation,
                }
            });
            entered.await.unwrap();
            jobs.cancel("peer", "message");
            assert!(
                tokio::time::timeout(Duration::from_millis(20), jobs.next_event())
                    .await
                    .is_err()
            );
            jobs.stop.cancel();
            assert!(jobs.next_event().await.is_none());
            assert!(
                tokio::time::timeout(Duration::from_millis(20), jobs.finish())
                    .await
                    .is_err()
            );
            assert!(jobs.has_jobs());
            assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 1);
            release.send(()).unwrap();
            jobs.finish().await.unwrap();
            assert!(!jobs.has_jobs());
            assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
            assert_eq!(std::fs::read(source).unwrap(), b"preserved original");
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn cancelled_save_result_is_not_revalidated_by_same_peer_or_message_reuse() {
        let root = tempfile::tempdir().unwrap();
        let mut jobs = FileJobs::new(root.path());
        let original = jobs.operation("peer", Some("message"), true).unwrap();
        jobs.cancel("peer", "message");
        let next = jobs.operation("peer", Some("message"), true).unwrap();
        assert!(original.stop.is_cancelled());
        assert!(!next.stop.is_cancelled());
        jobs.jobs.spawn(async move {
            TrackedFileEvent {
                event: FileEvent::Saved {
                    peer: "peer".into(),
                    id: "message".into(),
                    stored_name: "old.bin".into(),
                    path: Ok(root.path().join("old.bin")),
                },
                operation: original,
            }
        });
        assert!(jobs.next_event().await.is_none());
        jobs.finish().await.unwrap();
    }

    #[tokio::test]
    async fn replacement_save_revokes_only_older_saves_and_drain_observes_panics() {
        use sha2::{Digest, Sha256};
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.bin");
        let target = root.path().join("target.bin");
        std::fs::write(&source, b"fixture").unwrap();
        let mut jobs = FileJobs::new(root.path());
        let sending = jobs.operation("peer", Some("message"), false).unwrap();
        let saving = jobs.operation("peer", Some("message"), true).unwrap();
        jobs.save(
            "peer".into(),
            "message".into(),
            std::fs::File::open(&source).unwrap(),
            FileInfo {
                name: "fixture".into(),
                size: 7,
                sha256: format!("{:x}", Sha256::digest(b"fixture")),
                stored_name: "fixture.bin".into(),
            },
            target.clone(),
        )
        .unwrap();
        assert!(!sending.stop.is_cancelled());
        assert!(saving.stop.is_cancelled());
        assert!(matches!(
            jobs.next_event().await,
            Some(FileEvent::Saved { path: Ok(_), .. })
        ));
        assert_eq!(std::fs::read(target).unwrap(), b"fixture");
        jobs.jobs
            .spawn(async { panic!("synthetic FileJobs failure") });
        assert!(jobs.next_event().await.is_none());
        assert!(jobs.finish().await.is_err());
        assert!(jobs.finish().await.is_err());
    }

    #[tokio::test]
    async fn logout_drain_observes_join_failure_even_when_event_wait_returns_on_cancellation() {
        let root = tempfile::tempdir().unwrap();
        let mut jobs = FileJobs::new(root.path());
        jobs.jobs
            .spawn(async { panic!("synthetic cancelled FileJobs failure") });
        jobs.stop.cancel();
        assert!(jobs.next_event().await.is_none());
        assert!(jobs.has_jobs());
        assert!(jobs.finish().await.is_err());
        assert!(!jobs.has_jobs());
        assert!(jobs.finish().await.is_err());
    }

    mod publication {
        use super::*;
        use crate::chat_crypto::ChatIdentity;
        use crate::chat_store::{ChatMessage, ChatStore, RETENTION_MS};
        use crate::critical_intents::{IntentScope, IntentTicket};
        use sha2::{Digest, Sha256};
        use std::io::Seek;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{mpsc, Mutex};
        use tokio::sync::oneshot;

        type Authority = Arc<dyn Fn() -> bool + Send + Sync + 'static>;
        const CONTENT: &[u8] = b"original verified publication";
        const PREVIOUS: &[u8] = b"previous destination";
        const NOW: i64 = RETENTION_MS * 3;

        struct Fixture {
            root: tempfile::TempDir,
            source: PathBuf,
            destination: PathBuf,
            target: PathBuf,
        }

        impl Fixture {
            fn new() -> Self {
                let root = tempfile::tempdir().unwrap();
                let source = root.path().join("source.bin");
                let destination = root.path().join("destination");
                std::fs::create_dir(&destination).unwrap();
                let target = destination.join("target.bin");
                std::fs::write(&source, CONTENT).unwrap();
                std::fs::write(&target, PREVIOUS).unwrap();
                std::fs::write(destination.join(".unrelated.part"), b"keep").unwrap();
                Self {
                    root,
                    source,
                    destination,
                    target,
                }
            }

            fn info(&self) -> FileInfo {
                FileInfo {
                    name: "source.bin".into(),
                    size: CONTENT.len() as u64,
                    sha256: format!("{:x}", Sha256::digest(CONTENT)),
                    stored_name: "fixture.bin".into(),
                }
            }

            fn save(&self, jobs: &mut FileJobs, target: PathBuf) {
                jobs.save(
                    "peer".into(),
                    "message".into(),
                    std::fs::File::open(&self.source).unwrap(),
                    self.info(),
                    target,
                )
                .unwrap();
            }

            fn incoming(&self) -> ChatEngine {
                let cache = self.root.path().join("cache");
                let info = FileBroker::new(&cache)
                    .unwrap()
                    .stage(&self.source)
                    .unwrap();
                let store = ChatStore::open(&self.root.path().join("chat.sqlite")).unwrap();
                assert!(store
                    .save(
                        &ChatMessage {
                            id: "message".into(),
                            owner_id: "owner".into(),
                            conversation_id: "peer".into(),
                            sender_id: "peer".into(),
                            target_id: "owner".into(),
                            kind: "file".into(),
                            text: String::new(),
                            created_at: NOW,
                            sequence: 1,
                            delivery: "delivered".into(),
                            file: Some(serde_json::json!({
                                "name": info.name,
                                "size": info.size,
                                "sha256": info.sha256,
                                "storedName": info.stored_name,
                                "transferState": "available",
                                "sourceAvailable": true,
                                "progress": 1
                            })),
                        },
                        NOW,
                    )
                    .unwrap());
                let mut engine = ChatEngine::new("owner".into(), ChatIdentity::generate(), store);
                engine.set_file_root(&cache).unwrap();
                engine.set_friends(["peer".into()]);
                engine
            }

            fn assert_destination(&self, content: &[u8], entries: usize) {
                assert_eq!(std::fs::read(&self.target).unwrap(), content);
                assert_eq!(std::fs::read(&self.source).unwrap(), CONTENT);
                assert_eq!(
                    std::fs::read(self.destination.join(".unrelated.part")).unwrap(),
                    b"keep"
                );
                assert_eq!(
                    std::fs::read_dir(&self.destination).unwrap().count(),
                    entries
                );
            }
        }

        struct Release(Option<mpsc::Sender<()>>);

        impl Release {
            fn release(&mut self) {
                if let Some(sender) = self.0.take() {
                    let _ = sender.send(());
                }
            }
        }

        impl Drop for Release {
            fn drop(&mut self) {
                self.release();
            }
        }

        struct Gate {
            authority: Authority,
            checks: Arc<AtomicUsize>,
            entered: oneshot::Receiver<()>,
            release: Release,
        }

        fn gate(intents: Arc<CriticalIntents>, ticket: IntentTicket, panic_after: bool) -> Gate {
            let checks = Arc::new(AtomicUsize::new(0));
            let counted = checks.clone();
            let (started, entered) = oneshot::channel();
            let started = Mutex::new(Some(started));
            let (release, blocked) = mpsc::channel();
            let blocked = Mutex::new(blocked);
            let caller = std::thread::current().id();
            let authority = Arc::new(move || {
                assert_ne!(std::thread::current().id(), caller);
                let check = counted.fetch_add(1, Ordering::SeqCst);
                assert!(intents.is_current(ticket, IntentScope::Session));
                assert_eq!(
                    intents.commit(ticket, IntentScope::Session, || true),
                    Some(true)
                );
                if check == 1 {
                    started.lock().unwrap().take().unwrap().send(()).unwrap();
                    blocked
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(10))
                        .unwrap();
                    assert!(!panic_after, "original publication authority panic");
                }
                intents.is_current(ticket, IntentScope::Session)
            });
            Gate {
                authority,
                checks,
                entered,
                release: Release(Some(release)),
            }
        }

        async fn entered(gate: &mut Gate) {
            tokio::time::timeout(Duration::from_secs(10), &mut gate.entered)
                .await
                .unwrap()
                .unwrap();
        }

        async fn next(jobs: &mut FileJobs) -> Option<FileEvent> {
            tokio::time::timeout(Duration::from_secs(10), jobs.next_event())
                .await
                .unwrap()
        }

        async fn finish(jobs: &mut FileJobs) -> Result<(), FileError> {
            let result = tokio::time::timeout(Duration::from_secs(10), jobs.finish())
                .await
                .unwrap();
            assert!(!jobs.has_jobs());
            result
        }

        fn admit(fixture: &Fixture, jobs: &mut FileJobs, incoming: bool) -> Option<ChatEngine> {
            if incoming {
                let engine = fixture.incoming();
                jobs.set_target("peer", "message", fixture.target.clone());
                jobs.finish_incoming(&engine, "peer", "message", NOW)
                    .unwrap();
                Some(engine)
            } else {
                fixture.save(jobs, fixture.target.clone());
                None
            }
        }

        #[tokio::test]
        async fn leave_at_post_sync_keeps_bound_original_direct_and_incoming_authority() {
            for incoming in [false, true] {
                let fixture = Fixture::new();
                let intents = Arc::new(CriticalIntents::default());
                let ticket = intents.ticket();
                let mut gate = gate(intents.clone(), ticket, false);
                let mut jobs = FileJobs::with_stop_and_authority(
                    fixture.root.path(),
                    intents.session_token(ticket),
                    gate.authority.clone(),
                );
                let engine = admit(&fixture, &mut jobs, incoming);
                entered(&mut gate).await;
                fixture.assert_destination(PREVIOUS, 3);
                intents.request(CriticalAction::Leave);
                assert!(intents.is_current(ticket, IntentScope::Session));
                assert!(!intents.is_current(ticket, IntentScope::Voice));
                assert!(!jobs.operations.last().unwrap().stop.is_cancelled());
                gate.release.release();
                let event = next(&mut jobs).await;
                let drained = finish(&mut jobs).await;
                assert!(
                    matches!(event, Some(FileEvent::Saved { peer, id, path: Ok(path), .. })
                    if peer == "peer" && id == "message" && path == fixture.target)
                );
                drained.unwrap();
                assert_eq!(gate.checks.load(Ordering::SeqCst), 2);
                fixture.assert_destination(CONTENT, 2);
                drop(engine);
            }
        }

        #[tokio::test]
        async fn logout_at_post_sync_rejects_original_direct_and_incoming_even_with_fresh_session()
        {
            for incoming in [false, true] {
                let fixture = Fixture::new();
                let intents = Arc::new(CriticalIntents::default());
                let ticket = intents.ticket();
                let mut gate = gate(intents.clone(), ticket, false);
                let mut jobs = FileJobs::with_stop_and_authority(
                    fixture.root.path(),
                    CancellationToken::new(),
                    gate.authority.clone(),
                );
                let engine = admit(&fixture, &mut jobs, incoming);
                entered(&mut gate).await;
                fixture.assert_destination(PREVIOUS, 3);
                intents.request(CriticalAction::Logout);
                assert_eq!(intents.next().await, CriticalAction::Logout);
                assert!(intents.is_current(intents.ticket(), IntentScope::Session));
                assert!(!intents.is_current(ticket, IntentScope::Session));
                assert!(!jobs.operations.last().unwrap().stop.is_cancelled());
                gate.release.release();
                let event = next(&mut jobs).await;
                let drained = finish(&mut jobs).await;
                assert!(
                    matches!(event, Some(FileEvent::Saved { peer, id, path: Err(_), .. })
                    if peer == "peer" && id == "message")
                );
                drained.unwrap();
                assert_eq!(gate.checks.load(Ordering::SeqCst), 2);
                fixture.assert_destination(PREVIOUS, 2);
                drop(engine);
            }
        }

        #[tokio::test]
        async fn replacement_same_peer_message_cannot_reactivate_original_blocked_save() {
            let fixture = Fixture::new();
            let intents = Arc::new(CriticalIntents::default());
            let ticket = intents.ticket();
            let mut gate = gate(intents.clone(), ticket, false);
            let mut jobs = FileJobs::with_stop_and_authority(
                fixture.root.path(),
                intents.session_token(ticket),
                gate.authority.clone(),
            );
            fixture.save(&mut jobs, fixture.target.clone());
            entered(&mut gate).await;
            let original = jobs.operations.last().unwrap().clone();
            let replacement = fixture.destination.join("replacement.bin");
            fixture.save(&mut jobs, replacement.clone());
            let replacement_event = next(&mut jobs).await;
            let original_cancelled = original.stop.is_cancelled();
            let replacement_cancelled = jobs.operations.last().unwrap().stop.is_cancelled();
            gate.release.release();
            let original_event = next(&mut jobs).await;
            let drained = finish(&mut jobs).await;
            assert!(
                matches!(replacement_event, Some(FileEvent::Saved { path: Ok(path), .. })
                if path == replacement)
            );
            assert!(original_cancelled);
            assert!(!replacement_cancelled);
            assert!(original_event.is_none());
            drained.unwrap();
            assert_eq!(gate.checks.load(Ordering::SeqCst), 4);
            assert_eq!(std::fs::read(replacement).unwrap(), CONTENT);
            fixture.assert_destination(PREVIOUS, 3);
        }

        #[tokio::test]
        async fn cancellation_inside_accepting_post_sync_callback_still_rejects_publication() {
            let fixture = Fixture::new();
            let stop = CancellationToken::new();
            let cancel = stop.clone();
            let checks = Arc::new(AtomicUsize::new(0));
            let counted = checks.clone();
            let caller = std::thread::current().id();
            let mut jobs = FileJobs::with_stop_and_authority(
                fixture.root.path(),
                stop,
                Arc::new(move || {
                    assert_ne!(std::thread::current().id(), caller);
                    if counted.fetch_add(1, Ordering::SeqCst) == 1 {
                        cancel.cancel();
                    }
                    true
                }),
            );
            fixture.save(&mut jobs, fixture.target.clone());
            let event = next(&mut jobs).await;
            let drained = finish(&mut jobs).await;
            assert!(event.is_none());
            drained.unwrap();
            assert_eq!(checks.load(Ordering::SeqCst), 2);
            fixture.assert_destination(PREVIOUS, 2);
        }

        #[tokio::test]
        async fn cancel_and_disconnect_at_post_sync_remain_fail_closed_with_current_authority() {
            for disconnect in [false, true] {
                let fixture = Fixture::new();
                let intents = Arc::new(CriticalIntents::default());
                let ticket = intents.ticket();
                let mut gate = gate(intents.clone(), ticket, false);
                let mut jobs = FileJobs::with_stop_and_authority(
                    fixture.root.path(),
                    intents.session_token(ticket),
                    gate.authority.clone(),
                );
                fixture.save(&mut jobs, fixture.target.clone());
                entered(&mut gate).await;
                if disconnect {
                    jobs.disconnect("peer");
                } else {
                    jobs.cancel("peer", "message");
                }
                assert!(intents.is_current(ticket, IntentScope::Session));
                gate.release.release();
                let event = next(&mut jobs).await;
                let drained = finish(&mut jobs).await;
                assert!(event.is_none());
                drained.unwrap();
                assert_eq!(gate.checks.load(Ordering::SeqCst), 2);
                fixture.assert_destination(PREVIOUS, 2);
            }
        }

        #[tokio::test]
        async fn denied_admission_does_not_read_open_source_or_touch_destination() {
            let fixture = Fixture::new();
            let source = std::fs::File::open(&fixture.source).unwrap();
            let mut cursor = source.try_clone().unwrap();
            let mut options = std::fs::OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.share_mode(0);
            }
            let locked_target = options.open(&fixture.target).unwrap();
            let checks = Arc::new(AtomicUsize::new(0));
            let counted = checks.clone();
            let caller = std::thread::current().id();
            let mut jobs = FileJobs::with_stop_and_authority(
                fixture.root.path(),
                CancellationToken::new(),
                Arc::new(move || {
                    assert_ne!(std::thread::current().id(), caller);
                    counted.fetch_add(1, Ordering::SeqCst);
                    false
                }),
            );
            jobs.save(
                "peer".into(),
                "message".into(),
                source,
                fixture.info(),
                fixture.target.clone(),
            )
            .unwrap();
            let event = next(&mut jobs).await;
            let drained = finish(&mut jobs).await;
            assert!(matches!(event, Some(FileEvent::Saved { path: Err(_), .. })));
            drained.unwrap();
            assert_eq!(checks.load(Ordering::SeqCst), 1);
            assert_eq!(cursor.stream_position().unwrap(), 0);
            drop(locked_target);
            fixture.assert_destination(PREVIOUS, 2);
        }

        #[tokio::test]
        async fn post_sync_callback_panic_is_retained_after_original_event_join() {
            let fixture = Fixture::new();
            let intents = Arc::new(CriticalIntents::default());
            let mut gate = gate(intents.clone(), intents.ticket(), true);
            let mut jobs = FileJobs::with_stop_and_authority(
                fixture.root.path(),
                CancellationToken::new(),
                gate.authority.clone(),
            );
            fixture.save(&mut jobs, fixture.target.clone());
            entered(&mut gate).await;
            gate.release.release();
            let event = next(&mut jobs).await;
            let drained = finish(&mut jobs).await;
            assert!(event.is_none());
            assert!(drained.is_err());
            assert!(finish(&mut jobs).await.is_err());
            assert_eq!(gate.checks.load(Ordering::SeqCst), 2);
            fixture.assert_destination(PREVIOUS, 2);
        }

        #[tokio::test]
        async fn cancelled_wait_keeps_original_panicking_publication_join_for_drain() {
            let fixture = Fixture::new();
            let intents = Arc::new(CriticalIntents::default());
            let mut gate = gate(intents.clone(), intents.ticket(), true);
            let stop = CancellationToken::new();
            let mut jobs = FileJobs::with_stop_and_authority(
                fixture.root.path(),
                stop.clone(),
                gate.authority.clone(),
            );
            fixture.save(&mut jobs, fixture.target.clone());
            entered(&mut gate).await;
            stop.cancel();
            assert!(next(&mut jobs).await.is_none());
            assert!(jobs.has_jobs());
            assert!(
                tokio::time::timeout(Duration::from_millis(20), jobs.finish())
                    .await
                    .is_err()
            );
            assert!(jobs.has_jobs());
            fixture.assert_destination(PREVIOUS, 3);
            gate.release.release();
            assert!(finish(&mut jobs).await.is_err());
            assert!(finish(&mut jobs).await.is_err());
            assert_eq!(gate.checks.load(Ordering::SeqCst), 2);
            fixture.assert_destination(PREVIOUS, 2);
        }
    }
}
