//! Validated SQLite documents with acknowledged compare-and-swap saves.
use crate::{Change, Database, Record};
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Mutex;

pub struct Repository<T> {
    database: Database,
    domain: String,
    key: String,
    limit: usize,
    validate: fn(&T) -> Result<(), String>,
    revision: Mutex<Option<u64>>,
}
impl<T: Serialize + DeserializeOwned> Repository<T> {
    pub fn new(
        database: Database,
        domain: &str,
        key: &str,
        limit: usize,
        validate: fn(&T) -> Result<(), String>,
    ) -> Result<Self, String> {
        crate::identifier(domain)?;
        crate::identifier(key)?;
        if limit == 0 || limit > crate::MAX_PAYLOAD {
            return Err("Invalid repository size bound.".into());
        }
        Ok(Self {
            database,
            domain: domain.into(),
            key: key.into(),
            limit,
            validate,
            revision: Mutex::new(None),
        })
    }
    fn decode(&self, record: Record) -> Result<T, String> {
        if record.payload.len() > self.limit {
            return Err("Oversized SQLite document. Preserve the database for recovery.".into());
        }
        let value: T = serde_json::from_slice(&record.payload)
            .map_err(|_| "Invalid SQLite document. Preserve the database for recovery.")?;
        (self.validate)(&value)?;
        Ok(value)
    }
    fn hydrate(&self) -> Result<(u64, Option<T>), String> {
        let (revision, record) = self.database.read_with_revision(&self.domain, &self.key)?;
        Ok((
            revision,
            record.map(|record| self.decode(record)).transpose()?,
        ))
    }
    pub fn load(&self) -> Result<Option<T>, String> {
        self.load_snapshot().map(|(_,value)|value)
    }
    /// The returned document and expected revision come from the same read.
    pub fn load_snapshot(&self) -> Result<(u64,Option<T>), String> {
        let mut expected = self
            .revision
            .lock()
            .map_err(|_| "Repository access failed.")?;
        let (revision, value) = self.hydrate()?;
        *expected = Some(revision);
        Ok((revision,value))
    }
    /// Cache advances only after successful commit. A failed save leaves the
    /// last read revision unchanged and cannot hide a concurrent writer.
    pub fn save(&self, value: &T) -> Result<(), String> {
        (self.validate)(value)?;
        let payload =
            serde_json::to_vec(value).map_err(|_| "Cannot encode persistence document.")?;
        if payload.len() > self.limit {
            return Err("Persistence document exceeds its size bound.".into());
        }
        let mut expected = self
            .revision
            .lock()
            .map_err(|_| "Repository access failed.")?;
        let revision = match *expected {
            Some(revision) => revision,
            None => self.hydrate()?.0,
        };
        let revisions = self.database.commit(&[Change {
            domain: &self.domain,
            key: &self.key,
            expected_revision: revision,
            payload: Some(&payload),
        }])?;
        *expected = Some(revisions[0]);
        Ok(())
    }

    /// Explicit, confirmed domain reset. Preserve the previous database payload
    /// as recovery evidence. This cannot
    /// replace a corrupt SQLite file or bypass a concurrent database revision.
    pub fn reset(&self, value: &T) -> Result<(), String> {
        (self.validate)(value)?;
        let payload = serde_json::to_vec(value).map_err(|_| "Cannot encode reset document.")?;
        if payload.len() > self.limit {
            return Err("Reset document exceeds its size bound.".into());
        }
        let mut expected = self
            .revision
            .lock()
            .map_err(|_| "Repository access failed.")?;
        let (revision, previous) = self.database.read_with_revision(&self.domain, &self.key)?;
        if expected.is_some_and(|expected| expected != revision) {
            return Err("Persistence revision conflict. Reload before resetting.".into());
        }
        let archived_key = format!("{}:{}:{}", self.domain, self.key, revision);
        let mut changes = vec![Change {
            domain: &self.domain,
            key: &self.key,
            expected_revision: revision,
            payload: Some(&payload),
        }];
        if let Some(previous) = &previous {
            changes.push(Change {
                domain: "recovery/explicit-reset",
                key: &archived_key,
                expected_revision: 0,
                payload: Some(&previous.payload),
            });
        }
        let revisions = self.database.commit(&changes)?;
        *expected = Some(revisions[0]);
        Ok(())
    }
}

#[cfg(test)]
#[path = "repository_tests.rs"]
mod tests;
