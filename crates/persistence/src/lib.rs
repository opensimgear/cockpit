//! Shared durable boundary. Call only from storage/command workers, never from
//! input, telemetry, or output callbacks. Transactions contain database work only.
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Component, Path, PathBuf},
    time::Duration,
};

mod repository;
pub use repository::Repository;
mod migration;
pub use migration::{MigrationCapture, MigrationStatus, StagedSource};
mod namespace;
pub use namespace::register_namespace;

pub const SCHEMA_VERSION: i64 = 2;
pub const APPLICATION_ID: i64 = 0x4f534743;
pub const MAX_PAYLOAD: usize = 128 * 1024 * 1024;
pub const BUSY_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Clone, Debug)]
pub struct Database {
    path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub domain: String,
    pub key: String,
    pub format_version: u32,
    pub revision: u64,
    pub payload: Vec<u8>,
}

/// A missing record has revision zero. Deletion retains a revision tombstone,
/// so delete/recreate cannot admit an edit from an older reader (the ABA case).
pub struct Change<'a> {
    pub domain: &'a str,
    pub key: &'a str,
    pub format_version: u32,
    pub expected_revision: u64,
    pub payload: Option<&'a [u8]>,
}

/// Caller validates the complete typed payload and cross references before
/// constructing this snapshot. Protected sources must already be ciphertext.
pub struct Import<'a> {
    pub source: &'a str,
    pub domain: &'a str,
    pub key: &'a str,
    pub format_version: u32,
    pub payload: Option<&'a [u8]>,
}

fn failure(error: rusqlite::Error) -> String {
    use rusqlite::ErrorCode::*;
    let kind = match error.sqlite_error_code() {
        Some(DatabaseBusy | DatabaseLocked) => "busy; retry the operation",
        Some(ReadOnly | PermissionDenied) => "read-only or permission denied",
        Some(DiskFull) => "disk full",
        Some(DatabaseCorrupt | NotADatabase) => {
            "corrupt; preserve the database and restore a verified backup"
        }
        Some(CannotOpen) => "cannot open the configured database",
        _ => "operation failed",
    };
    // Never include SQL values, payloads, tokens, or native sealing errors.
    format!("SQLite {kind}.")
}

fn identifier(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err("Invalid persistence record identity.".into());
    }
    Ok(())
}

/// Reject symlinks/reparse points for the database and its parents/sidecars.
/// The same rule applies to migration sources; never scan unrelated trees.
pub fn check_path(path: &Path, create: bool) -> Result<(), String> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("Persistence requires an absolute path without traversal.".into());
    }
    let mut current = PathBuf::new();
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_))
            && matches!(components.peek(), Some(Component::RootDir))
        {
            continue;
        }
        let parent = components.peek().is_some();
        match std::fs::symlink_metadata(&current) {
            Ok(meta) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return Err("Persistence cannot follow reparse points.".into());
                    }
                }
                if meta.file_type().is_symlink()
                    || (parent && !meta.is_dir())
                    || (!parent && !meta.is_file())
                {
                    return Err("Persistence path is indirect or has the wrong type.".into());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if parent && create {
                    std::fs::create_dir(&current)
                        .map_err(|_| "Cannot create persistence directory.")?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::set_permissions(&current, std::fs::Permissions::from_mode(0o700))
                            .map_err(|_| "Cannot protect persistence directory.")?;
                    }
                }
            }
            Err(_) => return Err("Cannot inspect persistence path.".into()),
        }
    }
    Ok(())
}

impl Database {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let database = Self { path: path.into() };
        database.connection()?;
        Ok(database)
    }
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn connection(&self) -> Result<Connection, String> {
        check_path(&self.path, true)?;
        for suffix in ["-journal", "-wal", "-shm"] {
            let mut sidecar = self.path.as_os_str().to_os_string();
            sidecar.push(suffix);
            check_path(Path::new(&sidecar), false)?;
        }
        let mut connection = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(failure)?;
        connection.busy_timeout(BUSY_TIMEOUT).map_err(failure)?;
        connection
            .set_limit(
                rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH,
                MAX_PAYLOAD as i32 + 16384,
            )
            .map_err(failure)?;
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(failure)?;
        connection
            .pragma_update(None, "trusted_schema", false)
            .map_err(failure)?;
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(failure)?;
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(failure)?;
        let application: i64 = connection
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .map_err(failure)?;
        if version > SCHEMA_VERSION
            || (application != 0 && application != APPLICATION_ID)
            || (version != 0 && application == 0)
        {
            return Err("Unsupported SQLite schema/application. Use a compatible Cockpit build; no downgrade was attempted.".into());
        }
        if version == 0 && application == 0 {
            let existing: bool = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name NOT LIKE 'sqlite_%')",
                    [],
                    |row| row.get(0),
                )
                .map_err(failure)?;
            if existing {
                return Err(
                    "Unrecognized SQLite database. No Cockpit schema was installed.".into(),
                );
            }
        }
        // DELETE/FULL is deliberate: redirected configuration directories are
        // not guaranteed to support WAL shared memory. No checkpoint dependency.
        let mode: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .map_err(failure)?;
        if mode != "delete" {
            connection
                .pragma_update(None, "journal_mode", "DELETE")
                .map_err(failure)?;
        }
        if version < SCHEMA_VERSION {
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(failure)?;
            if version == 0 {
                tx.execute_batch(include_str!("schema.sql"))
                    .map_err(failure)?;
            }
            if version < 2 {
                tx.execute_batch(include_str!("migration.sql"))
                    .map_err(failure)?;
            }
            tx.pragma_update(None, "application_id", APPLICATION_ID)
                .map_err(failure)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)
                .map_err(failure)?;
            tx.commit().map_err(failure)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600))
                .map_err(|_| "Cannot protect SQLite database.")?;
        }
        Ok(connection)
    }

    pub fn read(&self, domain: &str, key: &str) -> Result<Option<Record>, String> {
        identifier(domain)?;
        identifier(key)?;
        read_record(&self.connection()?, domain, key)
    }
    pub fn revision(&self, domain: &str, key: &str) -> Result<u64, String> {
        identifier(domain)?;
        identifier(key)?;
        revision(&self.connection()?, domain, key)
    }
    pub fn read_with_revision(
        &self,
        domain: &str,
        key: &str,
    ) -> Result<(u64, Option<Record>), String> {
        identifier(domain)?;
        identifier(key)?;
        let row = self
            .connection()?
            .query_row(
                "SELECT format_version,revision,payload FROM records WHERE domain=?1 AND key=?2",
                params![domain, key],
                |row| {
                    Ok((
                        row.get::<_, u32>(0)?,
                        row.get::<_, i64>(1)? as u64,
                        row.get::<_, Option<Vec<u8>>>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(failure)?;
        Ok(match row {
            Some((format_version, revision, payload)) => (
                revision,
                payload.map(|payload| Record {
                    domain: domain.into(),
                    key: key.into(),
                    format_version,
                    revision,
                    payload,
                }),
            ),
            None => (0, None),
        })
    }
    /// All expected revisions are tested under one immediate transaction. Any
    /// mismatch or failed statement rolls back the entire related edit.
    pub fn commit(&self, changes: &[Change<'_>]) -> Result<Vec<u64>, String> {
        if changes.is_empty() || changes.len() > 1024 {
            return Err("Invalid persistence transaction size.".into());
        }
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        let mut seen = std::collections::BTreeSet::new();
        let mut revisions = Vec::with_capacity(changes.len());
        for change in changes {
            identifier(change.domain)?;
            identifier(change.key)?;
            if !seen.insert((change.domain, change.key))
                || change.format_version == 0
                || change.payload.is_some_and(|p| p.len() > MAX_PAYLOAD)
            {
                return Err("Invalid or duplicate persistence change.".into());
            }
            if revision(&tx, change.domain, change.key)? != change.expected_revision {
                return Err(
                    "Persistence revision conflict. Reload current settings before saving.".into(),
                );
            }
            let next = change
                .expected_revision
                .checked_add(1)
                .filter(|v| *v <= i64::MAX as u64)
                .ok_or("Persistence revision exhausted.")?;
            tx.execute(
                "INSERT INTO domains(name) VALUES(?1) ON CONFLICT DO NOTHING",
                [change.domain],
            )
            .map_err(failure)?;
            tx.execute("INSERT INTO records(domain,key,format_version,revision,payload) VALUES(?1,?2,?3,?4,?5)
                ON CONFLICT(domain,key) DO UPDATE SET format_version=excluded.format_version,revision=excluded.revision,payload=excluded.payload",
                params![change.domain, change.key, change.format_version, next as i64, change.payload]).map_err(failure)?;
            revisions.push(next);
        }
        tx.commit().map_err(failure)?;
        Ok(revisions)
    }

    /// Snapshots and receipts commit with the imported records. Reruns never
    /// overwrite saved records, including deletion tombstones. Missing optional
    /// sources receive a receipt too. An inaccessible source must return an error
    /// before calling this operation, never masquerade as a missing snapshot.
    pub fn import(&self, origin: &str, sources: &[Import<'_>]) -> Result<(), String> {
        identifier(origin)?;
        if sources.len() > 4096 {
            return Err("Too many migration sources.".into());
        }
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        for source in sources {
            identifier(source.source)?;
            identifier(source.domain)?;
            identifier(source.key)?;
            if source.format_version == 0 || source.payload.is_some_and(|p| p.len() > MAX_PAYLOAD) {
                return Err("Invalid migration snapshot.".into());
            }
            let imported: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM imports WHERE origin=?1 AND source=?2)",
                    params![origin, source.source],
                    |r| r.get(0),
                )
                .map_err(failure)?;
            if imported {
                continue;
            }
            tx.execute(
                "INSERT INTO domains(name) VALUES(?1) ON CONFLICT DO NOTHING",
                [source.domain],
            )
            .map_err(failure)?;
            if let Some(payload) = source.payload {
                tx.execute("INSERT INTO records(domain,key,format_version,revision,payload) VALUES(?1,?2,?3,1,?4) ON CONFLICT DO NOTHING",
                    params![source.domain, source.key, source.format_version, payload]).map_err(failure)?;
            }
            let checksum = source
                .payload
                .map(|bytes| format!("sha256:{:x}", Sha256::digest(bytes)));
            tx.execute("INSERT INTO imports(origin,source,domain,key,format_version,checksum,snapshot) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![origin, source.source, source.domain, source.key, source.format_version, checksum, source.payload]).map_err(failure)?;
        }
        tx.execute(
            "INSERT INTO origins(name) VALUES(?1) ON CONFLICT DO NOTHING",
            [origin],
        )
        .map_err(failure)?;
        tx.commit().map_err(failure)
    }

    pub fn origin_imported(&self, origin: &str) -> Result<bool, String> {
        identifier(origin)?;
        self.connection()?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM origins WHERE name=?1)",
                [origin],
                |row| row.get(0),
            )
            .map_err(failure)
    }

    /// SQLite's online backup includes committed WAL content if opening an older
    /// compatible database; never copy the main file of a live database.
    pub fn backup(&self, target: &Path) -> Result<(), String> {
        check_path(target, true)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let reserved = options
            .open(target)
            .map_err(|_| "Cannot reserve backup destination; it may already exist.")?;
        drop(reserved);
        self.connection()?
            .backup("main", target, None)
            .map_err(failure)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o600))
                .map_err(|_| "Cannot protect database backup.")?;
        }
        Ok(())
    }
    pub fn integrity_check(&self) -> Result<(), String> {
        let connection = self.connection()?;
        let result: String = connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(failure)?;
        if result != "ok" {
            return Err(
                "SQLite integrity check failed. Preserve the database for recovery.".into(),
            );
        }
        let invalid: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
                [],
                |row| row.get(0),
            )
            .map_err(failure)?;
        if invalid {
            return Err("SQLite references failed integrity checking.".into());
        }
        Ok(())
    }
}

fn revision(connection: &Connection, domain: &str, key: &str) -> Result<u64, String> {
    connection
        .query_row(
            "SELECT revision FROM records WHERE domain=?1 AND key=?2",
            params![domain, key],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map(|v| v.unwrap_or(0) as u64)
        .map_err(failure)
}
fn read_record(connection: &Connection, domain: &str, key: &str) -> Result<Option<Record>, String> {
    connection.query_row("SELECT format_version,revision,payload FROM records WHERE domain=?1 AND key=?2 AND payload IS NOT NULL", params![domain,key], |row| Ok(Record {
        domain: domain.into(), key: key.into(), format_version: row.get(0)?, revision: row.get::<_,i64>(1)? as u64, payload: row.get(2)?,
    })).optional().map_err(failure)
}

/// Bounded migration reader. Errors preserve the original and prevent a receipt.
pub fn snapshot(path: &Path, limit: usize) -> Result<Option<Vec<u8>>, String> {
    check_path(path, false)?;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err("Cannot read legacy persistence source. No import was committed.".into())
        }
    };
    let mut bytes = Vec::new();
    file.take(limit.min(MAX_PAYLOAD) as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot snapshot legacy persistence source.")?;
    if bytes.len() > limit.min(MAX_PAYLOAD) {
        return Err("Legacy persistence source exceeds its size limit.".into());
    }
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn test_directory()->tempfile::TempDir {
    let parent=std::env::temp_dir();
    #[cfg(unix)]
    let parent=parent.canonicalize().expect("canonical OS temporary directory");
    tempfile::tempdir_in(parent).expect("isolated persistence fixture")
}

#[cfg(feature = "test-support")]
pub mod test_support;
