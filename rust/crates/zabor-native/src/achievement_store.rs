use crate::achievements::{HiddenCatalog, HiddenPersistence, MAX_PAYLOAD_BYTES};
use crate::profile::{Profile, ProfileError};
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::cell::Cell;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[path = "achievement_authority.rs"]
mod authority;

use authority::{Authority, State};

fn checked(path: &Path) -> Result<(), ProfileError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err(ProfileError::Storage);
                }
            }
            if metadata.file_type().is_symlink() {
                return Err(ProfileError::Storage);
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ProfileError::Storage),
    }
}

fn destination(profile: &Profile, owner: &str) -> Result<PathBuf, ProfileError> {
    HiddenCatalog::empty(owner).map_err(|_| ProfileError::Storage)?;
    let directory = profile.root().join("achievements");
    checked(&directory)?;
    fs::create_dir_all(&directory).map_err(|_| ProfileError::Storage)?;
    let path = directory.join(format!("{:x}.json", Sha256::digest(owner.as_bytes())));
    checked(&path)?;
    Ok(path)
}

pub fn load(profile: &Profile, owner: &str) -> Result<HiddenCatalog, ProfileError> {
    HiddenStore::open(profile, owner)?.load()
}

#[cfg(test)]
type Checkpoint = Box<dyn Fn(&str) + Send>;

pub struct HiddenStore {
    authority: Authority,
    compatibility: CompatibilityStore,
    failed: Cell<bool>,
    #[cfg(test)]
    checkpoint: Option<Checkpoint>,
}

struct CompatibilityStore {
    _lock: File,
    destination: PathBuf,
    journal: PathBuf,
    owner: String,
}

fn read_limited(path: &Path, limit: usize) -> Result<Option<Vec<u8>>, ProfileError> {
    checked(path)?;
    let mut bytes = Vec::new();
    match File::open(path) {
        Ok(file) => {
            file.take(limit as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| ProfileError::Storage)?;
            if bytes.len() > limit {
                return Err(ProfileError::Storage);
            }
            Ok(Some(bytes))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(ProfileError::Storage),
    }
}

fn remove(path: &Path) -> Result<(), ProfileError> {
    checked(path)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ProfileError::Storage),
    }
}

fn prepare_file(path: &Path, bytes: &[u8]) -> Result<tempfile::TempPath, ProfileError> {
    checked(path)?;
    let mut file = tempfile::Builder::new()
        .prefix(".hidden.")
        .suffix(".tmp")
        .tempfile_in(path.parent().ok_or(ProfileError::Storage)?)
        .map_err(|_| ProfileError::Storage)?;
    file.write_all(bytes).map_err(|_| ProfileError::Storage)?;
    file.as_file()
        .sync_all()
        .map_err(|_| ProfileError::Storage)?;
    Ok(file.into_temp_path())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ProfileError> {
    prepare_file(path, bytes)?
        .persist(path)
        .map_err(|_| ProfileError::Storage)
}

fn validate_bytes(owner: &str, bytes: Option<&[u8]>) -> Result<HiddenCatalog, ProfileError> {
    match bytes {
        Some(bytes) => HiddenCatalog::from_durable(
            owner,
            std::str::from_utf8(bytes).map_err(|_| ProfileError::Storage)?,
        )
        .map_err(|_| ProfileError::Storage),
        None => HiddenCatalog::empty(owner).map_err(|_| ProfileError::Storage),
    }
}

impl HiddenStore {
    pub fn open(profile: &Profile, owner: &str) -> Result<Self, ProfileError> {
        let compatibility = CompatibilityStore::lock(profile, owner)?;
        let database = compatibility.destination.with_extension("sqlite3");
        let bootstrap = match fs::symlink_metadata(&database) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Some(compatibility.legacy_state()?)
            }
            Ok(_) => None,
            Err(_) => return Err(ProfileError::Storage),
        };
        let authority = Authority::open(&database, owner, || match bootstrap {
            Some(state) => Ok(state),
            None => compatibility.legacy_state(),
        })?;
        let state = authority.state()?;
        let store = Self {
            authority,
            compatibility,
            failed: Cell::new(false),
            #[cfg(test)]
            checkpoint: None,
        };
        checked(&store.compatibility.journal)?;
        let journal = match fs::symlink_metadata(&store.compatibility.journal) {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(_) => return Err(ProfileError::Storage),
        };
        let mirror = read_limited(&store.compatibility.destination, MAX_PAYLOAD_BYTES);
        if state.pending || journal || !matches!(mirror, Ok(ref bytes) if *bytes == state.payload) {
            store.compatibility.restore(state.payload.as_deref())?;
            if state.pending {
                store.authority.finish(state.payload.as_deref())?;
            }
        }
        Ok(store)
    }

    pub fn load(&self) -> Result<HiddenCatalog, ProfileError> {
        if self.failed.get() {
            return Err(ProfileError::Storage);
        }
        let state = self.authority.state()?;
        if state.pending {
            return Err(ProfileError::Storage);
        }
        validate_bytes(&self.compatibility.owner, state.payload.as_deref())
    }

    pub fn prepare(&self, plan: &HiddenPersistence) -> Result<PreparedHidden, ProfileError> {
        self.load()?;
        self.compatibility.prepare(plan)
    }

    pub fn replace_if<T>(
        &self,
        prepared: PreparedHidden,
        current: impl FnOnce() -> bool,
        accept: impl FnOnce() -> Option<T>,
    ) -> Result<Option<T>, ProfileError> {
        let completed = (|| {
            if self.failed.get() || prepared.destination != self.compatibility.destination {
                return Err(ProfileError::Storage);
            }
            let state = self.authority.state()?;
            if state.pending
                || read_limited(&self.compatibility.destination, MAX_PAYLOAD_BYTES)?
                    != state.payload
            {
                return Err(ProfileError::Storage);
            }
            let candidate = read_limited(&prepared.temporary, MAX_PAYLOAD_BYTES)?
                .ok_or(ProfileError::Storage)?;
            validate_bytes(&self.compatibility.owner, Some(&candidate))?;
            if !current() {
                return Ok(None);
            }
            self.authority.begin()?;
            #[cfg(test)]
            self.observe("rollback-record");
            let accepted = self.compatibility.replace_if(prepared, || true, accept)?;
            #[cfg(test)]
            self.observe("compatibility-finalized");
            if accepted.is_some() {
                if self.authority.finish(Some(&candidate)).is_err() {
                    let restored = self.authority.restore(state.payload.as_deref());
                    let mirror = self.compatibility.restore(state.payload.as_deref());
                    if restored.is_ok() && mirror.is_ok() {
                        let _ = self.authority.finish(state.payload.as_deref());
                    }
                    return Err(ProfileError::Storage);
                }
                #[cfg(test)]
                self.observe("authority-finalized");
            } else {
                self.authority.finish(state.payload.as_deref())?;
                #[cfg(test)]
                self.observe("rollback-finalized");
            }
            Ok(accepted)
        })();
        if completed.is_err() {
            self.failed.set(true);
        }
        completed
    }

    #[cfg(test)]
    fn observe(&self, stage: &str) {
        if let Some(checkpoint) = &self.checkpoint {
            checkpoint(stage);
        }
    }
}

impl CompatibilityStore {
    #[cfg(test)]
    fn open(profile: &Profile, owner: &str) -> Result<Self, ProfileError> {
        let store = Self::lock(profile, owner)?;
        store.recover()?;
        Ok(store)
    }

    fn lock(profile: &Profile, owner: &str) -> Result<Self, ProfileError> {
        let destination = destination(profile, owner)?;
        let lock = destination.with_extension("lock");
        checked(&lock)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock)
            .map_err(|_| ProfileError::Storage)?;
        lock.lock_exclusive().map_err(|_| ProfileError::Storage)?;
        let store = Self {
            journal: destination.with_extension("journal"),
            destination,
            owner: owner.into(),
            _lock: lock,
        };
        Ok(store)
    }

    fn legacy_state(&self) -> Result<State, ProfileError> {
        let journal = read_limited(&self.journal, MAX_PAYLOAD_BYTES + 5)?;
        let pending = journal.is_some();
        let payload = match journal {
            Some(journal) => {
                if journal.len() < 5 || &journal[..4] != b"ZNH1" {
                    return Err(ProfileError::Storage);
                }
                match journal[4] {
                    0 if journal.len() == 5 => None,
                    1 => Some(journal[5..].to_vec()),
                    _ => return Err(ProfileError::Storage),
                }
            }
            None => read_limited(&self.destination, MAX_PAYLOAD_BYTES)?,
        };
        validate_bytes(&self.owner, payload.as_deref())?;
        Ok(State { payload, pending })
    }

    #[cfg(test)]
    pub fn load(&self) -> Result<HiddenCatalog, ProfileError> {
        if read_limited(&self.journal, MAX_PAYLOAD_BYTES + 5)?.is_some() {
            return Err(ProfileError::Storage);
        }
        match read_limited(&self.destination, MAX_PAYLOAD_BYTES)? {
            Some(bytes) => HiddenCatalog::from_durable(
                &self.owner,
                std::str::from_utf8(&bytes).map_err(|_| ProfileError::Storage)?,
            )
            .map_err(|_| ProfileError::Storage),
            None => HiddenCatalog::empty(&self.owner).map_err(|_| ProfileError::Storage),
        }
    }

    pub fn prepare(&self, plan: &HiddenPersistence) -> Result<PreparedHidden, ProfileError> {
        if plan.owner() != self.owner {
            return Err(ProfileError::Storage);
        }
        Ok(PreparedHidden {
            temporary: prepare_file(&self.destination, plan.prepared_json().as_bytes())?,
            destination: self.destination.clone(),
        })
    }

    fn restore(&self, previous: Option<&[u8]>) -> Result<(), ProfileError> {
        match previous {
            Some(bytes) => write_atomic(&self.destination, bytes),
            None => remove(&self.destination),
        }?;
        remove(&self.journal)
    }

    #[cfg(test)]
    fn recover(&self) -> Result<(), ProfileError> {
        let Some(journal) = read_limited(&self.journal, MAX_PAYLOAD_BYTES + 5)? else {
            return Ok(());
        };
        if journal.len() < 5 || &journal[..4] != b"ZNH1" {
            return Err(ProfileError::Storage);
        }
        let previous = match journal[4] {
            0 if journal.len() == 5 => None,
            1 => {
                HiddenCatalog::from_durable(
                    &self.owner,
                    std::str::from_utf8(&journal[5..]).map_err(|_| ProfileError::Storage)?,
                )
                .map_err(|_| ProfileError::Storage)?;
                Some(&journal[5..])
            }
            _ => return Err(ProfileError::Storage),
        };
        self.restore(previous)
    }

    pub fn replace_if<T>(
        &self,
        prepared: PreparedHidden,
        current: impl FnOnce() -> bool,
        accept: impl FnOnce() -> Option<T>,
    ) -> Result<Option<T>, ProfileError> {
        if prepared.destination != self.destination
            || read_limited(&self.journal, MAX_PAYLOAD_BYTES + 5)?.is_some()
        {
            return Err(ProfileError::Storage);
        }
        let previous = read_limited(&self.destination, MAX_PAYLOAD_BYTES)?;
        if let Some(bytes) = &previous {
            HiddenCatalog::from_durable(
                &self.owner,
                std::str::from_utf8(bytes).map_err(|_| ProfileError::Storage)?,
            )
            .map_err(|_| ProfileError::Storage)?;
        }
        if !current() {
            return Ok(None);
        }
        let mut journal = b"ZNH1".to_vec();
        journal.push(u8::from(previous.is_some()));
        if let Some(bytes) = &previous {
            journal.extend_from_slice(bytes);
        }
        write_atomic(&self.journal, &journal)?;
        checked(&self.destination)?;
        if prepared.temporary.persist(&self.destination).is_err() {
            self.restore(previous.as_deref())?;
            return Err(ProfileError::Storage);
        }
        let accepted = accept();
        if accepted.is_none() {
            self.restore(previous.as_deref())?;
            return Ok(None);
        }
        if remove(&self.journal).is_err() {
            self.restore(previous.as_deref())?;
            return Err(ProfileError::Storage);
        }
        Ok(accepted)
    }
}

pub struct PreparedHidden {
    temporary: tempfile::TempPath,
    destination: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::achievements::{parse_result, AchievementTarget, AchievementsView};
    use crate::critical_intents::{CriticalAction, CriticalIntents};
    use serde_json::json;

    #[test]
    fn own_store_is_durable_isolated_and_stale_plan_cleans_up() {
        let root = tempfile::tempdir().unwrap();
        let profile = Profile::open(root.path()).unwrap();
        let intents = CriticalIntents::default();
        let mut view = AchievementsView::default();
        let query = view
            .begin(
                &intents,
                intents.ticket(),
                "owner-a",
                AchievementTarget::Own,
            )
            .unwrap();
        let data = parse_result(&json!(json!({"unlockedHiddenDefs":[{"id":"hidden-one","icon":"x","maxValue":1,"statKey":"hiddenStat","titleRu":"r","descriptionRu":"r","titleEn":"e","descriptionEn":"e"}]}).to_string())).unwrap();
        assert!(view.accept(&intents, &query, Ok(data)));
        let store = HiddenStore::open(&profile, "owner-a").unwrap();
        let plan = store.load().unwrap().prepare(&view).unwrap().unwrap();
        let prepared = store.prepare(&plan).unwrap();
        assert!(store
            .replace_if(
                prepared,
                || true,
                || plan.commit_if_current(&intents, &view)
            )
            .unwrap()
            .is_some());
        drop(store);
        let reloaded = load(&profile, "owner-a").unwrap();
        assert!(reloaded
            .prepare(&view)
            .unwrap()
            .unwrap()
            .prepared_json()
            .contains("hidden-one"));
        assert!(!load(&profile, "owner-b")
            .unwrap()
            .prepare(&view)
            .unwrap()
            .is_some());
        let plan = reloaded.prepare(&view).unwrap().unwrap();
        let store = HiddenStore::open(&profile, "owner-a").unwrap();
        let prepared = store.prepare(&plan).unwrap();
        let temporary = prepared.temporary.to_path_buf();
        intents.request(CriticalAction::Logout);
        assert!(store
            .replace_if(
                prepared,
                || true,
                || plan.commit_if_current(&intents, &view)
            )
            .unwrap()
            .is_none());
        drop(store);
        assert!(!temporary.exists());
        assert_eq!(
            fs::read_dir(root.path().join("achievements"))
                .unwrap()
                .filter(|entry| entry
                    .as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "json"))
                .count(),
            1
        );
    }

    #[test]
    fn replaced_view_and_failed_replace_leave_previous_owner_bytes_and_no_temporary_files() {
        let root = tempfile::tempdir().unwrap();
        let profile = Profile::open(root.path()).unwrap();
        let intents = CriticalIntents::default();
        let mut view = AchievementsView::default();
        let query = view
            .begin(&intents, intents.ticket(), "owner", AchievementTarget::Own)
            .unwrap();
        assert!(view.accept(&intents, &query, Ok(Default::default())));
        let store = HiddenStore::open(&profile, "owner").unwrap();
        let catalog = store.load().unwrap();
        let plan = catalog.prepare(&view).unwrap().unwrap();
        let prepared = store.prepare(&plan).unwrap();
        store
            .replace_if(
                prepared,
                || true,
                || plan.commit_if_current(&intents, &view),
            )
            .unwrap()
            .unwrap();
        let path = destination(&profile, "owner").unwrap();
        let original = fs::read(&path).unwrap();
        let plan = catalog.prepare(&view).unwrap().unwrap();
        let prepared = store.prepare(&plan).unwrap();
        let temporary = prepared.temporary.to_path_buf();
        view.begin(
            &intents,
            intents.ticket(),
            "owner",
            AchievementTarget::Peer("peer".into()),
        )
        .unwrap();
        assert!(store
            .replace_if(
                prepared,
                || true,
                || plan.commit_if_current(&intents, &view)
            )
            .unwrap()
            .is_none());
        assert!(!temporary.exists());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(catalog.prepare(&view).unwrap().is_none());
        let query = view
            .begin(&intents, intents.ticket(), "owner", AchievementTarget::Own)
            .unwrap();
        assert!(view.accept(&intents, &query, Ok(Default::default())));
        let plan = catalog.prepare(&view).unwrap().unwrap();
        let mut prepared = store.prepare(&plan).unwrap();
        let temporary = prepared.temporary.to_path_buf();
        prepared.destination = root.path().join("missing").join("owner.json");
        assert!(store
            .replace_if(
                prepared,
                || true,
                || plan.commit_if_current(&intents, &view)
            )
            .is_err());
        assert!(!temporary.exists());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(
            fs::read_dir(root.path().join("achievements"))
                .unwrap()
                .filter(|entry| entry
                    .as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "json"))
                .count(),
            1
        );
    }

    #[test]
    fn oversized_corrupt_or_directory_payload_is_rejected_without_rewriting_it() {
        let root = tempfile::tempdir().unwrap();
        let profile = Profile::open(root.path()).unwrap();
        let path = destination(&profile, "owner").unwrap();
        for payload in ["{".into(), " ".repeat(MAX_PAYLOAD_BYTES + 1)] {
            fs::write(&path, payload.as_bytes()).unwrap();
            assert!(load(&profile, "owner").is_err());
            assert_eq!(fs::read(&path).unwrap(), payload.as_bytes());
        }
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(load(&profile, "owner").is_err());
    }

    #[test]
    fn provisional_bytes_block_readers_and_logout_rolls_back_before_next_owner_writer() {
        use std::sync::{mpsc, Arc};
        use std::time::Duration;
        let root = tempfile::tempdir().unwrap();
        let profile = Arc::new(Profile::open(root.path()).unwrap());
        let path = destination(&profile, "owner").unwrap();
        let original = b"{\n}\n";
        fs::write(&path, original).unwrap();
        let intents = Arc::new(CriticalIntents::default());
        let mut view = AchievementsView::default();
        let query = view
            .begin(&intents, intents.ticket(), "owner", AchievementTarget::Own)
            .unwrap();
        assert!(view.accept(&intents, &query, Ok(Default::default())));
        let (entered, held) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let writer_profile = profile.clone();
        let writer_intents = intents.clone();
        let writer = std::thread::spawn(move || {
            let store = HiddenStore::open(&writer_profile, "owner").unwrap();
            let plan = store.load().unwrap().prepare(&view).unwrap().unwrap();
            let prepared = store.prepare(&plan).unwrap();
            store
                .replace_if(
                    prepared,
                    || true,
                    || {
                        assert!(store.load().is_err());
                        entered.send(()).unwrap();
                        released.recv_timeout(Duration::from_secs(3)).unwrap();
                        plan.commit_if_current(&writer_intents, &view)
                    },
                )
                .unwrap()
        });
        held.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{}");
        let reader_profile = profile.clone();
        let (read_done, read_result) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            read_done.send(load(&reader_profile, "owner")).unwrap();
        });
        assert!(read_result.recv_timeout(Duration::from_millis(50)).is_err());
        let revocation = intents.clone();
        let (revoked, observed) = mpsc::channel();
        let request = std::thread::spawn(move || {
            revocation.request(CriticalAction::Logout);
            revoked.send(()).unwrap();
        });
        let outside_gate = observed.recv_timeout(Duration::from_millis(500)).is_ok();
        release.send(()).unwrap();
        request.join().unwrap();
        assert!(outside_gate);
        assert!(writer.join().unwrap().is_none());
        assert_eq!(
            read_result
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
                .unwrap(),
            HiddenCatalog::empty("owner").unwrap()
        );
        reader.join().unwrap();
        assert_eq!(fs::read(&path).unwrap(), original);
        let next_intents = CriticalIntents::default();
        let mut next_view = AchievementsView::default();
        let query = next_view
            .begin(
                &next_intents,
                next_intents.ticket(),
                "owner",
                AchievementTarget::Own,
            )
            .unwrap();
        assert!(next_view.accept(&next_intents, &query, Ok(Default::default())));
        let store = HiddenStore::open(&profile, "owner").unwrap();
        let plan = store.load().unwrap().prepare(&next_view).unwrap().unwrap();
        let prepared = store.prepare(&plan).unwrap();
        assert!(store
            .replace_if(
                prepared,
                || true,
                || plan.commit_if_current(&next_intents, &next_view)
            )
            .unwrap()
            .is_some());
        assert_eq!(fs::read(&path).unwrap(), b"{}");
        assert!(!path.with_extension("journal").exists());
    }

    #[test]
    fn interrupted_publication_recovers_exact_previous_bytes_or_absence_and_corrupt_journals_fail_closed(
    ) {
        let load = |profile: &Profile, owner: &str| -> Result<HiddenCatalog, ProfileError> {
            CompatibilityStore::open(profile, owner)?.load()
        };
        let root = tempfile::tempdir().unwrap();
        let profile = Profile::open(root.path()).unwrap();
        let path = destination(&profile, "owner").unwrap();
        let journal = path.with_extension("journal");
        let original = b"{\r\n}\r\n";
        let mut interrupted = b"ZNH1\x01".to_vec();
        interrupted.extend_from_slice(original);
        fs::write(&journal, &interrupted).unwrap();
        fs::write(&path, b"{}").unwrap();
        assert!(load(&profile, "owner").is_ok());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!journal.exists());
        fs::write(&journal, b"ZNH1\x00").unwrap();
        fs::write(&path, b"{}").unwrap();
        assert_eq!(
            load(&profile, "owner").unwrap(),
            HiddenCatalog::empty("owner").unwrap()
        );
        assert!(!path.exists());
        for corrupt in [b"bad".as_slice(), b"ZNH1\x01not-json", b"ZNH1\x00trailing"] {
            fs::write(&journal, corrupt).unwrap();
            fs::write(&path, b"{}").unwrap();
            assert!(load(&profile, "owner").is_err());
            assert_eq!(fs::read(&journal).unwrap(), corrupt);
            assert_eq!(fs::read(&path).unwrap(), b"{}");
        }
    }

    #[test]
    fn rollback_failure_retains_journal_and_never_exposes_unresolved_publication() {
        let root = tempfile::tempdir().unwrap();
        let profile = Profile::open(root.path()).unwrap();
        let path = destination(&profile, "owner").unwrap();
        let original = b"{\n}\n";
        fs::write(&path, original).unwrap();
        let intents = CriticalIntents::default();
        let mut view = AchievementsView::default();
        let query = view
            .begin(&intents, intents.ticket(), "owner", AchievementTarget::Own)
            .unwrap();
        assert!(view.accept(&intents, &query, Ok(Default::default())));
        let store = HiddenStore::open(&profile, "owner").unwrap();
        let plan = store.load().unwrap().prepare(&view).unwrap().unwrap();
        let prepared = store.prepare(&plan).unwrap();
        assert!(store
            .replace_if::<()>(
                prepared,
                || true,
                || {
                    fs::remove_file(&path).unwrap();
                    fs::create_dir(&path).unwrap();
                    None
                }
            )
            .is_err());
        assert!(store.load().is_err());
        drop(store);
        assert!(load(&profile, "owner").is_err());
        assert!(path.with_extension("journal").exists());
        fs::remove_dir(&path).unwrap();
        assert!(load(&profile, "owner").is_ok());
        assert_eq!(fs::read(&path).unwrap(), original);
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

#[cfg(all(test, windows))]
#[path = "achievement_store_durability_tests.rs"]
mod durability_tests;
