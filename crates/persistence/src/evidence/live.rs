//! A retained connection, never a retained read snapshot. Each statement ends
//! before native dispatch, so commits between deliveries are always observable.
use super::Evidence;
use crate::{failure, Change, Database, PayloadCheck, APPLICATION_ID, BUSY_TIMEOUT};
use rusqlite::{params, Connection, TransactionBehavior};
use std::{path::Path, time::Duration};

const READ_BUSY: Duration = Duration::from_millis(5);

pub(super) struct LiveEvidence {
    connection: Connection,
    schema_version: i64,
}
impl LiveEvidence {
    pub(super) fn open(database: &Database) -> Result<Self, String> {
        let mut connection = database.connection()?;
        // Let brief shared-database commits finish, but never spend the
        // helper's 250 ms lease waiting for an exclusive SQLite writer.
        connection.busy_timeout(READ_BUSY).map_err(failure)?;
        let tx = connection.transaction().map_err(failure)?;
        crate::connection::validate_record_layout(&tx)?;
        let schema_version = tx
            .pragma_query_value(None, "schema_version", |row| row.get(0))
            .map_err(failure)?;
        tx.commit().map_err(failure)?;
        Ok(Self {
            connection,
            schema_version,
        })
    }
    pub(super) fn verify(&self, evidence: &Evidence) -> Result<(), String> {
        let path = evidence.database.path();
        storage_path(&self.connection, path)?;
        self.verify_record(evidence)?;
        // Also fence a path replacement concurrent with the statement.
        storage_path(&self.connection, path)
    }
    fn verify_record(&self, evidence: &Evidence) -> Result<(), String> {
        // One SELECT supplies an atomic epoch/revision/payload snapshot. BLOB
        // equality avoids allocating/copying the journal on every motion.
        let (epoch, revision, matches): (String, u64, bool) = self.connection
            .prepare_cached("SELECT i.epoch,CASE WHEN r.domain IS NULL THEN 0 ELSE r.revision END,r.payload IS ?3 FROM incarnation i LEFT JOIN records r ON r.domain=?1 AND r.key=?2 WHERE i.singleton=1")
            .map_err(failure)?
            .query_row(params![evidence.domain, evidence.key, evidence.payload()], |row| {
                Ok((row.get(0)?, row.get::<_, i64>(1)? as u64, row.get(2)?))
            }).map_err(failure)?;
        if epoch != evidence.database.epoch() {
            return Err(
                "The database was restored or replaced. Restart Cockpit before continuing.".into(),
            );
        }
        if revision != evidence.revision || !matches {
            return Err("Recovery evidence changed externally; preserving it.".into());
        }
        Ok(())
    }
    pub(super) fn write(
        &mut self,
        evidence: &Evidence,
        change: Change<'_>,
        prior: PayloadCheck<'_>,
    ) -> Result<Vec<u64>, String> {
        let path = evidence.database.path();
        storage_path(&self.connection, path)?;
        // Preserve the fast failure when an exclusive writer blocks reads.
        self.verify_record(evidence)?;
        self.connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(failure)?;
        let result = self.commit(&evidence.database, change, prior);
        self.connection.busy_timeout(READ_BUSY).map_err(failure)?;
        result
    }
    fn commit(
        &mut self,
        database: &Database,
        change: Change<'_>,
        prior: PayloadCheck<'_>,
    ) -> Result<Vec<u64>, String> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        let path = database.path();
        storage_path(&tx, path)?;
        // The checked open pinned the layout and FULL synchronous mode on
        // this connection. Reject any later identity, schema or journal change
        // under the same writer lock as the record comparison.
        let (application, schema, journal): (i64, i64, String) = tx.query_row(
            "SELECT (SELECT application_id FROM pragma_application_id),
             (SELECT schema_version FROM pragma_schema_version), (SELECT journal_mode FROM pragma_journal_mode)",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).map_err(failure)?;
        if application != APPLICATION_ID || schema != self.schema_version || journal != "delete" {
            return Err(
                "Recovery evidence database layout or journal mode changed externally.".into(),
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::metadata(path)
                .map_err(|_| "Cannot inspect persistence path.")?
                .permissions();
            if permissions.mode() & 0o777 != 0o600 {
                return Err("Cannot protect SQLite database.".into());
            }
        }
        let revisions = database.apply_exact(&tx, &[change], &[], &[prior])?;
        storage_path(&tx, path)?;
        tx.commit().map_err(failure)?;
        storage_path(&self.connection, path)?;
        Ok(revisions)
    }
}

fn storage_path(_connection: &Connection, path: &Path) -> Result<(), String> {
    // Ancestors are shared by all sidecars: inspect them once, then each
    // leaf. Reusing a connection must not silently follow new indirection.
    crate::check_path(path, false)?;
    if !path
        .try_exists()
        .map_err(|_| "Cannot inspect persistence path.")?
    {
        return Err("Recovery evidence storage was removed or replaced.".into());
    }
    crate::path::check_sidecars(path)?;
    #[cfg(unix)]
    {
        let mut moved: std::ffi::c_int = 0;
        // SAFETY: the retained connection owns a live SQLite handle; main
        // is NUL terminated and HAS_MOVED writes exactly one c_int. The
        // pinned bundled Unix VFS implements this using its open file's
        // device/inode, rejecting even an equal-byte/equal-epoch replacement.
        let result = unsafe {
            rusqlite::ffi::sqlite3_file_control(
                _connection.handle(),
                c"main".as_ptr(),
                rusqlite::ffi::SQLITE_FCNTL_HAS_MOVED,
                (&mut moved as *mut std::ffi::c_int).cast(),
            )
        };
        if result != rusqlite::ffi::SQLITE_OK || moved != 0 {
            return Err("Recovery evidence storage was removed or replaced.".into());
        }
    }
    // The pinned Windows SQLite VFS opens the main file with READ|WRITE
    // sharing only: its lifetime handle denies deletion/rename/replacement.
    Ok(())
}

#[cfg(test)]
mod tests;
