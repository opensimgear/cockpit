//! SQLite intent/receipt transitions. The OS lock owns the entire operation;
//! SQLite transactions end before any filesystem or network effect.
use super::*;
use osg_persistence::Evidence;
use serde::{Deserialize, Serialize};
use std::fs::File;
mod record;
const LIMIT: usize = 16 * 1024 * 1024;
const MAX_RECEIPTS: usize = 64;
const DOMAIN: &str = "recovery-dcs";
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum State {
    Completed,
    Restored,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    state: State,
    record: Value,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Document {
    #[serde(deserialize_with = "required_pending")]
    pub pending: Option<Value>,
    receipts: Vec<Receipt>,
}
fn required_pending<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Value>, D::Error> {
    Option::<Value>::deserialize(deserializer)
}
impl Document {
    fn decode(bytes: Option<&[u8]>) -> Result<Self, String> {
        let document: Self = bytes
            .map(|bytes| {
                serde_json::from_slice(bytes).map_err(|_| {
                    "Invalid DCS setup evidence. Preserve the database for inspection.".to_string()
                })
            })
            .transpose()?
            .unwrap_or_default();
        document.validate()?;
        Ok(document)
    }
    fn validate(&self) -> Result<(), String> {
        if self.receipts.len() > MAX_RECEIPTS {
            return Err("DCS setup receipt inventory exceeds its bound.".into());
        }
        let mut ids = BTreeSet::new();
        for value in self
            .pending
            .iter()
            .chain(self.receipts.iter().map(|r| &r.record))
        {
            record::validate(value)?;
            if !ids.insert(value["id"].as_str().unwrap()) {
                return Err("Duplicate DCS setup ownership identity.".into());
            }
        }
        Ok(())
    }
    fn completed(
        &self,
        target: &Path,
        work: &Path,
    ) -> Result<Vec<(usize, PathBuf, Value)>, String> {
        let target = checked_target(target)?;
        let mut records = Vec::new();
        for (index, receipt) in self.receipts.iter().enumerate().rev() {
            if receipt.state != State::Completed {
                continue;
            }
            let value = &receipt.record;
            let recorded = Path::new(value["target"].as_str().unwrap());
            if fs::canonicalize(recorded).ok().as_ref() != Some(&target) {
                continue;
            }
            checked_target(recorded)?;
            records.push((
                index,
                work.join(value["id"].as_str().unwrap())
                    .join("completed.json"),
                value.clone(),
            ));
        }
        Ok(records)
    }
}
pub(super) struct Ledger {
    evidence: Evidence,
    document: Document,
    lock: File,
}
impl Drop for Ledger {
    fn drop(&mut self) {
        let _ = self.lock.unlock();
    }
}
impl Ledger {
    pub fn inspect(work: &Path) -> Result<Document, String> {
        let bytes = Evidence::inspect(&work.join(JOURNAL), DOMAIN, LIMIT)?;
        Document::decode(bytes.as_deref())
    }
    pub fn open(work: &Path) -> Result<Self, String> {
        let lock_path = work.join("dcs-setup.lock");
        osg_persistence::check_path(&lock_path, true)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|_| "Cannot open DCS setup ownership lock.")?;
        lock.try_lock()
            .map_err(|_| "Another DCS setup operation owns this namespace.")?;
        let evidence = Evidence::open(&work.join(JOURNAL), DOMAIN, LIMIT)?;
        let document = Document::decode(evidence.payload())?;
        Ok(Self {
            evidence,
            document,
            lock,
        })
    }
    pub fn verify(&self) -> Result<(), String> {
        self.evidence.verify()
    }
    pub fn verify_pending(&self, record: &Value) -> Result<(), String> {
        self.verify()?;
        if self.document.pending.as_ref() != Some(record) {
            return Err(
                "DCS setup pending ownership changed; existing files were preserved.".into(),
            );
        }
        Ok(())
    }
    pub fn pending(&self) -> Option<Value> {
        self.document.pending.clone()
    }
    pub fn completed(
        &self,
        target: &Path,
        work: &Path,
    ) -> Result<Vec<(usize, PathBuf, Value)>, String> {
        self.verify()?;
        self.document.completed(target, work)
    }
    fn save(&mut self, next: Document) -> Result<(), String> {
        next.validate()?;
        let bytes = serde_json::to_vec(&next).map_err(|_| "Cannot encode DCS setup evidence.")?;
        self.evidence.write(&bytes)?;
        self.document = next;
        Ok(())
    }
    pub fn begin(&mut self, record: Value) -> Result<(), String> {
        self.verify()?;
        record::validate(&record)?;
        if self.document.pending.is_some() {
            return Err("Recover the interrupted DCS setup before starting again.".into());
        }
        if self.document.receipts.len() >= MAX_RECEIPTS {
            return Err(
                "DCS setup history is full. Reset development data before another installation."
                    .into(),
            );
        }
        let mut next = self.document.clone();
        next.pending = Some(record);
        self.save(next)
    }
    pub fn begin_removal(&mut self, record: &Value) -> Result<(), String> {
        self.verify()?;
        if self.document.pending.is_some() {
            return Err(
                "Recover the interrupted DCS setup before removing another installation.".into(),
            );
        }
        let index = self
            .document
            .receipts
            .iter()
            .position(|r| r.state == State::Completed && r.record == *record)
            .ok_or("The exact completed DCS setup receipt changed; preserve existing files.")?;
        let mut next = self.document.clone();
        next.receipts.remove(index);
        next.pending = Some(record.clone());
        self.save(next)
    }
    pub fn finish(&mut self, state: State) -> Result<(), String> {
        self.verify()?;
        let mut next = self.document.clone();
        let record = next
            .pending
            .take()
            .ok_or("No DCS setup operation owns this receipt.")?;
        next.receipts.push(Receipt { state, record });
        self.save(next)
    }
}
pub(super) fn pending(work: &Path) -> bool {
    Ledger::inspect(work).map_or(true, |document| document.pending.is_some())
}
pub(super) fn completed(
    target: &Path,
    work: &Path,
) -> Result<Vec<(usize, PathBuf, Value)>, String> {
    Ledger::inspect(work)?.completed(target, work)
}

#[cfg(test)]
mod tests;
