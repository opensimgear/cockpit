//! Persistent-mode ownership. No automatic recovery on application startup.
use super::policy::{Plan, State};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
const MAX_RECORD: u64 = 1_048_576;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub schema: u32,
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
        if self.schema != 1
            || uuid::Uuid::parse_str(&self.id)
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
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_RECORD {
        return Err("Visibility journal must be a bounded regular file.".into());
    }
    let mut data = vec![];
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_RECORD + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() as u64 > MAX_RECORD {
        return Err("Visibility journal is oversized.".into());
    }
    Ok(Some(data))
}
pub fn inspect(path: &Path) -> Result<Option<Record>, String> {
    bytes(path)?
        .map(|data| {
            let record: Record = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
            record.validate()?;
            Ok(record)
        })
        .transpose()
}
pub struct Store {
    path: PathBuf,
    expected: Option<Vec<u8>>,
    _lock: File,
}
impl Store {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let parent = path
            .parent()
            .ok_or("Invalid visibility journal location.")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let lock_path = path.with_extension("lock");
        if std::fs::symlink_metadata(&lock_path)
            .is_ok_and(|m| !m.is_file() || m.file_type().is_symlink())
        {
            return Err("Invalid visibility lock file.".into());
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|e| e.to_string())?;
        lock.try_lock()
            .map_err(|_| "Another Cockpit visibility owner is active.")?;
        let expected = bytes(&path)?;
        inspect(&path)?;
        Ok(Self {
            path,
            expected,
            _lock: lock,
        })
    }
    pub fn read(&self) -> Result<Option<Record>, String> {
        if bytes(&self.path)? != self.expected {
            return Err("Visibility journal changed externally; preserve it for recovery.".into());
        }
        inspect(&self.path)
    }
    pub fn write(&mut self, record: &Record) -> Result<(), String> {
        record.validate()?;
        self.read()?;
        let data = serde_json::to_vec(record).map_err(|e| e.to_string())?;
        if data.len() as u64 > MAX_RECORD {
            return Err("Visibility journal exceeds its bound.".into());
        }
        let mut temp = tempfile::NamedTempFile::new_in(self.path.parent().unwrap())
            .map_err(|e| e.to_string())?;
        temp.write_all(&data).map_err(|e| e.to_string())?;
        temp.as_file().sync_all().map_err(|e| e.to_string())?;
        temp.persist(&self.path).map_err(|e| e.to_string())?;
        self.expected = Some(data);
        #[cfg(unix)]
        File::open(self.path.parent().unwrap())
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn begin(&mut self, plan: Plan) -> Result<Record, String> {
        if self.read()?.is_some_and(|r| !r.clean) {
            return Err("Restore the previous visibility session before starting another.".into());
        }
        let record = Record {
            schema: 1,
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
