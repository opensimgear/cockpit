//! Typed/versioned document repository with native source import and CAS saves.
use crate::{Change, Database, Import, Record};
use serde::{de::DeserializeOwned, Serialize};
use std::{path::PathBuf, sync::Mutex};

pub struct Repository<T> {
    database: Database,
    domain: String,
    key: String,
    format_version: u32,
    source: Option<PathBuf>,
    limit: usize,
    validate: fn(&T) -> Result<(), String>,
    revision: Mutex<Option<u64>>,
}
impl<T: Serialize + DeserializeOwned> Repository<T> {
    pub fn new(
        database: Database,
        domain: &str,
        key: &str,
        format_version: u32,
        source: Option<PathBuf>,
        limit: usize,
        validate: fn(&T) -> Result<(), String>,
    ) -> Result<Self, String> {
        crate::identifier(domain)?;
        crate::identifier(key)?;
        if format_version == 0 || limit == 0 || limit > crate::MAX_PAYLOAD {
            return Err("Invalid repository format or size bound.".into());
        }
        Ok(Self {
            database,
            domain: domain.into(),
            key: key.into(),
            format_version,
            source,
            limit,
            validate,
            revision: Mutex::new(None),
        })
    }
    fn decode(&self, record: Record) -> Result<T, String> {
        if record.format_version != self.format_version || record.payload.len() > self.limit {
            return Err(
                "Unsupported or oversized SQLite document. Preserve the database for recovery."
                    .into(),
            );
        }
        let value: T = serde_json::from_slice(&record.payload)
            .map_err(|_| "Invalid SQLite document. Preserve the database for recovery.")?;
        (self.validate)(&value)?;
        Ok(value)
    }
    fn hydrate(&self) -> Result<(u64, Option<T>), String> {
        if let Some(source) = &self.source {
            let origin = format!("native:{}:{}", self.domain, self.key);
            if !self.database.origin_imported(&origin)? {
                // A previously committed record (including a tombstone) is
                // authoritative even if an additional legacy origin arrives late.
                if self.database.revision(&self.domain, &self.key)? == 0 {
                    let snapshot = crate::snapshot(source, self.limit)?;
                    if let Some(bytes) = &snapshot {
                        let value: T = serde_json::from_slice(bytes).map_err(|_| "Invalid legacy document. The source was preserved and import was not committed.")?;
                        (self.validate)(&value)?;
                    }
                    self.database.import(
                        &origin,
                        &[Import {
                            source: &source.to_string_lossy(),
                            domain: &self.domain,
                            key: &self.key,
                            format_version: self.format_version,
                            payload: snapshot.as_deref(),
                        }],
                    )?;
                } else {
                    self.database.import(&origin, &[])?;
                }
            }
        }
        let (revision, record) = self.database.read_with_revision(&self.domain, &self.key)?;
        Ok((
            revision,
            record.map(|record| self.decode(record)).transpose()?,
        ))
    }
    pub fn load(&self) -> Result<Option<T>, String> {
        let mut expected = self
            .revision
            .lock()
            .map_err(|_| "Repository access failed.")?;
        let (revision, value) = self.hydrate()?;
        *expected = Some(revision);
        Ok(value)
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
            format_version: self.format_version,
            expected_revision: revision,
            payload: Some(&payload),
        }])?;
        *expected = Some(revisions[0]);
        Ok(())
    }

    /// Explicit, confirmed domain reset. Preserve the previous database payload
    /// as recovery evidence and leave legacy sources untouched. This cannot
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
            format_version: self.format_version,
            expected_revision: revision,
            payload: Some(&payload),
        }];
        if let Some(previous) = &previous {
            changes.push(Change {
                domain: "recovery/explicit-reset",
                key: &archived_key,
                format_version: previous.format_version,
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
mod tests {
    use super::*;
    fn validate(value: &serde_json::Value) -> Result<(), String> {
        if value.is_object() {
            Ok(())
        } else {
            Err("Expected object".into())
        }
    }
    fn repository(db: &Database, source: PathBuf) -> Repository<serde_json::Value> {
        Repository::new(
            db.clone(),
            "profiles",
            "library",
            1,
            Some(source),
            1024,
            validate,
        )
        .unwrap()
    }
    #[test]
    fn explicit_reset_retains_database_evidence_and_never_rewrites_legacy_sources() {
        let dir = crate::test_directory();
        let source = dir.path().join("library.json");
        let db = Database::open(dir.path().join("cockpit.sqlite3")).unwrap();
        std::fs::write(&source, b"corrupt legacy document").unwrap();
        let store = repository(&db, source.clone());
        assert!(store.load().is_err());
        store.reset(&serde_json::json!({"reset":true})).unwrap();
        assert_eq!(store.load().unwrap().unwrap()["reset"], true);
        assert_eq!(std::fs::read(&source).unwrap(), b"corrupt legacy document");
        db.commit(&[Change {
            domain: "profiles",
            key: "library",
            format_version: 1,
            expected_revision: 1,
            payload: Some(b"corrupt database document"),
        }])
        .unwrap();
        let recovery = repository(&db, source.clone());
        assert!(recovery.load().is_err());
        recovery
            .reset(&serde_json::json!({"recovered":true}))
            .unwrap();
        assert_eq!(
            db.read("recovery/explicit-reset", "profiles:library:2")
                .unwrap()
                .unwrap()
                .payload,
            b"corrupt database document"
        );
        assert!(store
            .reset(&serde_json::json!({"stale":true}))
            .unwrap_err()
            .contains("conflict"));
        assert!(recovery.reset(&serde_json::json!([])).is_err());
        assert!(recovery
            .reset(&serde_json::json!({"long":"x".repeat(2048)}))
            .is_err());
    }
    #[test]
    fn import_preserves_source_validates_before_receipt_and_ignores_stale_files() {
        let dir = crate::test_directory();
        let source = dir.path().join("library.json");
        let db = Database::open(dir.path().join("cockpit.sqlite3")).unwrap();
        let store = repository(&db, source.clone());
        for invalid in ["broken", "[]"] {
            std::fs::write(&source, invalid).unwrap();
            assert!(store.load().is_err());
            assert!(!db.origin_imported("native:profiles:library").unwrap());
        }
        std::fs::write(&source, r#"{"saved":true}"#).unwrap();
        assert_eq!(store.load().unwrap().unwrap()["saved"], true);
        store.save(&serde_json::json!({"edited":true})).unwrap();
        std::fs::write(&source, b"stale broken legacy file").unwrap();
        assert_eq!(
            repository(&db, source.clone()).load().unwrap().unwrap()["edited"],
            true
        );
        assert_eq!(std::fs::read(&source).unwrap(), b"stale broken legacy file");
    }
    #[test]
    fn stale_reader_cannot_overwrite_a_new_save_and_failed_validation_does_not_advance() {
        let dir = crate::test_directory();
        let source = dir.path().join("library.json");
        let db = Database::open(dir.path().join("cockpit.sqlite3")).unwrap();
        let a = repository(&db, source.clone());
        let b = repository(&db, source.clone());
        assert!(a.load().unwrap().is_none());
        assert!(b.load().unwrap().is_none());
        a.save(&serde_json::json!({"a":1})).unwrap();
        assert!(b
            .save(&serde_json::json!({"b":2}))
            .unwrap_err()
            .contains("conflict"));
        assert!(a.save(&serde_json::json!([])).is_err());
        assert!(a
            .save(&serde_json::json!({"long":"x".repeat(2048)}))
            .is_err());
        a.save(&serde_json::json!({"a":2})).unwrap();
        assert_eq!(b.load().unwrap().unwrap()["a"], 2);
        b.save(&serde_json::json!({"b":3})).unwrap();
        assert!(!source.exists());
    }
    #[test]
    fn corrupt_and_future_database_documents_do_not_default_or_fallback() {
        let dir = crate::test_directory();
        let source = dir.path().join("library.json");
        let db = Database::open(dir.path().join("cockpit.sqlite3")).unwrap();
        for (version, payload, revision) in [
            (2, b"{}".as_slice(), 0),
            (1, b"{invalid".as_slice(), 1),
            (1, b"[]".as_slice(), 2),
        ] {
            db.commit(&[Change {
                domain: "profiles",
                key: "library",
                format_version: version,
                expected_revision: revision,
                payload: Some(payload),
            }])
            .unwrap();
            assert!(repository(&db, source.clone()).load().is_err());
        }
        assert!(
            Repository::<serde_json::Value>::new(db.clone(), "", "k", 1, None, 1, validate)
                .is_err()
        );
        assert!(Repository::<serde_json::Value>::new(db, "d", "k", 0, None, 1, validate).is_err());
    }
}
