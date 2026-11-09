use super::*;
use tokio::sync::mpsc;
use tokio::time::timeout;
use zabor_native::achievements::{parse_result, AchievementsView};
use zabor_native::critical_intents::CriticalAction;

fn fixture(intents: &CriticalIntents) -> (AchievementsView, AchievementResult) {
    let mut view = AchievementsView::default();
    let query = view
        .begin(intents, intents.ticket(), "owner", AchievementTarget::Own)
        .unwrap();
    let result = parse_result(&serde_json::json!("{\"unlockedHiddenDefs\":[{\"id\":\"hidden-one\",\"icon\":\"x\",\"maxValue\":1,\"statKey\":\"hiddenStat\",\"titleRu\":\"r\",\"descriptionRu\":\"r\",\"titleEn\":\"e\",\"descriptionEn\":\"e\"}]}"));
    (
        view,
        AchievementResult {
            query,
            result,
            catalog: None,
            storage_failed: false,
        },
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn held_ack_accepts_leave_but_rejects_logout_view_owner_cancel_and_timeout_without_stale_writes(
) {
    timeout(Duration::from_secs(15), async {
        for mode in [
            "accept",
            "leave",
            "logout",
            "view",
            "owner",
            "cancel",
            "timeout",
            "ack-view",
            "ack-owner",
            "ack-logout",
            "ack-cancel",
            "held-view",
            "held-owner",
        ] {
            let root = tempfile::tempdir().unwrap();
            let profile = Arc::new(Profile::open(root.path()).unwrap());
            let intents = Arc::new(CriticalIntents::default());
            let stop = CancellationToken::new();
            let io = AchievementIo::unverified_transaction_fixture(
                Some(profile.clone()),
                intents.clone(),
                stop.clone(),
            );
            let store = HiddenStore::open(&profile, "owner").unwrap();
            let empty = store.load().unwrap();
            drop(store);
            let (mut seed_view, mut seed) = fixture(&intents);
            seed.result = Ok(Default::default());
            assert!(seed_view.accept(&intents, &seed.query, seed.result));
            let store = HiddenStore::open(&profile, "owner").unwrap();
            let plan = empty.prepare(&seed_view).unwrap().unwrap();
            let prepared = store.prepare(&plan).unwrap();
            store
                .replace_if(
                    prepared,
                    || true,
                    || plan.commit_if_current(&intents, &seed_view),
                )
                .unwrap()
                .unwrap();
            drop(store);
            let path = std::fs::read_dir(root.path().join("achievements"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == "json")
                })
                .unwrap();
            let previous = std::fs::read(&path).unwrap();
            let (mut view, result) = fixture(&intents);
            let original = result.query.clone();
            let (updates, mut events) = mpsc::unbounded_channel();
            let ack_timeout = if mode == "timeout" {
                Duration::from_millis(100)
            } else if matches!(mode, "held-view" | "held-owner") {
                Duration::from_secs(20)
            } else {
                Duration::from_secs(2)
            };
            let mut job = io.start_timed(
                result,
                Arc::new(move |event| {
                    let _ = updates.send(event);
                }),
                ack_timeout,
            );
            let event = timeout(Duration::from_secs(3), events.recv())
                .await
                .unwrap()
                .unwrap();
            let Some(UiEvent::App(Event::AchievementProposal {
                result,
                plan,
                acknowledge,
            })) = event.for_current(&intents)
            else {
                panic!("original transaction did not propose after publication")
            };
            assert_eq!(result.query, original);
            assert!(result.catalog.is_some());
            match mode {
                "leave" => {
                    intents.request(CriticalAction::Leave);
                }
                "logout" => {
                    intents.request(CriticalAction::Logout);
                }
                "view" | "held-view" => view.close(),
                "owner" | "held-owner" => {
                    view.begin(
                        &intents,
                        intents.ticket(),
                        "replacement",
                        AchievementTarget::Own,
                    )
                    .unwrap();
                }
                "cancel" => stop.cancel(),
                _ => {}
            }
            let held_ack = if matches!(mode, "timeout" | "cancel" | "held-view" | "held-owner") {
                Some(acknowledge)
            } else {
                let accepted = view.accept(&intents, &result.query, result.result);
                let catalog = if accepted {
                    plan.commit_if_current(&intents, &view)
                } else {
                    None
                };
                if mode.starts_with("ack-") {
                    assert!(catalog.is_some());
                }
                match mode {
                    "ack-view" => view.close(),
                    "ack-owner" => {
                        view.begin(
                            &intents,
                            intents.ticket(),
                            "replacement",
                            AchievementTarget::Own,
                        )
                        .unwrap();
                    }
                    "ack-logout" => intents.request(CriticalAction::Logout),
                    "ack-cancel" => stop.cancel(),
                    _ => {}
                }
                let _ = acknowledge.send(catalog);
                None
            };
            let event = timeout(Duration::from_secs(3), &mut job.receiver)
                .await
                .unwrap()
                .unwrap();
            io.join(job.id).await.unwrap();
            io.finish().await.unwrap();
            drop(held_ack);
            assert!(io.jobs.lock().unwrap().is_empty());
            let UiEvent::App(Event::AchievementsStored { query, result }) = event else {
                panic!("original cleanup completion changed kind")
            };
            assert_eq!(query, original);
            let accepted = result.unwrap().is_some();
            assert_eq!(accepted, matches!(mode, "accept" | "leave"));
            if !accepted {
                assert_eq!(std::fs::read(&path).unwrap(), previous);
            }
            let stored = zabor_native::achievement_store::load(&profile, "owner").unwrap();
            let local = CriticalIntents::default();
            let (mut own, reply) = fixture(&local);
            assert!(own.accept(&local, &reply.query, Ok(Default::default())));
            let source = stored
                .prepare(&own)
                .unwrap()
                .unwrap()
                .prepared_json()
                .to_owned();
            assert_eq!(source.contains("hidden-one"), accepted);
            assert_eq!(
                zabor_native::achievement_store::load(&profile, "replacement").unwrap(),
                HiddenCatalog::empty("replacement").unwrap()
            );
            assert!(!std::fs::read_dir(root.path().join("achievements"))
                .unwrap()
                .any(|entry| entry
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "tmp" || extension == "journal")));
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_join_observer_keeps_original_handle_for_shutdown_rollback_and_drain() {
    timeout(Duration::from_secs(5), async {
        let root = tempfile::tempdir().unwrap();
        let profile = Arc::new(Profile::open(root.path()).unwrap());
        let intents = Arc::new(CriticalIntents::default());
        let stop = CancellationToken::new();
        let io = AchievementIo::unverified_transaction_fixture(
            Some(profile.clone()),
            intents.clone(),
            stop.clone(),
        );
        let (_, result) = fixture(&intents);
        let (updates, mut events) = mpsc::unbounded_channel();
        let job = io.start(
            result,
            Arc::new(move |event| {
                let _ = updates.send(event);
            }),
        );
        let proposal = events.recv().await.unwrap();
        let holder = io.jobs.lock().unwrap().get(&job.id).unwrap().clone();
        let joining = io.clone();
        let observer = tokio::spawn(async move { joining.join(job.id).await });
        wait_until_joining(&holder).await;
        observer.abort();
        assert!(observer.await.unwrap_err().is_cancelled());
        assert_eq!(io.jobs.lock().unwrap().len(), 1);
        stop.cancel();
        io.finish().await.unwrap();
        drop(proposal);
        assert!(io.jobs.lock().unwrap().is_empty());
        assert_eq!(
            zabor_native::achievement_store::load(&profile, "owner").unwrap(),
            HiddenCatalog::empty("owner").unwrap()
        );
        assert!(!std::fs::read_dir(root.path().join("achievements"))
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|extension| extension == "json"
                    || extension == "journal"
                    || extension == "tmp")));
    })
    .await
    .unwrap();
}

async fn wait_until_joining(holder: &BlockingJob) {
    timeout(Duration::from_secs(1), async {
        loop {
            if holder.try_lock().is_err() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_shutdown_observer_retains_handles_and_concurrent_join_drains_only_once() {
    timeout(Duration::from_secs(5), async {
        let root = tempfile::tempdir().unwrap();
        let profile = Arc::new(Profile::open(root.path()).unwrap());
        let intents = Arc::new(CriticalIntents::default());
        let stop = CancellationToken::new();
        let io = AchievementIo::unverified_transaction_fixture(
            Some(profile.clone()),
            intents.clone(),
            stop.clone(),
        );
        let (mut view, result) = fixture(&intents);
        let (updates, mut events) = mpsc::unbounded_channel();
        let mut job = io.start(
            result,
            Arc::new(move |event| {
                let _ = updates.send(event);
            }),
        );
        let proposal = events.recv().await.unwrap();
        let holder = io.jobs.lock().unwrap().get(&job.id).unwrap().clone();
        let finishing = io.clone();
        let observer = tokio::spawn(async move { finishing.finish().await });
        wait_until_joining(&holder).await;
        observer.abort();
        assert!(observer.await.unwrap_err().is_cancelled());
        assert_eq!(io.jobs.lock().unwrap().len(), 1);
        assert!(matches!(*holder.lock().await, BlockingState::Pending(_)));
        let joining = io.clone();
        let original = tokio::spawn(async move { joining.join(job.id).await });
        wait_until_joining(&holder).await;
        let finishing = io.clone();
        let shutdown = tokio::spawn(async move { finishing.finish().await });
        view.close();
        original.await.unwrap().unwrap();
        shutdown.await.unwrap().unwrap();
        assert!(matches!(*holder.lock().await, BlockingState::Done(true)));
        assert!(io.jobs.lock().unwrap().is_empty());
        assert!(matches!(
            job.receiver.try_recv().unwrap(),
            UiEvent::App(Event::AchievementsStored {
                result: Ok(None),
                ..
            })
        ));
        drop(proposal);
        assert_eq!(
            zabor_native::achievement_store::load(&profile, "owner").unwrap(),
            HiddenCatalog::empty("owner").unwrap()
        );
        assert!(!std::fs::read_dir(root.path().join("achievements"))
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|extension| extension == "json"
                    || extension == "journal"
                    || extension == "tmp")));
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discarded_ui_receiver_and_cancelled_observer_cannot_hide_failed_rollback_from_shutdown() {
    timeout(Duration::from_secs(8), async {
        for cancel_observer in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let profile = Arc::new(Profile::open(root.path()).unwrap());
            let intents = Arc::new(CriticalIntents::default());
            let stop = CancellationToken::new();
            let io = AchievementIo::unverified_transaction_fixture(
                Some(profile.clone()),
                intents.clone(),
                stop.clone(),
            );
            let (mut seed_view, seed) = fixture(&intents);
            assert!(seed_view.accept(&intents, &seed.query, Ok(Default::default())));
            let store = HiddenStore::open(&profile, "owner").unwrap();
            let plan = store.load().unwrap().prepare(&seed_view).unwrap().unwrap();
            let prepared = store.prepare(&plan).unwrap();
            store
                .replace_if(
                    prepared,
                    || true,
                    || plan.commit_if_current(&intents, &seed_view),
                )
                .unwrap()
                .unwrap();
            drop(store);
            let path = std::fs::read_dir(root.path().join("achievements"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == "json")
                })
                .unwrap();
            let previous = std::fs::read(&path).unwrap();
            let (_, result) = fixture(&intents);
            let (updates, mut events) = mpsc::unbounded_channel();
            let job = io.start(
                result,
                Arc::new(move |event| {
                    let _ = updates.send(event);
                }),
            );
            let proposal = events.recv().await.unwrap();
            let holder = io.jobs.lock().unwrap().get(&job.id).unwrap().clone();
            let id = job.id;
            drop(job.receiver);
            if cancel_observer {
                let joining = io.clone();
                let observer = tokio::spawn(async move { joining.join(id).await });
                wait_until_joining(&holder).await;
                observer.abort();
                assert!(observer.await.unwrap_err().is_cancelled());
            }
            std::fs::remove_file(&path).unwrap();
            std::fs::create_dir(&path).unwrap();
            stop.cancel();
            drop(proposal);
            assert!(io.join(id).await.is_err());
            assert!(matches!(*holder.lock().await, BlockingState::Done(false)));
            assert!(io.finish().await.is_err());
            assert!(io.jobs.lock().unwrap().is_empty());
            assert!(path.with_extension("journal").exists());
            assert!(zabor_native::achievement_store::load(&profile, "owner").is_err());
            std::fs::remove_dir(&path).unwrap();
            assert!(zabor_native::achievement_store::load(&profile, "owner").is_ok());
            assert_eq!(std::fs::read(&path).unwrap(), previous);
            assert!(!path.with_extension("journal").exists());
            assert!(io.finish().await.is_err());
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn production_constructor_without_metadata_ordering_preserves_history_and_never_proposes_success(
) {
    timeout(Duration::from_secs(5), async {
        let root = tempfile::tempdir().unwrap();
        let profile = Arc::new(Profile::open(root.path()).unwrap());
        let intents = Arc::new(CriticalIntents::default());
        let stop = CancellationToken::new();
        let store = HiddenStore::open(&profile, "owner").unwrap();
        let (mut seed, reply) = fixture(&intents);
        assert!(seed.accept(&intents, &reply.query, Ok(Default::default())));
        let plan = store.load().unwrap().prepare(&seed).unwrap().unwrap();
        let prepared = store.prepare(&plan).unwrap();
        store
            .replace_if(
                prepared,
                || true,
                || plan.commit_if_current(&intents, &seed),
            )
            .unwrap()
            .unwrap();
        drop(store);
        let path = std::fs::read_dir(root.path().join("achievements"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .unwrap();
        let previous = std::fs::read(&path).unwrap();
        let (_, reply) = fixture(&intents);
        let io = AchievementIo::new(Some(profile.clone()), intents.clone(), stop);
        let (updates, mut events) = mpsc::unbounded_channel();
        let job = io.start(
            reply,
            Arc::new(move |event| {
                let _ = updates.send(event);
            }),
        );
        let UiEvent::App(Event::Achievements(result)) = job.receiver.await.unwrap() else {
            panic!("missing metadata ordering fabricated a durable completion")
        };
        assert!(result.storage_failed);
        assert!(result.catalog.is_none());
        assert!(result.result.is_ok());
        assert!(events.try_recv().is_err());
        assert!(io.join(job.id).await.is_err());
        assert!(io.finish().await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), previous);
        assert!(!path.with_extension("journal").exists());
        let journal = path.with_extension("journal");
        std::fs::write(&journal, b"ZNH1\x00").unwrap();
        let (_, reply) = fixture(&intents);
        let job = io.start(
            reply,
            Arc::new(|_| panic!("unverified recovery must not propose UI success")),
        );
        let UiEvent::App(Event::Achievements(result)) = job.receiver.await.unwrap() else {
            panic!("unverified recovery fabricated durable completion")
        };
        assert!(result.storage_failed && result.catalog.is_none());
        assert!(io.join(job.id).await.is_err());
        assert!(io.finish().await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), previous);
        assert_eq!(std::fs::read(&journal).unwrap(), b"ZNH1\x00");
        assert!(!std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|extension| extension == "tmp")));
    })
    .await
    .unwrap();
}
