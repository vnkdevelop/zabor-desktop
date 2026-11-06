use crate::auth::Credentials;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

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
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSession {
    pub user_id: String,
    pub username: String,
    pub password: Zeroizing<String>,
}

impl Profile {
    pub fn default_path() -> Result<PathBuf, ProfileError> {
        std::env::var_os("APPDATA")
            .map(|path| PathBuf::from(path).join("zabor-desktop-native-beta"))
            .ok_or(ProfileError::Storage)
    }

    pub fn open(root: &Path) -> Result<Self, ProfileError> {
        std::fs::create_dir_all(root).map_err(|_| ProfileError::Storage)?;
        let root = root.canonicalize().map_err(|_| ProfileError::Storage)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("profile.lock"))
            .map_err(|_| ProfileError::Storage)?;
        lock.try_lock_exclusive()
            .map_err(|_| ProfileError::Locked)?;
        Ok(Self { root, _lock: lock })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn save_session(
        &self,
        user_id: &str,
        credentials: &Credentials,
    ) -> Result<(), ProfileError> {
        let session = SavedSession {
            user_id: user_id.into(),
            username: credentials.username.clone(),
            password: credentials.password.clone(),
        };
        let data = Zeroizing::new(serde_json::to_vec(&session).map_err(|_| ProfileError::Secret)?);
        self.write_secret("session.enc", &data)
    }

    pub fn load_session(&self) -> Result<Option<SavedSession>, ProfileError> {
        self.read_secret("session.enc")?
            .map(|data| serde_json::from_slice(&data).map_err(|_| ProfileError::Secret))
            .transpose()
    }

    pub fn clear_session(&self) -> Result<(), ProfileError> {
        match std::fs::remove_file(self.root.join("session.enc")) {
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
