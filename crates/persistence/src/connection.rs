//! Pinned current-layout initialization; no import or schema upgrades.
use super::*;
impl Database {
    pub(crate) fn connect(&self, create: bool) -> Result<Connection, String> {
        if rusqlite::version() != SQLITE_RUNTIME_VERSION {
            return Err("Unsupported SQLite runtime. Use the pinned bundled Cockpit build.".into());
        }
        check_path(&self.path, create)?;
        for suffix in ["-journal", "-wal", "-shm"] {
            let mut sidecar = self.path.as_os_str().to_os_string();
            sidecar.push(suffix);
            check_path(Path::new(&sidecar), false)?;
        }
        let mut connection = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | if create {
                    OpenFlags::SQLITE_OPEN_CREATE
                } else {
                    OpenFlags::empty()
                }
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
        let (_, existing) = identity(&connection)?;
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
        if !existing && create {
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(failure)?;
            // Another connection can finish initialization while this one is
            // waiting for the writer. Recheck under the same transaction.
            if !identity(&tx)?.1 {
                tx.execute_batch(include_str!("schema.sql"))
                    .map_err(failure)?;
                tx.execute_batch(include_str!("profile_indexes.sql"))
                    .map_err(failure)?;
                tx.pragma_update(None, "application_id", APPLICATION_ID)
                    .map_err(failure)?;
            }
            tx.commit().map_err(failure)?;
        }
        let columns = connection
            .prepare("SELECT name FROM pragma_table_info('records') ORDER BY cid")
            .map_err(failure)?
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(failure)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(failure)?;
        if columns != ["domain", "key", "revision", "payload"] {
            return Err(
                "SQLite layout differs from this development build. Start with a fresh database."
                    .into(),
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600))
                .map_err(|_| "Cannot protect SQLite database.")?;
        }
        crate::epoch::read(&connection)?;
        Ok(connection)
    }
}
