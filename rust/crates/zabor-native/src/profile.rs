use crate::auth::Credentials;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use zeroize::Zeroizing;

#[path = "profile_import.rs"]
pub mod import;

const SESSION_JOURNAL: &str = ".session-commit.pending";

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("profile is already open")]
    Locked,
    #[error("profile storage operation failed")]
    Storage,
    #[error("profile secret is invalid or unavailable to this Windows user")]
    Secret,
}

pub struct Profile {
    root: PathBuf,
    _lock: File,
    session: Mutex<()>,
    _import_lock: Option<File>,
    _directory_lock: Option<File>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSession {
    pub user_id: String,
    pub username: String,
    pub password: Zeroizing<String>,
}

pub struct PreparedSession {
    root: PathBuf,
    temporary: tempfile::TempPath,
}

impl Profile {
    pub fn default_path() -> Result<PathBuf, ProfileError> {
        std::env::var_os("APPDATA")
            .map(|path| PathBuf::from(path).join("zabor-desktop-native-beta"))
            .ok_or(ProfileError::Storage)
    }

    pub fn open(root: &Path) -> Result<Self, ProfileError> {
        import::check_path(root, true).map_err(|_| ProfileError::Storage)?;
        let import_lock = import::profile_gate(root).map_err(|_| ProfileError::Storage)?;
        std::fs::create_dir_all(root).map_err(|_| ProfileError::Storage)?;
        let root = root.canonicalize().map_err(|_| ProfileError::Storage)?;
        let directory_lock = import::directory_guard(&root).map_err(|_| ProfileError::Storage)?;
        import::check_path(&root.join("profile.lock"), true).map_err(|_| ProfileError::Storage)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("profile.lock"))
            .map_err(|_| ProfileError::Storage)?;
        lock.try_lock_exclusive()
            .map_err(|_| ProfileError::Locked)?;
        Ok(Self {
            root,
            _lock: lock,
            session: Mutex::new(()),
            _import_lock: import_lock,
            _directory_lock: directory_lock,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn save_session(
        &self,
        user_id: &str,
        credentials: &Credentials,
    ) -> Result<(), ProfileError> {
        let _session = self.session_lock()?;
        let session = SavedSession {
            user_id: user_id.into(),
            username: credentials.username.clone(),
            password: credentials.password.clone(),
        };
        let data = Zeroizing::new(serde_json::to_vec(&session).map_err(|_| ProfileError::Secret)?);
        self.write_secret("session.enc", &data)?;
        self.remove_session_journal()
    }

    #[cfg(windows)]
    pub fn prepare_session(
        &self,
        user_id: &str,
        credentials: &Credentials,
    ) -> Result<PreparedSession, ProfileError> {
        let session = SavedSession {
            user_id: user_id.into(),
            username: credentials.username.clone(),
            password: credentials.password.clone(),
        };
        let data = Zeroizing::new(serde_json::to_vec(&session).map_err(|_| ProfileError::Secret)?);
        if data.len() > 1024 * 1024 {
            return Err(ProfileError::Secret);
        }
        let encrypted = windows_dpapi::encrypt_data(
            &data,
            windows_dpapi::Scope::User,
            Some(b"zabor-native-v1"),
        )
        .map_err(|_| ProfileError::Secret)?;
        let mut file = tempfile::Builder::new()
            .prefix(".session.enc.")
            .suffix(".tmp")
            .tempfile_in(&self.root)
            .map_err(|_| ProfileError::Storage)?;
        file.write_all(b"ZNS1").map_err(|_| ProfileError::Storage)?;
        file.write_all(&encrypted)
            .map_err(|_| ProfileError::Storage)?;
        file.as_file()
            .sync_all()
            .map_err(|_| ProfileError::Storage)?;
        Ok(PreparedSession {
            root: self.root.clone(),
            temporary: file.into_temp_path(),
        })
    }

    #[cfg(not(windows))]
    pub fn prepare_session(
        &self,
        _user_id: &str,
        _credentials: &Credentials,
    ) -> Result<PreparedSession, ProfileError> {
        Err(ProfileError::Secret)
    }

    pub fn commit_session(&self, prepared: PreparedSession) -> Result<(), ProfileError> {
        let _session = self.session_lock()?;
        self.persist_session(prepared)?;
        self.remove_session_journal()
    }

    fn persist_session(&self, prepared: PreparedSession) -> Result<(), ProfileError> {
        if prepared.root != self.root {
            return Err(ProfileError::Storage);
        }
        prepared
            .temporary
            .persist(self.root.join("session.enc"))
            .map_err(|_| ProfileError::Storage)
    }

    pub fn commit_session_if(
        &self,
        prepared: PreparedSession,
        accept: impl Fn() -> bool,
        revoked: impl Fn() -> bool,
    ) -> Result<bool, ProfileError> {
        let _session = self.session_lock()?;
        if prepared.root != self.root {
            return Err(ProfileError::Storage);
        }
        if self.read_limited(SESSION_JOURNAL)?.is_some() {
            return Err(ProfileError::Storage);
        }
        let previous = self.read_limited("session.enc")?;
        if !accept() {
            return Ok(false);
        }
        let mut journal = b"ZNR1".to_vec();
        journal.push(u8::from(previous.is_some()));
        if let Some(previous) = &previous {
            journal.extend_from_slice(previous);
        }
        self.atomic_write(SESSION_JOURNAL, &journal)?;
        if let Err(error) = self.persist_session(prepared) {
            let _ = self.remove_session_journal();
            return Err(error);
        }
        if accept() {
            self.remove_session_journal()?;
            return Ok(true);
        }
        if revoked() {
            self.remove_session()?;
        } else {
            let restored = match previous {
                Some(previous) => self.atomic_write("session.enc", &previous),
                None => self.remove_session(),
            };
            if let Err(error) = restored {
                let _ = self.remove_session();
                return Err(error);
            }
            if revoked() {
                self.remove_session()?;
            }
        }
        self.remove_session_journal()?;
        Ok(false)
    }

    pub fn load_session(&self) -> Result<Option<SavedSession>, ProfileError> {
        let _session = self.session_lock()?;
        if self.read_limited(SESSION_JOURNAL)?.is_some() {
            return Err(ProfileError::Storage);
        }
        self.read_secret("session.enc")?
            .map(|data| serde_json::from_slice(&data).map_err(|_| ProfileError::Secret))
            .transpose()
    }

    pub fn clear_session(&self) -> Result<(), ProfileError> {
        let _session = self.session_lock()?;
        self.remove_session()
    }

    pub fn clear_session_if(&self, accept: impl FnOnce() -> bool) -> Result<bool, ProfileError> {
        let _session = self.session_lock()?;
        if !accept() {
            return Ok(false);
        }
        self.remove_session()?;
        Ok(true)
    }

    fn session_lock(&self) -> Result<MutexGuard<'_, ()>, ProfileError> {
        self.session.lock().map_err(|_| ProfileError::Storage)
    }

    fn remove_session(&self) -> Result<(), ProfileError> {
        self.remove_session_file("session.enc")?;
        self.remove_session_journal()
    }

    fn remove_session_journal(&self) -> Result<(), ProfileError> {
        self.remove_session_file(SESSION_JOURNAL)
    }

    fn remove_session_file(&self, name: &str) -> Result<(), ProfileError> {
        match std::fs::remove_file(self.root.join(name)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(ProfileError::Storage),
        }
    }

    pub fn save_identity(&self, identity: &str) -> Result<(), ProfileError> {
        self.write_secret("chat-identity.enc", identity.as_bytes())
    }
    pub fn load_identity(&self) -> Result<Option<Zeroizing<Vec<u8>>>, ProfileError> {
        self.read_secret("chat-identity.enc")
    }

    #[cfg(windows)]
    fn write_secret(&self, name: &str, data: &[u8]) -> Result<(), ProfileError> {
        if data.len() > 1024 * 1024 {
            return Err(ProfileError::Secret);
        }
        let encrypted =
            windows_dpapi::encrypt_data(data, windows_dpapi::Scope::User, Some(b"zabor-native-v1"))
                .map_err(|_| ProfileError::Secret)?;
        let mut file = b"ZNS1".to_vec();
        file.extend_from_slice(&encrypted);
        self.atomic_write(name, &file)
    }

    #[cfg(windows)]
    fn read_secret(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, ProfileError> {
        let Some(data) = self.read_limited(name)? else {
            return Ok(None);
        };
        let body = data.strip_prefix(b"ZNS1").ok_or(ProfileError::Secret)?;
        windows_dpapi::decrypt_data(body, windows_dpapi::Scope::User, Some(b"zabor-native-v1"))
            .map(|data| Some(Zeroizing::new(data)))
            .map_err(|_| ProfileError::Secret)
    }

    #[cfg(not(windows))]
    fn write_secret(&self, _name: &str, _data: &[u8]) -> Result<(), ProfileError> {
        Err(ProfileError::Secret)
    }
    #[cfg(not(windows))]
    fn read_secret(&self, _name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, ProfileError> {
        Err(ProfileError::Secret)
    }

    pub fn read_settings(&self) -> Result<serde_json::Value, ProfileError> {
        self.read_limited("settings.json")?
            .map(|bytes| serde_json::from_slice(&bytes).map_err(|_| ProfileError::Storage))
            .unwrap_or_else(|| Ok(serde_json::json!({})))
    }

    pub fn write_settings(&self, settings: &serde_json::Value) -> Result<(), ProfileError> {
        self.atomic_write(
            "settings.json",
            &serde_json::to_vec(settings).map_err(|_| ProfileError::Storage)?,
        )
    }

    fn read_limited(&self, name: &str) -> Result<Option<Vec<u8>>, ProfileError> {
        import::check_path(&self.root.join(name), true).map_err(|_| ProfileError::Storage)?;
        let file = match File::open(self.root.join(name)) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(ProfileError::Storage),
        };
        let mut bytes = Vec::new();
        file.take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ProfileError::Storage)?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(ProfileError::Storage);
        }
        Ok(Some(bytes))
    }

    fn atomic_write(&self, name: &str, bytes: &[u8]) -> Result<(), ProfileError> {
        import::check_path(&self.root.join(name), true).map_err(|_| ProfileError::Storage)?;
        let mut nonce = [0_u8; 8];
        getrandom::fill(&mut nonce).map_err(|_| ProfileError::Storage)?;
        let temporary = self
            .root
            .join(format!(".{name}.{:016x}.tmp", u64::from_le_bytes(nonce)));
        let operation = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|_| ProfileError::Storage)?;
            file.write_all(bytes).map_err(|_| ProfileError::Storage)?;
            file.sync_all().map_err(|_| ProfileError::Storage)?;
            drop(file);
            std::fs::rename(&temporary, self.root.join(name)).map_err(|_| ProfileError::Storage)
        })();
        if operation.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        operation
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn fixture_credentials() -> Credentials {
        Credentials {
            username: "fixture".into(),
            password: Zeroizing::new("fixture-only".into()),
        }
    }

    #[test]
    fn rejected_final_session_admission_never_changes_existing_encrypted_session() {
        let root = tempfile::tempdir().unwrap();
        let profile = Profile::open(root.path()).unwrap();
        profile
            .save_session("original-owner", &fixture_credentials())
            .unwrap();
        let previous = std::fs::read(root.path().join("session.enc")).unwrap();
        let prepared = profile
            .prepare_session("rejected-owner", &fixture_credentials())
            .unwrap();
        assert!(!profile
            .commit_session_if(prepared, || false, || panic!("No write was dispatched"))
            .unwrap());
        assert_eq!(
            std::fs::read(root.path().join("session.enc")).unwrap(),
            previous
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[test]
    fn unaccepted_final_session_write_rolls_back_exact_encrypted_bytes_or_absent_destination() {
        for existing in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let profile = Profile::open(root.path()).unwrap();
            if existing {
                profile
                    .save_session("original-owner", &fixture_credentials())
                    .unwrap();
            }
            let previous = std::fs::read(root.path().join("session.enc")).ok();
            let prepared = profile
                .prepare_session("cancelled-owner", &fixture_credentials())
                .unwrap();
            let admissions = AtomicUsize::new(0);
            assert!(!profile
                .commit_session_if(
                    prepared,
                    || admissions.fetch_add(1, Ordering::AcqRel) == 0,
                    || false
                )
                .unwrap());
            assert_eq!(admissions.load(Ordering::Acquire), 2);
            assert_eq!(
                std::fs::read(root.path().join("session.enc")).ok(),
                previous
            );
            assert_eq!(
                std::fs::read_dir(root.path()).unwrap().count(),
                if existing { 2 } else { 1 }
            );
        }
    }

    #[test]
    fn revoked_final_session_write_removes_credentials_without_touching_identity_or_settings() {
        let root = tempfile::tempdir().unwrap();
        let profile = Profile::open(root.path()).unwrap();
        profile
            .save_session("original-owner", &fixture_credentials())
            .unwrap();
        profile.save_identity("fixture-key-material").unwrap();
        profile
            .write_settings(&serde_json::json!({"language":"ru"}))
            .unwrap();
        let prepared = profile
            .prepare_session("revoked-owner", &fixture_credentials())
            .unwrap();
        let admissions = AtomicUsize::new(0);
        assert!(!profile
            .commit_session_if(
                prepared,
                || admissions.fetch_add(1, Ordering::AcqRel) == 0,
                || true
            )
            .unwrap());
        assert!(profile.load_session().unwrap().is_none());
        assert_eq!(profile.read_settings().unwrap()["language"], "ru");
        assert_eq!(
            &**profile.load_identity().unwrap().as_ref().unwrap(),
            b"fixture-key-material"
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 3);
    }

    #[test]
    fn logout_during_final_rollback_rechecks_revocation_before_releasing_session_resource() {
        let root = tempfile::tempdir().unwrap();
        let profile = Profile::open(root.path()).unwrap();
        profile
            .save_session("original-owner", &fixture_credentials())
            .unwrap();
        profile
            .write_settings(&serde_json::json!({"language":"ru"}))
            .unwrap();
        let previous = std::fs::read(root.path().join("session.enc")).unwrap();
        let prepared = profile
            .prepare_session("cancelled-owner", &fixture_credentials())
            .unwrap();
        let admissions = AtomicUsize::new(0);
        let revocations = AtomicUsize::new(0);
        assert!(!profile
            .commit_session_if(
                prepared,
                || admissions.fetch_add(1, Ordering::AcqRel) == 0,
                || {
                    if revocations.fetch_add(1, Ordering::AcqRel) == 0 {
                        return false;
                    }
                    assert_eq!(
                        std::fs::read(root.path().join("session.enc")).unwrap(),
                        previous
                    );
                    true
                },
            )
            .unwrap());
        assert_eq!(revocations.load(Ordering::Acquire), 2);
        assert!(profile.load_session().unwrap().is_none());
        assert_eq!(profile.read_settings().unwrap()["language"], "ru");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[test]
    fn denied_native_rollback_and_removal_report_failure_without_false_acceptance_or_temp_leaks() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = tempfile::tempdir().unwrap();
        let profile = Profile::open(root.path()).unwrap();
        profile
            .save_session("original-owner", &fixture_credentials())
            .unwrap();
        let previous = std::fs::read(root.path().join("session.enc")).unwrap();
        let prepared = profile
            .prepare_session("cancelled-owner", &fixture_credentials())
            .unwrap();
        let admissions = AtomicUsize::new(0);
        let exclusive = Mutex::new(None);
        let result = profile.commit_session_if(
            prepared,
            || {
                if admissions.fetch_add(1, Ordering::AcqRel) == 0 {
                    return true;
                }
                *exclusive.lock().unwrap() = Some(
                    OpenOptions::new()
                        .read(true)
                        .share_mode(0)
                        .open(root.path().join("session.enc"))
                        .unwrap(),
                );
                false
            },
            || false,
        );
        assert!(result.is_err());
        drop(exclusive);
        assert_eq!(admissions.load(Ordering::Acquire), 2);
        let journal = std::fs::read(root.path().join(SESSION_JOURNAL)).unwrap();
        assert_eq!(journal.strip_prefix(b"ZNR1\x01").unwrap(), previous);
        assert!(!journal
            .windows(fixture_credentials().password.len())
            .any(|window| window == fixture_credentials().password.as_bytes()));
        drop(profile);
        let profile = Profile::open(root.path()).unwrap();
        assert!(profile.load_session().is_err());
        let prepared = profile
            .prepare_session("next-owner", &fixture_credentials())
            .unwrap();
        assert!(profile
            .commit_session_if(
                prepared,
                || panic!("Unresolved journal must block admission"),
                || false
            )
            .is_err());
        profile.clear_session().unwrap();
        assert!(profile.load_session().unwrap().is_none());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn provisional_session_is_not_visible_to_serialized_readers_and_next_owner_writers() {
        use std::sync::mpsc;
        use std::time::Duration;
        let root = tempfile::tempdir().unwrap();
        let profile = Profile::open(root.path()).unwrap();
        profile
            .save_session("original-owner", &fixture_credentials())
            .unwrap();
        let prepared = profile
            .prepare_session("provisional-owner", &fixture_credentials())
            .unwrap();
        let (entered, waiting) = mpsc::channel();
        let (release, held) = mpsc::channel();
        let admissions = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let writing_profile = &profile;
            let admissions = &admissions;
            let write = scope.spawn(move || {
                writing_profile.commit_session_if(
                    prepared,
                    || {
                        if admissions.fetch_add(1, Ordering::AcqRel) == 0 {
                            return true;
                        }
                        entered.send(()).unwrap();
                        held.recv().unwrap();
                        false
                    },
                    || false,
                )
            });
            waiting.recv_timeout(Duration::from_secs(2)).unwrap();
            let (read_started, reading) = mpsc::channel();
            let (read_done, done) = mpsc::channel();
            let reading_profile = &profile;
            let read = scope.spawn(move || {
                read_started.send(()).unwrap();
                let owner = reading_profile.load_session().unwrap().unwrap().user_id;
                read_done.send(owner).unwrap();
            });
            reading.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(matches!(done.try_recv(), Err(mpsc::TryRecvError::Empty)));
            release.send(()).unwrap();
            assert!(!write.join().unwrap().unwrap());
            assert_eq!(
                done.recv_timeout(Duration::from_secs(2)).unwrap(),
                "original-owner"
            );
            read.join().unwrap();
        });
        let prepared = profile
            .prepare_session("next-owner", &fixture_credentials())
            .unwrap();
        assert!(profile
            .commit_session_if(prepared, || true, || false)
            .unwrap());
        assert_eq!(
            profile.load_session().unwrap().unwrap().user_id,
            "next-owner"
        );
        assert!(!profile.clear_session_if(|| false).unwrap());
        assert_eq!(
            profile.load_session().unwrap().unwrap().user_id,
            "next-owner"
        );
        assert!(profile.clear_session_if(|| true).unwrap());
        assert!(profile.load_session().unwrap().is_none());
    }

    #[test]
    fn native_session_sharing_failure_does_not_publish_or_lose_existing_credentials() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = tempfile::tempdir().unwrap();
        let profile = Profile::open(root.path()).unwrap();
        profile
            .save_session("original-owner", &fixture_credentials())
            .unwrap();
        let previous = std::fs::read(root.path().join("session.enc")).unwrap();
        let prepared = profile
            .prepare_session("failed-owner", &fixture_credentials())
            .unwrap();
        let exclusive = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(root.path().join("session.enc"))
            .unwrap();
        assert!(profile
            .commit_session_if(
                prepared,
                || panic!("Unreadable rollback must not admit a save"),
                || false
            )
            .is_err());
        drop(exclusive);
        assert_eq!(
            std::fs::read(root.path().join("session.enc")).unwrap(),
            previous
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[test]
    fn prepared_session_preserves_dpapi_format_and_only_replaces_destination_at_commit() {
        let root = tempfile::tempdir().unwrap();
        let profile = Profile::open(root.path()).unwrap();
        let credentials = Credentials {
            username: "fixture".into(),
            password: Zeroizing::new("fixture-only".into()),
        };
        profile
            .save_session("original-owner", &credentials)
            .unwrap();
        let original = std::fs::read(root.path().join("session.enc")).unwrap();
        let prepared = profile
            .prepare_session("prepared-owner", &credentials)
            .unwrap();
        assert_eq!(
            std::fs::read(root.path().join("session.enc")).unwrap(),
            original
        );
        let bytes = std::fs::read(&prepared.temporary).unwrap();
        assert!(bytes.starts_with(b"ZNS1"));
        assert!(!bytes
            .windows(credentials.password.len())
            .any(|window| window == credentials.password.as_bytes()));
        profile.commit_session(prepared).unwrap();
        let session = profile.load_session().unwrap().unwrap();
        assert_eq!(session.user_id, "prepared-owner");
        assert_eq!(session.password.as_str(), credentials.password.as_str());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[test]
    fn dropped_or_wrong_profile_session_preparation_cleans_only_its_own_temporary_file() {
        let root = tempfile::tempdir().unwrap();
        let other_root = tempfile::tempdir().unwrap();
        let profile = Profile::open(root.path()).unwrap();
        let other = Profile::open(other_root.path()).unwrap();
        let credentials = Credentials {
            username: "fixture".into(),
            password: Zeroizing::new("fixture-only".into()),
        };
        profile
            .save_session("original-owner", &credentials)
            .unwrap();
        other.save_session("other-owner", &credentials).unwrap();
        profile
            .write_settings(&serde_json::json!({"language":"ru"}))
            .unwrap();
        let prepared = profile
            .prepare_session("prepared-owner", &credentials)
            .unwrap();
        let path = prepared.temporary.to_path_buf();
        assert!(path.exists());
        drop(prepared);
        assert!(!path.exists());
        let prepared = profile
            .prepare_session("prepared-owner", &credentials)
            .unwrap();
        let path = prepared.temporary.to_path_buf();
        assert!(other.commit_session(prepared).is_err());
        assert!(!path.exists());
        assert_eq!(
            profile.load_session().unwrap().unwrap().user_id,
            "original-owner"
        );
        assert_eq!(
            other.load_session().unwrap().unwrap().user_id,
            "other-owner"
        );
        assert_eq!(profile.read_settings().unwrap()["language"], "ru");
    }

    #[test]
    fn windows_profile_protects_secrets_and_logout_retains_other_data() {
        let temporary = tempfile::tempdir().unwrap();
        let profile = Profile::open(temporary.path()).unwrap();
        assert!(matches!(
            Profile::open(temporary.path()),
            Err(ProfileError::Locked)
        ));
        let credentials = Credentials {
            username: "fixture".into(),
            password: Zeroizing::new("local-fixture-password".into()),
        };
        profile.save_session("fixture-owner", &credentials).unwrap();
        profile.save_identity("fixture-key-material").unwrap();
        profile
            .write_settings(&serde_json::json!({"language":"en"}))
            .unwrap();
        let bytes = std::fs::read(temporary.path().join("session.enc")).unwrap();
        assert!(!bytes
            .windows(credentials.password.len())
            .any(|window| window == credentials.password.as_bytes()));
        assert_eq!(
            profile.load_session().unwrap().unwrap().password.as_str(),
            credentials.password.as_str()
        );
        profile.save_session("fixture-owner", &credentials).unwrap();
        profile.clear_session().unwrap();
        assert!(profile.load_session().unwrap().is_none());
        assert_eq!(
            &**profile.load_identity().unwrap().as_ref().unwrap(),
            b"fixture-key-material"
        );
        assert_eq!(profile.read_settings().unwrap()["language"], "en");
    }
}
