//! Stopped/quiesced restore. The native owner must stop workers and reconcile
//! current external effects before calling; holding this lock alone is insufficient.
use super::*;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::Read;
pub fn snapshot_hash(path: &Path) -> Result<String, String> {
    check_path(path, false)?;
    let mut file = File::open(path).map_err(|_| "Cannot read backup database.")?;
    if file
        .metadata()
        .map_err(|_| "Cannot inspect backup database.")?
        .len()
        > 1024 * 1024 * 1024
    {
        return Err("Backup database exceeds its byte budget.".into());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    let mut total = 0u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "Cannot read backup database.")?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > 1024 * 1024 * 1024 {
            return Err("Backup database exceeds its byte budget.".into());
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub struct NamespaceLease {
    path: PathBuf,
    _lock: File,
}
impl NamespaceLease {
    pub fn acquire(database: &Path) -> Result<Self, String> {
        check_path(database, true)?;
        let lock_path = database.with_extension("authority.lock");
        check_path(&lock_path, true)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|_| "Cannot open Cockpit namespace ownership lock.")?;
        lock.try_lock().map_err(|_| {
            "Another Cockpit process owns this database. Close it before continuing."
        })?;
        Ok(Self {
            path: database.to_owned(),
            _lock: lock,
        })
    }
}
impl Drop for NamespaceLease {
    fn drop(&mut self) {
        let _ = self._lock.unlock();
    }
}

impl Database {
    /// Fence quiesced cleanup evidence across the queued-restart boundary.
    pub fn recovery_fingerprint(&self) -> Result<String, String> {
        let mut connection = self.connection()?;
        let tx = connection.transaction().map_err(failure)?;
        self.verify_epoch(&tx)?;
        let mut statement=tx.prepare("SELECT domain,key,revision,payload FROM records WHERE domain LIKE 'recovery-%' OR domain LIKE 'recovery/%' ORDER BY domain,key LIMIT 1025").map_err(failure)?;
        let mut rows = statement.query([]).map_err(failure)?;
        let mut hash = Sha256::new();
        let mut count = 0;
        let mut total = 0;
        while let Some(row) = rows.next().map_err(failure)? {
            count += 1;
            let domain: String = row.get(0).map_err(failure)?;
            let key: String = row.get(1).map_err(failure)?;
            let revision: i64 = row.get(2).map_err(failure)?;
            let payload: Option<Vec<u8>> = row.get(3).map_err(failure)?;
            total += payload.as_ref().map_or(0, Vec::len);
            if count > 1024 || total > 64 * 1024 * 1024 {
                return Err("Recovery evidence exceeds the restore admission budget.".into());
            }
            for bytes in [
                domain.as_bytes(),
                key.as_bytes(),
                &revision.to_le_bytes(),
                payload.as_deref().unwrap_or(&[]),
            ] {
                hash.update((bytes.len() as u64).to_le_bytes());
                hash.update(bytes);
            }
            hash.update([payload.is_some() as u8]);
        }
        Ok(format!("{:x}", hash.finalize()))
    }

    /// Copy a verified current-layout snapshot under one transaction. Related
    /// indexes rebuild through the same triggers as ordinary saves. The source
    /// is an immutable backup; no source connection survives into the mutation.
    /// Returns a newly pinned handle; all handles from before restore stay stale.
    pub fn restore_from(
        &self,
        source: &Path,
        expected_hash: &str,
        lease: &NamespaceLease,
    ) -> Result<Self, String> {
        self.restore_with_fresh_host(source, expected_hash, lease, None)
    }
    /// The optional ciphertext is a newly issued, empty native host identity;
    /// historical secret rows are always excluded from the source snapshot.
    pub fn restore_with_fresh_host(
        &self,
        source: &Path,
        expected_hash: &str,
        lease: &NamespaceLease,
        fresh_host: Option<&[u8]>,
    ) -> Result<Self, String> {
        if fresh_host.is_some_and(|bytes| bytes.is_empty() || bytes.len() > 1024 * 1024) {
            return Err("Invalid fresh protected host seed.".into());
        }
        if lease.path != self.path || source == self.path {
            return Err("Restore requires ownership of the exact destination namespace.".into());
        }
        check_path(source, false)?;
        let metadata =
            std::fs::metadata(source).map_err(|_| "The backup database is unavailable.")?;
        if metadata.len() == 0 || metadata.len() > 1024 * 1024 * 1024 {
            return Err("The backup database exceeds its size bounds.".into());
        }
        let snapshot = Self::open_existing(source)?;
        snapshot.integrity_check()?;
        let mut connection = self.connection()?;
        connection
            .execute(
                "ATTACH DATABASE ?1 AS snapshot",
                [source.to_str().ok_or("Invalid backup database path.")?],
            )
            .map_err(failure)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        self.verify_epoch(&tx)?;
        let actual: String = tx
            .query_row(
                "SELECT epoch FROM snapshot.incarnation WHERE singleton=1",
                [],
                |r| r.get(0),
            )
            .map_err(failure)?;
        if actual != snapshot.epoch {
            return Err("The backup changed during restore. Existing data was preserved.".into());
        }
        if snapshot_hash(source)? != expected_hash {
            return Err("The backup hash changed. Existing data was preserved.".into());
        }
        // Remove historical authority. Backups still retain the original evidence
        // for inspection, but none can authorize an output/cleanup/trust effect.
        tx.execute_batch("DELETE FROM profile_documents; DELETE FROM records WHERE domain!='storage'; DELETE FROM domains WHERE name!='storage';
            INSERT OR IGNORE INTO domains SELECT name FROM snapshot.domains;
            INSERT INTO records SELECT domain,key,revision,payload FROM snapshot.records
            WHERE domain NOT IN ('secrets','security','companion-client','storage') AND domain NOT LIKE 'recovery-%' AND domain NOT LIKE 'recovery/%'
            AND NOT (domain='profiles' AND key='selection')
            AND NOT (domain='companion' AND key IN ('listener','firmware'));
            DELETE FROM records WHERE domain='storage' AND key='pending-restore';
            UPDATE incarnation SET epoch=lower(hex(randomblob(16))) WHERE singleton=1;") .map_err(failure)?;
        if let Some(ciphertext) = fresh_host {
            tx.execute("INSERT OR IGNORE INTO domains(name) VALUES('secrets')", [])
                .map_err(failure)?;
            tx.execute("INSERT INTO records(domain,key,revision,payload) VALUES('secrets','companion-host',1,?1)",[ciphertext]).map_err(failure)?;
        }
        let invalid: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
                [],
                |r| r.get(0),
            )
            .map_err(failure)?;
        if invalid {
            return Err(
                "The backup contains invalid references. Existing data was preserved.".into(),
            );
        }
        let next = epoch::read(&tx)?;
        tx.commit().map_err(failure)?;
        Ok(Self {
            path: self.path.clone(),
            epoch: next,
        })
    }
}
#[cfg(test)]
mod tests;
