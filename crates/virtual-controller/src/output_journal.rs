//! Bounded local output recovery intent. Loading never sends reports or acquires
//! a driver. The OS file lock is held for the lifetime of a live/recovery session.
use crate::vjoy::Binding;
use osg_mapping::engine::RunToken;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
const LIMIT: usize = 65_536;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Intent,
    Confirmed,
    Releasing,
    Clean,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub schema: u32,
    pub token: RunToken,
    pub bindings: Vec<Binding>,
    pub phase: Phase,
}
impl Record {
    fn validate(&self) -> Result<(), String> {
        if self.schema != 1
            || self.token.generation.0 == 0
            || uuid::Uuid::parse_str(&self.token.session)
                .ok()
                .map(|u| u.to_string())
                .as_deref()
                != Some(&self.token.session)
            || self.bindings.is_empty()
            || self.bindings.len() > 8
        {
            return Err("Invalid output recovery record.".into());
        }
        let mut slots = BTreeSet::new();
        let mut targets = BTreeSet::new();
        for binding in &self.bindings {
            binding.validate()?;
            if !slots.insert(binding.slot) || !targets.insert(&binding.target) {
                return Err("Duplicate output recovery binding.".into());
            }
        }
        Ok(())
    }
    pub fn pending(&self) -> bool {
        self.phase != Phase::Clean
    }
}
fn regular(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
            Err("Recovery paths must be regular files.".into())
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
fn bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
    regular(path)?;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let mut data = Vec::new();
    file.take(LIMIT as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() > LIMIT {
        return Err("Output recovery record exceeds 64 KiB.".into());
    }
    Ok(Some(data))
}
fn decode(data: &[u8]) -> Result<Record, String> {
    let record: Record =
        serde_json::from_slice(data).map_err(|e| format!("Invalid output recovery record: {e}"))?;
    record.validate()?;
    Ok(record)
}
/// Read-only startup/status inspection; missing storage is not an error.
pub fn inspect(path: &Path) -> Result<Option<Record>, String> {
    bytes(path)?.as_deref().map(decode).transpose()
}
pub struct Store {
    path: PathBuf,
    _lock: File,
    expected: Option<Vec<u8>>,
    record: Option<Record>,
}
impl Store {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let parent = path
            .parent()
            .ok_or("Output recovery requires a storage directory.")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let lock_path = path.with_extension("lock");
        regular(&lock_path)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|e| e.to_string())?;
        lock.try_lock()
            .map_err(|e| format!("Another Cockpit session owns output recovery storage: {e}"))?;
        let expected = bytes(&path)?;
        let record = expected.as_deref().map(decode).transpose()?;
        Ok(Self {
            path,
            _lock: lock,
            expected,
            record,
        })
    }
    pub fn record(&self) -> Option<&Record> {
        self.record.as_ref()
    }
    fn save(&mut self, record: Record) -> Result<(), String> {
        record.validate()?;
        if bytes(&self.path)? != self.expected {
            return Err("Output recovery record changed externally; preserving it.".into());
        }
        let data = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
        if data.len() > LIMIT {
            return Err("Output recovery record exceeds 64 KiB.".into());
        }
        let parent = self.path.parent().unwrap();
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        temp.write_all(&data).map_err(|e| e.to_string())?;
        temp.as_file().sync_all().map_err(|e| e.to_string())?;
        temp.persist(&self.path).map_err(|e| e.to_string())?;
        self.expected = Some(data);
        self.record = Some(record);
        #[cfg(unix)]
        File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn begin(&mut self, token: RunToken, bindings: Vec<Binding>) -> Result<(), String> {
        if self.record.as_ref().is_some_and(Record::pending) {
            return Err("Resolve the interrupted output run before starting another.".into());
        }
        self.save(Record {
            schema: 1,
            token,
            bindings,
            phase: Phase::Intent,
        })
    }
    pub fn transition(&mut self, phase: Phase) -> Result<(), String> {
        let mut record = self
            .record
            .clone()
            .ok_or("No output recovery intent is recorded.")?;
        let allowed = matches!(
            (record.phase, phase),
            (Phase::Intent, Phase::Confirmed | Phase::Clean)
                | (Phase::Confirmed, Phase::Releasing)
                | (Phase::Releasing, Phase::Releasing | Phase::Clean)
        );
        if !allowed {
            return Err("Invalid output recovery phase transition.".into());
        }
        record.phase = phase;
        self.save(record)
    }
    /// An explicit user acknowledgement clears ambiguous intent without touching
    /// the driver. It requires the exact run identity shown by inspection.
    pub fn acknowledge(&mut self, token: &RunToken) -> Result<(), String> {
        let record = self
            .record
            .as_ref()
            .ok_or("No output recovery record exists.")?;
        if &record.token != token || !matches!(record.phase, Phase::Intent | Phase::Releasing) {
            return Err("Only this inspected ambiguous run can be acknowledged.".into());
        }
        let mut record = record.clone();
        record.phase = Phase::Clean;
        self.save(record)
    }
}
#[cfg(test)]
#[path = "output_journal_tests.rs"]
mod tests;
