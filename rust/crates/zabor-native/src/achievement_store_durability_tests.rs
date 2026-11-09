use super::*;
use crate::achievements::{parse_result, AchievementTarget, AchievementsView};
use crate::critical_intents::{CriticalAction, CriticalIntents};
use rusqlite::{Connection, OpenFlags};
use serde_json::json;
use std::cell::RefCell;
use std::os::windows::fs::OpenOptionsExt;
use std::sync::{mpsc, Arc};
use std::time::Duration;

#[path = "achievement_process_fixture.rs"]
mod process_fixture;

fn owner_path(root: &Path, owner: &str) -> PathBuf {
    let directory = root.join("achievements");
    fs::create_dir_all(&directory).unwrap();
    directory.join(format!("{:x}.json", Sha256::digest(owner.as_bytes())))
}

fn previous() -> Vec<u8> {
    format!("\r\n{}\r\n", json!({"old-hidden":{"id":"old-hidden","icon":"x","maxValue":1,"statKey":"oldHidden","titleRu":"r","descriptionRu":"r","titleEn":"e","descriptionEn":"e"}})).into_bytes()
}

fn plan(store: &HiddenStore, intents: &CriticalIntents) -> (AchievementsView, HiddenPersistence) {
    let mut view = AchievementsView::default();
    let query = view
        .begin(intents, intents.ticket(), "owner", AchievementTarget::Own)
        .unwrap();
    let data = parse_result(&json!(json!({"unlockedHiddenDefs":[{"id":"fresh-hidden","icon":"x","maxValue":1,"statKey":"freshHidden","titleRu":"r","descriptionRu":"r","titleEn":"e","descriptionEn":"e"}]}).to_string()));
    assert!(view.accept(intents, &query, data));
    let plan = store.load().unwrap().prepare(&view).unwrap().unwrap();
    (view, plan)
}

fn database_state(path: &Path) -> (Option<Vec<u8>>, i64) {
    let reader = Connection::open_with_flags(
        path.with_extension("sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    reader
        .query_row(
            "SELECT payload,pending FROM hidden_state WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

#[test]
fn hidden_durability_exact_legacy_import_and_authority_corruption_never_clobber_history() {
    for kind in ["plain", "legacy-bytes", "legacy-absence"] {
        let root = tempfile::tempdir().unwrap();
        let path = owner_path(root.path(), "owner");
        let previous = previous();
        let expected = if kind == "legacy-absence" {
            None
        } else {
            Some(previous.clone())
        };
        if kind == "plain" {
            fs::write(&path, &previous).unwrap();
        } else {
            fs::write(&path, b"{}").unwrap();
            let mut journal = b"ZNH1".to_vec();
            journal.push(u8::from(expected.is_some()));
            if let Some(expected) = &expected {
                journal.extend_from_slice(expected);
            }
            fs::write(path.with_extension("journal"), journal).unwrap();
        }
        let profile = Profile::open(root.path()).unwrap();
        let store = HiddenStore::open(&profile, "owner").unwrap();
        assert_eq!(fs::read(&path).ok(), expected);
        assert_eq!(database_state(&path), (expected.clone(), 0));
        assert!(!path.with_extension("journal").exists());
        let connection = store.authority.connection();
        for (name, expected) in [
            ("synchronous", 3_i64),
            ("journal_size_limit", -1),
            ("mmap_size", 0),
            ("max_page_count", 512),
        ] {
            assert_eq!(
                connection
                    .pragma_query_value(None, name, |row| row.get::<_, i64>(0))
                    .unwrap(),
                expected
            );
        }
        assert_eq!(
            connection
                .pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
                .unwrap(),
            "persist"
        );
        assert_eq!(
            connection
                .pragma_query_value(None, "locking_mode", |row| row.get::<_, String>(0))
                .unwrap(),
            "normal"
        );
        assert_eq!(rusqlite::version_number(), 3_050_002);
        let intents = CriticalIntents::default();
        let (view, plan) = plan(&store, &intents);
        let prepared = store.prepare(&plan).unwrap();
        let candidate = plan.prepared_json().as_bytes().to_vec();
        assert!(store
            .replace_if(
                prepared,
                || true,
                || plan.commit_if_current(&intents, &view)
            )
            .unwrap()
            .is_some());
        drop(store);
        assert_eq!(database_state(&path), (Some(candidate.clone()), 0));
        fs::write(&path, b"{}").unwrap();
        fs::write(path.with_extension("journal"), b"ZNH1\x00").unwrap();
        let store = HiddenStore::open(&profile, "owner").unwrap();
        assert_eq!(fs::read(&path).unwrap(), candidate);
        assert!(!path.with_extension("journal").exists());
        drop(store);
        let copied = owner_path(root.path(), "other-owner");
        fs::copy(
            path.with_extension("sqlite3"),
            copied.with_extension("sqlite3"),
        )
        .unwrap();
        assert!(HiddenStore::open(&profile, "other-owner").is_err());
        assert_eq!(fs::read(&path).unwrap(), candidate);
        fs::write(path.with_extension("sqlite3"), b"not-sqlite").unwrap();
        fs::write(path.with_extension("journal"), b"ZNH1\x00").unwrap();
        assert!(HiddenStore::open(&profile, "owner").is_err());
        assert_eq!(fs::read(&path).unwrap(), candidate);
        assert_eq!(
            fs::read(path.with_extension("journal")).unwrap(),
            b"ZNH1\x00"
        );
    }
    for corrupt in [b"bad".as_slice(), b"ZNH1\x01not-json", b"ZNH1\x00trailing"] {
        let root = tempfile::tempdir().unwrap();
        let path = owner_path(root.path(), "owner");
        let previous = previous();
        fs::write(&path, &previous).unwrap();
        fs::write(path.with_extension("journal"), corrupt).unwrap();
        let profile = Profile::open(root.path()).unwrap();
        assert!(HiddenStore::open(&profile, "owner").is_err());
        assert_eq!(fs::read(&path).unwrap(), previous);
        assert_eq!(fs::read(path.with_extension("journal")).unwrap(), corrupt);
        assert!(!path.with_extension("sqlite3").exists());
    }
}

#[test]
fn hidden_durability_changed_pragma_contract_is_rejected_before_publication() {
    for (name, value) in [
        ("synchronous", "0"),
        ("journal_size_limit", "0"),
        ("mmap_size", "4096"),
        ("trusted_schema", "1"),
        ("locking_mode", "EXCLUSIVE"),
        ("journal_mode", "DELETE"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let path = owner_path(root.path(), "owner");
        let previous = previous();
        fs::write(&path, &previous).unwrap();
        let profile = Profile::open(root.path()).unwrap();
        let store = HiddenStore::open(&profile, "owner").unwrap();
        let intents = CriticalIntents::default();
        let (_, plan) = plan(&store, &intents);
        let prepared = store.prepare(&plan).unwrap();
        store
            .authority
            .connection()
            .pragma_update(None, name, value)
            .unwrap();
        assert!(store
            .replace_if::<()>(
                prepared,
                || panic!("unsupported contract reached admission"),
                || panic!("unsupported contract reached acceptance")
            )
            .is_err());
        assert!(store.load().is_err());
        assert_eq!(fs::read(&path).unwrap(), previous);
        assert!(!path.with_extension("journal").exists());
        drop(store);
        assert!(HiddenStore::open(&profile, "owner").is_ok());
        assert_eq!(fs::read(&path).unwrap(), previous);
    }
}

#[test]
fn hidden_durability_owner_lock_hides_provisional_bytes_without_holding_intent_gates() {
    let root = tempfile::tempdir().unwrap();
    let path = owner_path(root.path(), "owner");
    let previous = previous();
    fs::write(&path, &previous).unwrap();
    let profile = Arc::new(Profile::open(root.path()).unwrap());
    let intents = Arc::new(CriticalIntents::default());
    let (entered, held) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let writer_profile = profile.clone();
    let writer_intents = intents.clone();
    let writer = std::thread::spawn(move || {
        let store = HiddenStore::open(&writer_profile, "owner").unwrap();
        let (view, plan) = plan(&store, &writer_intents);
        let prepared = store.prepare(&plan).unwrap();
        store
            .replace_if(
                prepared,
                || true,
                || {
                    assert!(store.load().is_err());
                    entered.send(()).unwrap();
                    released.recv_timeout(Duration::from_secs(5)).unwrap();
                    plan.commit_if_current(&writer_intents, &view)
                },
            )
            .unwrap()
    });
    held.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(database_state(&path), (Some(previous.clone()), 1));
    assert!(std::str::from_utf8(&fs::read(&path).unwrap())
        .unwrap()
        .contains("fresh-hidden"));
    let (loaded, received) = mpsc::channel();
    let reader_profile = profile.clone();
    let reader = std::thread::spawn(move || {
        loaded.send(load(&reader_profile, "owner")).unwrap();
    });
    assert!(received.recv_timeout(Duration::from_millis(50)).is_err());
    let controls = intents.clone();
    let (revoked, revocation) = mpsc::channel();
    let revoker = std::thread::spawn(move || {
        controls.request(CriticalAction::Logout);
        revoked.send(()).unwrap();
    });
    let outside_gate = revocation.recv_timeout(Duration::from_millis(500)).is_ok();
    release.send(()).unwrap();
    revoker.join().unwrap();
    assert!(outside_gate);
    assert!(writer.join().unwrap().is_none());
    assert!(received
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .is_ok());
    reader.join().unwrap();
    assert_eq!(fs::read(&path).unwrap(), previous);
    assert_eq!(database_state(&path), (Some(previous), 0));
}

#[test]
fn hidden_durability_native_sharing_and_commit_failures_keep_exact_rollback_and_fail_closed() {
    for existing in [false, true] {
        for mode in ["sharing", "directory", "commit"] {
            let root = tempfile::tempdir().unwrap();
            let path = owner_path(root.path(), "owner");
            let previous = existing.then(previous);
            if let Some(previous) = &previous {
                fs::write(&path, previous).unwrap();
            }
            let profile = Profile::open(root.path()).unwrap();
            let store = HiddenStore::open(&profile, "owner").unwrap();
            let intents = CriticalIntents::default();
            let (mut view, plan) = plan(&store, &intents);
            let prepared = store.prepare(&plan).unwrap();
            let sharing = if mode == "sharing" {
                if !existing {
                    fs::write(&path, b"{}").unwrap();
                }
                Some(
                    OpenOptions::new()
                        .read(true)
                        .share_mode(3)
                        .open(&path)
                        .unwrap(),
                )
            } else {
                None
            };
            let reader = RefCell::new(None);
            let outcome = store.replace_if(
                prepared,
                || true,
                || {
                    if mode == "directory" {
                        fs::remove_file(&path).unwrap();
                        fs::create_dir(&path).unwrap();
                        view.close();
                    }
                    if mode == "commit" {
                        let connection = Connection::open_with_flags(
                            path.with_extension("sqlite3"),
                            OpenFlags::SQLITE_OPEN_READ_ONLY,
                        )
                        .unwrap();
                        connection.execute_batch("BEGIN").unwrap();
                        connection
                            .query_row("SELECT pending FROM hidden_state WHERE id=1", [], |row| {
                                row.get::<_, i64>(0)
                            })
                            .unwrap();
                        *reader.borrow_mut() = Some(connection);
                    }
                    plan.commit_if_current(&intents, &view)
                },
            );
            assert!(outcome.is_err());
            assert!(store.load().is_err());
            drop(sharing);
            drop(reader);
            drop(store);
            if mode == "directory" {
                assert!(load(&profile, "owner").is_err());
                fs::remove_dir(&path).unwrap();
            }
            if mode == "sharing" && !existing {
                fs::remove_file(&path).unwrap();
            }
            assert!(load(&profile, "owner").is_ok());
            assert_eq!(fs::read(&path).ok(), previous);
            assert_eq!(database_state(&path), (previous, 0));
            assert!(!path.with_extension("journal").exists());
            assert!(!fs::read_dir(path.parent().unwrap())
                .unwrap()
                .any(|entry| entry
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "tmp")));
        }
    }
}

#[test]
fn hidden_durability_process_child() {
    let Some(root) = std::env::var_os(process_fixture::ROOT) else {
        return;
    };
    let stage = std::env::var(process_fixture::STAGE).unwrap();
    let accepted = std::env::var(process_fixture::ACCEPT).unwrap() == "1";
    let profile = Profile::open(Path::new(&root)).unwrap();
    let mut store = HiddenStore::open(&profile, "owner").unwrap();
    let target = stage.clone();
    store.checkpoint = Some(Box::new(move |observed| {
        if target == observed {
            process_fixture::checkpoint(observed);
        }
    }));
    let intents = CriticalIntents::default();
    let (mut view, plan) = plan(&store, &intents);
    let prepared = store.prepare(&plan).unwrap();
    let result = store
        .replace_if(
            prepared,
            || true,
            || {
                if stage == "proposed" {
                    process_fixture::checkpoint("proposed");
                }
                if !accepted {
                    view.close();
                }
                plan.commit_if_current(&intents, &view)
            },
        )
        .unwrap();
    assert_eq!(result.is_some(), accepted);
}

#[test]
fn hidden_durability_process_termination_recovers_original_bytes_or_committed_authority() {
    for existing in [false, true] {
        for (stage, accepted) in [
            ("rollback-record", true),
            ("proposed", true),
            ("compatibility-finalized", true),
            ("compatibility-finalized", false),
            ("authority-finalized", true),
            ("rollback-finalized", false),
        ] {
            let root = tempfile::tempdir().unwrap();
            let path = owner_path(root.path(), "owner");
            let previous = existing.then(previous);
            if let Some(previous) = &previous {
                fs::write(&path, previous).unwrap();
            }
            let process = process_fixture::Process::start(
                "achievement_store::durability_tests::hidden_durability_process_child",
                root.path(),
                stage,
                accepted,
            );
            process.checkpoint(stage);
            let (committed, pending) = database_state(&path);
            assert_eq!(
                pending,
                i64::from(!matches!(
                    stage,
                    "authority-finalized" | "rollback-finalized"
                ))
            );
            if stage != "authority-finalized" {
                assert_eq!(committed, previous);
            }
            process.terminate();
            if stage == "authority-finalized" {
                fs::write(&path, b"{}").unwrap();
                fs::write(path.with_extension("journal"), b"ZNH1\x00").unwrap();
            } else if path.with_extension("journal").exists() {
                fs::remove_file(path.with_extension("journal")).unwrap();
            }
            let profile = Profile::open(root.path()).unwrap();
            let store = HiddenStore::open(&profile, "owner").unwrap();
            assert!(store.load().is_ok());
            let expected = if stage == "authority-finalized" {
                committed
            } else {
                previous
            };
            assert_eq!(fs::read(&path).ok(), expected);
            assert_eq!(database_state(&path), (expected, 0));
            assert!(!path.with_extension("journal").exists());
            drop(store);
            drop(profile);
            root.close().unwrap();
            println!("hidden durability process recovery stage={stage} accepted={accepted} existing={existing} exact_previous_or_commit=true compatibility_not_authority=true");
        }
    }
}
