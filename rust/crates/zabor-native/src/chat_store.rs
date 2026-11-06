use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const RETENTION_MS: i64 = 14 * 24 * 60 * 60 * 1000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub owner_id: String,
    pub conversation_id: String,
    pub sender_id: String,
    pub target_id: String,
    pub kind: String,
    pub text: String,
    pub created_at: i64,
    pub sequence: i64,
    pub delivery: String,
    pub file: Option<serde_json::Value>,
}

#[derive(Debug, thiserror::Error)]
#[error("chat storage operation failed")]
pub struct StoreError;

pub struct ChatStore {
    connection: Connection,
}

impl ChatStore {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let connection = Connection::open(path).map_err(|_| StoreError)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(2))
            .map_err(|_| StoreError)?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS messages(owner TEXT NOT NULL,id TEXT NOT NULL,conversation TEXT NOT NULL,created INTEGER NOT NULL,sequence INTEGER NOT NULL,body TEXT NOT NULL,PRIMARY KEY(owner,id)); CREATE INDEX IF NOT EXISTS messages_conversation ON messages(owner,conversation,sequence,created); CREATE INDEX IF NOT EXISTS messages_retention ON messages(owner,created);").map_err(|_| StoreError)?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS file_deletions(owner TEXT NOT NULL,peer TEXT NOT NULL,id TEXT NOT NULL,incoming INTEGER NOT NULL,created INTEGER NOT NULL,PRIMARY KEY(owner,peer,id,incoming));").map_err(|_| StoreError)?;
        Ok(Self { connection })
    }

    pub fn save(&self, message: &ChatMessage, now: i64) -> Result<bool, StoreError> {
        if message.created_at <= now.saturating_sub(RETENTION_MS) {
            return Ok(false);
        }
        if message.owner_id.is_empty()
            || message.id.is_empty()
            || message.conversation_id.is_empty()
            || message.sequence < 0
            || !["text", "file"].contains(&message.kind.as_str())
            || !["pending", "sent", "delivered", "read", "failed"]
                .contains(&message.delivery.as_str())
        {
            return Err(StoreError);
        }
        let body = serde_json::to_string(message).map_err(|_| StoreError)?;
        if body.len() > 2 * 1024 * 1024 {
            return Err(StoreError);
        }
        self.connection.execute("INSERT INTO messages(owner,id,conversation,created,sequence,body) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(owner,id) DO UPDATE SET conversation=excluded.conversation,created=excluded.created,sequence=excluded.sequence,body=excluded.body", params![message.owner_id,message.id,message.conversation_id,message.created_at,message.sequence,body]).map_err(|_| StoreError)?;
        Ok(true)
    }

    pub fn conversation(
        &self,
        owner: &str,
        conversation: &str,
        now: i64,
    ) -> Result<Vec<ChatMessage>, StoreError> {
        let mut query = self.connection.prepare("SELECT body FROM messages WHERE owner=?1 AND conversation=?2 AND created>?3 ORDER BY sequence,created,id").map_err(|_| StoreError)?;
        let rows = query
            .query_map(
                params![owner, conversation, now.saturating_sub(RETENTION_MS)],
                |row| row.get::<_, String>(0),
            )
            .map_err(|_| StoreError)?;
        rows.map(|row| serde_json::from_str(&row.map_err(|_| StoreError)?).map_err(|_| StoreError))
            .collect()
    }

    pub fn delete(&self, owner: &str, id: &str) -> Result<(), StoreError> {
        self.connection
            .execute(
                "DELETE FROM messages WHERE owner=?1 AND id=?2",
                params![owner, id],
            )
            .map_err(|_| StoreError)?;
        Ok(())
    }

    pub fn message(
        &self,
        owner: &str,
        peer: &str,
        id: &str,
        now: i64,
    ) -> Result<Option<ChatMessage>, StoreError> {
        let body: Option<String> = self.connection.query_row("SELECT body FROM messages WHERE owner=?1 AND conversation=?2 AND id=?3 AND created>?4", params![owner, peer, id, now.saturating_sub(RETENTION_MS)], |row| row.get(0)).optional().map_err(|_| StoreError)?;
        body.map(|body| serde_json::from_str(&body).map_err(|_| StoreError))
            .transpose()
    }

    pub fn set_file(
        &self,
        owner: &str,
        peer: &str,
        id: &str,
        file: &serde_json::Value,
        now: i64,
    ) -> Result<bool, StoreError> {
        let file = serde_json::to_string(file).map_err(|_| StoreError)?;
        self.connection.execute("UPDATE messages SET body=json_set(body,'$.file',json(?1)) WHERE owner=?2 AND conversation=?3 AND id=?4 AND created>?5 AND json_extract(body,'$.kind')='file'", params![file, owner, peer, id, now.saturating_sub(RETENTION_MS)]).map(|count| count == 1).map_err(|_| StoreError)
    }

    pub fn store_file_deletion(
        &self,
        owner: &str,
        peer: &str,
        id: &str,
        incoming: bool,
        now: i64,
    ) -> Result<(), StoreError> {
        self.connection.execute("INSERT INTO file_deletions(owner,peer,id,incoming,created) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(owner,peer,id,incoming) DO UPDATE SET created=excluded.created", params![owner, peer, id, incoming, now]).map_err(|_| StoreError)?;
        Ok(())
    }

    pub fn recover_transfers(&self, owner: &str) -> Result<(), StoreError> {
        self.connection.execute("UPDATE messages SET body=json_set(body,'$.file.transferState','failed','$.file.progress',0) WHERE owner=?1 AND json_extract(body,'$.file.transferState')='transferring'", params![owner]).map_err(|_| StoreError)?;
        Ok(())
    }

    pub fn file_deletions(
        &self,
        owner: &str,
        peer: &str,
        incoming: bool,
        now: i64,
    ) -> Result<Vec<String>, StoreError> {
        let mut query = self.connection.prepare("SELECT id FROM file_deletions WHERE owner=?1 AND peer=?2 AND incoming=?3 AND created>?4").map_err(|_| StoreError)?;
        let rows = query
            .query_map(
                params![owner, peer, incoming, now.saturating_sub(RETENTION_MS)],
                |row| row.get(0),
            )
            .map_err(|_| StoreError)?;
        rows.map(|row| row.map_err(|_| StoreError)).collect()
    }

    pub fn delete_from_conversation(
        &self,
        owner: &str,
        peer: &str,
        id: &str,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                "DELETE FROM messages WHERE owner=?1 AND conversation=?2 AND id=?3",
                params![owner, peer, id],
            )
            .map_err(|_| StoreError)?;
        Ok(())
    }

    pub fn accept_peer_key(
        &self,
        owner: &str,
        peer: &str,
        fingerprint: &str,
    ) -> Result<bool, StoreError> {
        self.connection.execute_batch("CREATE TABLE IF NOT EXISTS trusted_keys(owner TEXT NOT NULL,peer TEXT NOT NULL,fingerprint TEXT NOT NULL,PRIMARY KEY(owner,peer))").map_err(|_| StoreError)?;
        self.connection
            .execute(
                "INSERT OR IGNORE INTO trusted_keys(owner,peer,fingerprint) VALUES(?1,?2,?3)",
                params![owner, peer, fingerprint],
            )
            .map_err(|_| StoreError)?;
        let stored: String = self
            .connection
            .query_row(
                "SELECT fingerprint FROM trusted_keys WHERE owner=?1 AND peer=?2",
                params![owner, peer],
                |row| row.get(0),
            )
            .map_err(|_| StoreError)?;
        Ok(stored == fingerprint)
    }

    pub fn store_deletion(
        &self,
        owner: &str,
        peer: &str,
        id: &str,
        incoming: bool,
    ) -> Result<(), StoreError> {
        self.connection.execute_batch("CREATE TABLE IF NOT EXISTS deleted_messages(owner TEXT NOT NULL,peer TEXT NOT NULL,id TEXT NOT NULL,incoming INTEGER NOT NULL,PRIMARY KEY(owner,peer,id,incoming))").map_err(|_| StoreError)?;
        self.connection.execute("INSERT OR IGNORE INTO deleted_messages(owner,peer,id,incoming) VALUES(?1,?2,?3,?4)", params![owner,peer,id,incoming]).map_err(|_| StoreError)?;
        Ok(())
    }

    pub fn deletions(
        &self,
        owner: &str,
        peer: &str,
        incoming: bool,
    ) -> Result<Vec<String>, StoreError> {
        self.connection.execute_batch("CREATE TABLE IF NOT EXISTS deleted_messages(owner TEXT NOT NULL,peer TEXT NOT NULL,id TEXT NOT NULL,incoming INTEGER NOT NULL,PRIMARY KEY(owner,peer,id,incoming))").map_err(|_| StoreError)?;
        let mut query = self
            .connection
            .prepare("SELECT id FROM deleted_messages WHERE owner=?1 AND peer=?2 AND incoming=?3")
            .map_err(|_| StoreError)?;
        let rows = query
            .query_map(params![owner, peer, incoming], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| StoreError)?;
        rows.map(|row| row.map_err(|_| StoreError)).collect()
    }

    pub fn prune(&mut self, owner: &str, now: i64, files: &Path) -> Result<usize, StoreError> {
        let cutoff = now.saturating_sub(RETENTION_MS);
        let transaction = self.connection.transaction().map_err(|_| StoreError)?;
        transaction
            .execute(
                "DELETE FROM file_deletions WHERE owner=?1 AND created<=?2",
                params![owner, cutoff],
            )
            .map_err(|_| StoreError)?;
        {
            let mut query = transaction.prepare("SELECT DISTINCT json_extract(body,'$.file.storedName') FROM messages WHERE owner=?1 AND created<=?2 AND json_extract(body,'$.file.storedName') IS NOT NULL AND json_extract(body,'$.file.storedName') NOT IN (SELECT json_extract(body,'$.file.storedName') FROM messages WHERE created>?2 AND json_extract(body,'$.file.storedName') IS NOT NULL)").map_err(|_| StoreError)?;
            let names = query
                .query_map(params![owner, cutoff], |row| row.get::<_, String>(0))
                .map_err(|_| StoreError)?;
            for name in names {
                let name = name.map_err(|_| StoreError)?;
                if !valid_stored_name(&name) {
                    return Err(StoreError);
                }
                match std::fs::remove_file(files.join(&name)) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err(StoreError),
                }
            }
        }
        let removed = transaction
            .execute(
                "DELETE FROM messages WHERE owner=?1 AND created<=?2",
                params![owner, cutoff],
            )
            .map_err(|_| StoreError)?;
        transaction.commit().map_err(|_| StoreError)?;
        Ok(removed)
    }
}

pub fn valid_stored_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let device = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || (1..=9).any(|index| stem == format!("COM{index}") || stem == format!("LPT{index}"));
    !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('.')
        && !name.ends_with('.')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        && !device
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_isolation_ordering_and_retention_match_the_existing_store() {
        let temporary = tempfile::tempdir().unwrap();
        let mut store = ChatStore::open(&temporary.path().join("chat.sqlite3")).unwrap();
        let now = RETENTION_MS * 3;
        let mut message = ChatMessage {
            id: "shared-id".into(),
            owner_id: "alice".into(),
            conversation_id: "bob".into(),
            sender_id: "alice".into(),
            target_id: "bob".into(),
            kind: "text".into(),
            text: "hello".into(),
            created_at: now,
            sequence: 1,
            delivery: "pending".into(),
            file: None,
        };
        assert!(store.save(&message, now).unwrap());
        message.owner_id = "other".into();
        assert!(store.save(&message, now).unwrap());
        store.delete("alice", "shared-id").unwrap();
        assert!(store.conversation("alice", "bob", now).unwrap().is_empty());
        assert_eq!(store.conversation("other", "bob", now).unwrap().len(), 1);
        assert_eq!(
            store
                .prune("other", now + RETENTION_MS, temporary.path())
                .unwrap(),
            1
        );
        assert!(store
            .conversation("other", "bob", now + RETENTION_MS)
            .unwrap()
            .is_empty());
        assert!(!store.save(&message, now + RETENTION_MS).unwrap());
        for name in [
            "../session.enc",
            "..\\session.enc",
            "C:secret",
            "CON",
            "file:stream",
        ] {
            assert!(!valid_stored_name(name));
        }
    }
}
