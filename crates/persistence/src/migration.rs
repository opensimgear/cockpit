//! Resumable, staged import. Collect/validate every required native/webview
//! snapshot before authority changes. No external effect belongs in this API.
use crate::{failure, identifier, Database, Import, MAX_PAYLOAD};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::Path};

const MAX_SOURCES: usize = 4096;
const MAX_BATCH_BYTES: usize = 384 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationStatus {
    pub generation: u64,
    pub ready: bool,
    pub pending_origins: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedSource {
    pub origin: String,
    pub source: String,
    pub domain: String,
    pub key: String,
    pub format_version: u32,
    pub checksum: Option<String>,
    pub payload: Option<Vec<u8>>,
}

/// Immutable native inspection capture; only the service can construct it.
/// Validators inspect it outside the transaction, then commit checks its exact
/// generation again. Browser callers cannot supply a replacement source list.
pub struct MigrationCapture {
    generation: u64,
    sources: Vec<StagedSource>,
}
impl MigrationCapture {
    pub fn sources(&self) -> &[StagedSource] {
        &self.sources
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

fn digest(sources: &[Import<'_>]) -> Result<String, String> {
    if sources.len() > MAX_SOURCES {
        return Err("Too many migration sources.".into());
    }
    let mut bytes = 0usize;
    let mut ids = BTreeSet::new();
    let mut keys = BTreeSet::new();
    let mut hasher = Sha256::new();
    for source in sources {
        identifier(source.source)?;
        identifier(source.domain)?;
        identifier(source.key)?;
        if !ids.insert(source.source)
            || !keys.insert((source.domain, source.key))
            || source.format_version == 0
            || source.payload.is_some_and(|p| p.len() > MAX_PAYLOAD)
        {
            return Err("Invalid or duplicate migration snapshot.".into());
        }
        bytes = bytes
            .checked_add(source.payload.map_or(0, <[u8]>::len))
            .filter(|n| *n <= MAX_BATCH_BYTES)
            .ok_or("Migration snapshots exceed their aggregate bound.")?;
        // Length-prefix fields so source/path/payload boundaries are unambiguous.
        for field in [
            source.source.as_bytes(),
            source.domain.as_bytes(),
            source.key.as_bytes(),
            &source.format_version.to_le_bytes(),
            source.payload.unwrap_or_default(),
        ] {
            hasher.update((field.len() as u64).to_le_bytes());
            hasher.update(field);
        }
        hasher.update([u8::from(source.payload.is_some())]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

impl Database {
    /// Native bootstrap freezes the required origins. Frontend callers cannot
    /// omit an inaccessible source or replace requirements on a retry.
    pub fn begin_migration(&self, required_origins: &[&str]) -> Result<MigrationStatus, String> {
        let required: BTreeSet<_> = required_origins.iter().copied().collect();
        if required.is_empty() || required.len() != required_origins.len() || required.len() > 32 {
            return Err("Invalid required migration origins.".into());
        }
        for origin in &required {
            identifier(origin)?;
        }
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        let existing = {
            let mut query = tx
                .prepare("SELECT origin FROM migration_requirements ORDER BY origin")
                .map_err(failure)?;
            let origins = query
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(failure)?
                .collect::<Result<BTreeSet<_>, _>>()
                .map_err(failure)?;
            origins
        };
        if !existing.is_empty()
            && existing.iter().map(String::as_str).collect::<BTreeSet<_>>() != required
        {
            return Err("Migration requirements changed. Preserve the existing staged sources and use explicit recovery.".into());
        }
        for origin in required {
            tx.execute(
                "INSERT INTO migration_requirements VALUES(?1) ON CONFLICT DO NOTHING",
                [origin],
            )
            .map_err(failure)?;
        }
        tx.commit().map_err(failure)?;
        self.migration_status()
    }

    pub fn migration_status(&self) -> Result<MigrationStatus, String> {
        let mut connection = self.connection()?;
        let tx = connection.transaction().map_err(failure)?;
        let (generation, ready) = tx
            .query_row(
                "SELECT generation,ready FROM migration_state WHERE singleton=1",
                [],
                |row| Ok((row.get::<_, i64>(0)? as u64, row.get(1)?)),
            )
            .map_err(failure)?;
        let pending_origins = {
            let mut query=tx.prepare("SELECT origin FROM migration_requirements WHERE origin NOT IN (SELECT origin FROM migration_origins) ORDER BY origin").map_err(failure)?;
            let origins = query
                .query_map([], |row| row.get(0))
                .map_err(failure)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(failure)?;
            origins
        };
        tx.commit().map_err(failure)?;
        Ok(MigrationStatus {
            generation,
            ready,
            pending_origins,
        })
    }

    /// Staged data is recoverable but cannot be returned by live repositories.
    /// Byte-identical retries are idempotent. A changed snapshot uses CAS and
    /// invalidates earlier captures, including a capture under validation.
    pub fn stage_migration(
        &self,
        origin: &str,
        expected_generation: u64,
        sources: &[Import<'_>],
    ) -> Result<MigrationStatus, String> {
        identifier(origin)?;
        let checksum = digest(sources)?;
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        let (generation, ready): (i64, bool) = tx
            .query_row(
                "SELECT generation,ready FROM migration_state WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(failure)?;
        if ready {
            return Err("SQLite authority already committed. Legacy snapshots cannot change current settings.".into());
        }
        let previous: Option<String> = tx
            .query_row(
                "SELECT checksum FROM migration_origins WHERE origin=?1",
                [origin],
                |row| row.get(0),
            )
            .optional()
            .map_err(failure)?;
        if previous.as_deref() == Some(&checksum) {
            drop(tx);
            return self.migration_status();
        }
        if generation as u64 != expected_generation {
            return Err(
                "Migration revision conflict. Refresh migration status and retry the snapshot."
                    .into(),
            );
        }
        let declared: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM migration_requirements WHERE origin=?1)",
                [origin],
                |row| row.get(0),
            )
            .map_err(failure)?;
        if !declared {
            return Err("This migration origin was not declared by native bootstrap.".into());
        }
        let other_bytes: i64 = tx
            .query_row(
                "SELECT coalesce(sum(length(payload)),0) FROM migration_snapshots WHERE origin<>?1",
                [origin],
                |row| row.get(0),
            )
            .map_err(failure)?;
        let incoming_bytes: usize = sources
            .iter()
            .map(|s| s.payload.map_or(0, <[u8]>::len))
            .sum();
        if other_bytes as usize + incoming_bytes > MAX_BATCH_BYTES {
            return Err("Migration snapshots exceed their aggregate bound.".into());
        }
        tx.execute("INSERT INTO migration_origins VALUES(?1,?2) ON CONFLICT(origin) DO UPDATE SET checksum=excluded.checksum",params![origin,checksum]).map_err(failure)?;
        tx.execute("DELETE FROM migration_snapshots WHERE origin=?1", [origin])
            .map_err(failure)?;
        for source in sources {
            let hash = source
                .payload
                .map(|p| format!("sha256:{:x}", Sha256::digest(p)));
            tx.execute(
                "INSERT INTO migration_snapshots VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    origin,
                    source.source,
                    source.domain,
                    source.key,
                    source.format_version,
                    hash,
                    source.payload
                ],
            )
            .map_err(failure)?;
        }
        tx.execute(
            "UPDATE migration_state SET generation=generation+1 WHERE singleton=1",
            [],
        )
        .map_err(failure)?;
        tx.commit().map_err(failure)?;
        self.migration_status()
    }

    pub fn capture_migration(&self) -> Result<MigrationCapture, String> {
        let mut connection = self.connection()?;
        let tx = connection.transaction().map_err(failure)?;
        let (generation, ready): (i64, bool) = tx
            .query_row(
                "SELECT generation,ready FROM migration_state WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(failure)?;
        if ready {
            return Err("SQLite migration already completed.".into());
        }
        let requirements: i64 = tx
            .query_row("SELECT count(*) FROM migration_requirements", [], |row| {
                row.get(0)
            })
            .map_err(failure)?;
        let received: i64 = tx
            .query_row("SELECT count(*) FROM migration_origins", [], |row| {
                row.get(0)
            })
            .map_err(failure)?;
        if requirements == 0 || received != requirements {
            return Err(
                "Required native/webview snapshots are still pending. No authority was changed."
                    .into(),
            );
        }
        let sources = {
            let mut query=tx.prepare("SELECT origin,source,domain,key,format_version,checksum,payload FROM migration_snapshots ORDER BY origin,source").map_err(failure)?;
            let rows = query
                .query_map([], |row| {
                    Ok(StagedSource {
                        origin: row.get(0)?,
                        source: row.get(1)?,
                        domain: row.get(2)?,
                        key: row.get(3)?,
                        format_version: row.get(4)?,
                        checksum: row.get(5)?,
                        payload: row.get(6)?,
                    })
                })
                .map_err(failure)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(failure)?;
            rows
        };
        tx.commit().map_err(failure)?;
        Ok(MigrationCapture {
            generation: generation as u64,
            sources,
        })
    }

    /// Validator must cover every staged domain, format, source checksum and
    /// cross-reference. It can return a recovery diagnostic without changing
    /// authority. Snapshot backup completes before the short authority commit.
    pub fn finish_migration(
        &self,
        capture: MigrationCapture,
        backup: &Path,
        validate: impl FnOnce(&[StagedSource]) -> Result<(), String>,
    ) -> Result<(), String> {
        validate(&capture.sources)?;
        let mut keys = std::collections::BTreeMap::new();
        for source in &capture.sources {
            if source.checksum
                != source
                    .payload
                    .as_deref()
                    .map(|p| format!("sha256:{:x}", Sha256::digest(p)))
            {
                return Err(
                    "Migration snapshot checksum mismatch. Preserve sources for recovery.".into(),
                );
            }
            // Multiple origins may agree, but conflicting records require an
            // explicit native resolution rather than accidental origin order.
            if let Some(previous) = keys.insert(
                (&source.domain, &source.key),
                (source.format_version, &source.payload),
            ) {
                if previous != (source.format_version, &source.payload) {
                    return Err("Conflicting migration origins. Resolve the source explicitly before cutover.".into());
                }
            }
        }
        self.backup(backup)?;
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        let (generation, ready): (i64, bool) = tx
            .query_row(
                "SELECT generation,ready FROM migration_state WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(failure)?;
        if ready || generation as u64 != capture.generation {
            return Err("Migration changed during validation. The protected snapshot remains available; validate again before cutover.".into());
        }
        for source in &capture.sources {
            tx.execute(
                "INSERT INTO domains VALUES(?1) ON CONFLICT DO NOTHING",
                [&source.domain],
            )
            .map_err(failure)?;
            if let Some(payload) = &source.payload {
                tx.execute(
                    "INSERT INTO records VALUES(?1,?2,?3,1,?4) ON CONFLICT DO NOTHING",
                    params![source.domain, source.key, source.format_version, payload],
                )
                .map_err(failure)?;
            }
            tx.execute(
                "INSERT INTO imports VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT DO NOTHING",
                params![
                    source.origin,
                    source.source,
                    source.domain,
                    source.key,
                    source.format_version,
                    source.checksum,
                    source.payload
                ],
            )
            .map_err(failure)?;
        }
        tx.execute("INSERT INTO origins SELECT origin FROM migration_origins WHERE true ON CONFLICT DO NOTHING",[]).map_err(failure)?;
        tx.execute("UPDATE migration_state SET ready=1,generation=generation+1,backup_path=?1 WHERE singleton=1",[backup.to_string_lossy().as_ref()]).map_err(failure)?;
        tx.commit().map_err(failure)
    }
}

#[cfg(test)]
#[path = "migration_tests.rs"]
mod tests;
