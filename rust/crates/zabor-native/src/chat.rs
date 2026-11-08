use crate::chat_crypto::{self, ChatIdentity};
use crate::chat_store::{ChatMessage, ChatStore, RETENTION_MS};
use crate::files::{FileBroker, FileInfo, FileRevocations};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    #[error("chat storage is unavailable")]
    Storage,
    #[error("chat packet is invalid")]
    Packet,
    #[error("peer key does not match the trusted identity")]
    Identity,
}

pub struct OutboundPacket {
    pub peer: String,
    pub payload: String,
    pub direct: Option<String>,
}

struct Download {
    metadata: Value,
    registered: Value,
    transfer: Option<String>,
    touched: i64,
}

pub struct FriendRevocations {
    owner: String,
    original: Arc<()>,
    files: Option<FileRevocations>,
    downloads: Vec<(String, String, Download)>,
}

pub struct ChatEngine {
    owner: String,
    original: Arc<()>,
    identity: ChatIdentity,
    store: ChatStore,
    friends: BTreeSet<String>,
    keys: BTreeMap<String, Zeroizing<[u8; 32]>>,
    files: Option<FileBroker>,
    file_requests: Vec<(String, String)>,
    file_cancels: Vec<(String, String)>,
    downloads: BTreeMap<(String, String), Download>,
}

impl ChatEngine {
    pub fn new(owner: String, identity: ChatIdentity, store: ChatStore) -> Self {
        Self {
            owner,
            original: Arc::new(()),
            identity,
            store,
            friends: BTreeSet::new(),
            keys: BTreeMap::new(),
            files: None,
            file_requests: Vec::new(),
            file_cancels: Vec::new(),
            downloads: BTreeMap::new(),
        }
    }

    pub fn set_friends(&mut self, friends: impl IntoIterator<Item = String>) {
        let revoked = self.replace_friends(friends);
        let _ = self.finish_friend_revocations(revoked);
    }

    pub fn replace_friends(
        &mut self,
        friends: impl IntoIterator<Item = String>,
    ) -> FriendRevocations {
        self.friends = friends.into_iter().collect();
        self.keys.retain(|peer, _| self.friends.contains(peer));
        let revoked: Vec<_> = self
            .downloads
            .keys()
            .filter(|(peer, _)| !self.friends.contains(peer))
            .cloned()
            .collect();
        let downloads = revoked
            .into_iter()
            .filter_map(|(peer, id)| {
                self.downloads
                    .remove(&(peer.clone(), id.clone()))
                    .map(|download| (peer, id, download))
            })
            .collect();
        self.file_requests
            .retain(|(peer, _)| self.friends.contains(peer));
        let files = self
            .files
            .as_mut()
            .map(|files| files.revoke_peers(&self.friends));
        FriendRevocations {
            owner: self.owner.clone(),
            original: self.original.clone(),
            files,
            downloads,
        }
    }

    pub fn finish_friend_revocations(
        &mut self,
        revoked: FriendRevocations,
    ) -> Result<(), ChatError> {
        if revoked.owner != self.owner || !Arc::ptr_eq(&revoked.original, &self.original) {
            return Err(ChatError::Packet);
        }
        if let Some(files) = revoked.files {
            files.finish();
        }
        let mut failed = false;
        for (peer, id, mut download) in revoked.downloads {
            if self.downloads.contains_key(&(peer.clone(), id.clone())) {
                continue;
            }
            download.metadata["transferState"] = json!("failed");
            download.metadata["progress"] = json!(0);
            if self
                .store
                .set_file_if(
                    &self.owner,
                    &peer,
                    &id,
                    &download.registered,
                    &download.metadata,
                    download.touched,
                )
                .is_err()
            {
                failed = true;
            }
        }
        if failed {
            Err(ChatError::Storage)
        } else {
            Ok(())
        }
    }

    pub fn set_file_root(&mut self, path: &Path) -> Result<(), ChatError> {
        if self.files.is_some() {
            return Err(ChatError::Storage);
        }
        let files = FileBroker::new(path).map_err(|_| ChatError::Storage)?;
        files.recover_partials().map_err(|_| ChatError::Storage)?;
        self.store
            .recover_transfers(&self.owner)
            .map_err(|_| ChatError::Storage)?;
        self.files = Some(files);
        Ok(())
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn is_friend(&self, peer: &str) -> bool {
        self.friends.contains(peer)
    }

    pub fn saved_file(
        &self,
        peer: &str,
        id: &str,
        stored: &str,
        path: &Path,
        now: i64,
    ) -> Result<(), ChatError> {
        self.ensure_friend(peer)?;
        if !self
            .store
            .set_saved_path(&self.owner, peer, id, stored, path, now)
            .map_err(|_| ChatError::Storage)?
        {
            return Err(ChatError::Packet);
        }
        Ok(())
    }

    pub fn saved_path(
        &self,
        peer: &str,
        id: &str,
        now: i64,
    ) -> Result<Option<std::path::PathBuf>, ChatError> {
        self.ensure_friend(peer)?;
        Ok(self
            .store
            .message(&self.owner, peer, id, now)
            .map_err(|_| ChatError::Storage)?
            .and_then(|message| message.file)
            .and_then(|file| file["savedPath"].as_str().map(std::path::PathBuf::from)))
    }

    pub fn forget_saved_path(&mut self, peer: &str, id: &str, now: i64) -> Result<(), ChatError> {
        self.ensure_friend(peer)?;
        let key = (peer.into(), id.into());
        let expected = if let Some(download) = self.downloads.get(&key) {
            Some(download.registered.clone())
        } else {
            self.store
                .message(&self.owner, peer, id, now)
                .map_err(|_| ChatError::Storage)?
                .and_then(|message| message.file)
        };
        if let Some(expected) = expected {
            let mut file = expected.clone();
            file["savedPath"] = Value::Null;
            if !self
                .store
                .set_file_if(&self.owner, peer, id, &expected, &file, now)
                .map_err(|_| ChatError::Storage)?
            {
                return Ok(());
            }
            if let Some(download) = self.downloads.get_mut(&key) {
                download.registered = file;
                download.metadata["savedPath"] = Value::Null;
            }
        }
        Ok(())
    }

    pub fn file_broker(&self) -> Result<&FileBroker, ChatError> {
        self.files.as_ref().ok_or(ChatError::Storage)
    }

    pub fn send_file(
        &self,
        peer: &str,
        file: FileInfo,
        now: i64,
    ) -> Result<Vec<OutboundPacket>, ChatError> {
        self.ensure_friend(peer)?;
        let id = crate::files::random_id().map_err(|_| ChatError::Storage)?;
        let sequence = self
            .messages(peer, now)?
            .iter()
            .map(|message| message.sequence)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .filter(|sequence| *sequence <= 9_007_199_254_740_991)
            .ok_or(ChatError::Packet)?;
        let message = ChatMessage {
            id,
            owner_id: self.owner.clone(),
            conversation_id: peer.into(),
            sender_id: self.owner.clone(),
            target_id: peer.into(),
            kind: "file".into(),
            text: String::new(),
            created_at: now,
            sequence,
            delivery: "pending".into(),
            file: Some(
                json!({"name":file.name,"size":file.size,"sha256":file.sha256,"storedName":file.stored_name,"savedPath":null,"sourceAvailable":true,"transferState":"available","progress":0}),
            ),
        };
        let packet = if self.keys.contains_key(peer) {
            self.secure(
                peer,
                json!({"type":"message","message":wire_message(&message)}),
            )?
        } else {
            self.key_packet(peer, false)?
        };
        if !self
            .store
            .save(&message, now)
            .map_err(|_| ChatError::Storage)?
        {
            return Err(ChatError::Storage);
        }
        Ok(vec![packet])
    }

    pub fn request_file(
        &mut self,
        peer: &str,
        id: &str,
        now: i64,
    ) -> Result<OutboundPacket, ChatError> {
        self.ensure_friend(peer)?;
        if self.downloads.len() >= 8 || self.downloads.contains_key(&(peer.into(), id.into())) {
            return Err(ChatError::Packet);
        }
        let message = self
            .store
            .message(&self.owner, peer, id, now)
            .map_err(|_| ChatError::Storage)?
            .ok_or(ChatError::Packet)?;
        if message.sender_id != peer || message.kind != "file" {
            return Err(ChatError::Packet);
        }
        let mut metadata = message.file.ok_or(ChatError::Packet)?;
        let expected = metadata.clone();
        if let Some(name) = metadata["storedName"].as_str() {
            if self.file_broker()?.open_source(name).is_ok() {
                return Err(ChatError::Packet);
            }
            metadata["storedName"] = Value::Null;
            metadata["sourceAvailable"] = json!(false);
        }
        let packet = self.secure(peer, json!({"type":"file-request","messageId":id}))?;
        metadata["transferState"] = json!("transferring");
        metadata["progress"] = json!(0);
        if !self
            .store
            .set_file_if(&self.owner, peer, id, &expected, &metadata, now)
            .map_err(|_| ChatError::Storage)?
        {
            return Err(ChatError::Packet);
        }
        self.downloads.insert(
            (peer.into(), id.into()),
            Download {
                registered: metadata.clone(),
                metadata,
                transfer: None,
                touched: now,
            },
        );
        Ok(packet)
    }

    pub fn take_file_requests(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.file_requests)
    }
    pub fn take_file_cancels(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.file_cancels)
    }

    pub fn cancel_file(
        &mut self,
        peer: &str,
        id: &str,
        now: i64,
    ) -> Result<OutboundPacket, ChatError> {
        self.ensure_friend(peer)?;
        self.finish_download(peer, id, "missing", now)?;
        self.secure(peer, json!({"type":"file-cancel","messageId":id}))
    }

    fn finish_download(
        &mut self,
        peer: &str,
        id: &str,
        state: &str,
        now: i64,
    ) -> Result<(), ChatError> {
        if let Some(files) = self.files.as_mut() {
            files.abort_message(peer, id);
        }
        if let Some(mut download) = self.downloads.remove(&(peer.into(), id.into())) {
            download.metadata["transferState"] = json!(state);
            download.metadata["progress"] = json!(0);
            self.store
                .set_file_if(
                    &self.owner,
                    peer,
                    id,
                    &download.registered,
                    &download.metadata,
                    now.max(download.touched),
                )
                .map_err(|_| ChatError::Storage)?;
        }
        Ok(())
    }

    pub fn fail_downloads(&mut self, peer: Option<&str>, now: i64) -> Vec<OutboundPacket> {
        let pending: Vec<_> = self
            .downloads
            .keys()
            .filter(|(id, _)| peer.is_none_or(|peer| peer == id))
            .cloned()
            .collect();
        let mut packets = Vec::new();
        for (peer, id) in pending {
            let _ = self.finish_download(&peer, &id, "failed", now);
            if let Ok(packet) = self.secure(&peer, json!({"type":"file-cancel","messageId":id})) {
                packets.push(packet);
            }
        }
        packets
    }

    pub fn has_downloads(&self) -> bool {
        !self.downloads.is_empty()
    }
    pub fn is_downloading(&self, peer: &str, id: &str) -> bool {
        self.downloads.contains_key(&(peer.into(), id.into()))
    }

    pub fn clear_file(
        &mut self,
        peer: &str,
        id: &str,
        now: i64,
    ) -> Result<Vec<OutboundPacket>, ChatError> {
        self.ensure_friend(peer)?;
        self.finish_download(peer, id, "deleted", now)?;
        if self.file_cancels.len() < 8 {
            self.file_cancels.push((peer.into(), id.into()));
        }
        let message = self
            .store
            .message(&self.owner, peer, id, now)
            .map_err(|_| ChatError::Storage)?
            .ok_or(ChatError::Packet)?;
        let mut file = message.file.ok_or(ChatError::Packet)?;
        self.store
            .store_file_deletion(&self.owner, peer, id, false, now)
            .map_err(|_| ChatError::Storage)?;
        if let Some(name) = file["storedName"].as_str() {
            self.file_broker()?
                .delete(name)
                .map_err(|_| ChatError::Storage)?;
        }
        file["storedName"] = Value::Null;
        file["savedPath"] = Value::Null;
        file["sourceAvailable"] = json!(false);
        file["progress"] = json!(0);
        file["transferState"] = json!("deleted");
        self.store
            .set_file(&self.owner, peer, id, &file, now)
            .map_err(|_| ChatError::Storage)?;
        Ok(vec![if self.keys.contains_key(peer) {
            self.secure(peer, json!({"type":"file-delete","messageId":id}))?
        } else {
            self.key_packet(peer, false)?
        }])
    }

    pub fn expire_downloads(&mut self, now: i64) -> Vec<OutboundPacket> {
        let expired: Vec<_> = self
            .downloads
            .iter()
            .filter(|(_, item)| now.saturating_sub(item.touched) >= 30_000)
            .map(|(key, _)| key.clone())
            .collect();
        let mut packets = Vec::new();
        for (peer, id) in expired {
            let _ = self.finish_download(&peer, &id, "failed", now);
            if let Ok(packet) = self.secure(&peer, json!({"type":"file-cancel","messageId":id})) {
                packets.push(packet);
            }
        }
        packets
    }

    pub fn cached_file(
        &self,
        peer: &str,
        id: &str,
        now: i64,
    ) -> Result<(std::fs::File, FileInfo), ChatError> {
        self.ensure_friend(peer)?;
        let message = self
            .store
            .message(&self.owner, peer, id, now)
            .map_err(|_| ChatError::Storage)?
            .ok_or(ChatError::Packet)?;
        let metadata = message.file.ok_or(ChatError::Packet)?;
        Ok((
            self.file_broker()?
                .open_source(metadata["storedName"].as_str().ok_or(ChatError::Packet)?)
                .map_err(|_| ChatError::Storage)?,
            FileInfo {
                name: metadata["name"].as_str().ok_or(ChatError::Packet)?.into(),
                size: metadata["size"].as_u64().ok_or(ChatError::Packet)?,
                sha256: metadata["sha256"].as_str().ok_or(ChatError::Packet)?.into(),
                stored_name: metadata["storedName"]
                    .as_str()
                    .ok_or(ChatError::Packet)?
                    .into(),
            },
        ))
    }

    pub fn unavailable(&self, peer: &str, id: &str) -> Result<OutboundPacket, ChatError> {
        self.secure(peer, json!({"type":"file-unavailable","messageId":id}))
    }

    pub fn outgoing_file(
        &self,
        peer: &str,
        id: &str,
        now: i64,
    ) -> Result<(std::fs::File, u64, String), ChatError> {
        self.ensure_friend(peer)?;
        let message = self
            .store
            .message(&self.owner, peer, id, now)
            .map_err(|_| ChatError::Storage)?
            .filter(|message| message.sender_id == self.owner)
            .ok_or(ChatError::Packet)?;
        let metadata = message.file.as_ref().ok_or(ChatError::Packet)?;
        let file = self
            .file_broker()?
            .open_source(metadata["storedName"].as_str().ok_or(ChatError::Packet)?)
            .map_err(|_| ChatError::Storage)?;
        let size = metadata["size"].as_u64().ok_or(ChatError::Packet)?;
        if file.metadata().map_err(|_| ChatError::Storage)?.len() != size {
            return Err(ChatError::Storage);
        }
        Ok((
            file,
            size,
            metadata["sha256"].as_str().ok_or(ChatError::Packet)?.into(),
        ))
    }

    pub fn receive_file(
        &mut self,
        peer: &str,
        frame: &[u8],
        now: i64,
    ) -> Result<(String, f32, bool), ChatError> {
        self.ensure_friend(peer)?;
        let (header, bytes) = crate::files::decode_frame(frame).map_err(|_| ChatError::Packet)?;
        if header.sender_id != peer || header.target_id != self.owner {
            return Err(ChatError::Packet);
        }
        let key = (peer.to_owned(), header.message_id.clone());
        let download = self.downloads.get_mut(&key).ok_or(ChatError::Packet)?;
        if (header.kind == "file-begin" && download.transfer.is_some())
            || (header.kind != "file-begin"
                && download.transfer.as_deref() != Some(&header.transfer_id))
        {
            return Err(ChatError::Packet);
        }
        let metadata = &mut download.metadata;
        let files = self.files.as_mut().ok_or(ChatError::Storage)?;
        let result = (|| {
            let progress = match header.kind.as_str() {
                "file-begin" => {
                    files
                        .begin(
                            peer,
                            &header.message_id,
                            &header.transfer_id,
                            metadata["size"].as_u64().ok_or(ChatError::Packet)?,
                            metadata["sha256"].as_str().ok_or(ChatError::Packet)?,
                        )
                        .map_err(|_| ChatError::Storage)?;
                    metadata["transferState"] = json!("transferring");
                    download.transfer = Some(header.transfer_id.clone());
                    0.0
                }
                "file-chunk" => files
                    .append(peer, &header, bytes)
                    .map_err(|_| ChatError::Storage)?,
                "file-complete" => {
                    let name = files
                        .complete(peer, &header.message_id, &header.transfer_id)
                        .map_err(|_| ChatError::Storage)?;
                    let mut completed = metadata.clone();
                    completed["storedName"] = json!(name);
                    completed["transferState"] = json!("available");
                    completed["sourceAvailable"] = json!(true);
                    completed["progress"] = json!(1);
                    if !matches!(
                        self.store.set_file_if(
                            &self.owner,
                            peer,
                            &header.message_id,
                            &download.registered,
                            &completed,
                            now
                        ),
                        Ok(true)
                    ) {
                        let _ = files.delete(&name);
                        return Err(ChatError::Storage);
                    }
                    *metadata = completed;
                    1.0
                }
                _ => return Err(ChatError::Packet),
            };
            metadata["progress"] = json!(progress);
            download.touched = now;
            Ok((
                header.message_id.clone(),
                progress,
                header.kind == "file-complete",
            ))
        })();
        match result {
            Ok(result) => {
                if result.2 {
                    self.downloads.remove(&key);
                }
                Ok(result)
            }
            Err(error) => {
                let _ = self.finish_download(peer, &header.message_id, "failed", now);
                Err(error)
            }
        }
    }

    pub fn open(&self, peer: &str) -> Result<OutboundPacket, ChatError> {
        self.ensure_friend(peer)?;
        self.key_packet(peer, false)
    }

    pub fn messages(&self, peer: &str, now: i64) -> Result<Vec<ChatMessage>, ChatError> {
        self.ensure_friend(peer)?;
        let mut messages = self
            .store
            .conversation(&self.owner, peer, now)
            .map_err(|_| ChatError::Storage)?;
        for message in &mut messages {
            if let Some(download) = self.downloads.get(&(peer.into(), message.id.clone())) {
                message.file = Some(download.metadata.clone());
            }
        }
        Ok(messages)
    }

    pub fn send_text(
        &self,
        peer: &str,
        text: &str,
        now: i64,
    ) -> Result<Vec<OutboundPacket>, ChatError> {
        self.ensure_friend(peer)?;
        if text.trim().is_empty() || text.encode_utf16().count() > 4000 {
            return Err(ChatError::Packet);
        }
        let mut id = [0_u8; 16];
        getrandom::fill(&mut id).map_err(|_| ChatError::Packet)?;
        let id = id
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let sequence = self
            .messages(peer, now)?
            .iter()
            .map(|message| message.sequence)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .filter(|value| *value <= 9_007_199_254_740_991)
            .ok_or(ChatError::Packet)?;
        let mut message = ChatMessage {
            id,
            owner_id: self.owner.clone(),
            conversation_id: peer.into(),
            sender_id: self.owner.clone(),
            target_id: peer.into(),
            kind: "text".into(),
            text: text.trim().into(),
            created_at: now,
            sequence,
            delivery: "pending".into(),
            file: None,
        };
        self.store
            .save(&message, now)
            .map_err(|_| ChatError::Storage)?;
        if !self.keys.contains_key(peer) {
            return Ok(vec![self.key_packet(peer, false)?]);
        }
        let packet = self.secure(
            peer,
            json!({"type":"message","message":wire_message(&message)}),
        )?;
        message.delivery = "sent".into();
        self.store
            .save(&message, now)
            .map_err(|_| ChatError::Storage)?;
        Ok(vec![packet])
    }

    pub fn mark_read(&self, peer: &str, now: i64) -> Result<Vec<OutboundPacket>, ChatError> {
        let messages = self.messages(peer, now)?;
        let mut sequence = 0;
        let mut changed = false;
        for message in messages
            .into_iter()
            .filter(|message| message.sender_id == peer)
        {
            sequence = sequence.max(message.sequence);
            if message.delivery != "read" {
                changed |= self
                    .store
                    .set_delivery(&self.owner, peer, &message.id, "read", now)
                    .map_err(|_| ChatError::Storage)?;
            }
        }
        if !changed || sequence == 0 || !self.keys.contains_key(peer) {
            return Ok(Vec::new());
        }
        Ok(vec![self.secure(
            peer,
            json!({"type":"read-through","sequence":sequence}),
        )?])
    }

    pub fn remove(
        &mut self,
        peer: &str,
        id: &str,
        now: i64,
    ) -> Result<Vec<OutboundPacket>, ChatError> {
        self.ensure_friend(peer)?;
        self.finish_download(peer, id, "missing", now)?;
        self.file_cancels.push((peer.into(), id.into()));
        if let Some(message) = self
            .store
            .message(&self.owner, peer, id, now)
            .map_err(|_| ChatError::Storage)?
        {
            if let Some(name) = message
                .file
                .as_ref()
                .and_then(|file| file["storedName"].as_str())
            {
                self.file_broker()?
                    .delete(name)
                    .map_err(|_| ChatError::Storage)?;
            }
        }
        self.store
            .store_deletion(&self.owner, peer, id, false)
            .map_err(|_| ChatError::Storage)?;
        self.store
            .delete_from_conversation(&self.owner, peer, id)
            .map_err(|_| ChatError::Storage)?;
        if self.keys.contains_key(peer) {
            Ok(vec![self.secure(
                peer,
                json!({"type":"message-delete","messageId":id}),
            )?])
        } else {
            Ok(vec![self.key_packet(peer, false)?])
        }
    }

    pub fn receive(
        &mut self,
        peer: &str,
        serialized: &str,
        now: i64,
    ) -> Result<Vec<OutboundPacket>, ChatError> {
        self.ensure_friend(peer)?;
        if serialized.len() > 64 * 1024 {
            return Err(ChatError::Packet);
        }
        let packet: Value = serde_json::from_str(serialized).map_err(|_| ChatError::Packet)?;
        self.validate_packet(peer, &packet)?;
        if packet["type"] == "key" {
            let public = &packet["publicKey"];
            let key = self
                .identity
                .derive(public)
                .map_err(|_| ChatError::Identity)?;
            #[derive(Serialize)]
            struct Fingerprint<'a> {
                kty: &'a str,
                crv: &'a str,
                x: &'a str,
                y: &'a str,
            }
            let fingerprint = serde_json::to_string(&Fingerprint {
                kty: "EC",
                crv: "P-256",
                x: public["x"].as_str().ok_or(ChatError::Packet)?,
                y: public["y"].as_str().ok_or(ChatError::Packet)?,
            })
            .map_err(|_| ChatError::Packet)?;
            if !self
                .store
                .accept_peer_key(&self.owner, peer, &fingerprint)
                .map_err(|_| ChatError::Storage)?
            {
                return Err(ChatError::Identity);
            }
            let had_key = self.keys.insert(peer.into(), key).is_some();
            let mut output = Vec::new();
            if packet["reply"] != true && !had_key {
                output.push(self.key_packet(peer, true)?);
            }
            for message in self.messages(peer, now)?.into_iter().filter(|message| {
                message.sender_id == self.owner
                    && ["pending", "sent"].contains(&message.delivery.as_str())
            }) {
                output.push(self.secure(
                    peer,
                    json!({"type":"message","message":wire_message(&message)}),
                )?);
            }
            for id in self
                .store
                .deletions(&self.owner, peer, false)
                .map_err(|_| ChatError::Storage)?
            {
                output.push(self.secure(peer, json!({"type":"message-delete","messageId":id}))?);
            }
            for id in self
                .store
                .file_deletions(&self.owner, peer, false, now)
                .map_err(|_| ChatError::Storage)?
            {
                output.push(self.secure(peer, json!({"type":"file-delete","messageId":id}))?);
            }
            if !had_key {
                output.push(self.secure(peer, json!({"type":"sync-request"}))?);
            }
            return Ok(output);
        }
        if packet["type"] != "encrypted" {
            return Err(ChatError::Packet);
        }
        let Some(key) = self.keys.get(peer) else {
            return Ok(vec![self.key_packet(peer, false)?]);
        };
        let plaintext =
            chat_crypto::decrypt(key, packet["payload"].as_str().ok_or(ChatError::Packet)?)
                .map_err(|_| ChatError::Packet)?;
        let inner: Value = serde_json::from_str(&plaintext).map_err(|_| ChatError::Packet)?;
        self.validate_packet(peer, &inner)?;
        self.control(peer, inner, now)
    }

    pub fn prune(&mut self, now: i64, files: &Path) -> Result<usize, ChatError> {
        self.store
            .prune(&self.owner, now, files)
            .map_err(|_| ChatError::Storage)
    }

    pub fn receive_direct(
        &mut self,
        peer: &str,
        serialized: &str,
        now: i64,
    ) -> Result<Vec<OutboundPacket>, ChatError> {
        self.ensure_friend(peer)?;
        if serialized.len() > 64 * 1024 {
            return Err(ChatError::Packet);
        }
        let packet: Value = serde_json::from_str(serialized).map_err(|_| ChatError::Packet)?;
        self.validate_packet(peer, &packet)?;
        if packet["type"] == "key" || packet["type"] == "encrypted" {
            return self.receive(peer, serialized, now);
        }
        self.control(peer, packet, now)
    }

    fn control(
        &mut self,
        peer: &str,
        packet: Value,
        now: i64,
    ) -> Result<Vec<OutboundPacket>, ChatError> {
        let messages = self.messages(peer, now)?;
        let mut output = Vec::new();
        match packet["type"].as_str().ok_or(ChatError::Packet)? {
            "message" => {
                let raw = &packet["message"];
                let id = raw["id"]
                    .as_str()
                    .filter(|id| !id.is_empty() && id.len() <= 128)
                    .ok_or(ChatError::Packet)?;
                if raw["senderId"] != peer
                    || raw["targetId"] != self.owner
                    || ![Some("text"), Some("file")].contains(&raw["kind"].as_str())
                {
                    return Err(ChatError::Packet);
                }
                let text = raw["text"]
                    .as_str()
                    .filter(|text| text.encode_utf16().count() <= 4000)
                    .ok_or(ChatError::Packet)?;
                let mut file = if raw["kind"] == "file" {
                    let file = &raw["file"];
                    let name = file["name"]
                        .as_str()
                        .filter(|name| !name.is_empty() && name.encode_utf16().count() <= 255)
                        .ok_or(ChatError::Packet)?;
                    let size = file["size"]
                        .as_u64()
                        .filter(|size| *size > 0 && *size <= 9_007_199_254_740_991)
                        .ok_or(ChatError::Packet)?;
                    let hash = file["sha256"]
                        .as_str()
                        .filter(|hash| {
                            hash.len() == 64
                                && hash.bytes().all(|byte| {
                                    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
                                })
                        })
                        .ok_or(ChatError::Packet)?;
                    if !text.is_empty() {
                        return Err(ChatError::Packet);
                    }
                    Some(
                        json!({"name":name,"size":size,"sha256":hash,"storedName":null,"savedPath":null,"sourceAvailable":false,"transferState":"missing","progress":0}),
                    )
                } else {
                    if text.trim().is_empty() || !raw["file"].is_null() {
                        return Err(ChatError::Packet);
                    }
                    None
                };
                if let Some(file) = &mut file {
                    if self
                        .store
                        .file_deletions(&self.owner, peer, true, now)
                        .map_err(|_| ChatError::Storage)?
                        .iter()
                        .chain(
                            self.store
                                .file_deletions(&self.owner, peer, false, now)
                                .map_err(|_| ChatError::Storage)?
                                .iter(),
                        )
                        .any(|deleted| deleted == id)
                    {
                        file["transferState"] = json!("deleted");
                    }
                }
                let created_at = raw["createdAt"]
                    .as_i64()
                    .filter(|created| {
                        *created > 0
                            && *created > now.saturating_sub(RETENTION_MS)
                            && *created <= now.saturating_add(300000)
                    })
                    .ok_or(ChatError::Packet)?;
                let sequence = raw["sequence"]
                    .as_i64()
                    .filter(|sequence| *sequence > 0 && *sequence <= 9_007_199_254_740_991)
                    .ok_or(ChatError::Packet)?;
                if self
                    .store
                    .deletions(&self.owner, peer, true)
                    .map_err(|_| ChatError::Storage)?
                    .iter()
                    .any(|deleted| deleted == id)
                {
                    return Ok(output);
                }
                if !messages.iter().any(|message| message.id == id) {
                    let message = ChatMessage {
                        id: id.into(),
                        owner_id: self.owner.clone(),
                        conversation_id: peer.into(),
                        sender_id: peer.into(),
                        target_id: self.owner.clone(),
                        kind: raw["kind"].as_str().unwrap().into(),
                        text: text.into(),
                        created_at,
                        sequence,
                        delivery: "delivered".into(),
                        file,
                    };
                    self.store
                        .save(&message, now)
                        .map_err(|_| ChatError::Storage)?;
                }
                output.push(self.secure(peer, json!({"type":"ack","messageId":id}))?);
            }
            "ack" | "ack-through" | "read" | "read-through" => {
                let read = packet["type"].as_str().unwrap().starts_with("read");
                let id = packet["messageId"].as_str();
                let sequence = packet["sequence"].as_i64();
                for message in messages.into_iter().filter(|message| {
                    message.sender_id == self.owner
                        && (id == Some(message.id.as_str())
                            || sequence.is_some_and(|sequence| message.sequence <= sequence))
                }) {
                    if message.delivery == "read" {
                        continue;
                    }
                    self.store
                        .set_delivery(
                            &self.owner,
                            peer,
                            &message.id,
                            if read { "read" } else { "delivered" },
                            now,
                        )
                        .map_err(|_| ChatError::Storage)?;
                }
            }
            "sync-request" => {
                let mut sequence = 0;
                let mut acknowledged = 0;
                let mut missing = Vec::new();
                for message in messages.iter().filter(|message| message.sender_id == peer) {
                    if message.sequence == acknowledged + 1 {
                        acknowledged = message.sequence;
                    }
                    if message.sequence > sequence + 1 {
                        missing.push([sequence + 1, message.sequence - 1]);
                    }
                    sequence = sequence.max(message.sequence);
                }
                let missing = &missing[missing.len().saturating_sub(64)..];
                output.push(self.secure(
                    peer,
                    json!({"type":"sync-state","sequence":sequence,"missing":missing}),
                )?);
                if acknowledged > 0 {
                    output.push(
                        self.secure(peer, json!({"type":"ack-through","sequence":acknowledged}))?,
                    );
                }
            }
            "sync-state" => {
                let sequence = packet["sequence"].as_i64().unwrap_or(0);
                let ranges = packet["missing"].as_array();
                for message in messages
                    .iter()
                    .filter(|message| message.sender_id == self.owner)
                {
                    let missing = ranges.is_some_and(|ranges| {
                        ranges.iter().take(64).any(|range| {
                            range[0]
                                .as_i64()
                                .is_some_and(|start| message.sequence >= start)
                                && range[1].as_i64().is_some_and(|end| message.sequence <= end)
                        })
                    });
                    if message.sequence > sequence || missing {
                        output.push(self.secure(
                            peer,
                            json!({"type":"message","message":wire_message(message)}),
                        )?);
                    }
                }
                output.push(self.secure(peer, json!({"type":"sync-complete"}))?);
            }
            "message-delete" => {
                let id = packet["messageId"]
                    .as_str()
                    .filter(|id| id.len() <= 128)
                    .ok_or(ChatError::Packet)?;
                self.finish_download(peer, id, "missing", now)?;
                if self.file_cancels.len() < 8 {
                    self.file_cancels.push((peer.into(), id.into()));
                }
                self.store
                    .store_deletion(&self.owner, peer, id, true)
                    .map_err(|_| ChatError::Storage)?;
                if let Some(name) = messages
                    .iter()
                    .find(|message| message.id == id)
                    .and_then(|message| message.file.as_ref())
                    .and_then(|file| file["storedName"].as_str())
                {
                    self.file_broker()?
                        .delete(name)
                        .map_err(|_| ChatError::Storage)?;
                }
                self.store
                    .delete_from_conversation(&self.owner, peer, id)
                    .map_err(|_| ChatError::Storage)?;
            }
            "ping" => {
                output.push(self.secure(peer, json!({"type":"pong","sentAt":packet["sentAt"]}))?)
            }
            "pong" | "sync-complete" => {}
            "file-request" => {
                let id = packet["messageId"]
                    .as_str()
                    .filter(|id| !id.is_empty() && id.len() <= 128)
                    .ok_or(ChatError::Packet)?;
                if !messages.iter().any(|message| {
                    message.id == id && message.sender_id == self.owner && message.kind == "file"
                }) {
                    output.push(self.unavailable(peer, id)?);
                    return Ok(output);
                }
                if self.file_requests.len() < 8
                    && !self
                        .file_requests
                        .iter()
                        .any(|(friend, message)| friend == peer && message == id)
                {
                    self.file_requests.push((peer.into(), id.into()));
                }
            }
            "file-unavailable" | "file-delete" => {
                if packet["type"] == "file-delete" {
                    let id = packet["messageId"]
                        .as_str()
                        .filter(|id| !id.is_empty() && id.len() <= 128)
                        .ok_or(ChatError::Packet)?;
                    self.store
                        .store_file_deletion(&self.owner, peer, id, true, now)
                        .map_err(|_| ChatError::Storage)?;
                    if self.file_cancels.len() < 8 {
                        self.file_cancels.push((peer.into(), id.into()));
                    }
                }
                if packet["type"] == "file-unavailable"
                    && !self.downloads.contains_key(&(
                        peer.into(),
                        packet["messageId"].as_str().unwrap_or("").into(),
                    ))
                {
                    return Ok(output);
                }
                if let Some(id) = packet["messageId"].as_str() {
                    self.finish_download(peer, id, "deleted", now)?;
                }
                if let Some(mut message) = messages.into_iter().find(|message| {
                    (packet["type"] == "file-delete" || message.sender_id == peer)
                        && Some(message.id.as_str()) == packet["messageId"].as_str()
                }) {
                    if let Some(file) = message.file.as_mut() {
                        if let Some(name) = file["storedName"].as_str() {
                            self.file_broker()?
                                .delete(name)
                                .map_err(|_| ChatError::Storage)?;
                        }
                        file["storedName"] = Value::Null;
                        file["transferState"] = json!("deleted");
                        file["sourceAvailable"] = json!(false);
                        self.store
                            .save(&message, now)
                            .map_err(|_| ChatError::Storage)?;
                    }
                }
            }
            "file-cancel" => {
                if let Some(id) = packet["messageId"].as_str() {
                    if self.file_cancels.len() < 8 {
                        self.file_cancels.push((peer.into(), id.into()));
                    }
                }
            }
            _ => return Err(ChatError::Packet),
        }
        Ok(output)
    }

    fn ensure_friend(&self, peer: &str) -> Result<(), ChatError> {
        if self.friends.contains(peer) {
            Ok(())
        } else {
            Err(ChatError::Packet)
        }
    }

    fn validate_packet(&self, peer: &str, packet: &Value) -> Result<(), ChatError> {
        if packet["version"] != 1 || packet["senderId"] != peer || packet["targetId"] != self.owner
        {
            return Err(ChatError::Packet);
        }
        Ok(())
    }

    fn key_packet(&self, peer: &str, reply: bool) -> Result<OutboundPacket, ChatError> {
        Ok(OutboundPacket { peer: peer.into(), direct: None, payload: json!({"version":1,"senderId":self.owner,"targetId":peer,"type":"key","reply":reply,"publicKey":self.identity.public_key().map_err(|_| ChatError::Identity)?}).to_string() })
    }

    fn secure(&self, peer: &str, mut packet: Value) -> Result<OutboundPacket, ChatError> {
        let key = self.keys.get(peer).ok_or(ChatError::Identity)?;
        packet["version"] = json!(1);
        packet["senderId"] = json!(self.owner);
        packet["targetId"] = json!(peer);
        let direct = packet.to_string();
        let payload =
            chat_crypto::encrypt(key, &packet.to_string()).map_err(|_| ChatError::Packet)?;
        let payload = json!({"version":1,"senderId":self.owner,"targetId":peer,"type":"encrypted","payload":payload}).to_string();
        if payload.len() > 64 * 1024 {
            return Err(ChatError::Packet);
        }
        Ok(OutboundPacket {
            peer: peer.into(),
            payload,
            direct: Some(direct),
        })
    }
}

fn wire_message(message: &ChatMessage) -> Value {
    let file = message
        .file
        .as_ref()
        .map(|file| json!({"name":file["name"],"size":file["size"],"sha256":file["sha256"]}));
    json!({"id":message.id,"senderId":message.sender_id,"targetId":message.target_id,"kind":message.kind,"text":message.text,"createdAt":message.created_at,"sequence":message.sequence,"file":file})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[test]
    fn encrypted_delivery_ack_read_and_key_pinning_survive_storage() {
        let root = tempfile::tempdir().unwrap();
        let mut alice = ChatEngine::new(
            "alice".into(),
            ChatIdentity::generate(),
            ChatStore::open(&root.path().join("alice.sqlite")).unwrap(),
        );
        let mut bob = ChatEngine::new(
            "bob".into(),
            ChatIdentity::generate(),
            ChatStore::open(&root.path().join("bob.sqlite")).unwrap(),
        );
        alice.set_friends(["bob".into()]);
        bob.set_friends(["alice".into()]);
        let now = RETENTION_MS * 3;
        let mut queue: VecDeque<_> = alice
            .send_text("bob", "local encrypted fixture", now)
            .unwrap()
            .into_iter()
            .map(|packet| ("alice", packet))
            .collect();
        let mut delivered = 0;
        while let Some((sender, packet)) = queue.pop_front() {
            assert!(!packet.payload.contains("local encrypted fixture"));
            let (next_sender, replies) = if sender == "alice" {
                ("bob", bob.receive(sender, &packet.payload, now).unwrap())
            } else {
                (
                    "alice",
                    alice.receive(sender, &packet.payload, now).unwrap(),
                )
            };
            queue.extend(replies.into_iter().map(|packet| (next_sender, packet)));
            delivered += 1;
            assert!(delivered < 32);
        }
        assert_eq!(
            bob.messages("alice", now).unwrap()[0].text,
            "local encrypted fixture"
        );
        assert_eq!(alice.messages("bob", now).unwrap()[0].delivery, "delivered");
        for packet in bob.mark_read("alice", now).unwrap() {
            alice.receive("bob", &packet.payload, now).unwrap();
        }
        assert!(bob.mark_read("alice", now).unwrap().is_empty());
        assert_eq!(alice.messages("bob", now).unwrap()[0].delivery, "read");
        let mut replacement = ChatEngine::new(
            "bob".into(),
            ChatIdentity::generate(),
            ChatStore::open(&root.path().join("other.sqlite")).unwrap(),
        );
        replacement.set_friends(["alice".into()]);
        assert!(matches!(
            alice.receive("bob", &replacement.open("alice").unwrap().payload, now),
            Err(ChatError::Identity)
        ));
    }
}
