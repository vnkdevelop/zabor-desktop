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
        Self {
            root: root.into(),
            stop,
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
        self.jobs.spawn_blocking(move || TrackedFileEvent {
            event: FileEvent::Saved {
                peer,
                id,
                stored_name: info.stored_name,
                path: files::save_verified(
                    source,
                    &target,
                    info.size,
                    &info.sha256,
                    &operation.stop,
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
}
