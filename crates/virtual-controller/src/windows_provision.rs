//! Elevated provisioning storage. Fixed SYSTEM-hive paths keep the descriptor
//! and ownership journal in one registry transaction. No SDK handle is retained.
use crate::{
    provision::{Backend, Entry, Journal, MutationError, Snapshot},
    registry::{tree, Key, Transaction},
    vjoy::Slot,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use windows_sys::Win32::System::Registry::*;
const PARAMETERS: &str = "SYSTEM\\CurrentControlSet\\Services\\vjoy\\Parameters";
const JOURNALS: &str = "SYSTEM\\CurrentControlSet\\Control\\OpenSimGearVJoyProvision";
const OEM: &str =
    "SYSTEM\\CurrentControlSet\\Control\\MediaProperties\\PrivateProperties\\Joystick\\OEM";
pub type Inspector = Box<dyn FnMut() -> Result<Vec<Slot>, String>>;
pub type Refresher = Box<dyn FnMut() -> Result<(), crate::windows_refresh::Failure>>;
pub struct NativeBackend {
    hive: HKEY,
    parameters: String,
    journals: String,
    operation: String,
    protected: bool,
    ancillary: Vec<(HKEY, String)>,
    inspect: Inspector,
    refresh: Refresher,
}
fn operation_id(id: &str) -> Result<(), String> {
    if uuid::Uuid::parse_str(id)
        .ok()
        .map(|u| u.to_string())
        .as_deref()
        != Some(id)
    {
        return Err("Provisioning operation must be a canonical UUID.".into());
    }
    Ok(())
}
fn entry(key: &Key) -> Result<Entry, String> {
    let names = key.names(false)?;
    let descriptor = key.value("HidReportDescriptor")?;
    let size = key.value("HidReportDescriptorSize")?;
    let unknown = !key.names(true)?.is_empty()
        || names.iter().any(|n| {
            !["hidreportdescriptor", "hidreportdescriptorsize"]
                .contains(&n.to_ascii_lowercase().as_str())
        })
        || descriptor
            .as_ref()
            .is_some_and(|v| v.kind != REG_BINARY || v.data.len() > 2048)
        || size
            .as_ref()
            .is_some_and(|v| v.kind != REG_DWORD || v.data.len() != 4);
    Ok(Entry {
        descriptor: descriptor.map(|v| v.data),
        size: size.and_then(|v| v.data.try_into().ok().map(u32::from_le_bytes)),
        unknown,
    })
}
fn entries(parameters: &Key) -> Result<(BTreeMap<u8, Entry>, bool), String> {
    let mut unexpected = !parameters.names(false)?.is_empty();
    let mut entries = BTreeMap::new();
    for name in parameters.names(true)? {
        if name.eq_ignore_ascii_case("Wdf") {
            continue;
        }
        let slot = name
            .strip_prefix("Device")
            .and_then(|v| v.parse::<u8>().ok());
        if let Some(slot) =
            slot.filter(|id| (1..=16).contains(id) && name == format!("Device{id:02}"))
        {
            let key = Key::open(parameters.0, &name, KEY_READ, None)?
                .ok_or("vJoy registry changed during inspection.")?;
            entries.insert(slot, entry(&key)?);
        } else {
            unexpected = true;
        }
    }
    Ok((entries, unexpected))
}
impl NativeBackend {
    /// Construct callbacks without loading the SDK, elevating, or changing a key.
    pub fn native(operation: &str) -> Result<Self, String> {
        Self::open(
            operation,
            Box::new(crate::inspection::current),
            Box::new(crate::windows_refresh::refresh),
        )
    }
    /// Only the explicit elevated helper constructs this backend.
    pub fn open(operation: &str, inspect: Inspector, refresh: Refresher) -> Result<Self, String> {
        operation_id(operation)?;
        let mut ancillary = vec![];
        for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            for id in ["VID_1234&PID_BEAD", "VID_1234&PID_0FFB"] {
                ancillary.push((hive, format!("{OEM}\\{id}")));
            }
        }
        Ok(Self {
            hive: HKEY_LOCAL_MACHINE,
            parameters: PARAMETERS.into(),
            journals: JOURNALS.into(),
            operation: operation.into(),
            protected: true,
            ancillary,
            inspect,
            refresh,
        })
    }
    /// Called explicitly by the elevated helper before writing its first journal.
    pub fn prepare_journals(&self) -> Result<(), String> {
        crate::journal_security::protected(self.hive, &self.journals).map(|_| ())
    }
    /// Stage only the qualified default layout, retaining this operation's identity.
    pub fn provision(&mut self, target: u8) -> Result<Journal, String> {
        let mut plan = crate::provision::plan(self.snapshot()?, target)?;
        plan.id = self.operation.clone();
        crate::provision::execute(self, plan)
    }
    fn parameters(&self) -> Result<Key, String> {
        Key::open(self.hive, &self.parameters, KEY_READ, None)?
            .ok_or("Installed vJoy parameters are missing.".into())
    }
    fn journal_path(&self) -> String {
        format!("{}\\{}", self.journals, self.operation)
    }
    pub fn read_journal(&self) -> Result<Journal, String> {
        let key = Key::open(self.hive, &self.journal_path(), KEY_READ, None)?
            .ok_or("Provisioning journal is missing.")?;
        self.checked_record(&key)
    }
    fn checked_record(&self, key: &Key) -> Result<Journal, String> {
        if self.protected {
            crate::journal_security::verify_protected(key)?;
        }
        let record = Self::read_record(key)?;
        crate::provision::validate_journal(&record)?;
        if record.id != self.operation {
            return Err("Journal belongs to another operation.".into());
        }
        Ok(record)
    }
    fn settled(key: &Key) -> Result<(), String> {
        match key.value("RefreshPending")? {
            None => Ok(()), // Older journals never launched the new refresh helper.
            Some(value) if value.kind == REG_DWORD && value.data == 0u32.to_le_bytes() => Ok(()),
            Some(_) => Err("Driver refresh is pending or uncertain; preserve configuration for explicit inspection.".into()),
        }
    }
    fn refresh_guard(&self) -> Result<Key, String> {
        let key = Key::open(self.hive, &self.journal_path(), KEY_READ | KEY_WRITE, None)?
            .ok_or("Provisioning journal is missing.")?;
        self.checked_record(&key)?;
        Self::settled(&key)?;
        Ok(key)
    }
    fn read_record(key: &Key) -> Result<Journal, String> {
        let record = key
            .value("Journal")?
            .ok_or("Provisioning journal is missing.")?;
        if record.kind != REG_BINARY {
            return Err("Invalid provisioning journal type.".into());
        }
        serde_json::from_slice(&record.data)
            .map_err(|e| format!("Invalid provisioning journal: {e}"))
    }
    fn write_record(key: &Key, record: &Journal) -> Result<(), String> {
        key.write(
            "Journal",
            REG_BINARY,
            &serde_json::to_vec(record).map_err(|e| e.to_string())?,
        )
    }
    fn fingerprint(&self, parameters: &Key) -> Result<String, String> {
        let mut budget = 262144;
        let mut snapshots = Vec::new();
        let wdf = Key::open(parameters.0, "Wdf", KEY_READ, None)?;
        snapshots.push(
            wdf.as_ref()
                .map(|key| tree(key, 0, &mut budget))
                .transpose()?,
        );
        for (hive, path) in &self.ancillary {
            snapshots.push(
                Key::open(*hive, path, KEY_READ, None)?
                    .as_ref()
                    .map(|key| tree(key, 0, &mut budget))
                    .transpose()?,
            );
        }
        let bytes = serde_json::to_vec(&snapshots).map_err(|e| e.to_string())?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
    fn mutate_create(&mut self, slot: u8, value: &Entry) -> Result<(), String> {
        let tx = Transaction::new()?;
        let journal = Key::open(
            self.hive,
            &self.journal_path(),
            KEY_READ | KEY_WRITE,
            Some(&tx),
        )?
        .ok_or("Provisioning journal is missing.")?;
        let mut record = self.checked_record(&journal)?;
        Self::settled(&journal)?;
        if record.committed
            || record.owned.get(&slot) != Some(value)
            || record.before.entries.contains_key(&slot)
            || record.written.contains(&slot)
        {
            return Err("Descriptor creation does not match unused journal intent.".into());
        }
        if value.unknown
            || value.descriptor.as_ref() != Some(&crate::provision::descriptor(slot)?)
            || value.size != Some(123)
        {
            return Err("Only the pinned descriptor can be provisioned.".into());
        }
        let parameters = Key::open(self.hive, &self.parameters, KEY_READ | KEY_WRITE, Some(&tx))?
            .ok_or("vJoy parameters disappeared.")?;
        let (device, created) = Key::create(parameters.0, &format!("Device{slot:02}"), Some(&tx))?;
        if !created {
            return Err(
                "An external configurator created the slot; ownership was not taken.".into(),
            );
        }
        device.write(
            "HidReportDescriptor",
            REG_BINARY,
            value.descriptor.as_ref().unwrap(),
        )?;
        device.write("HidReportDescriptorSize", REG_DWORD, &123u32.to_le_bytes())?;
        record.written.insert(slot);
        Self::write_record(&journal, &record)?;
        drop(device);
        drop(parameters);
        drop(journal);
        tx.commit()
    }
}
impl Backend for NativeBackend {
    fn snapshot(&mut self) -> Result<Snapshot, String> {
        let parameters = self.parameters()?;
        let (entries, unexpected_parameters) = entries(&parameters)?;
        let ancillary_fingerprint = self.fingerprint(&parameters)?;
        let published = (self.inspect)()?;
        Ok(Snapshot {
            entries,
            published,
            ancillary_fingerprint,
            unexpected_parameters,
        })
    }
    fn journal(&mut self, record: &Journal) -> Result<(), String> {
        crate::provision::validate_journal(record)?;
        if record.id != self.operation || record.schema != 1 {
            return Err("Provisioning journal identity does not match this operation.".into());
        }
        if let Some(existing) =
            Key::open(self.hive, &self.journal_path(), KEY_READ | KEY_WRITE, None)?
        {
            let old = self.checked_record(&existing)?;
            Self::settled(&existing)?;
            if old.id != record.id
                || old.target != record.target
                || old.owned != record.owned
                || old.written != record.written
                || serde_json::to_value(&old.before).unwrap()
                    != serde_json::to_value(&record.before).unwrap()
                || (old.committed && !record.committed)
            {
                return Err("Provisioning journal changed; preserving it.".into());
            }
            Self::write_record(&existing, record)?;
            existing.flush()
        } else {
            if !record.written.is_empty() || record.committed {
                return Err("A new journal cannot claim previous slot ownership.".into());
            }
            // The caller creates this fixed SYSTEM-hive parent with an admin-only
            // write ACL before entering provisioning; no user-supplied file path.
            let parent = Key::open(self.hive, &self.journals, KEY_READ | KEY_WRITE, None)?
                .ok_or("Protected provisioning journal directory is missing.")?;
            let (key, created) = if self.protected {
                crate::journal_security::verify_protected(&parent)?;
                crate::journal_security::protected(parent.0, &self.operation)?
            } else {
                Key::create(parent.0, &self.operation, None)?
            };
            if !created {
                return Err("Provisioning operation already exists.".into());
            }
            Self::write_record(&key, record)?;
            key.flush()
        }
    }
    fn create(&mut self, slot: u8, value: &Entry) -> Result<(), MutationError> {
        self.mutate_create(slot, value)
            .map_err(|message| MutationError {
                // The durable ledger and descriptor commit together, including when
                // the caller cannot confirm the commit's response.
                owned: self.read_journal().is_ok_and(|j| j.written.contains(&slot)),
                message,
            })
    }
    fn remove(&mut self, slot: u8) -> Result<(), String> {
        let tx = Transaction::new()?;
        let journal = Key::open(
            self.hive,
            &self.journal_path(),
            KEY_READ | KEY_WRITE,
            Some(&tx),
        )?
        .ok_or("Provisioning journal is missing.")?;
        let mut record = self.checked_record(&journal)?;
        Self::settled(&journal)?;
        if record.committed
            || !record.written.contains(&slot)
            || record.before.entries.contains_key(&slot)
        {
            return Err("Recovery cannot remove an unowned or committed slot.".into());
        }
        let expected = record
            .owned
            .get(&slot)
            .ok_or("Missing owned descriptor intent.")?;
        let parameters = Key::open(self.hive, &self.parameters, KEY_READ | KEY_WRITE, Some(&tx))?
            .ok_or("vJoy parameters disappeared.")?;
        let name = format!("Device{slot:02}");
        if let Some(device) = Key::open(parameters.0, &name, KEY_READ, Some(&tx))? {
            if entry(&device)? != *expected {
                return Err("Owned descriptor changed externally; preserving it.".into());
            }
            drop(device);
            parameters.delete_child(&name, &tx)?;
        }
        // Remove ownership in the same transaction. A subsequently recreated
        // byte-identical key must never be attributed to this old operation.
        record.written.remove(&slot);
        Self::write_record(&journal, &record)?;
        drop(parameters);
        drop(journal);
        tx.commit()
    }
    fn refresh(&mut self) -> Result<(), String> {
        let key = self.refresh_guard()?;
        // Flush before spawning: crashes between spawn and return remain fenced.
        key.write("RefreshPending", REG_DWORD, &1u32.to_le_bytes())?;
        key.flush()?;
        let result = (self.refresh)();
        if result.as_ref().err().is_none_or(|e| e.quiescent) {
            key.write("RefreshPending", REG_DWORD, &0u32.to_le_bytes())?;
            key.flush()?;
        }
        result.map_err(|e| e.message)
    }
    fn safe_to_restore(&mut self) -> Result<(), String> {
        self.refresh_guard().map(|_| ())
    }
}
#[cfg(test)]
#[path = "windows_provision_tests.rs"]
mod tests;
