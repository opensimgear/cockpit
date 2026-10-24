//! Persistent-mode ownership. No automatic recovery on application startup.
use super::policy::{Plan, State};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};
const MAX_RECORD: u64 = 1_048_576;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub id: String,
    pub plan: Plan,
    pub position: usize,
    pub pending: Option<usize>,
    pub restoring: bool,
    pub clean: bool,
}
impl Record {
    pub fn validate(&self) -> Result<Vec<State>, String> {
        let steps = self.plan.steps()?;
        if uuid::Uuid::parse_str(&self.id)
                .ok()
                .map(|u| u.to_string())
                .as_deref()
                != Some(self.id.as_str())
            || self.position >= steps.len()
            || self.pending.is_some_and(|p| {
                p >= steps.len()
                    || if self.restoring {
                        p.checked_add(1) != Some(self.position)
                    } else {
                        self.position.checked_add(1) != Some(p)
                    }
            })
            || (self.clean && (self.position != 0 || self.pending.is_some()))
        {
            return Err("Invalid visibility recovery journal; no policy was changed.".into());
        }
        Ok(steps)
    }
}
fn bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
    osg_persistence::Evidence::inspect(path,"recovery-visibility",MAX_RECORD as usize)
}
pub fn inspect(path: &Path) -> Result<Option<Record>, String> {
    bytes(path)?.as_deref().map(decode).transpose()
}
fn decode(data:&[u8])->Result<Record,String>{
    let record:Record=serde_json::from_slice(data).map_err(|e|e.to_string())?;
    record.validate()?;Ok(record)
}
pub struct Store {
    evidence: osg_persistence::Evidence,
    _lock: File,
}
impl Drop for Store {
    fn drop(&mut self){let _=self._lock.unlock();}
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
            .map_err(|_| "Another Cockpit visibility owner is active.")?;
        let evidence = osg_persistence::Evidence::open(&path,"recovery-visibility",MAX_RECORD as usize)?;
        if let Some(data)=evidence.payload(){decode(data)?;}
        Ok(Self {
            evidence,
            _lock: lock,
        })
    }
    pub fn read(&self) -> Result<Option<Record>, String> {
        self.evidence.verify()?;
        self.evidence.payload().map(decode).transpose()
    }
    pub fn write(&mut self, record: &Record) -> Result<(), String> {
        record.validate()?;
        self.read()?;
        let data = serde_json::to_vec(record).map_err(|e| e.to_string())?;
        if data.len() as u64 > MAX_RECORD {
            return Err("Visibility journal exceeds its bound.".into());
        }
        self.evidence.write(&data)
    }
    pub fn begin(&mut self, plan: Plan) -> Result<Record, String> {
        if self.read()?.is_some_and(|r| !r.clean) {
            return Err("Restore the previous visibility session before starting another.".into());
        }
        let record = Record {
            id: uuid::Uuid::new_v4().to_string(),
            plan,
            position: 0,
            pending: None,
            restoring: false,
            clean: false,
        };
        self.write(&record)?;
        Ok(record)
    }
}
/// Adapter must change exactly one field, after re-reading the entire expected
/// state. HidHide has no external compare-and-swap; races remain a qualification gap.
pub trait Backend {
    fn read(&mut self) -> Result<State, String>;
    fn change(&mut self, expected: &State, desired: &State) -> Result<(), String>;
}
impl<T: Backend + ?Sized> Backend for Box<T> {
    fn read(&mut self) -> Result<State, String> {
        (**self).read()
    }
    fn change(&mut self, expected: &State, desired: &State) -> Result<(), String> {
        (**self).change(expected, desired)
    }
}
fn step(
    backend: &mut impl Backend,
    store: &mut Store,
    record: &mut Record,
    next: usize,
) -> Result<(), String> {
    let steps = record.validate()?;
    if backend.read()? != steps[record.position] {
        return Err(
            "External HidHide changes conflict with this session; preserving current policy."
                .into(),
        );
    }
    record.pending = Some(next);
    store.write(record)?; // Durable ownership intent before the driver call.
    backend.change(&steps[record.position], &steps[next])?;
    if backend.read()? != steps[next] {
        return Err("HidHide mutation readback did not match; recovery needs attention.".into());
    }
    record.position = next;
    record.pending = None;
    store.write(record)
}
pub fn apply(
    backend: &mut impl Backend,
    store: &mut Store,
    plan: Plan,
    mut capture: impl FnMut() -> Result<(), String>,
) -> Result<Record, String> {
    capture()?;
    plan.validate()?;
    if backend.read()? != plan.before {
        return Err("HidHide changed after preflight.".into());
    }
    let mut record = store.begin(plan)?;
    let result: Result<(), String> = (|| {
        let count = record.validate()?.len();
        for next in 1..count {
            capture()?;
            step(backend, store, &mut record, next)?;
        }
        capture()?; // Caller proves fresh capture after hiding, before arming mappings.
        Ok(())
    })();
    if let Err(error) = result {
        return Err(match restore(backend, store, &record.id) {
            Ok(_) => format!("{error} Owned visibility changes restored."),
            Err(recovery) => format!("{error} Explicit recovery requires attention: {recovery}"),
        });
    }
    Ok(record)
}
pub fn restore(backend: &mut impl Backend, store: &mut Store, id: &str) -> Result<Record, String> {
    let mut record = store.read()?.ok_or("No visibility session to restore.")?;
    if record.id != id {
        return Err("Stale visibility session ID.".into());
    }
    if record.clean {
        return Ok(record);
    }
    let steps = record.validate()?;
    let actual = backend.read()?;
    if let Some(pending) = record.pending {
        if actual == steps[pending] {
            record.position = pending;
        } else if actual != steps[record.position] {
            return Err("Interrupted visibility mutation conflicts with external state.".into());
        }
        record.pending = None;
    } else if actual != steps[record.position] {
        return Err("External visibility changes are preserved; inspect recovery conflict.".into());
    }
    record.restoring = true;
    store.write(&record)?;
    while record.position > 0 {
        let previous = record.position - 1;
        step(backend, store, &mut record, previous)?;
    }
    record.clean = true;
    store.write(&record)?;
    Ok(record)
}
#[cfg(test)]
#[path = "journal_tests.rs"]
mod tests;
