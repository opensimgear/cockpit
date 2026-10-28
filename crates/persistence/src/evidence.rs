//! Revisioned recovery bytes. Domain validators and OS lifetime ownership stay
//! with the adapter; database transactions never contain external effects.
use crate::{Change, Database, PayloadCheck};
use std::path::Path;
use std::sync::Mutex;

mod live;

pub struct Evidence {
    database: Database,
    domain: String,
    key: String,
    limit: usize,
    revision: u64,
    payload: Option<Vec<u8>>,
    live: Option<Mutex<live::Reader>>,
}
impl Evidence {
    pub fn open(anchor: &Path, domain: &str, limit: usize) -> Result<Self, String> {
        let key = crate::namespace::evidence_key(anchor)?;
        crate::identifier(domain)?;
        if limit == 0 || limit > crate::MAX_PAYLOAD {
            return Err("Invalid recovery evidence bound.".into());
        }
        let database = Database::for_namespace(anchor)?;
        let (revision, record) = database.read_with_revision(domain, &key)?;
        let payload = record.map(|r| r.payload);
        bounded(payload.as_deref(), limit)?;
        Ok(Self {
            database,
            domain: domain.into(),
            key,
            limit,
            revision,
            payload,
            live: None,
        })
    }
    /// Retain a checked connection for repeated pre-dispatch verification.
    /// The adapter must retain its OS lifetime ownership alongside this value.
    pub fn open_live(anchor: &Path, domain: &str, limit: usize) -> Result<Self, String> {
        let mut evidence = Self::open(anchor, domain, limit)?;
        evidence.live = Some(Mutex::new(live::Reader::open(&evidence.database)?));
        evidence.verify()?;
        Ok(evidence)
    }
    /// Passive inspection never creates a database or reads retired JSON files.
    pub fn inspect(anchor: &Path, domain: &str, limit: usize) -> Result<Option<Vec<u8>>, String> {
        let key = crate::namespace::evidence_key(anchor)?;
        crate::identifier(domain)?;
        let Some(database) = Database::existing_for_namespace(anchor)? else {
            return Ok(None);
        };
        let payload = database.read(domain, &key)?.map(|r| r.payload);
        bounded(payload.as_deref(), limit)?;
        Ok(payload)
    }
    pub fn payload(&self) -> Option<&[u8]> {
        self.payload.as_deref()
    }
    /// Bounded discovery of one adapter's sibling records, excluding tombstones.
    pub fn siblings(anchor: &Path, domain: &str, limit: usize) -> Result<Vec<String>, String> {
        let key = crate::namespace::evidence_key(anchor)?;
        crate::identifier(domain)?;
        if limit == 0 || limit > 1024 {
            return Err("Invalid evidence inventory bound.".into());
        }
        let prefix = key
            .rsplit_once('/')
            .map_or("", |(parent, _)| &key[..parent.len() + 1]);
        let Some(database) = Database::existing_for_namespace(anchor)? else {
            return Ok(vec![]);
        };
        let connection = database.connection()?;
        let mut statement=connection.prepare("SELECT substr(key,length(?2)+1) FROM records WHERE domain=?1 AND payload IS NOT NULL AND substr(key,1,length(?2))=?2 AND instr(substr(key,length(?2)+1),'/')=0 ORDER BY key LIMIT ?3").map_err(crate::failure)?;
        let keys = statement
            .query_map(rusqlite::params![domain, prefix, limit as i64 + 1], |row| {
                row.get(0)
            })
            .map_err(crate::failure)?
            .collect::<Result<Vec<String>, _>>()
            .map_err(crate::failure)?;
        if keys.len() > limit {
            return Err("Recovery evidence inventory exceeds its bound.".into());
        }
        Ok(keys)
    }
    pub fn verify(&self) -> Result<(), String> {
        if let Some(reader) = &self.live {
            return reader
                .try_lock()
                .map_err(|_| "Recovery evidence verification is already in progress.")?
                .verify(self);
        }
        let (revision, record) = self.database.read_with_revision(&self.domain, &self.key)?;
        if revision != self.revision
            || record.as_ref().map(|r| r.payload.as_slice()) != self.payload()
        {
            return Err("Recovery evidence changed externally; preserving it.".into());
        }
        Ok(())
    }
    pub fn write(&mut self, payload: &[u8]) -> Result<(), String> {
        bounded(Some(payload), self.limit)?;
        if self.live.is_some() {
            self.verify()?;
        }
        let revisions = self.database.commit_exact(
            &[Change {
                domain: &self.domain,
                key: &self.key,
                expected_revision: self.revision,
                payload: Some(payload),
            }],
            &[],
            &[PayloadCheck {
                domain: &self.domain,
                key: &self.key,
                expected_revision: self.revision,
                expected_payload: self.payload(),
            }],
        )?;
        self.revision = revisions[0];
        self.payload = Some(payload.to_vec());
        Ok(())
    }
}
fn bounded(payload: Option<&[u8]>, limit: usize) -> Result<(), String> {
    if payload.is_some_and(|p| p.len() > limit) {
        return Err("Recovery evidence exceeds its bound.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
