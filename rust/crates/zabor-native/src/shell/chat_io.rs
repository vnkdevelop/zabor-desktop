use futures_util::FutureExt;
use std::future::Future;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use zabor_native::chat::{ChatEngine, ChatError, FriendRevocations};
use zabor_native::chat_transport::ChatTransport;

#[derive(Default)]
pub(super) struct ChatIo {
    pub(super) file_io: Arc<super::file_io::FileIo>,
    jobs: Mutex<Vec<Arc<ChatJob>>>,
    drain: tokio::sync::Mutex<()>,
    failed: Arc<AtomicBool>,
}

struct ChatJob {
    join: tokio::sync::Mutex<Option<JoinHandle<()>>>,
}

struct JobFailure(Arc<AtomicBool>);

impl Drop for JobFailure {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.store(true, Ordering::Release);
        }
    }
}

pub(super) struct ChatChange {
    pub(super) resources: super::file_io::OwnedFiles,
    pub(super) revoked: Option<FriendRevocations>,
    pub(super) friends: Vec<String>,
}

impl ChatIo {
    fn dispatch<T: Send + 'static>(
        &self,
        operation: impl FnOnce() -> T + Send + 'static,
    ) -> oneshot::Receiver<T> {
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
        let join = tokio::task::spawn_blocking(move || {
            let _failure = JobFailure(failed);
            let _ = sender.send(operation());
        });
        jobs.push(Arc::new(ChatJob {
            join: tokio::sync::Mutex::new(Some(join)),
        }));
        receiver
    }

    pub(super) fn reconcile(
        &self,
        mut change: ChatChange,
    ) -> impl Future<Output = Result<(ChatChange, bool), ChatError>> + Send + 'static {
        let failed = self.failed.clone();
        let receiver = self.dispatch(move || {
            let result = match change.revoked.take() {
                Some(revoked) => match change.resources.engine.as_mut() {
                    Some(engine) => engine.finish_friend_revocations(revoked).is_ok(),
                    None => false,
                },
                None => true,
            };
            if let Some(transport) = change.resources.transport.as_mut() {
                transport.set_friends(&change.friends);
            }
            if !result {
                failed.store(true, Ordering::Release);
            }
            (change, result)
        });
        async move { receiver.await.map_err(|_| ChatError::Storage) }
    }

    pub(super) async fn before_store<T>(
        &self,
        open: impl FnOnce() -> Result<T, ChatError>,
    ) -> Result<T, ChatError> {
        self.finish().await?;
        open()
    }

    pub(super) async fn finish(&self) -> Result<(), ChatError> {
        let _drain = self.drain.lock().await;
        loop {
            let jobs = self
                .jobs
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            if jobs.is_empty() {
                if self.file_io.finish().await.is_err() {
                    self.failed.store(true, Ordering::Release);
                }
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
            Err(ChatError::Storage)
        } else {
            Ok(())
        }
    }
}

pub(super) async fn reconcile(
    work: &super::SessionWork<'_>,
    io: &ChatIo,
    chat: &mut Option<ChatEngine>,
    transport: &mut Option<ChatTransport>,
    files: &mut Option<zabor_native::file_jobs::FileJobs>,
    previous: &[String],
    friends: &[String],
) {
    let change = work.commit(|| {
        if let Some(files) = files.as_mut() {
            for peer in previous.iter().filter(|peer| !friends.contains(peer)) {
                files.disconnect(peer);
            }
        }
        let mut resources = io.file_io.own();
        resources.jobs = files.take();
        resources.engine = chat.take();
        resources.transport = transport.take();
        let revoked = resources
            .engine
            .as_mut()
            .map(|engine| engine.replace_friends(friends.iter().cloned()));
        ChatChange {
            resources,
            revoked,
            friends: friends.to_vec(),
        }
    });
    let Some(change) = change else {
        return;
    };
    if change.resources.engine.is_none() && change.resources.transport.is_none() {
        return;
    }
    match io.reconcile(change).await {
        Ok((change, success)) => {
            let mut change = Some(change);
            work.commit(|| {
                let returned = change.as_mut().unwrap();
                *chat = returned.resources.engine.take();
                *transport = returned.resources.transport.take();
                *files = returned.resources.jobs.take();
            });
            drop(change);
            if !success {
                work.publish(super::UiEvent::Toast("chat.storageFailed"));
            }
        }
        Err(_) => work.publish(super::UiEvent::Toast("chat.storageFailed")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;
    use zabor_native::chat_crypto::ChatIdentity;
    use zabor_native::chat_store::{ChatStore, RETENTION_MS};
    use zabor_native::critical_intents::{CriticalAction, CriticalIntents};
    use zabor_native::files::{encode_frame, ChunkHeader};

    fn engine(root: &std::path::Path, name: &str) -> ChatEngine {
        let mut engine = ChatEngine::new(
            name.into(),
            ChatIdentity::generate(),
            ChatStore::open(&root.join(format!("{name}.sqlite"))).unwrap(),
        );
        engine.set_file_root(&root.join(name)).unwrap();
        engine.set_friends(["peer".into()]);
        engine
    }

    fn downloading(root: &std::path::Path) -> (ChatEngine, String) {
        let now = RETENTION_MS * 3;
        let mut alice = engine(root, "alice");
        let mut bob = engine(root, "bob");
        alice.set_friends(["bob".into()]);
        bob.set_friends(["alice".into()]);
        let source = root.join("original.bin");
        std::fs::write(&source, b"verified original file").unwrap();
        let info = alice.file_broker().unwrap().stage(&source).unwrap();
        let packets = alice.send_file("bob", info, now).unwrap();
        let mut queue: VecDeque<_> = packets.into_iter().map(|packet| (true, packet)).collect();
        let mut count = 0;
        while let Some((sender, packet)) = queue.pop_front() {
            count += 1;
            assert!(count < 100);
            let replies = if sender {
                bob.receive("alice", &packet.payload, now).unwrap()
            } else {
                alice.receive("bob", &packet.payload, now).unwrap()
            };
            queue.extend(replies.into_iter().map(|packet| (!sender, packet)));
        }
        let id = bob.messages("alice", now).unwrap()[0].id.clone();
        bob.request_file("alice", &id, now).unwrap();
        for (kind, payload, offset) in [
            ("file-begin", &[][..], None),
            ("file-chunk", &b"verified"[..], Some(0)),
        ] {
            let header = ChunkHeader {
                version: 1,
                sender_id: "alice".into(),
                target_id: "bob".into(),
                message_id: id.clone(),
                transfer_id: "original-transfer".into(),
                kind: kind.into(),
                offset,
            };
            bob.receive_file("alice", &encode_frame(&header, payload).unwrap(), now)
                .unwrap();
        }
        assert!(bob.has_downloads());
        (bob, id)
    }

    fn retiring_download(
        root: &std::path::Path,
        io: &ChatIo,
    ) -> (ChatStore, rusqlite::Connection, String) {
        let (mut original, id) = downloading(root);
        let store = ChatStore::open(&root.join("bob.sqlite")).unwrap();
        let holder = rusqlite::Connection::open(root.join("bob.sqlite")).unwrap();
        holder.execute_batch("BEGIN IMMEDIATE").unwrap();
        let revoked = original.replace_friends(Vec::new());
        let mut resources = io.file_io.own();
        resources.engine = Some(original);
        drop(io.reconcile(ChatChange {
            resources,
            revoked: Some(revoked),
            friends: Vec::new(),
        }));
        (store, holder, id)
    }

    #[tokio::test]
    async fn cancelled_waiter_and_cancelled_drain_keep_original_blocking_join() {
        let io = ChatIo::default();
        let (entered, started) = oneshot::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let observed = Arc::new(AtomicBool::new(false));
        let completed = observed.clone();
        let receiver = io.dispatch(move || {
            entered.send(()).unwrap();
            blocked.recv_timeout(Duration::from_secs(5)).unwrap();
            completed.store(true, Ordering::Release);
        });
        started.await.unwrap();
        drop(receiver);
        assert!(tokio::time::timeout(Duration::from_millis(20), io.finish())
            .await
            .is_err());
        assert!(!observed.load(Ordering::Acquire));
        assert_eq!(io.jobs.lock().unwrap().len(), 1);
        release.send(()).unwrap();
        io.finish().await.unwrap();
        assert!(observed.load(Ordering::Acquire));
        assert!(io.jobs.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn concurrent_drains_observe_original_jobs_and_admission_during_drain() {
        let io = Arc::new(ChatIo::default());
        let (entered, started) = oneshot::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let receiver = io.dispatch(move || {
            entered.send(()).unwrap();
            blocked.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        started.await.unwrap();
        drop(receiver);
        let first = {
            let io = io.clone();
            tokio::spawn(async move { io.finish().await })
        };
        let second = {
            let io = io.clone();
            tokio::spawn(async move { io.finish().await })
        };
        let observed = Arc::new(AtomicUsize::new(0));
        let completed = observed.clone();
        let receiver = io.dispatch(move || {
            completed.fetch_add(1, Ordering::AcqRel);
        });
        drop(receiver);
        release.send(()).unwrap();
        first.await.unwrap().unwrap();
        second.await.unwrap().unwrap();
        assert_eq!(observed.load(Ordering::Acquire), 1);
        assert!(io.jobs.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn original_panic_is_observed_before_pruning_and_stays_a_drain_error() {
        let io = ChatIo::default();
        assert!(io
            .dispatch(|| panic!("synthetic original chat-cleanup panic"))
            .await
            .is_err());
        io.dispatch(|| ()).await.unwrap();
        assert!(io.finish().await.is_err());
        assert!(io.finish().await.is_err());
    }

    #[tokio::test]
    async fn cross_engine_cleanup_rejection_is_retained_without_retagging_original() {
        let root = tempfile::tempdir().unwrap();
        let io = ChatIo::default();
        let mut original = engine(root.path(), "owner");
        let revoked = original.replace_friends(Vec::new());
        let replacement = ChatEngine::new(
            "owner".into(),
            ChatIdentity::generate(),
            ChatStore::open(&root.path().join("replacement.sqlite")).unwrap(),
        );
        let mut resources = io.file_io.own();
        resources.engine = Some(replacement);
        let (change, success) = io
            .reconcile(ChatChange {
                resources,
                revoked: Some(revoked),
                friends: Vec::new(),
            })
            .await
            .unwrap();
        assert!(!success);
        assert_eq!(change.resources.engine.as_ref().unwrap().owner(), "owner");
        drop(change);
        assert!(io.finish().await.is_err());
        assert!(io.finish().await.is_err());
    }

    #[tokio::test]
    async fn stale_worker_ticket_never_detaches_or_revokes_replacement_chat() {
        let root = tempfile::tempdir().unwrap();
        let io = ChatIo::default();
        let intents = CriticalIntents::default();
        let original = intents.ticket();
        let stop = CancellationToken::new();
        let update = |_| panic!("stale chat worker published");
        let work = super::super::SessionWork::new(&intents, original, &stop, &update);
        intents.request(CriticalAction::Logout);
        assert_eq!(intents.next().await, CriticalAction::Logout);
        let mut chat = Some(engine(root.path(), "replacement"));
        let mut transport = None;
        let mut files = None;
        reconcile(
            &work,
            &io,
            &mut chat,
            &mut transport,
            &mut files,
            &["peer".into()],
            &[],
        )
        .await;
        assert!(chat.as_ref().unwrap().is_friend("peer"));
        assert_eq!(chat.as_ref().unwrap().owner(), "replacement");
        assert!(io.jobs.lock().unwrap().is_empty());
        io.finish().await.unwrap();
    }

    #[tokio::test]
    async fn actual_revocation_returns_original_chat_after_leave_with_no_acceptance_io() {
        let root = tempfile::tempdir().unwrap();
        let io = ChatIo::default();
        let intents = CriticalIntents::default();
        let ticket = intents.ticket();
        let stop = CancellationToken::new();
        let update = |_| panic!("successful chat reconciliation must not publish an error");
        let work = super::super::SessionWork::new(&intents, ticket, &stop, &update);
        let mut chat = Some(engine(root.path(), "owner"));
        let mut transport = None;
        let mut files = None;
        intents.request(CriticalAction::Leave);
        reconcile(
            &work,
            &io,
            &mut chat,
            &mut transport,
            &mut files,
            &["peer".into()],
            &[],
        )
        .await;
        assert_eq!(chat.as_ref().unwrap().owner(), "owner");
        assert!(!chat.as_ref().unwrap().is_friend("peer"));
        assert_eq!(intents.next().await, CriticalAction::Leave);
        io.finish().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn actual_sqlite_cleanup_wait_keeps_leave_responsive_and_returns_original_owner() {
        let root = tempfile::tempdir().unwrap();
        let io = ChatIo::default();
        let (original, id) = downloading(root.path());
        let holder = rusqlite::Connection::open(root.path().join("bob.sqlite")).unwrap();
        holder.execute_batch("BEGIN IMMEDIATE").unwrap();
        let intents = Arc::new(CriticalIntents::default());
        let ticket = intents.ticket();
        let stop = CancellationToken::new();
        let update = |_| panic!("successful off-lock SQLite cleanup published error");
        let work = super::super::SessionWork::new(&intents, ticket, &stop, &update);
        let mut chat = Some(original);
        let mut transport = None;
        let mut files = None;
        let previous = ["alice".into()];
        let waiting = work.run(reconcile(
            &work,
            &io,
            &mut chat,
            &mut transport,
            &mut files,
            &previous,
            &[],
        ));
        tokio::pin!(waiting);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut waiting)
                .await
                .is_err()
        );
        let request = {
            let intents = intents.clone();
            tokio::task::spawn_blocking(move || intents.request(CriticalAction::Leave))
        };
        let responsiveness = tokio::time::timeout(Duration::from_millis(500), request).await;
        holder.execute_batch("ROLLBACK").unwrap();
        responsiveness.unwrap().unwrap();
        assert_eq!(waiting.await, Some(()));
        assert_eq!(intents.next().await, CriticalAction::Leave);
        io.finish().await.unwrap();
        assert!(io.jobs.lock().unwrap().is_empty());
        let store = ChatStore::open(&root.path().join("bob.sqlite")).unwrap();
        let file = store
            .message("bob", "alice", &id, RETENTION_MS * 3)
            .unwrap()
            .unwrap()
            .file
            .unwrap();
        assert_eq!(file["transferState"], json!("failed"));
        assert_eq!(
            std::fs::read(root.path().join("original.bin")).unwrap(),
            b"verified original file"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelled_actual_sqlite_cleanup_is_still_observed_without_replacement_publication() {
        let root = tempfile::tempdir().unwrap();
        let io = ChatIo::default();
        let (original, id) = downloading(root.path());
        let holder = rusqlite::Connection::open(root.path().join("bob.sqlite")).unwrap();
        holder.execute_batch("BEGIN IMMEDIATE").unwrap();
        let intents = Arc::new(CriticalIntents::default());
        let stop = CancellationToken::new();
        let update = |_| panic!("revoked original cleanup published into replacement owner");
        let work = super::super::SessionWork::new(&intents, intents.ticket(), &stop, &update);
        let mut chat = Some(original);
        let mut transport = None;
        let mut files = None;
        let previous = ["alice".into()];
        let waiting = work.run(reconcile(
            &work,
            &io,
            &mut chat,
            &mut transport,
            &mut files,
            &previous,
            &[],
        ));
        tokio::pin!(waiting);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut waiting)
                .await
                .is_err()
        );
        intents.request(CriticalAction::Logout);
        assert_eq!(waiting.await, None);
        assert!(tokio::time::timeout(Duration::from_millis(20), io.finish())
            .await
            .is_err());
        assert_eq!(io.jobs.lock().unwrap().len(), 1);
        holder.execute_batch("ROLLBACK").unwrap();
        io.finish().await.unwrap();
        assert_eq!(intents.next().await, CriticalAction::Logout);
        let store = ChatStore::open(&root.path().join("bob.sqlite")).unwrap();
        let file = store
            .message("bob", "alice", &id, RETENTION_MS * 3)
            .unwrap()
            .unwrap()
            .file
            .unwrap();
        assert_eq!(file["transferState"], json!("failed"));
        assert!(io.jobs.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn readmission_same_metadata_waits_until_original_revocation_write_is_observed() {
        let root = tempfile::tempdir().unwrap();
        let io = ChatIo::default();
        let (store, holder, id) = retiring_download(root.path(), &io);
        let calls = AtomicUsize::new(0);
        let mut admission = Box::pin(io.before_store(|| {
            calls.fetch_add(1, Ordering::AcqRel);
            store
                .message("bob", "alice", &id, RETENTION_MS * 3)
                .map_err(|_| ChatError::Storage)
        }));
        let early = tokio::time::timeout(Duration::from_millis(20), &mut admission).await;
        let admitted_early = early.is_ok();
        holder.execute_batch("ROLLBACK").unwrap();
        let message = match early {
            Ok(message) => message,
            Err(_) => admission.await,
        };
        io.finish().await.unwrap();
        assert!(
            !admitted_early,
            "replacement store was admitted before original cleanup"
        );
        assert_eq!(calls.load(Ordering::Acquire), 1);
        let message = message.unwrap().unwrap();
        let original = message.file.unwrap();
        assert_eq!(original["transferState"], json!("failed"));
        let mut replacement = original.clone();
        replacement["transferState"] = json!("transferring");
        replacement["progress"] = json!(0);
        assert!(store
            .set_file_if(
                "bob",
                "alice",
                &id,
                &original,
                &replacement,
                RETENTION_MS * 3
            )
            .unwrap());
        io.finish().await.unwrap();
        assert_eq!(
            store
                .message("bob", "alice", &id, RETENTION_MS * 3)
                .unwrap()
                .unwrap()
                .file
                .unwrap(),
            replacement
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn readmission_logout_cancels_new_waiter_without_losing_original_cleanup() {
        let root = tempfile::tempdir().unwrap();
        let io = ChatIo::default();
        let (_store, holder, _id) = retiring_download(root.path(), &io);
        let intents = CriticalIntents::default();
        let stop = CancellationToken::new();
        let update = |_| panic!("revoked replacement-store admission published");
        let work = super::super::SessionWork::new(&intents, intents.ticket(), &stop, &update);
        let calls = AtomicUsize::new(0);
        let mut admission = Box::pin(work.run(io.before_store(|| {
            calls.fetch_add(1, Ordering::AcqRel);
            Ok(())
        })));
        let early = tokio::time::timeout(Duration::from_millis(20), &mut admission).await;
        let admitted_early = early.is_ok();
        intents.request(CriticalAction::Logout);
        let result = match early {
            Ok(result) => result,
            Err(_) => admission.await,
        };
        let pending = tokio::time::timeout(Duration::from_millis(20), io.finish()).await;
        holder.execute_batch("ROLLBACK").unwrap();
        io.finish().await.unwrap();
        assert!(
            !admitted_early,
            "replacement admission ignored retained cleanup"
        );
        assert!(result.is_none());
        assert!(pending.is_err());
        assert_eq!(calls.load(Ordering::Acquire), 0);
        assert!(io.jobs.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn readmission_original_cleanup_failure_cannot_admit_another_store() {
        let io = ChatIo::default();
        assert!(io
            .dispatch(|| panic!("synthetic failed original cleanup"))
            .await
            .is_err());
        let calls = AtomicUsize::new(0);
        let result = io
            .before_store(|| {
                calls.fetch_add(1, Ordering::AcqRel);
                Ok(())
            })
            .await;
        assert!(io.finish().await.is_err());
        assert!(
            result.is_err(),
            "failed original cleanup admitted replacement store"
        );
        assert_eq!(calls.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn current_reconciliation_returns_the_original_admitted_file_job() {
        let root = tempfile::tempdir().unwrap();
        let io = ChatIo::default();
        let intents = Arc::new(CriticalIntents::default());
        let ticket = intents.ticket();
        let stop = CancellationToken::new();
        let update = |_| panic!("current file owner reconciliation published an error");
        let work = super::super::SessionWork::new(&intents, ticket, &stop, &update);
        let source = root.path().join("source.bin");
        std::fs::write(&source, b"preserved original").unwrap();
        let mut chat = Some(engine(root.path(), "owner"));
        let mut transport = None;
        let mut files = Some(super::super::file_io::FileIo::bound(
            &root.path().join("owner"),
            intents.clone(),
            ticket,
            stop.clone(),
        ));
        files
            .as_mut()
            .unwrap()
            .stage("peer".into(), source.clone())
            .unwrap();
        reconcile(
            &work,
            &io,
            &mut chat,
            &mut transport,
            &mut files,
            &["peer".into()],
            &["peer".into()],
        )
        .await;
        assert_eq!(chat.as_ref().unwrap().owner(), "owner");
        assert!(files.as_ref().unwrap().has_jobs());
        assert!(matches!(
            files.as_mut().unwrap().next_event().await,
            Some(zabor_native::file_jobs::FileEvent::Staged { file: Ok(_), .. })
        ));
        drop(
            io.file_io
                .retire_session(files.take(), chat.take(), transport.take()),
        );
        io.finish().await.unwrap();
        assert_eq!(std::fs::read(source).unwrap(), b"preserved original");
    }
}
