//! Pinned current-layout initialization; no import or schema upgrades.
use super::*;
impl Database {
    pub(crate) fn connect(&self, create: bool, enable_wal: bool) -> Result<Connection, String> {
        #[cfg(test)]
        OPEN_ATTEMPTS.with(|attempts| attempts.set(attempts.get() + 1));
        if rusqlite::version() != SQLITE_RUNTIME_VERSION {
            return Err("Unsupported SQLite runtime. Use the pinned bundled Cockpit build.".into());
        }
        check_path(&self.path, create)?;
        crate::path::check_sidecars(&self.path)?;
        journal_mode::check_open(&self.path, self.journal, enable_wal)?;
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
        let mode: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .map_err(failure)?;
        if mode != self.journal_mode() {
            if !enable_wal || mode != "delete" || !matches!(self.journal, JournalMode::Wal(_)) {
                return Err("Persistence journal mode changed. Restart Cockpit with the original database location.".into());
            }
            let selected: String = connection.query_row("PRAGMA main.journal_mode=WAL", [], |row| row.get(0))
                .map_err(failure)?;
            if selected != self.journal_mode() {
                return Err("Cannot enable write-ahead logging for the Cockpit database.".into());
            }
            crate::path::check_sidecars(&self.path)?;
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
        validate_record_layout(&connection)?;
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

pub(crate) fn validate_record_layout(connection: &Connection) -> Result<(), String> {
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
    Ok(())
}

#[cfg(test)]
thread_local! {
    pub(crate) static OPEN_ATTEMPTS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}
