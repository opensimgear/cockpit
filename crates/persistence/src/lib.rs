//! Shared durable boundary. Call only from storage/command workers, never from
//! input, telemetry, or output callbacks. Transactions contain database work only.
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

mod connection;
mod epoch;
mod restore;
pub use restore::{snapshot_hash, NamespaceLease};
mod repository;
pub use repository::Repository;
mod path;
pub use path::check_path;
mod namespace;
pub use namespace::register_namespace;
mod profile_index;
pub use profile_index::{ProfileIndex, ProfileReference};
mod observation;
pub use observation::ObservationWriter;
mod evidence;
pub use evidence::Evidence;

pub const APPLICATION_ID: i64 = 0x4f534743;
pub const MAX_PAYLOAD: usize = 128 * 1024 * 1024;
pub const BUSY_TIMEOUT: Duration = Duration::from_millis(250);
pub const SQLITE_RUNTIME_VERSION: &str = "3.53.2";

#[derive(Clone, Debug)]
pub struct Database {
    path: PathBuf,
    epoch: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub domain: String,
    pub key: String,
    pub revision: u64,
    pub payload: Vec<u8>,
}

/// A missing record has revision zero. Deletion retains a revision tombstone,
/// so delete/recreate cannot admit an edit from an older reader (the ABA case).
pub struct Change<'a> {
    pub domain: &'a str,
    pub key: &'a str,
    pub expected_revision: u64,
    pub payload: Option<&'a [u8]>,
}
/// A read dependency is rechecked in the writer transaction without changing
/// its revision. Use it when retaining references owned by another domain.
pub struct RevisionCheck<'a> {
    pub domain: &'a str,
    pub key: &'a str,
    pub expected_revision: u64,
}
/// Recovery admissions fence both the revision and the exact evidence bytes.
pub struct PayloadCheck<'a> {
    pub domain: &'a str,
    pub key: &'a str,
    pub expected_revision: u64,
    pub expected_payload: Option<&'a [u8]>,
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

fn identity(connection: &Connection) -> Result<(i64, bool), String> {
    let (application,existing)=connection.query_row(
        "SELECT (SELECT application_id FROM pragma_application_id), EXISTS(SELECT 1 FROM sqlite_master WHERE name NOT LIKE 'sqlite_%')",
        [],|row|Ok((row.get::<_,i64>(0)?,row.get::<_,bool>(1)?)),
    ).map_err(failure)?;
    if application != 0 && application != APPLICATION_ID {
        return Err("This SQLite database belongs to another application.".into());
    }
    if application == 0 && existing {
        return Err("Unrecognized SQLite database. No Cockpit tables were installed.".into());
    }
    Ok((application, existing))
}

impl Database {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        Self::open_in(path.into(), true)
    }
    /// Passive current-layout open. Never creates a missing database.
    pub fn open_existing(path: impl Into<PathBuf>) -> Result<Self, String> {
        Self::open_in(path.into(), false)
    }
    fn open_in(path: PathBuf, create: bool) -> Result<Self, String> {
        let mut database = Self {
            path,
            epoch: String::new(),
        };
        let connection = database.connect(create)?;
        database.epoch = epoch::read(&connection)?;
        Ok(database)
    }
    fn connection(&self) -> Result<Connection, String> {
        let connection = self.connect(false)?;
        self.verify_epoch(&connection)?;
        Ok(connection)
    }
    fn verify_epoch(&self, connection: &Connection) -> Result<(), String> {
        if epoch::read(connection)? != self.epoch {
            return Err(
                "The database was restored or replaced. Restart Cockpit before continuing.".into(),
            );
        }
        Ok(())
    }
    pub fn epoch(&self) -> &str {
        &self.epoch
    }
    pub fn path(&self) -> &Path {
        &self.path
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
                "SELECT revision,payload FROM records WHERE domain=?1 AND key=?2",
                params![domain, key],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)? as u64,
                        row.get::<_, Option<Vec<u8>>>(1)?,
                    ))
                },
            )
            .optional()
            .map_err(failure)?;
        Ok(match row {
            Some((revision, payload)) => (
                revision,
                payload.map(|payload| Record {
                    domain: domain.into(),
                    key: key.into(),
                    revision,
                    payload,
                }),
            ),
            None => (0, None),
        })
    }
    /// Capture related ciphertext/content records under one read snapshot.
    /// The transaction ends before the caller validates or decrypts payloads.
    pub fn read_many(&self, keys: &[(&str, &str)]) -> Result<Vec<(u64, Option<Record>)>, String> {
        if keys.is_empty() || keys.len() > 1024 {
            return Err("Invalid persistence snapshot size.".into());
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(failure)?;
        self.verify_epoch(&transaction)?;
        let mut records = Vec::with_capacity(keys.len());
        for (domain, key) in keys {
            identifier(domain)?;
            identifier(key)?;
            records.push((
                revision(&transaction, domain, key)?,
                read_record(&transaction, domain, key)?,
            ));
        }
        transaction.commit().map_err(failure)?;
        Ok(records)
    }
    /// All expected revisions are tested under one immediate transaction. Any
    /// mismatch or failed statement rolls back the entire related edit.
    pub fn commit(&self, changes: &[Change<'_>]) -> Result<Vec<u64>, String> {
        self.commit_checked(changes, &[])
    }
    pub fn commit_checked(
        &self,
        changes: &[Change<'_>],
        checks: &[RevisionCheck<'_>],
    ) -> Result<Vec<u64>, String> {
        self.commit_exact(changes, checks, &[])
    }
    pub fn commit_exact(
        &self,
        changes: &[Change<'_>],
        checks: &[RevisionCheck<'_>],
        payloads: &[PayloadCheck<'_>],
    ) -> Result<Vec<u64>, String> {
        if changes.is_empty() || changes.len() + checks.len() + payloads.len() > 1024 {
            return Err("Invalid persistence transaction size.".into());
        }
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        self.verify_epoch(&tx)?;
        for check in payloads {
            identifier(check.domain)?;
            identifier(check.key)?;
            if revision(&tx, check.domain, check.key)? != check.expected_revision
                || read_record(&tx, check.domain, check.key)?
                    .as_ref()
                    .map(|r| r.payload.as_slice())
                    != check.expected_payload
            {
                return Err("Recovery evidence changed externally; preserving it.".into());
            }
        }
        for check in checks {
            identifier(check.domain)?;
            identifier(check.key)?;
            if revision(&tx, check.domain, check.key)? != check.expected_revision {
                return Err("Persistence reference revision conflict. Reload current settings before saving.".into());
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut revisions = Vec::with_capacity(changes.len());
        for change in changes {
            identifier(change.domain)?;
            identifier(change.key)?;
            if !seen.insert((change.domain, change.key))
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
            tx.execute("INSERT INTO records(domain,key,revision,payload) VALUES(?1,?2,?3,?4)
                ON CONFLICT(domain,key) DO UPDATE SET revision=excluded.revision,payload=excluded.payload",
                params![change.domain, change.key, next as i64, change.payload]).map_err(failure)?;
            revisions.push(next);
        }
        tx.commit().map_err(failure)?;
        Ok(revisions)
    }

    /// Use the SQLite backup API; never copy the main file of a live database.
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
    connection.query_row("SELECT revision,payload FROM records WHERE domain=?1 AND key=?2 AND payload IS NOT NULL", params![domain,key], |row| Ok(Record {
        domain: domain.into(), key: key.into(), revision: row.get::<_,i64>(0)? as u64, payload: row.get(1)?,
    })).optional().map_err(failure)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod transaction_tests;

#[cfg(test)]
pub(crate) fn test_directory() -> tempfile::TempDir {
    let parent = std::env::temp_dir();
    #[cfg(unix)]
    let parent = parent
        .canonicalize()
        .expect("canonical OS temporary directory");
    tempfile::tempdir_in(parent).expect("isolated persistence fixture")
}

#[cfg(feature = "test-support")]
pub mod test_support;
