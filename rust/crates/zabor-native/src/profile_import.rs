use super::{Profile, ProfileError};
use crate::auth::Credentials;
use crate::chat_crypto::ChatIdentity;
use crate::chat_store::{valid_stored_name, ChatMessage, ChatStore, RETENTION_MS};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::{Component, Path, PathBuf};
use zeroize::Zeroizing;

pub const BETA_PROFILE: &str = "zabor-desktop-native-beta";
const STAGE: &str = ".zabor-desktop-native-beta.import-stage";
const JOURNAL: &str = ".zabor-desktop-native-beta.import.pending.json";
const GATE: &str = ".zabor-desktop-native-beta.import.lock";
const ROLLBACK: &str = ".zabor-desktop-native-beta.import-rollback";
const ABORTED: &str = ".zabor-desktop-native-beta.import-aborted";
const RECEIPT: &str = "import-receipt.json";
const MAX_EXPORT: usize = 64 * 1024 * 1024;
const MAX_SNAPSHOT: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("unsafe profile path or unsupported filesystem entry")]
    Path,
    #[error("profile copy storage operation failed")]
    Storage,
    #[error("profile is in use")]
    Locked,
    #[error("beta profile already exists")]
    Exists,
    #[error("explicit import recovery is required")]
    Recovery,
    #[error("invalid Electron export or unverified owner/key")]
    Invalid,
    #[error("Electron snapshot exporter failed")]
    Export,
}

impl From<ProfileError> for ImportError {
    fn from(_: ProfileError) -> Self {
        Self::Storage
    }
}

pub struct ImportOwner {
    user_id: String,
    username: String,
    public_key: Option<Value>,
}

impl ImportOwner {
    pub fn verified(
        user_id: &str,
        username: &str,
        public_key: Option<Value>,
    ) -> Result<Self, ImportError> {
        if !valid_id(user_id) || username.is_empty() || username.len() > 4096 {
            return Err(ImportError::Invalid);
        }
        if let Some(key) = &public_key {
            fingerprint(key)?;
            ChatIdentity::generate()
                .derive(key)
                .map_err(|_| ImportError::Invalid)?;
        }
        Ok(Self {
            user_id: user_id.into(),
            username: username.into(),
            public_key,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ElectronExport {
    version: u32,
    session: ElectronSession,
    identity: Option<Zeroizing<String>>,
    app_settings: Value,
    local_storage: BTreeMap<String, String>,
    messages: Vec<ChatMessage>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ElectronSession {
    user_id: String,
    login: String,
    password: Zeroizing<String>,
    settings: Value,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    operation: String,
    token: String,
    owner: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    token: String,
    owner: String,
    created: i64,
    files: BTreeMap<String, String>,
}

pub struct Snapshot {
    directory: tempfile::TempDir,
    source: PathBuf,
    _source_lock: File,
    source_inventory: BTreeMap<PathBuf, Option<(u64, String)>>,
}

impl Snapshot {
    pub fn root(&self) -> &Path {
        self.directory.path()
    }

    pub fn close(self) -> Result<(), ImportError> {
        let unchanged = self.verify_source_unchanged();
        self.directory.close().map_err(|_| ImportError::Storage)?;
        unchanged
    }

    pub fn verify_source_unchanged(&self) -> Result<(), ImportError> {
        if inventory(&self.source)? == self.source_inventory {
            Ok(())
        } else {
            Err(ImportError::Locked)
        }
    }

    pub fn copy(source: &Path, scratch: &Path) -> Result<Self, ImportError> {
        check_path(source, false)?;
        check_path(scratch, false)?;
        let source = source.canonicalize().map_err(|_| ImportError::Storage)?;
        let scratch = scratch.canonicalize().map_err(|_| ImportError::Storage)?;
        if scratch.starts_with(&source) || source.starts_with(&scratch) {
            return Err(ImportError::Path);
        }
        let source_lock = read_handle(&source, true)?;
        let source_inventory = inventory(&source)?;
        let before = source_entries(&source)?;
        let mut handles = Vec::new();
        let mut total = 0_u64;
        for relative in &before {
            let path = source.join(relative);
            check_path(&path, false)?;
            let metadata = std::fs::symlink_metadata(&path).map_err(|_| ImportError::Storage)?;
            let input = read_handle(&path, metadata.is_dir())?;
            total = total
                .checked_add(input.metadata().map_err(|_| ImportError::Storage)?.len())
                .ok_or(ImportError::Invalid)?;
            if total > MAX_SNAPSHOT {
                return Err(ImportError::Invalid);
            }
            handles.push((relative, input));
        }
        if before != source_entries(&source)? {
            return Err(ImportError::Locked);
        }
        let directory = tempfile::Builder::new()
            .prefix("zabor-electron-copy-")
            .tempdir_in(scratch)
            .map_err(|_| ImportError::Storage)?;
        for (relative, input) in &mut handles {
            let target = directory.path().join(relative.as_path());
            if input.metadata().map_err(|_| ImportError::Storage)?.is_dir() {
                std::fs::create_dir_all(&target).map_err(|_| ImportError::Storage)?;
            } else {
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(target)
                    .map_err(|_| ImportError::Storage)?;
                std::io::copy(input, &mut output).map_err(|_| ImportError::Storage)?;
                output.sync_all().map_err(|_| ImportError::Storage)?;
            }
        }
        if before != source_entries(&source)? {
            return Err(ImportError::Locked);
        }
        if source_inventory != inventory(&source)? {
            return Err(ImportError::Locked);
        }
        write_new(
            &directory.path().join(".native-copy-snapshot"),
            b"zabor-electron-copy-v1",
        )?;
        Ok(Self {
            directory,
            source,
            _source_lock: source_lock,
            source_inventory,
        })
    }
}

pub struct PreparedImport {
    parent: PathBuf,
    journal: Journal,
    _gate: File,
}

impl PreparedImport {
    pub fn copy(
        source: &Path,
        scratch: &Path,
        parent: &Path,
        owner: &ImportOwner,
        now: i64,
        extract: impl FnOnce(&Snapshot) -> Result<Zeroizing<Vec<u8>>, ImportError>,
    ) -> Result<Self, ImportError> {
        let snapshot = Snapshot::copy(source, scratch)?;
        let prepared = extract(&snapshot)
            .and_then(|exported| Self::prepare(&snapshot, parent, owner, &exported, now));
        snapshot.close()?;
        prepared
    }

    pub fn prepare(
        snapshot: &Snapshot,
        parent: &Path,
        owner: &ImportOwner,
        exported: &Zeroizing<Vec<u8>>,
        now: i64,
    ) -> Result<Self, ImportError> {
        snapshot.verify_source_unchanged()?;
        if exported.len() > MAX_EXPORT || now < RETENTION_MS {
            return Err(ImportError::Invalid);
        }
        let data: ElectronExport =
            serde_json::from_slice(exported).map_err(|_| ImportError::Invalid)?;
        if data.version != 1
            || data.session.user_id != owner.user_id
            || data.session.login != owner.username
            || data.session.password.is_empty()
            || data.session.password.len() > 64 * 1024
            || !data.session.settings.is_object()
            || !data.app_settings.is_object()
            || data.messages.len() > 50_000
            || data.local_storage.len() > 50_000
        {
            return Err(ImportError::Invalid);
        }
        match (&data.identity, &owner.public_key) {
            (Some(identity), Some(expected)) => {
                if identity.len() > 16 * 1024 {
                    return Err(ImportError::Invalid);
                }
                let key = ChatIdentity::import(identity).map_err(|_| ImportError::Invalid)?;
                if fingerprint(&key.public_key().map_err(|_| ImportError::Invalid)?)?
                    != fingerprint(expected)?
                {
                    return Err(ImportError::Invalid);
                }
            }
            (None, None) => {}
            _ => return Err(ImportError::Invalid),
        }
        check_path(parent, false)?;
        let parent = parent.canonicalize().map_err(|_| ImportError::Storage)?;
        if parent.starts_with(snapshot.root())
            || snapshot.root().starts_with(parent.join(BETA_PROFILE))
            || parent.starts_with(&snapshot.source)
            || snapshot.source.starts_with(parent.join(BETA_PROFILE))
        {
            return Err(ImportError::Path);
        }
        let gate = gate(&parent)?;
        require_clean(&parent)?;
        if present(&parent.join(BETA_PROFILE))? {
            return Err(ImportError::Exists);
        }
        let mut nonce = [0_u8; 16];
        getrandom::fill(&mut nonce).map_err(|_| ImportError::Storage)?;
        let token = hex(&nonce);
        let journal = Journal {
            version: 1,
            operation: "publish".into(),
            token,
            owner: owner.user_id.clone(),
        };
        write_new(
            &parent.join(JOURNAL),
            &serde_json::to_vec(&journal).map_err(|_| ImportError::Storage)?,
        )?;
        let stage = parent.join(STAGE);
        std::fs::create_dir(&stage).map_err(|_| ImportError::Storage)?;
        build_stage(snapshot, &stage, &data, owner, now)?;
        snapshot.verify_source_unchanged()?;
        let files = hashes(&stage, false)?;
        let receipt = Receipt {
            version: 1,
            token: journal.token.clone(),
            owner: journal.owner.clone(),
            created: now,
            files,
        };
        write_new(
            &stage.join(RECEIPT),
            &serde_json::to_vec(&receipt).map_err(|_| ImportError::Storage)?,
        )?;
        verify_stage(&stage, &journal)?;
        Ok(Self {
            parent,
            journal,
            _gate: gate,
        })
    }

    pub fn publish(mut self, accept: impl Fn() -> bool) -> Result<bool, ImportError> {
        let journal = load_journal(&self.parent)?;
        if journal.token != self.journal.token
            || journal.owner != self.journal.owner
            || journal.operation != "publish"
        {
            return Err(ImportError::Recovery);
        }
        verify_stage(&self.parent.join(STAGE), &self.journal)?;
        if present(&self.parent.join(BETA_PROFILE))? {
            return Err(ImportError::Exists);
        }
        if !accept() {
            self.mark_aborted()?;
            quarantine(&self.parent, STAGE, ABORTED)?;
            finish(&self.parent)?;
            return Ok(false);
        }
        std::fs::rename(self.parent.join(STAGE), self.parent.join(BETA_PROFILE))
            .map_err(|_| ImportError::Storage)?;
        if !accept() {
            self.mark_aborted()?;
            quarantine(&self.parent, BETA_PROFILE, ABORTED)?;
            finish(&self.parent)?;
            return Ok(false);
        }
        finish(&self.parent)?;
        Ok(true)
    }

    fn mark_aborted(&mut self) -> Result<(), ImportError> {
        self.journal.operation = "abort".into();
        check_path(&self.parent.join(JOURNAL), false)?;
        let mut file =
            tempfile::NamedTempFile::new_in(&self.parent).map_err(|_| ImportError::Storage)?;
        file.write_all(&serde_json::to_vec(&self.journal).map_err(|_| ImportError::Storage)?)
            .map_err(|_| ImportError::Storage)?;
        file.as_file()
            .sync_all()
            .map_err(|_| ImportError::Storage)?;
        file.persist(self.parent.join(JOURNAL))
            .map_err(|_| ImportError::Storage)?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub enum RecoveryAction {
    PublishVerifiedStage,
    AbortUnpublishedStage,
    ConfirmPublished,
    FinishRollback,
    FinishAbortedPublication,
}

pub fn recover(parent: &Path, action: RecoveryAction) -> Result<(), ImportError> {
    check_path(parent, false)?;
    let _gate = gate(parent)?;
    let journal = load_journal(parent)?;
    let stage = present(&parent.join(STAGE))?;
    let beta = present(&parent.join(BETA_PROFILE))?;
    match (journal.operation.as_str(), action) {
        ("publish", RecoveryAction::PublishVerifiedStage) if stage && !beta => {
            verify_stage(&parent.join(STAGE), &journal)?;
            std::fs::rename(parent.join(STAGE), parent.join(BETA_PROFILE))
                .map_err(|_| ImportError::Storage)?;
        }
        ("publish", RecoveryAction::AbortUnpublishedStage) if !beta => {
            if stage {
                quarantine(parent, STAGE, ABORTED)?;
            }
        }
        ("publish", RecoveryAction::ConfirmPublished) if beta && !stage => {
            verify_stage(&parent.join(BETA_PROFILE), &journal)?
        }
        ("rollback", RecoveryAction::FinishRollback) if !stage => {
            if beta && !present(&parent.join(ROLLBACK))? {
                verify_receipt(&parent.join(BETA_PROFILE), &journal)?;
                quarantine(parent, BETA_PROFILE, ROLLBACK)?;
            } else if !beta && present(&parent.join(ROLLBACK))? {
                verify_receipt(&parent.join(ROLLBACK), &journal)?;
            } else {
                return Err(ImportError::Recovery);
            }
        }
        ("abort", RecoveryAction::FinishAbortedPublication) => {
            if stage && !beta {
                verify_receipt(&parent.join(STAGE), &journal)?;
                quarantine(parent, STAGE, ABORTED)?;
            } else if beta && !stage {
                verify_receipt(&parent.join(BETA_PROFILE), &journal)?;
                quarantine(parent, BETA_PROFILE, ABORTED)?;
            } else if !beta && !stage && present(&parent.join(ABORTED))? {
                verify_receipt(&parent.join(ABORTED), &journal)?;
            } else {
                return Err(ImportError::Recovery);
            }
        }
        _ => return Err(ImportError::Recovery),
    }
    finish(parent)
}

pub fn rollback(parent: &Path, owner: &ImportOwner) -> Result<PathBuf, ImportError> {
    check_path(parent, false)?;
    let _gate = gate(parent)?;
    require_clean(parent)?;
    if present(&parent.join(ROLLBACK))? {
        return Err(ImportError::Exists);
    }
    if present(&parent.join(BETA_PROFILE).join(".session-commit.pending"))? {
        return Err(ImportError::Recovery);
    }
    let receipt: Receipt = read_json(&parent.join(BETA_PROFILE).join(RECEIPT))?;
    if receipt.version != 1 || receipt.owner != owner.user_id || !valid_token(&receipt.token) {
        return Err(ImportError::Invalid);
    }
    let journal = Journal {
        version: 1,
        operation: "rollback".into(),
        token: receipt.token,
        owner: receipt.owner,
    };
    write_new(
        &parent.join(JOURNAL),
        &serde_json::to_vec(&journal).map_err(|_| ImportError::Storage)?,
    )?;
    quarantine(parent, BETA_PROFILE, ROLLBACK)?;
    finish(parent)?;
    Ok(parent.join(ROLLBACK))
}

fn build_stage(
    snapshot: &Snapshot,
    stage: &Path,
    data: &ElectronExport,
    owner: &ImportOwner,
    now: i64,
) -> Result<(), ImportError> {
    let profile = Profile::open(stage)?;
    profile.save_session(
        &owner.user_id,
        &Credentials {
            username: owner.username.clone(),
            password: data.session.password.clone(),
        },
    )?;
    if let Some(identity) = &data.identity {
        profile.save_identity(identity)?;
    }
    let mut settings = data.session.settings.clone();
    let allowed = [
        "inputVolume",
        "outputVolume",
        "selectedInput",
        "selectedOutput",
        "noiseSuppression",
        "language",
        "openAtLogin",
        "minimizeToTray",
        "micThresholdMode",
        "manualThresholdValue",
        "smartNoiseModel",
        "speechAnalyzer",
        "echoCancellation",
        "suppressionStrength",
        "ultraLowLatency",
    ];
    settings
        .as_object_mut()
        .ok_or(ImportError::Invalid)?
        .retain(|key, _| allowed.contains(&key.as_str()));
    for (key, value) in settings.as_object().ok_or(ImportError::Invalid)? {
        let valid = if ["openAtLogin", "minimizeToTray"].contains(&key.as_str()) {
            value.is_boolean()
        } else {
            crate::audio_settings::AudioSettings::default()
                .edited(key, value.clone())
                .is_some()
        };
        if !valid {
            return Err(ImportError::Invalid);
        }
    }
    for key in ["openAtLogin", "minimizeToTray"] {
        if let Some(value) = data.app_settings.get(key) {
            if !value.is_boolean() {
                return Err(ImportError::Invalid);
            }
            settings[key] = value.clone();
        }
    }
    if let Some(relay) = data.local_storage.get("zabor_relay_only_ice") {
        if !["true", "false"].contains(&relay.as_str()) {
            return Err(ImportError::Invalid);
        }
        settings["relayOnly"] = json!(relay == "true");
    }
    profile.write_settings(&settings)?;
    let files = stage.join("chat-files");
    std::fs::create_dir(&files).map_err(|_| ImportError::Storage)?;
    let store = ChatStore::open(&stage.join("chat.sqlite3")).map_err(|_| ImportError::Storage)?;
    let (deleted_messages, deleted_files) = import_deletions(&store, data, owner, now)?;
    let mut ids = BTreeSet::new();
    for original in &data.messages {
        if original.owner_id != owner.user_id {
            continue;
        }
        if original.created_at <= now.saturating_sub(RETENTION_MS) {
            continue;
        }
        if deleted_messages.contains(&(original.conversation_id.clone(), original.id.clone())) {
            continue;
        }
        if !valid_id(&original.id)
            || !valid_id(&original.conversation_id)
            || original.created_at > now
            || original.sequence < 0
            || original.text.len() > 64 * 1024
            || !ids.insert(original.id.clone())
            || !((original.sender_id == owner.user_id
                && original.target_id == original.conversation_id)
                || (original.target_id == owner.user_id
                    && original.sender_id == original.conversation_id))
        {
            return Err(ImportError::Invalid);
        }
        let mut message = original.clone();
        if message.kind == "file" {
            let deleted =
                deleted_files.contains(&(message.conversation_id.clone(), message.id.clone()));
            import_file(snapshot, &files, &mut message, deleted)?;
        } else if message.file.is_some() {
            return Err(ImportError::Invalid);
        }
        store
            .save(&message, now)
            .map_err(|_| ImportError::Invalid)?;
    }
    for (key, value) in &data.local_storage {
        let prefix = format!("zabor_chat_key_{}_", owner.user_id);
        if let Some(peer) = key.strip_prefix(&prefix) {
            if !valid_id(peer) {
                return Err(ImportError::Invalid);
            }
            let key: Value = serde_json::from_str(value).map_err(|_| ImportError::Invalid)?;
            ChatIdentity::generate()
                .derive(&key)
                .map_err(|_| ImportError::Invalid)?;
            if !store
                .accept_peer_key(&owner.user_id, peer, &fingerprint(&key)?)
                .map_err(|_| ImportError::Storage)?
            {
                return Err(ImportError::Invalid);
            }
        }
    }
    drop(store);
    let database =
        rusqlite::Connection::open(stage.join("chat.sqlite3")).map_err(|_| ImportError::Storage)?;
    database
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")
        .map_err(|_| ImportError::Storage)?;
    drop(database);
    drop(profile);
    for path in tree_entries(stage)? {
        if path.is_file() {
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .and_then(|file| file.sync_all())
                .map_err(|_| ImportError::Storage)?;
        }
    }
    Ok(())
}

type DeletedIds = BTreeSet<(String, String)>;

fn import_deletions(
    store: &ChatStore,
    data: &ElectronExport,
    owner: &ImportOwner,
    now: i64,
) -> Result<(DeletedIds, DeletedIds), ImportError> {
    let mut messages = BTreeSet::new();
    let mut files = BTreeSet::new();
    for (key, value) in &data.local_storage {
        for (prefix, incoming) in [
            (format!("zabor_chat_deletions_{}_", owner.user_id), false),
            (
                format!("zabor_chat_deleted_incoming_{}_", owner.user_id),
                true,
            ),
        ] {
            if let Some(peer) = key.strip_prefix(&prefix) {
                if !valid_id(peer) {
                    return Err(ImportError::Invalid);
                }
                let packets: Vec<Value> =
                    serde_json::from_str(value).map_err(|_| ImportError::Invalid)?;
                if packets.len() > 50_000 {
                    return Err(ImportError::Invalid);
                }
                for packet in packets {
                    let id = packet["messageId"]
                        .as_str()
                        .filter(|id| valid_id(id))
                        .ok_or(ImportError::Invalid)?;
                    match packet["type"].as_str() {
                        Some("message-delete") => {
                            store
                                .store_deletion(&owner.user_id, peer, id, incoming)
                                .map_err(|_| ImportError::Storage)?;
                            messages.insert((peer.into(), id.into()));
                        }
                        Some("file-delete") => {
                            store
                                .store_file_deletion(&owner.user_id, peer, id, incoming, now)
                                .map_err(|_| ImportError::Storage)?;
                            files.insert((peer.into(), id.into()));
                        }
                        _ => return Err(ImportError::Invalid),
                    }
                }
            }
        }
    }
    Ok((messages, files))
}

fn import_file(
    snapshot: &Snapshot,
    files: &Path,
    message: &mut ChatMessage,
    forced_deleted: bool,
) -> Result<(), ImportError> {
    let original = message.file.as_ref().ok_or(ImportError::Invalid)?;
    let name = original["name"]
        .as_str()
        .filter(|name| !name.is_empty() && name.len() <= 4096)
        .ok_or(ImportError::Invalid)?;
    let size = original["size"]
        .as_u64()
        .filter(|size| *size <= MAX_SNAPSHOT)
        .ok_or(ImportError::Invalid)?;
    let sha = original["sha256"]
        .as_str()
        .filter(|sha| sha.len() == 64 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or(ImportError::Invalid)?;
    let mut stored = None;
    let deleted = forced_deleted || original["transferState"] == "deleted";
    if let Some(source_name) = original["storedName"].as_str() {
        if source_name.is_empty()
            || source_name.len() > 200
            || source_name.starts_with('.')
            || source_name.ends_with(['.', ' '])
            || source_name
                .chars()
                .any(|character| character.is_control() || "\\/:<>\"|?*".contains(character))
        {
            return Err(ImportError::Path);
        }
        let source = snapshot.root().join("chat-files").join(source_name);
        if !deleted && present(&source)? {
            let (source_size, hash) = hash_file(&source)?;
            if source_size != size || hash != sha.to_ascii_lowercase() {
                return Err(ImportError::Invalid);
            }
            let target_name = format!("{hash}.bin");
            if !valid_stored_name(&target_name) {
                return Err(ImportError::Path);
            }
            let target = files.join(&target_name);
            if !present(&target)? {
                let mut input = read_handle(&source, false)?;
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)
                    .map_err(|_| ImportError::Storage)?;
                std::io::copy(&mut input, &mut output).map_err(|_| ImportError::Storage)?;
                output.sync_all().map_err(|_| ImportError::Storage)?;
                drop(output);
                if hash_file(&target)? != (size, hash) {
                    return Err(ImportError::Invalid);
                }
            }
            stored = Some(target_name);
        }
    }
    message.file = Some(
        json!({"name":name,"size":size,"sha256":sha,"storedName":stored,"savedPath":null,"sourceAvailable":stored.is_some(),"transferState":if deleted {"deleted"} else if stored.is_some() {"available"} else {"missing"},"progress":0}),
    );
    Ok(())
}

pub(super) fn check_path(path: &Path, missing: bool) -> Result<(), ImportError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| ImportError::Path)?
            .join(path)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir | Component::CurDir => return Err(ImportError::Path),
            #[cfg(windows)]
            Component::Normal(name)
                if name.to_string_lossy().contains(':')
                    || name.to_string_lossy().ends_with(['.', ' ']) =>
            {
                return Err(ImportError::Path)
            }
            #[cfg(windows)]
            Component::Prefix(prefix)
                if !matches!(
                    prefix.kind(),
                    std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
                ) =>
            {
                return Err(ImportError::Path)
            }
            _ => current.push(component),
        }
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err(ImportError::Path);
                    }
                }
                if metadata.file_type().is_symlink() || !(metadata.is_file() || metadata.is_dir()) {
                    return Err(ImportError::Path);
                }
            }
            Err(error) if missing && error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(ImportError::Path),
        }
    }
    Ok(())
}

pub(super) fn profile_gate(root: &Path) -> Result<Option<File>, ImportError> {
    let resolved = if root.exists() {
        root.canonicalize().map_err(|_| ImportError::Path)?
    } else {
        root.to_path_buf()
    };
    if resolved
        .file_name()
        .is_none_or(|name| !name.to_string_lossy().eq_ignore_ascii_case(BETA_PROFILE))
    {
        return Ok(None);
    }
    let parent = resolved
        .parent()
        .filter(|path| path.is_dir())
        .ok_or(ImportError::Path)?;
    let gate = gate(parent)?;
    if present(&parent.join(JOURNAL))? || present(&parent.join(STAGE))? {
        return Err(ImportError::Recovery);
    }
    Ok(Some(gate))
}

fn gate(parent: &Path) -> Result<File, ImportError> {
    check_path(&parent.join(GATE), true)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(parent.join(GATE))
        .map_err(|_| ImportError::Storage)?;
    file.try_lock_exclusive().map_err(|_| ImportError::Locked)?;
    Ok(file)
}

fn present(path: &Path) -> Result<bool, ImportError> {
    check_path(path, true)?;
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(ImportError::Storage),
    }
}

fn require_clean(parent: &Path) -> Result<(), ImportError> {
    if present(&parent.join(JOURNAL))? || present(&parent.join(STAGE))? {
        Err(ImportError::Recovery)
    } else {
        Ok(())
    }
}

fn load_journal(parent: &Path) -> Result<Journal, ImportError> {
    let journal: Journal = read_json(&parent.join(JOURNAL))?;
    if journal.version != 1
        || !valid_token(&journal.token)
        || !valid_id(&journal.owner)
        || !["publish", "rollback", "abort"].contains(&journal.operation.as_str())
    {
        return Err(ImportError::Recovery);
    }
    Ok(journal)
}

fn verify_receipt(root: &Path, journal: &Journal) -> Result<Receipt, ImportError> {
    let receipt: Receipt = read_json(&root.join(RECEIPT))?;
    if receipt.version != 1 || receipt.token != journal.token || receipt.owner != journal.owner {
        return Err(ImportError::Recovery);
    }
    Ok(receipt)
}

fn verify_stage(root: &Path, journal: &Journal) -> Result<(), ImportError> {
    let receipt = verify_receipt(root, journal)?;
    if receipt.files != hashes(root, true)?
        || present(&root.join(".session-commit.pending"))?
        || present(&root.join("chat.sqlite3-wal"))?
        || present(&root.join("chat.sqlite3-shm"))?
        || present(&root.join("chat.sqlite3-journal"))?
    {
        return Err(ImportError::Recovery);
    }
    Ok(())
}

fn hashes(root: &Path, receipt: bool) -> Result<BTreeMap<String, String>, ImportError> {
    let mut hashes = BTreeMap::new();
    for path in tree_entries(root)? {
        if !path.is_file() || (receipt && path == root.join(RECEIPT)) {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| ImportError::Path)?
            .to_str()
            .ok_or(ImportError::Path)?
            .replace('\\', "/");
        hashes.insert(relative, hash_file(&path)?.1);
    }
    Ok(hashes)
}

fn quarantine(parent: &Path, source: &str, target: &str) -> Result<(), ImportError> {
    if present(&parent.join(target))? {
        return Err(ImportError::Exists);
    }
    tree_entries(&parent.join(source))?;
    std::fs::rename(parent.join(source), parent.join(target)).map_err(|_| ImportError::Storage)
}

fn finish(parent: &Path) -> Result<(), ImportError> {
    check_path(&parent.join(JOURNAL), false)?;
    std::fs::remove_file(parent.join(JOURNAL)).map_err(|_| ImportError::Storage)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, ImportError> {
    serde_json::from_slice(&read_bytes(path, MAX_EXPORT)?).map_err(|_| ImportError::Recovery)
}

fn read_bytes(path: &Path, max: usize) -> Result<Vec<u8>, ImportError> {
    check_path(path, false)?;
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| ImportError::Storage)?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ImportError::Storage)?;
    if bytes.len() > max {
        return Err(ImportError::Invalid);
    }
    Ok(bytes)
}

fn hash_file(path: &Path) -> Result<(u64, String), ImportError> {
    check_path(path, false)?;
    let mut input = read_handle(path, false)?.take(MAX_SNAPSHOT + 1);
    let mut hash = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = Zeroizing::new([0_u8; 64 * 1024]);
    loop {
        let read = input
            .read(&mut buffer[..])
            .map_err(|_| ImportError::Storage)?;
        if read == 0 {
            break;
        }
        size += read as u64;
        if size > MAX_SNAPSHOT {
            return Err(ImportError::Invalid);
        }
        hash.update(&buffer[..read]);
    }
    Ok((size, hex(&hash.finalize())))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), ImportError> {
    check_path(path, true)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| ImportError::Storage)?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| ImportError::Storage)
}

fn source_entries(source: &Path) -> Result<BTreeSet<PathBuf>, ImportError> {
    let mut entries = BTreeSet::new();
    for name in [
        "Local State",
        "session.enc",
        "session.json",
        "chat-identity.enc",
        "app-settings.json",
        "IndexedDB",
        "Local Storage",
        "chat-files",
    ] {
        let path = source.join(name);
        if present(&path)? {
            entries.insert(PathBuf::from(name));
            if path.is_dir() {
                for path in tree_entries(&path)? {
                    entries.insert(
                        path.strip_prefix(source)
                            .map_err(|_| ImportError::Path)?
                            .to_path_buf(),
                    );
                }
            }
        }
    }
    if entries.len() > 100_000 {
        return Err(ImportError::Invalid);
    }
    Ok(entries)
}

fn inventory(source: &Path) -> Result<BTreeMap<PathBuf, Option<(u64, String)>>, ImportError> {
    let mut files = BTreeMap::new();
    let mut total = 0_u64;
    for path in tree_entries(source)? {
        let relative = path
            .strip_prefix(source)
            .map_err(|_| ImportError::Path)?
            .to_path_buf();
        let value = if path.is_dir() {
            None
        } else {
            let size = std::fs::metadata(&path)
                .map_err(|_| ImportError::Storage)?
                .len();
            total = total.checked_add(size).ok_or(ImportError::Invalid)?;
            if total > MAX_SNAPSHOT {
                return Err(ImportError::Invalid);
            }
            let hashed = hash_file(&path)?;
            if hashed.0 != size {
                return Err(ImportError::Locked);
            }
            Some(hashed)
        };
        files.insert(relative, value);
    }
    Ok(files)
}

fn tree_entries(root: &Path) -> Result<Vec<PathBuf>, ImportError> {
    check_path(root, false)?;
    let mut paths = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).map_err(|_| ImportError::Storage)? {
            let path = entry.map_err(|_| ImportError::Storage)?.path();
            check_path(&path, false)?;
            if path.is_dir() {
                pending.push(path.clone());
            }
            paths.push(path);
            if paths.len() > 100_000 {
                return Err(ImportError::Invalid);
            }
        }
    }
    paths.sort();
    Ok(paths)
}

fn read_handle(path: &Path, directory: bool) -> Result<File, ImportError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .share_mode(if directory { 3 } else { 1 })
            .custom_flags(0x00200000 | if directory { 0x02000000 } else { 0 });
    }
    let mut file = options.open(path).map_err(|_| ImportError::Locked)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if file
            .metadata()
            .map_err(|_| ImportError::Storage)?
            .file_attributes()
            & 0x400
            != 0
        {
            return Err(ImportError::Path);
        }
    }
    if !directory {
        file.rewind().map_err(|_| ImportError::Storage)?;
    }
    Ok(file)
}

pub(super) fn directory_guard(path: &Path) -> Result<Option<File>, ImportError> {
    #[cfg(windows)]
    {
        read_handle(path, true).map(Some)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(None)
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 512
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-.:".contains(&byte))
}

fn valid_token(token: &str) -> bool {
    token.len() == 32 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn fingerprint(key: &Value) -> Result<String, ImportError> {
    if key["kty"] != "EC"
        || key["crv"] != "P-256"
        || key["x"].as_str().is_none()
        || key["y"].as_str().is_none()
    {
        return Err(ImportError::Invalid);
    }
    #[derive(Serialize)]
    struct Fingerprint<'a> {
        kty: &'a str,
        crv: &'a str,
        x: &'a str,
        y: &'a str,
    }
    serde_json::to_string(&Fingerprint {
        kty: "EC",
        crv: "P-256",
        x: key["x"].as_str().ok_or(ImportError::Invalid)?,
        y: key["y"].as_str().ok_or(ImportError::Invalid)?,
    })
    .map_err(|_| ImportError::Invalid)
}

#[cfg(all(test, windows))]
#[path = "profile_import_tests.rs"]
mod tests;
