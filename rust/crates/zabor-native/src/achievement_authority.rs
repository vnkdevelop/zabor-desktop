use super::{checked, validate_bytes};
use crate::profile::ProfileError;
use rusqlite::{params, Connection, OpenFlags, Transaction, TransactionBehavior};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const APPLICATION_ID: i64 = 0x5a4e4832;
const SCHEMA: &str = "CREATE TABLE hidden_state(id INTEGER PRIMARY KEY CHECK(id=1),owner TEXT NOT NULL,payload BLOB CHECK(payload IS NULL OR length(payload)<=262144),pending INTEGER NOT NULL CHECK(pending IN (0,1))) STRICT";
const MAX_DATABASE_BYTES: u64 = 4 * 1024 * 1024;

pub(super) struct State {
    pub(super) payload: Option<Vec<u8>>,
    pub(super) pending: bool,
}

pub(super) struct Authority {
    connection: Connection,
    owner: String,
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn bounded(path: &Path) -> Result<(), ProfileError> {
    checked(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && metadata.len() <= MAX_DATABASE_BYTES => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(ProfileError::Storage),
    }
}

impl Authority {
    pub(super) fn open(
        path: &Path,
        owner: &str,
        legacy: impl FnOnce() -> Result<State, ProfileError>,
    ) -> Result<Self, ProfileError> {
        bounded(path)?;
        bounded(&sidecar(path, "-journal"))?;
        for suffix in ["-wal", "-shm"] {
            let sidecar = sidecar(path, suffix);
            checked(&sidecar)?;
            match fs::symlink_metadata(sidecar) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(ProfileError::Storage),
            }
        }
        if !cfg!(windows) {
            return Err(ProfileError::Storage);
        }
        let connection = Connection::open_with_flags_and_vfs(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_PRIVATE_CACHE
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
            "win32",
        )
        .map_err(|_| ProfileError::Storage)?;
        Self::configure(&connection)?;
        let authority = Self {
            connection,
            owner: owner.to_owned(),
        };
        let application: i64 = authority
            .connection
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .map_err(|_| ProfileError::Storage)?;
        let version: i64 = authority
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|_| ProfileError::Storage)?;
        let objects: i64 = authority
            .connection
            .query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0))
            .map_err(|_| ProfileError::Storage)?;
        if application == 0 && version == 0 && objects == 0 {
            let legacy = legacy()?;
            validate_bytes(owner, legacy.payload.as_deref())?;
            let transaction =
                Transaction::new_unchecked(&authority.connection, TransactionBehavior::Immediate)
                    .map_err(|_| ProfileError::Storage)?;
            let initialized = (|| {
                transaction
                    .execute_batch(SCHEMA)
                    .map_err(|_| ProfileError::Storage)?;
                transaction
                    .execute(
                        "INSERT INTO hidden_state(id,owner,payload,pending) VALUES(1,?1,?2,?3)",
                        params![owner, legacy.payload, i64::from(legacy.pending)],
                    )
                    .map_err(|_| ProfileError::Storage)?;
                transaction
                    .pragma_update(None, "application_id", APPLICATION_ID)
                    .map_err(|_| ProfileError::Storage)?;
                transaction
                    .pragma_update(None, "user_version", 1_i64)
                    .map_err(|_| ProfileError::Storage)
            })();
            if initialized.is_err() {
                transaction.rollback().map_err(|_| ProfileError::Storage)?;
                return Err(ProfileError::Storage);
            }
            authority.commit(transaction)?;
        } else if application != APPLICATION_ID || version != 1 || objects != 1 {
            return Err(ProfileError::Storage);
        }
        let schema: String = authority
            .connection
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE type='table' AND name='hidden_state'",
                [],
                |row| row.get(0),
            )
            .map_err(|_| ProfileError::Storage)?;
        let integrity: String = authority
            .connection
            .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
            .map_err(|_| ProfileError::Storage)?;
        if schema != SCHEMA || integrity != "ok" {
            return Err(ProfileError::Storage);
        }
        authority.state()?;
        Ok(authority)
    }

    fn configure(connection: &Connection) -> Result<(), ProfileError> {
        connection
            .busy_timeout(Duration::from_millis(100))
            .map_err(|_| ProfileError::Storage)?;
        for (name, value) in [
            ("synchronous", 3_i64),
            ("journal_size_limit", -1),
            ("mmap_size", 0),
            ("trusted_schema", 0),
        ] {
            connection
                .pragma_update(None, name, value)
                .map_err(|_| ProfileError::Storage)?;
        }
        let mode: String = connection
            .query_row("PRAGMA journal_mode=PERSIST", [], |row| row.get(0))
            .map_err(|_| ProfileError::Storage)?;
        let locking: String = connection
            .query_row("PRAGMA locking_mode=NORMAL", [], |row| row.get(0))
            .map_err(|_| ProfileError::Storage)?;
        let pages: i64 = connection
            .query_row("PRAGMA max_page_count=512", [], |row| row.get(0))
            .map_err(|_| ProfileError::Storage)?;
        if mode != "persist" || locking != "normal" || pages != 512 {
            return Err(ProfileError::Storage);
        }
        Self::verify(connection)
    }

    fn verify(connection: &Connection) -> Result<(), ProfileError> {
        if rusqlite::version_number() != 3_050_002 || !connection.is_autocommit() {
            return Err(ProfileError::Storage);
        }
        let unsafe_build: i64 = connection
            .query_row(
                "SELECT sqlite_compileoption_used('NO_SYNC') + sqlite_compileoption_used('OMIT_DISKIO')",
                [],
                |row| row.get(0),
            )
            .map_err(|_| ProfileError::Storage)?;
        if unsafe_build != 0 {
            return Err(ProfileError::Storage);
        }
        for (name, expected) in [
            ("synchronous", 3_i64),
            ("journal_size_limit", -1),
            ("mmap_size", 0),
            ("trusted_schema", 0),
            ("page_size", 4096),
            ("max_page_count", 512),
        ] {
            let actual: i64 = connection
                .pragma_query_value(None, name, |row| row.get(0))
                .map_err(|_| ProfileError::Storage)?;
            if actual != expected {
                return Err(ProfileError::Storage);
            }
        }
        for (name, expected) in [("journal_mode", "persist"), ("locking_mode", "normal")] {
            let actual: String = connection
                .pragma_query_value(None, name, |row| row.get(0))
                .map_err(|_| ProfileError::Storage)?;
            if actual != expected {
                return Err(ProfileError::Storage);
            }
        }
        Ok(())
    }

    pub(super) fn state(&self) -> Result<State, ProfileError> {
        Self::verify(&self.connection)?;
        let rows: i64 = self
            .connection
            .query_row("SELECT count(*) FROM hidden_state", [], |row| row.get(0))
            .map_err(|_| ProfileError::Storage)?;
        let (owner, payload, pending): (String, Option<Vec<u8>>, i64) = self
            .connection
            .query_row(
                "SELECT owner,payload,pending FROM hidden_state WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|_| ProfileError::Storage)?;
        if rows != 1 || owner != self.owner || !matches!(pending, 0 | 1) {
            return Err(ProfileError::Storage);
        }
        validate_bytes(&self.owner, payload.as_deref())?;
        Ok(State {
            payload,
            pending: pending == 1,
        })
    }

    pub(super) fn begin(&self) -> Result<(), ProfileError> {
        self.update(None, true, false)
    }

    pub(super) fn finish(&self, payload: Option<&[u8]>) -> Result<(), ProfileError> {
        self.update(payload, false, true)
    }

    pub(super) fn restore(&self, payload: Option<&[u8]>) -> Result<(), ProfileError> {
        self.update(payload, true, true)
    }

    fn update(
        &self,
        payload: Option<&[u8]>,
        pending: bool,
        replace: bool,
    ) -> Result<(), ProfileError> {
        Self::verify(&self.connection)?;
        if replace {
            validate_bytes(&self.owner, payload)?;
        }
        let transaction =
            Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)
                .map_err(|_| ProfileError::Storage)?;
        let changed = if replace {
            transaction.execute(
                if pending {
                    "UPDATE hidden_state SET payload=?1,pending=?2 WHERE id=1 AND owner=?3"
                } else {
                    "UPDATE hidden_state SET payload=?1,pending=?2 WHERE id=1 AND owner=?3 AND pending=1"
                },
                params![payload, i64::from(pending), self.owner],
            )
        } else {
            transaction.execute(
                "UPDATE hidden_state SET pending=1 WHERE id=1 AND owner=?1 AND pending=0",
                params![self.owner],
            )
        };
        if !matches!(changed, Ok(1)) {
            transaction.rollback().map_err(|_| ProfileError::Storage)?;
            return Err(ProfileError::Storage);
        }
        self.commit(transaction)
    }

    fn commit(&self, transaction: Transaction<'_>) -> Result<(), ProfileError> {
        let committed = transaction.commit();
        if !self.connection.is_autocommit() {
            self.connection
                .execute_batch("ROLLBACK")
                .map_err(|_| ProfileError::Storage)?;
            return Err(ProfileError::Storage);
        }
        committed.map_err(|_| ProfileError::Storage)
    }

    #[cfg(test)]
    pub(super) fn connection(&self) -> &Connection {
        &self.connection
    }
}
