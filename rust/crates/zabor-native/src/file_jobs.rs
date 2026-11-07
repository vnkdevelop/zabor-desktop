use crate::chat::{ChatEngine, OutboundPacket};
use crate::chat_transport::ChatTransport;
use crate::files::{self, FileBroker, FileError, FileInfo, FileTransfer, StagedFile};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
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
    jobs: JoinSet<FileEvent>,
    sending: BTreeMap<(String, String), (u64, CancellationToken)>,
    pending: BTreeMap<(String, String), Instant>,
    targets: BTreeMap<(String, String), PathBuf>,
    next_job: u64,
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
            sending: BTreeMap::new(),
            pending: BTreeMap::new(),
            targets: BTreeMap::new(),
            next_job: 1,
        }
    }

    pub fn stage(&mut self, peer: String, source: PathBuf) -> Result<(), FileError> {
        if self.stop.is_cancelled() || self.jobs.len() >= MAX_FILE_JOBS {
            return Err(FileError);
        }
        let root = self.root.clone();
        let stop = self.stop.child_token();
        self.jobs.spawn_blocking(move || FileEvent::Staged {
            peer,
            file: FileBroker::new(&root)
                .and_then(|broker| broker.stage_cancellable(&source, &stop)),
        });
        Ok(())
    }

    pub fn can_stage(&self, count: usize) -> bool {
        !self.stop.is_cancelled()
            && count > 0
            && count <= 20
            && self.jobs.len().saturating_add(count) <= MAX_FILE_JOBS
    }

    pub fn reveal(&mut self, peer: String, id: String, path: PathBuf) -> Result<(), FileError> {
        if self.stop.is_cancelled() || self.jobs.len() >= MAX_FILE_JOBS {
            return Err(FileError);
        }
        let stop = self.stop.child_token();
        self.jobs.spawn_blocking(move || FileEvent::Revealed {
            peer,
            id,
            success: !stop.is_cancelled() && files::reveal_saved(&path).is_ok(),
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
        if self.stop.is_cancelled() || self.jobs.len() >= MAX_FILE_JOBS {
            return Err(FileError);
        }
        let stop = self.stop.child_token();
        self.jobs.spawn_blocking(move || FileEvent::Saved {
            peer,
            id,
            stored_name: info.stored_name,
            path: files::save_verified(source, &target, info.size, &info.sha256, &stop),
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
        let key = (peer.into(), id.into());
        self.pending.remove(&key);
        self.targets.remove(&key);
        if let Some((_, stop)) = self.sending.remove(&key) {
            stop.cancel();
        }
    }

    pub fn disconnect(&mut self, peer: &str) {
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
        for (peer, id) in engine.take_file_cancels() {
            self.cancel(&peer, &id);
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
                FileEvent::Sent {
                    peer,
                    id,
                    job,
                    success,
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
                _ = self.stop.cancelled() => { self.jobs.abort_all(); return None; },
                event = self.jobs.join_next() => event?,
            };
            if self.stop.is_cancelled() {
                self.jobs.abort_all();
                return None;
            }
            let Ok(event) = event else {
                continue;
            };
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
            return Some(event);
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
            jobs.jobs.spawn(async move {
                let file = staged;
                started_job.notify_one();
                std::future::pending::<()>().await;
                FileEvent::Staged {
                    peer: "peer".into(),
                    file: Ok(file),
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
}
