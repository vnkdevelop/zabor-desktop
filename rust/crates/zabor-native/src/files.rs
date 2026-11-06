use crate::chat_store::valid_stored_name;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

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

impl FileBroker {
    pub fn new(root: &Path) -> Result<Self, FileError> {
        std::fs::create_dir_all(root).map_err(|_| FileError)?;
        Ok(Self {
            root: root.canonicalize().map_err(|_| FileError)?,
            incoming: BTreeMap::new(),
        })
    }

    pub fn stage(&self, source: &Path) -> Result<FileInfo, FileError> {
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
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|_| FileError)?;
            let mut buffer = vec![0_u8; 1024 * 1024];
            let mut digest = Sha256::new();
            let mut size = 0_u64;
            loop {
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
        result
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
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| FileError)?;
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
        || header.message_id.len() > 128
        || header.transfer_id.len() > 128
        || !["file-begin", "file-chunk", "file-complete"].contains(&header.kind.as_str())
    {
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
pub async fn send_file(
    mut input: tokio::fs::File,
    channel: libwebrtc::data_channel::DataChannel,
    owner: String,
    peer: String,
    message: String,
    size: u64,
    stop: tokio_util::sync::CancellationToken,
) -> Result<(), FileError> {
    use libwebrtc::data_channel::DataChannelState;
    use tokio::io::AsyncReadExt;
    let transfer = random_id()?;
    let mut header = ChunkHeader {
        version: 1,
        sender_id: owner,
        target_id: peer,
        message_id: message,
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
            .is_none_or(|end| end > size)
        {
            return Err(FileError);
        }
        header.kind = "file-chunk".into();
        header.offset = Some(offset);
        send_chunk(&channel, &encode_frame(&header, &buffer[..count])?, &stop).await?;
        offset += count as u64;
    }
    if offset != size {
        return Err(FileError);
    }
    header.kind = "file-complete".into();
    header.offset = None;
    send_chunk(&channel, &encode_frame(&header, &[])?, &stop).await
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
}
