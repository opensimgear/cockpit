//! Bounded local output recovery intent. Loading never sends reports or acquires
//! a driver. The OS file lock is held for the lifetime of a live/recovery session.
use crate::vjoy::Binding;
use osg_mapping::engine::RunToken;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
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
    pub token: RunToken,
    pub bindings: Vec<Binding>,
    pub phase: Phase,
}
impl Record {
    fn validate(&self) -> Result<(), String> {
        if self.token.generation.0 == 0
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
fn bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
    osg_persistence::Evidence::inspect(path,"recovery-output",LIMIT)
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
    evidence: osg_persistence::Evidence,
    _lock: File,
    record: Option<Record>,
}
impl Drop for Store {
    fn drop(&mut self) {
        // A concurrently spawned Unix child can briefly inherit the open file
        // description until exec. Closing only our descriptor leaves its flock
        // held; explicitly release ownership before closing the local handle.
        let _ = self._lock.unlock();
    }
}
impl Store {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        osg_persistence::check_path(&path,false)?;
        let lock_path=path.with_extension("lock");
        osg_persistence::check_path(&lock_path,true)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|e| e.to_string())?;
        lock.try_lock()
            .map_err(|e| format!("Another Cockpit session owns output recovery storage: {e}"))?;
        let evidence = osg_persistence::Evidence::open(&path,"recovery-output",LIMIT)?;
        let record = evidence.payload().map(decode).transpose()?;
        Ok(Self {
            evidence,
            _lock: lock,
            record,
        })
    }
    pub fn record(&self) -> Option<&Record> {
        self.record.as_ref()
    }
    /// Read-only comparison while this Store still holds the ownership lock.
    pub fn verify_current(&self) -> Result<(), String> {
        self.evidence.verify()
    }
    fn save(&mut self, record: Record) -> Result<(), String> {
        record.validate()?;
        self.evidence.verify()?;
        let data = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
        if data.len() > LIMIT {
            return Err("Output recovery record exceeds 64 KiB.".into());
        }
        self.evidence.write(&data)?;
        self.record = Some(record);
        Ok(())
    }
    pub fn begin(&mut self, token: RunToken, bindings: Vec<Binding>) -> Result<(), String> {
        if self.record.as_ref().is_some_and(Record::pending) {
            return Err("Resolve the interrupted output run before starting another.".into());
        }
        self.save(Record {
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
