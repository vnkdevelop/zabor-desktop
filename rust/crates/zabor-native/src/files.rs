use crate::chat_store::valid_stored_name;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub sha256: String,
    pub stored_name: String,
}

#[derive(Debug, thiserror::Error)]
#[error("file transfer failed")]
pub struct FileError;

pub struct StagedFile {
    info: Option<FileInfo>,
    root: PathBuf,
}

impl StagedFile {
    pub fn info(&self) -> &FileInfo {
        self.info.as_ref().expect("uncommitted staged file")
    }
    pub fn commit(mut self) -> FileInfo {
        self.info.take().expect("uncommitted staged file")
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        if let Some(info) = &self.info {
            let _ = std::fs::remove_file(self.root.join(&info.stored_name));
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkHeader {
    pub version: u8,
    pub sender_id: String,
    pub target_id: String,
    pub message_id: String,
    pub transfer_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
}

struct Incoming {
    peer: String,
    message: String,
    expected: u64,
    hash: String,
    written: u64,
    digest: Sha256,
    writer: Option<BufWriter<File>>,
    temporary: PathBuf,
    target: PathBuf,
}

impl Drop for Incoming {
    fn drop(&mut self) {
        self.writer.take();
        let _ = std::fs::remove_file(&self.temporary);
    }
}

pub struct FileBroker {
    root: PathBuf,
    incoming: BTreeMap<String, Incoming>,
}

pub(crate) struct FileRevocations(Vec<Incoming>);

impl FileRevocations {
    pub(crate) fn finish(self) {
        drop(self.0);
    }
}

impl FileBroker {
    pub fn new(root: &Path) -> Result<Self, FileError> {
        std::fs::create_dir_all(root).map_err(|_| FileError)?;
        Ok(Self {
            root: root.canonicalize().map_err(|_| FileError)?,
            incoming: BTreeMap::new(),
        })
    }

    pub fn stage(&self, source: &Path) -> Result<FileInfo, FileError> {
        self.stage_cancellable(source, &CancellationToken::new())
            .map(StagedFile::commit)
    }

    pub fn stage_cancellable(
        &self,
        source: &Path,
        stop: &CancellationToken,
    ) -> Result<StagedFile, FileError> {
        let name = source
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty() && name.encode_utf16().count() <= 255)
            .ok_or(FileError)?
            .to_owned();
        let mut input = File::open(source).map_err(|_| FileError)?;
        let expected = input.metadata().map_err(|_| FileError)?.len();
        if expected == 0 || expected > 9_007_199_254_740_991 {
            return Err(FileError);
        }
        let stored_name = format!("{}.bin", random_id()?);
        let temporary = self.root.join(format!(".{stored_name}.part"));
        let result = (|| {
            let mut output = open_partial(&temporary).map_err(|_| FileError)?;
            let mut buffer = vec![0_u8; 1024 * 1024];
            let mut digest = Sha256::new();
            let mut size = 0_u64;
            loop {
                if stop.is_cancelled() {
                    return Err(FileError);
                }
                let count = input.read(&mut buffer).map_err(|_| FileError)?;
                if count == 0 {
                    break;
                }
                size = size.checked_add(count as u64).ok_or(FileError)?;
                if size > expected {
                    return Err(FileError);
                }
                digest.update(&buffer[..count]);
                output.write_all(&buffer[..count]).map_err(|_| FileError)?;
            }
            if size != expected {
                return Err(FileError);
            }
            output.sync_all().map_err(|_| FileError)?;
            drop(output);
            if stop.is_cancelled() {
                return Err(FileError);
            }
            std::fs::rename(&temporary, self.root.join(&stored_name)).map_err(|_| FileError)?;
            Ok(FileInfo {
                name,
                size,
                sha256: format!("{:x}", digest.finalize()),
                stored_name,
            })
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result.map(|info| StagedFile {
            info: Some(info),
            root: self.root.clone(),
        })
    }

    pub fn open_source(&self, name: &str) -> Result<File, FileError> {
        if !valid_stored_name(name) {
            return Err(FileError);
        }
        let path = self.root.join(name);
        if std::fs::symlink_metadata(&path)
            .map_err(|_| FileError)?
            .file_type()
            .is_symlink()
        {
            return Err(FileError);
        }
        File::open(path).map_err(|_| FileError)
    }

    pub fn begin(
        &mut self,
        peer: &str,
        message: &str,
        transfer: &str,
        size: u64,
        hash: &str,
    ) -> Result<(), FileError> {
        if self.incoming.len() >= 8
            || self.incoming.contains_key(transfer)
            || self
                .incoming
                .values()
                .any(|item| item.peer == peer && item.message == message)
            || transfer.is_empty()
            || message.is_empty()
            || transfer.len() > 128
            || message.len() > 128
            || size == 0
            || size > 9_007_199_254_740_991
            || hash.len() != 64
            || !hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(FileError);
        }
        let name = random_id()?;
        let temporary = self.root.join(format!(".{name}.part"));
        let target = self.root.join(format!("{name}.bin"));
        let file = open_partial(&temporary).map_err(|_| FileError)?;
        self.incoming.insert(
            transfer.into(),
            Incoming {
                peer: peer.into(),
                message: message.into(),
                expected: size,
                hash: hash.into(),
                written: 0,
                digest: Sha256::new(),
                writer: Some(BufWriter::with_capacity(4 * 1024 * 1024, file)),
                temporary,
                target,
            },
        );
        Ok(())
    }

    pub fn append(
        &mut self,
        peer: &str,
        header: &ChunkHeader,
        bytes: &[u8],
    ) -> Result<f32, FileError> {
        let transfer = self
            .incoming
            .get_mut(&header.transfer_id)
            .ok_or(FileError)?;
        if transfer.peer != peer
            || transfer.message != header.message_id
            || header.offset != Some(transfer.written)
            || bytes.is_empty()
            || bytes.len() > 192 * 1024
            || transfer
                .written
                .checked_add(bytes.len() as u64)
                .is_none_or(|total| total > transfer.expected)
        {
            return Err(FileError);
        }
        transfer
            .writer
            .as_mut()
            .ok_or(FileError)?
            .write_all(bytes)
            .map_err(|_| FileError)?;
        transfer.digest.update(bytes);
        transfer.written += bytes.len() as u64;
        Ok(transfer.written as f32 / transfer.expected as f32)
    }

    pub fn complete(
        &mut self,
        peer: &str,
        message: &str,
        transfer: &str,
    ) -> Result<String, FileError> {
        if self
            .incoming
            .get(transfer)
            .is_none_or(|item| item.peer != peer || item.message != message)
        {
            return Err(FileError);
        }
        let mut incoming = self.incoming.remove(transfer).ok_or(FileError)?;
        if incoming.written != incoming.expected
            || format!("{:x}", incoming.digest.clone().finalize()) != incoming.hash
        {
            return Err(FileError);
        }
        let mut writer = incoming.writer.take().ok_or(FileError)?;
        writer.flush().map_err(|_| FileError)?;
        writer.get_ref().sync_all().map_err(|_| FileError)?;
        drop(writer);
        std::fs::rename(&incoming.temporary, &incoming.target).map_err(|_| FileError)?;
        incoming
            .target
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .ok_or(FileError)
    }

    pub fn abort(&mut self, transfer: &str) {
        self.incoming.remove(transfer);
    }
    pub fn abort_message(&mut self, peer: &str, message: &str) {
        self.incoming
            .retain(|_, transfer| transfer.peer != peer || transfer.message != message);
    }

    pub fn retain_peers(&mut self, peers: &std::collections::BTreeSet<String>) {
        self.incoming
            .retain(|_, transfer| peers.contains(&transfer.peer));
    }

    pub(crate) fn revoke_peers(
        &mut self,
        peers: &std::collections::BTreeSet<String>,
    ) -> FileRevocations {
        let transfers: Vec<_> = self
            .incoming
            .iter()
            .filter(|(_, transfer)| !peers.contains(&transfer.peer))
            .map(|(id, _)| id.clone())
            .collect();
        FileRevocations(
            transfers
                .into_iter()
                .filter_map(|id| self.incoming.remove(&id))
                .collect(),
        )
    }

    pub fn recover_partials(&self) -> Result<(), FileError> {
        for entry in std::fs::read_dir(&self.root).map_err(|_| FileError)? {
            let entry = entry.map_err(|_| FileError)?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Some(id) = name
                .strip_prefix('.')
                .and_then(|name| name.strip_suffix(".part"))
                .map(|name| name.strip_suffix(".bin").unwrap_or(name))
            else {
                continue;
            };
            if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                continue;
            }
            if self
                .incoming
                .values()
                .any(|incoming| incoming.temporary == entry.path())
            {
                continue;
            }
            if let Err(error) = std::fs::remove_file(entry.path()) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    return Err(FileError);
                }
            }
        }
        Ok(())
    }

    pub fn delete(&self, name: &str) -> Result<(), FileError> {
        if !valid_stored_name(name) {
            return Err(FileError);
        }
        match std::fs::remove_file(self.root.join(name)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(FileError),
        }
    }
}

pub fn encode_frame(header: &ChunkHeader, bytes: &[u8]) -> Result<Vec<u8>, FileError> {
    let json = serde_json::to_vec(header).map_err(|_| FileError)?;
    if json.len() > 64 * 1024 || bytes.len() > 192 * 1024 {
        return Err(FileError);
    }
    let mut frame = Vec::with_capacity(4 + json.len() + bytes.len());
    frame.extend_from_slice(&(json.len() as u32).to_be_bytes());
    frame.extend_from_slice(&json);
    frame.extend_from_slice(bytes);
    Ok(frame)
}

pub fn decode_frame(frame: &[u8]) -> Result<(ChunkHeader, &[u8]), FileError> {
    let length = u32::from_be_bytes(
        frame
            .get(..4)
            .ok_or(FileError)?
            .try_into()
            .map_err(|_| FileError)?,
    ) as usize;
    if length == 0
        || length > 64 * 1024
        || 4 + length > frame.len()
        || frame.len() - 4 - length > 192 * 1024
    {
        return Err(FileError);
    }
    let header: ChunkHeader =
        serde_json::from_slice(&frame[4..4 + length]).map_err(|_| FileError)?;
    if header.version != 1
        || header.message_id.is_empty()
        || header.transfer_id.is_empty()
        || header.message_id.len() > 128
        || header.transfer_id.len() > 128
        || !["file-begin", "file-chunk", "file-complete"].contains(&header.kind.as_str())
    {
        return Err(FileError);
    }
    if header.kind != "file-chunk" && (frame.len() != 4 + length || header.offset.is_some()) {
        return Err(FileError);
    }
    if header.kind == "file-chunk" && header.offset.is_none() {
        return Err(FileError);
    }
    Ok((header, &frame[4 + length..]))
}

pub fn random_id() -> Result<String, FileError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| FileError)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(feature = "native-media")]
pub struct FileTransfer {
    pub owner: String,
    pub peer: String,
    pub message: String,
    pub size: u64,
    pub sha256: String,
}

#[cfg(feature = "native-media")]
pub async fn send_file(
    mut input: tokio::fs::File,
    channel: libwebrtc::data_channel::DataChannel,
    file: FileTransfer,
    stop: tokio_util::sync::CancellationToken,
) -> Result<(), FileError> {
    use libwebrtc::data_channel::DataChannelState;
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let mut hash = Sha256::new();
    let mut verify = vec![0_u8; 1024 * 1024];
    let mut verified = 0_u64;
    loop {
        let count = tokio::select! { _ = stop.cancelled() => return Err(FileError), count = input.read(&mut verify) => count.map_err(|_| FileError)? };
        if count == 0 {
            break;
        }
        verified = verified
            .checked_add(count as u64)
            .filter(|size| *size <= file.size)
            .ok_or(FileError)?;
        hash.update(&verify[..count]);
    }
    if verified != file.size || format!("{:x}", hash.finalize()) != file.sha256 {
        return Err(FileError);
    }
    input.rewind().await.map_err(|_| FileError)?;
    let transfer = random_id()?;
    let mut header = ChunkHeader {
        version: 1,
        sender_id: file.owner,
        target_id: file.peer,
        message_id: file.message,
        transfer_id: transfer,
        kind: "file-begin".into(),
        offset: None,
    };
    send_chunk(&channel, &encode_frame(&header, &[])?, &stop).await?;
    let mut buffer = vec![0_u8; 16 * 1024];
    let mut offset = 0_u64;
    loop {
        if stop.is_cancelled() || channel.state() != DataChannelState::Open {
            return Err(FileError);
        }
        let count = tokio::select! { _ = stop.cancelled() => return Err(FileError), count = input.read(&mut buffer) => count.map_err(|_| FileError)? };
        if count == 0 {
            break;
        }
        if offset
            .checked_add(count as u64)
            .is_none_or(|end| end > file.size)
        {
            return Err(FileError);
        }
        header.kind = "file-chunk".into();
        header.offset = Some(offset);
        send_chunk(&channel, &encode_frame(&header, &buffer[..count])?, &stop).await?;
        offset += count as u64;
    }
    if offset != file.size {
        return Err(FileError);
    }
    header.kind = "file-complete".into();
    header.offset = None;
    send_chunk(&channel, &encode_frame(&header, &[])?, &stop).await
}

pub fn save_verified(
    source: File,
    target: &Path,
    size: u64,
    sha256: &str,
    stop: &CancellationToken,
) -> Result<PathBuf, FileError> {
    save_verified_if(source, target, size, sha256, stop, || true)
}

fn open_partial(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    options.open(path)
}

pub fn save_verified_if(
    mut source: File,
    target: &Path,
    size: u64,
    sha256: &str,
    stop: &CancellationToken,
    accept: impl Fn() -> bool,
) -> Result<PathBuf, FileError> {
    if !accept() || stop.is_cancelled() {
        return Err(FileError);
    }
    let parent = target
        .parent()
        .filter(|parent| parent.is_dir())
        .ok_or(FileError)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|_| FileError)?;
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut hash = Sha256::new();
    let mut written = 0_u64;
    loop {
        if stop.is_cancelled() {
            return Err(FileError);
        }
        let count = source.read(&mut buffer).map_err(|_| FileError)?;
        if count == 0 {
            break;
        }
        written = written
            .checked_add(count as u64)
            .filter(|total| *total <= size)
            .ok_or(FileError)?;
        hash.update(&buffer[..count]);
        temporary
            .write_all(&buffer[..count])
            .map_err(|_| FileError)?;
    }
    if written != size || format!("{:x}", hash.finalize()) != sha256 || stop.is_cancelled() {
        return Err(FileError);
    }
    temporary.as_file().sync_all().map_err(|_| FileError)?;
    if !accept() || stop.is_cancelled() {
        return Err(FileError);
    }
    temporary.persist(target).map_err(|_| FileError)?;
    Ok(target.to_owned())
}

pub fn reveal_saved(path: &Path) -> Result<(), FileError> {
    if !path.is_file() || !path.is_absolute() {
        return Err(FileError);
    }
    #[cfg(windows)]
    {
        let windows = std::env::var_os("WINDIR").ok_or(FileError)?;
        std::process::Command::new(PathBuf::from(windows).join("explorer.exe"))
            .arg("/select,")
            .arg(path)
            .spawn()
            .map_err(|_| FileError)?;
        Ok(())
    }
    #[cfg(not(windows))]
    Err(FileError)
}

#[cfg(feature = "native-media")]
async fn send_chunk(
    channel: &libwebrtc::data_channel::DataChannel,
    bytes: &[u8],
    stop: &tokio_util::sync::CancellationToken,
) -> Result<(), FileError> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    while channel.buffered_amount() > 1024 * 1024 {
        if channel.state() != libwebrtc::data_channel::DataChannelState::Open
            || tokio::time::Instant::now() >= deadline
        {
            return Err(FileError);
        }
        tokio::select! { _ = stop.cancelled() => return Err(FileError), _ = tokio::time::sleep(std::time::Duration::from_millis(5)) => {} }
    }
    if stop.is_cancelled() {
        return Err(FileError);
    }
    channel.send(bytes, true).map_err(|_| FileError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_order_hash_verification_and_abort_preserve_complete_files() {
        let directory = tempfile::tempdir().unwrap();
        let mut files = FileBroker::new(directory.path()).unwrap();
        let content = b"local transfer fixture";
        let hash = format!("{:x}", Sha256::digest(content));
        files
            .begin("alice", "message", "transfer", content.len() as u64, &hash)
            .unwrap();
        let mut header = ChunkHeader {
            version: 1,
            sender_id: "alice".into(),
            target_id: "bob".into(),
            message_id: "message".into(),
            transfer_id: "transfer".into(),
            kind: "file-chunk".into(),
            offset: Some(1),
        };
        assert!(files.append("alice", &header, content).is_err());
        header.offset = Some(0);
        let frame = encode_frame(&header, content).unwrap();
        let (parsed, payload) = decode_frame(&frame).unwrap();
        assert!(files.append("mallory", &parsed, payload).is_err());
        assert_eq!(files.append("alice", &parsed, payload).unwrap(), 1.0);
        let name = files.complete("alice", "message", "transfer").unwrap();
        assert_eq!(
            std::fs::read(directory.path().join(&name)).unwrap(),
            content
        );
        files
            .begin(
                "alice",
                "other",
                "corrupt",
                content.len() as u64,
                &"0".repeat(64),
            )
            .unwrap();
        header.transfer_id = "corrupt".into();
        header.message_id = "other".into();
        files.append("alice", &header, content).unwrap();
        assert!(files.complete("alice", "other", "corrupt").is_err());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        assert!(files.open_source("../session.enc").is_err());
    }

    #[test]
    fn cancellation_at_post_sync_acceptance_preserves_destination_and_removes_only_own_temporary() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.bin");
        let target = root.path().join("target.bin");
        std::fs::write(&source, b"verified").unwrap();
        std::fs::write(&target, b"previous destination").unwrap();
        let stop = CancellationToken::new();
        let checks = AtomicUsize::new(0);
        assert!(save_verified_if(
            File::open(&source).unwrap(),
            &target,
            8,
            &format!("{:x}", Sha256::digest(b"verified")),
            &stop,
            || {
                if checks.fetch_add(1, Ordering::Relaxed) == 1 {
                    stop.cancel();
                }
                true
            }
        )
        .is_err());
        assert_eq!(checks.load(Ordering::Relaxed), 2);
        assert_eq!(std::fs::read(&target).unwrap(), b"previous destination");
        assert_eq!(std::fs::read(&source).unwrap(), b"verified");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[test]
    fn recovery_never_unlinks_registered_active_partial_and_keeps_unrelated_files() {
        let root = tempfile::tempdir().unwrap();
        let mut broker = FileBroker::new(root.path()).unwrap();
        broker
            .begin(
                "peer",
                "message",
                "transfer",
                1,
                &format!("{:x}", Sha256::digest(b"a")),
            )
            .unwrap();
        let active = broker.incoming["transfer"].temporary.clone();
        let abandoned = root.path().join(format!(".{}.part", "a".repeat(32)));
        let unrelated = root.path().join("unrelated.part");
        std::fs::write(&abandoned, b"abandoned").unwrap();
        std::fs::write(&unrelated, b"keep").unwrap();
        broker.recover_partials().unwrap();
        assert!(active.exists());
        assert!(!abandoned.exists());
        assert_eq!(std::fs::read(&unrelated).unwrap(), b"keep");
        #[cfg(windows)]
        assert!(FileBroker::new(root.path())
            .unwrap()
            .recover_partials()
            .is_err());
        assert!(active.exists());
        broker.abort("transfer");
        assert!(!active.exists());
    }

    #[test]
    fn dropping_revocations_cleans_only_the_detached_original_partial() {
        let root = tempfile::tempdir().unwrap();
        let mut broker = FileBroker::new(root.path()).unwrap();
        let hash = format!("{:x}", Sha256::digest(b"a"));
        broker
            .begin("peer", "message", "transfer", 1, &hash)
            .unwrap();
        let original = broker.incoming["transfer"].temporary.clone();
        let revoked = broker.revoke_peers(&std::collections::BTreeSet::new());
        broker
            .begin("peer", "message", "transfer", 1, &hash)
            .unwrap();
        let current = broker.incoming["transfer"].temporary.clone();
        drop(revoked);
        assert!(!original.exists());
        assert!(current.exists());
        broker.abort("transfer");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
