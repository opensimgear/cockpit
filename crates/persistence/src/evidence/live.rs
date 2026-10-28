//! A retained connection, never a retained read snapshot. Each statement ends
//! before native dispatch, so commits between deliveries are always observable.
use super::Evidence;
use crate::{failure, Database};
use rusqlite::{params, Connection};
use std::{path::Path, time::Duration};

pub(super) struct Reader {
    connection: Connection,
}
impl Reader {
    pub(super) fn open(database: &Database) -> Result<Self, String> {
        let connection = database.connection()?;
        // Let brief shared-database commits finish, but never spend the
        // helper's 250 ms lease waiting for an exclusive SQLite writer.
        connection.busy_timeout(Duration::from_millis(5)).map_err(failure)?;
        connection
            .pragma_update(None, "query_only", true)
            .map_err(failure)?;
        Ok(Self { connection })
    }
    pub(super) fn verify(&self, evidence: &Evidence) -> Result<(), String> {
        self.storage_path(evidence.database.path())?;
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
        // Also fence a path replacement concurrent with the statement.
        self.storage_path(evidence.database.path())
    }
    fn storage_path(&self, path: &Path) -> Result<(), String> {
        // Ancestors are shared by all sidecars: inspect them once, then each
        // leaf. Reusing a connection must not silently follow new indirection.
        crate::check_path(path, false)?;
        if !path
            .try_exists()
            .map_err(|_| "Cannot inspect persistence path.")?
        {
            return Err("Recovery evidence storage was removed or replaced.".into());
        }
        for suffix in ["-journal", "-wal", "-shm"] {
            let mut sidecar = path.as_os_str().to_os_string();
            sidecar.push(suffix);
            crate::path::check_leaf(Path::new(&sidecar))?;
        }
        #[cfg(unix)]
        {
            let mut moved: std::ffi::c_int = 0;
            // SAFETY: the retained connection owns a live SQLite handle; main
            // is NUL terminated and HAS_MOVED writes exactly one c_int. The
            // pinned bundled Unix VFS implements this using its open file's
            // device/inode, rejecting even an equal-byte/equal-epoch replacement.
            let result = unsafe {
                rusqlite::ffi::sqlite3_file_control(
                    self.connection.handle(),
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
}

#[cfg(test)]
mod tests;
