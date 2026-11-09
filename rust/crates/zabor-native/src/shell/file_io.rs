use futures_util::FutureExt;
use std::future::Future;
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use zabor_native::chat::ChatEngine;
use zabor_native::chat_transport::ChatTransport;
use zabor_native::critical_intents::{CriticalIntents, IntentScope, IntentTicket};
use zabor_native::file_jobs::FileJobs;
use zabor_native::files::FileError;

#[derive(Default)]
pub(super) struct FileIo {
    jobs: Mutex<Vec<Arc<FileJob>>>,
    drain: tokio::sync::Mutex<()>,
    failed: Arc<AtomicBool>,
}

struct FileJob {
    join: tokio::sync::Mutex<Option<JoinHandle<()>>>,
}

pub(super) struct OwnedFiles {
    pub(super) jobs: Option<FileJobs>,
    pub(super) engine: Option<ChatEngine>,
    pub(super) transport: Option<ChatTransport>,
    io: Arc<FileIo>,
}

impl OwnedFiles {
    #[cfg(test)]
    pub(super) fn as_ref(&self) -> Option<&FileJobs> {
        self.jobs.as_ref()
    }

    pub(super) fn split(
        &mut self,
    ) -> (
        &mut Option<ChatEngine>,
        &mut Option<ChatTransport>,
        &mut Option<FileJobs>,
    ) {
        (&mut self.engine, &mut self.transport, &mut self.jobs)
    }

    #[cfg(test)]
    pub(super) fn replace(&mut self, jobs: FileJobs) {
        drop(self.retire());
        self.jobs = Some(jobs);
    }

    pub(super) fn retire(
        &mut self,
    ) -> Option<impl Future<Output = Result<(), FileError>> + Send + 'static> {
        if self.jobs.is_none() && self.engine.is_none() && self.transport.is_none() {
            return None;
        }
        Some(
            self.io
                .retire_session(self.jobs.take(), self.engine.take(), self.transport.take()),
        )
    }
}

impl Drop for OwnedFiles {
    fn drop(&mut self) {
        drop(self.retire());
    }
}

impl FileIo {
    pub(super) fn own(self: &Arc<Self>) -> OwnedFiles {
        OwnedFiles {
            jobs: None,
            engine: None,
            transport: None,
            io: self.clone(),
        }
    }

    fn publication_authority(
        intents: Arc<CriticalIntents>,
        ticket: IntentTicket,
        stop: CancellationToken,
    ) -> Arc<dyn Fn() -> bool + Send + Sync + 'static> {
        Arc::new(move || !stop.is_cancelled() && intents.is_current(ticket, IntentScope::Session))
    }

    pub(super) fn bound(
        root: &Path,
        intents: Arc<CriticalIntents>,
        ticket: IntentTicket,
        stop: CancellationToken,
    ) -> FileJobs {
        FileJobs::with_stop_and_authority(
            root,
            intents.session_token(ticket),
            Self::publication_authority(intents, ticket, stop),
        )
    }

    fn dispatch(
        &self,
        operation: impl Future<Output = Result<(), FileError>> + Send + 'static,
    ) -> oneshot::Receiver<Result<(), FileError>> {
        let (sender, receiver) = oneshot::channel();
        let failed = self.failed.clone();
        let mut jobs = self.jobs.lock().unwrap_or_else(|error| error.into_inner());
        jobs.retain(|job| {
            let Ok(mut join) = job.join.try_lock() else {
                return true;
            };
            if let Some(handle) = join.as_mut().filter(|handle| handle.is_finished()) {
                if let Some(result) = handle.now_or_never() {
                    if result.is_err() {
                        failed.store(true, Ordering::Release);
                    }
                    join.take();
                }
            }
            join.is_some()
        });
        let join = tokio::spawn(async move {
            let result = operation.await;
            if result.is_err() {
                failed.store(true, Ordering::Release);
            }
            let _ = sender.send(result);
        });
        jobs.push(Arc::new(FileJob {
            join: tokio::sync::Mutex::new(Some(join)),
        }));
        receiver
    }

    #[cfg(test)]
    pub(super) fn retire(
        &self,
        files: FileJobs,
    ) -> impl Future<Output = Result<(), FileError>> + Send + 'static {
        self.retire_session(Some(files), None, None)
    }

    pub(super) fn retire_session(
        &self,
        mut files: Option<FileJobs>,
        mut engine: Option<ChatEngine>,
        transport: Option<ChatTransport>,
    ) -> impl Future<Output = Result<(), FileError>> + Send + 'static {
        let receiver = self.dispatch(async move {
            let result = match files.as_mut() {
                Some(files) => files.finish().await,
                None => Ok(()),
            };
            tokio::task::spawn_blocking(move || {
                if let Some(engine) = engine.as_mut() {
                    engine.fail_downloads(None, super::now_ms());
                }
                drop((files, engine, transport));
            })
            .await
            .map_err(|_| FileError)?;
            result
        });
        async move { receiver.await.map_err(|_| FileError)? }
    }

    pub(super) async fn finish(&self) -> Result<(), FileError> {
        let _drain = self.drain.lock().await;
        loop {
            let jobs = self
                .jobs
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            if jobs.is_empty() {
                break;
            }
            for job in &jobs {
                let mut join = job.join.lock().await;
                if let Some(handle) = join.as_mut() {
                    if handle.await.is_err() {
                        self.failed.store(true, Ordering::Release);
                    }
                    join.take();
                }
            }
            self.jobs
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .retain(|job| !jobs.iter().any(|finished| Arc::ptr_eq(job, finished)));
        }
        if self.failed.load(Ordering::Acquire) {
            Err(FileError)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    #[tokio::test]
    async fn cancelled_waiter_and_drain_preserve_original_completion() {
        let io = Arc::new(FileIo::default());
        let (started, entered) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let completed = Arc::new(AtomicBool::new(false));
        let observed = completed.clone();
        let waiter = io.dispatch(async move {
            let _ = started.send(());
            released.await.map_err(|_| FileError)?;
            observed.store(true, Ordering::Release);
            Ok(())
        });
        entered.await.unwrap();
        drop(waiter);
        let owner = io.clone();
        let draining = tokio::spawn(async move { owner.finish().await });
        tokio::task::yield_now().await;
        draining.abort();
        let _ = draining.await;
        assert!(!completed.load(Ordering::Acquire));
        assert!(tokio::time::timeout(Duration::from_millis(20), io.finish())
            .await
            .is_err());
        release.send(()).unwrap();
        io.finish().await.unwrap();
        assert!(completed.load(Ordering::Acquire));
        assert!(io.jobs.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn completed_panic_and_failure_survive_pruning_and_repeated_drains() {
        let io = FileIo::default();
        assert!(io
            .dispatch(async { panic!("synthetic file cleanup failure") })
            .await
            .is_err());
        io.dispatch(async { Ok(()) }).await.unwrap().unwrap();
        assert!(io.finish().await.is_err());
        assert!(io.finish().await.is_err());
        let io = FileIo::default();
        assert!(io
            .dispatch(async { Err(FileError) })
            .await
            .unwrap()
            .is_err());
        assert!(io.finish().await.is_err());
    }

    #[tokio::test]
    async fn original_file_jobs_are_joined_before_cache_retirement() {
        let workspace = tempfile::tempdir().unwrap();
        let source = workspace.path().join("source.bin");
        let cache = workspace.path().join("cache");
        std::fs::write(&source, b"preserved original").unwrap();
        let mut jobs = FileJobs::new(&cache);
        jobs.stage("peer".into(), source.clone()).unwrap();
        let io = FileIo::default();
        io.retire(jobs).await.unwrap();
        io.finish().await.unwrap();
        assert_eq!(std::fs::read(source).unwrap(), b"preserved original");
        if cache.exists() {
            assert_eq!(std::fs::read_dir(cache).unwrap().count(), 0);
        }
    }

    #[tokio::test]
    async fn worker_abort_registers_original_file_jobs_for_session_drain() {
        let workspace = tempfile::tempdir().unwrap();
        let source = workspace.path().join("source.bin");
        let cache = workspace.path().join("cache");
        std::fs::write(&source, b"preserved original").unwrap();
        let intents = Arc::new(zabor_native::critical_intents::CriticalIntents::default());
        let session_io = super::super::session_io::SessionIo::new(
            None,
            intents,
            tokio_util::sync::CancellationToken::new(),
        );
        let io = session_io.file_io.clone();
        let worker_io = io.clone();
        let worker_cache = cache.clone();
        let worker_source = source.clone();
        let (started, entered) = oneshot::channel();
        let worker = tokio::spawn(async move {
            let mut owner = worker_io.own();
            let mut jobs = FileJobs::new(&worker_cache);
            jobs.stage("peer".into(), worker_source).unwrap();
            owner.replace(jobs);
            started.send(()).unwrap();
            std::future::pending::<()>().await;
            drop(owner);
        });
        entered.await.unwrap();
        worker.abort();
        assert!(worker.await.unwrap_err().is_cancelled());
        assert_eq!(io.jobs.lock().unwrap().len(), 1);
        session_io.finish().await.unwrap();
        assert!(io.jobs.lock().unwrap().is_empty());
        assert_eq!(std::fs::read(source).unwrap(), b"preserved original");
        if cache.exists() {
            assert_eq!(std::fs::read_dir(cache).unwrap().count(), 0);
        }
    }

    #[tokio::test]
    async fn replacement_and_unpolled_retirement_keep_both_original_owners() {
        let workspace = tempfile::tempdir().unwrap();
        let source = workspace.path().join("source.bin");
        std::fs::write(&source, b"preserved original").unwrap();
        let io = Arc::new(FileIo::default());
        let mut owner = io.own();
        for name in ["previous", "replacement"] {
            let mut jobs = FileJobs::new(&workspace.path().join(name));
            jobs.stage("peer".into(), source.clone()).unwrap();
            owner.replace(jobs);
        }
        drop(owner.retire());
        assert!(owner.as_ref().is_none());
        drop(owner);
        assert_eq!(io.jobs.lock().unwrap().len(), 2);
        io.finish().await.unwrap();
        for name in ["previous", "replacement"] {
            let cache = workspace.path().join(name);
            if cache.exists() {
                assert_eq!(std::fs::read_dir(cache).unwrap().count(), 0);
            }
        }
        assert_eq!(std::fs::read(source).unwrap(), b"preserved original");
    }

    #[tokio::test]
    async fn bound_publication_keeps_strong_original_authority_and_rejects_global_stop() {
        let workspace = tempfile::tempdir().unwrap();
        let source = workspace.path().join("source.bin");
        let target = workspace.path().join("target.bin");
        let content = b"preserved original";
        std::fs::write(&source, content).unwrap();
        std::fs::write(&target, b"previous destination").unwrap();
        let intents = Arc::new(CriticalIntents::default());
        let original = intents.ticket();
        let stop = CancellationToken::new();
        let authority = FileIo::publication_authority(intents.clone(), original, stop.clone());
        let mut jobs = FileIo::bound(workspace.path(), intents.clone(), original, stop.clone());
        assert!(authority());
        intents.request(zabor_native::critical_intents::CriticalAction::Leave);
        assert!(authority());
        stop.cancel();
        assert!(!authority());
        jobs.save(
            "peer".into(),
            "message".into(),
            std::fs::File::open(&source).unwrap(),
            zabor_native::files::FileInfo {
                name: "source.bin".into(),
                size: content.len() as u64,
                sha256: format!("{:x}", Sha256::digest(content)),
                stored_name: "fixture.bin".into(),
            },
            target.clone(),
        )
        .unwrap();
        assert!(matches!(
            jobs.next_event().await,
            Some(zabor_native::file_jobs::FileEvent::Saved { path: Err(_), .. })
        ));
        let retained = Arc::downgrade(&intents);
        drop(authority);
        drop(intents);
        assert!(retained.upgrade().is_some());
        jobs.finish().await.unwrap();
        drop(jobs);
        assert!(retained.upgrade().is_none());
        assert_eq!(std::fs::read(source).unwrap(), content);
        assert_eq!(std::fs::read(target).unwrap(), b"previous destination");
    }

    #[cfg(windows)]
    async fn retained_original_publication(panics: bool) {
        for reconciliation in [false, true] {
            let workspace = tempfile::tempdir().unwrap();
            let source = workspace.path().join("source.bin");
            let target = workspace.path().join("target.bin");
            let database = workspace.path().join("original.sqlite");
            let content = b"preserved original";
            std::fs::write(&source, content).unwrap();
            std::fs::write(&target, b"previous destination").unwrap();
            let intents = Arc::new(CriticalIntents::default());
            let ticket = intents.ticket();
            let stop = CancellationToken::new();
            let session_io =
                super::super::session_io::SessionIo::new(None, intents.clone(), stop.clone());
            let io = session_io.file_io.clone();
            assert!(Arc::ptr_eq(&io, &session_io.chat_io.file_io));
            let authority = FileIo::publication_authority(intents.clone(), ticket, stop);
            let checks = Arc::new(AtomicUsize::new(0));
            let counted = checks.clone();
            let (started, entered) = oneshot::channel();
            let started = Mutex::new(Some(started));
            let (release, blocked) = std::sync::mpsc::channel();
            let blocked = Mutex::new(blocked);
            let mut jobs = FileJobs::with_stop_and_authority(
                workspace.path(),
                intents.session_token(ticket),
                Arc::new(move || {
                    if counted.fetch_add(1, Ordering::AcqRel) == 1 {
                        started.lock().unwrap().take().unwrap().send(()).unwrap();
                        blocked
                            .lock()
                            .unwrap()
                            .recv_timeout(Duration::from_secs(5))
                            .unwrap();
                        assert!(!panics, "original publication authority failed");
                    }
                    authority()
                }),
            );
            jobs.save(
                "peer".into(),
                "message".into(),
                std::fs::File::open(&source).unwrap(),
                zabor_native::files::FileInfo {
                    name: "source.bin".into(),
                    size: content.len() as u64,
                    sha256: format!("{:x}", Sha256::digest(content)),
                    stored_name: "fixture.bin".into(),
                },
                target.clone(),
            )
            .unwrap();
            let mut owner = io.own();
            owner.jobs = Some(jobs);
            owner.engine = Some(ChatEngine::new(
                "owner".into(),
                zabor_native::chat_crypto::ChatIdentity::generate(),
                zabor_native::chat_store::ChatStore::open(&database).unwrap(),
            ));
            tokio::time::timeout(Duration::from_secs(5), entered)
                .await
                .unwrap()
                .unwrap();
            if reconciliation {
                drop(
                    session_io
                        .chat_io
                        .reconcile(super::super::chat_io::ChatChange {
                            resources: owner,
                            revoked: None,
                            friends: Vec::new(),
                        }),
                );
            } else {
                drop(owner.retire());
                drop(owner);
            }
            intents.request(zabor_native::critical_intents::CriticalAction::Logout);
            assert!(
                tokio::time::timeout(Duration::from_millis(20), session_io.finish())
                    .await
                    .is_err()
            );
            assert!(std::fs::rename(&database, workspace.path().join("too-early.sqlite")).is_err());
            assert_eq!(std::fs::read(&target).unwrap(), b"previous destination");
            release.send(()).unwrap();
            let result = tokio::time::timeout(Duration::from_secs(5), session_io.finish())
                .await
                .unwrap();
            assert_eq!(result.is_err(), panics);
            assert_eq!(session_io.finish().await.is_err(), panics);
            assert_eq!(checks.load(Ordering::Acquire), 2);
            std::fs::rename(&database, workspace.path().join("retired.sqlite")).unwrap();
            assert_eq!(std::fs::read(source).unwrap(), content);
            assert_eq!(std::fs::read(target).unwrap(), b"previous destination");
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn cancelled_resource_drain_retains_original_database_until_publication_join() {
        retained_original_publication(false).await;
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn original_publication_panic_retires_database_but_keeps_session_failure() {
        retained_original_publication(true).await;
    }
}
