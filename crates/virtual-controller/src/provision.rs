//! Bounded provisioning transactions. Native activation remains qualification-gated.
//! Unknown/FFB layouts are preserved. The journal precedes registry mutations.
use crate::{
    descriptor::Layout,
    vjoy::{Slot, Status},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Entry {
    pub descriptor: Option<Vec<u8>>,
    pub size: Option<u32>,
    pub unknown: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub entries: BTreeMap<u8, Entry>,
    pub published: Vec<Slot>,
    pub ancillary_fingerprint: String,
    pub unexpected_parameters: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Journal {
    pub schema: u32,
    pub id: String,
    pub before: Snapshot,
    pub owned: BTreeMap<u8, Entry>,
    pub target: u8,
    pub written: BTreeSet<u8>,
    pub committed: bool,
}
pub struct MutationError {
    pub owned: bool,
    pub message: String,
}
pub trait Backend {
    fn snapshot(&mut self) -> Result<Snapshot, String>;
    fn journal(&mut self, record: &Journal) -> Result<(), String>;
    fn create(&mut self, slot: u8, entry: &Entry) -> Result<(), MutationError>;
    fn remove(&mut self, slot: u8) -> Result<(), String>;
    fn refresh(&mut self) -> Result<(), String>;
    /// Refuse recovery while a native refresh may still be changing the stack.
    fn safe_to_restore(&mut self) -> Result<(), String> {
        Ok(())
    }
}
pub fn descriptor(slot: u8) -> Result<Vec<u8>, String> {
    Layout::LEGACY.descriptor(slot)
}
fn entry(slot: u8) -> Result<Entry, String> {
    entry_layout(slot, Layout::LEGACY)
}
fn entry_layout(slot: u8, layout: Layout) -> Result<Entry, String> {
    let bytes = layout.descriptor(slot)?;
    Ok(Entry {
        size: Some(bytes.len() as u32),
        descriptor: Some(bytes),
        unknown: false,
    })
}
fn entry_shape(slot: u8, value: &Entry) -> Result<Layout, String> {
    let bytes = value.descriptor.as_deref().ok_or("Missing descriptor.")?;
    if value.unknown || value.size != Some(bytes.len() as u32) {
        return Err("Unknown or incomplete descriptor.".into());
    }
    Layout::from_descriptor(slot, bytes)
}
fn qualified(snapshot: &Snapshot) -> Result<(), String> {
    if snapshot.unexpected_parameters
        || snapshot.published.is_empty()
        || snapshot.published.len() > 16
        || snapshot.ancillary_fingerprint.len() != 64
    {
        return Err("vJoy configuration is outside the qualified provisioning shape.".into());
    }
    for slot in &snapshot.published {
        if slot.status != Status::Free {
            return Err("Close all vJoy feeders before changing driver layouts.".into());
        }
        Layout::from_slot(slot)?;
    }
    let ids: BTreeSet<_> = snapshot.published.iter().map(|s| s.id).collect();
    if ids.len() != snapshot.published.len() || ids.iter().any(|id| !(1..=16).contains(id)) {
        return Err("Invalid or duplicate published slot IDs.".into());
    }
    if snapshot.entries.is_empty() {
        if snapshot.published.len() != 1
            || snapshot.published[0].id != 1
            || Layout::from_slot(&snapshot.published[0])? != Layout::LEGACY
        {
            return Err("Unexpected implicit vJoy slots.".into());
        }
    } else {
        if snapshot.entries.len() != snapshot.published.len() {
            return Err("Registry and published slots do not agree.".into());
        }
        for published in &snapshot.published {
            let value = snapshot
                .entries
                .get(&published.id)
                .ok_or("Missing published descriptor.")?;
            if entry_shape(published.id, value)? != Layout::from_slot(published)? {
                return Err("Registry descriptor and published layout differ.".into());
            }
        }
    }
    Ok(())
}
pub fn plan(snapshot: Snapshot, target: u8) -> Result<Journal, String> {
    plan_layout(snapshot, target, Layout::LEGACY)
}
pub fn plan_layout(snapshot: Snapshot, target: u8, layout: Layout) -> Result<Journal, String> {
    layout.validate()?;
    qualified(&snapshot)?;
    if snapshot.published.iter().any(|s| s.id == target) || snapshot.entries.contains_key(&target) {
        return Err("Choose an unused vJoy slot; existing slots are never overwritten.".into());
    }
    let mut owned = BTreeMap::new();
    if snapshot.entries.is_empty() {
        owned.insert(1, entry(1)?);
    }
    owned.insert(target, entry_layout(target, layout)?);
    Ok(Journal {
        schema: 1,
        id: uuid::Uuid::new_v4().to_string(),
        before: snapshot,
        owned,
        target,
        written: BTreeSet::new(),
        committed: false,
    })
}
fn same_config(a: &Snapshot, b: &Snapshot) -> bool {
    a.entries == b.entries
        && a.ancillary_fingerprint == b.ancillary_fingerprint
        && a.unexpected_parameters == b.unexpected_parameters
        && a.published.iter().map(|s| s.id).collect::<Vec<_>>()
            == b.published.iter().map(|s| s.id).collect::<Vec<_>>()
}
pub fn create(backend: &mut impl Backend, target: u8) -> Result<Journal, String> {
    let journal = plan(backend.snapshot()?, target)?;
    execute(backend, journal)
}
/// Execute a validated plan with the operation ID shared by the helper and guardian.
pub fn execute(backend: &mut impl Backend, journal: Journal) -> Result<Journal, String> {
    execute_controlled(backend, journal, || Ok(()))
}
/// Cancellation is cooperative: finish an in-flight refresh before considering
/// rollback, and never cancel recovery itself after configuration was changed.
pub fn execute_controlled(
    backend: &mut impl Backend,
    mut journal: Journal,
    mut check: impl FnMut() -> Result<(), String>,
) -> Result<Journal, String> {
    validate_journal(&journal)?;
    check()?;
    if journal.committed || !journal.written.is_empty() {
        return Err("Provisioning requires a fresh uncommitted plan.".into());
    }
    backend.journal(&journal)?;
    let result = (|| {
        check()?;
        let current = backend.snapshot()?;
        qualified(&current)?;
        if !same_config(&current, &journal.before) {
            return Err("vJoy changed after preflight; no overwrite performed.".into());
        }
        for (slot, entry) in &journal.owned {
            check()?;
            match backend.create(*slot, entry) {
                Ok(()) => {
                    journal.written.insert(*slot);
                }
                Err(error) => {
                    if error.owned {
                        journal.written.insert(*slot);
                        let _ = backend.journal(&journal);
                    }
                    return Err(error.message);
                }
            }
            backend.journal(&journal)?;
        }
        check()?;
        backend.refresh()?;
        check()?;
        let actual = backend.snapshot()?;
        qualified(&actual)?;
        let mut expected = journal.before.entries.clone();
        expected.extend(journal.owned.clone());
        if actual.entries != expected
            || actual.ancillary_fingerprint != journal.before.ancillary_fingerprint
        {
            return Err("Provisioned layout readback or OEM preservation failed.".into());
        }
        check()?;
        journal.committed = true;
        backend.journal(&journal)?;
        Ok(())
    })();
    if let Err(error) = result {
        journal.committed = false;
        let recovery = restore(backend, &journal);
        return Err(match recovery {
            Ok(()) => format!("{error} Original configuration restored."),
            Err(conflict) => format!("{error} Recovery requires attention: {conflict}"),
        });
    }
    Ok(journal)
}
/// Validate durable journal intent without loading or mutating a driver.
pub fn validate_journal(journal: &Journal) -> Result<(), String> {
    if journal.schema != 1
        || journal
            .written
            .iter()
            .any(|s| !journal.owned.contains_key(s))
        || uuid::Uuid::parse_str(&journal.id)
            .ok()
            .map(|id| id.to_string())
            .as_deref()
            != Some(&journal.id)
        || (journal.committed && journal.written.len() != journal.owned.len())
        || journal.owned.is_empty()
        || journal.owned.len() > 2
        || !journal.owned.contains_key(&journal.target)
        || journal
            .owned
            .keys()
            .any(|s| *s != 1 && *s != journal.target)
    {
        return Err("Invalid provisioning recovery journal.".into());
    }
    qualified(&journal.before)?;
    if journal
        .before
        .published
        .iter()
        .any(|s| s.id == journal.target)
        || journal.owned.len()
            != if journal.before.entries.is_empty() {
                2
            } else {
                1
            }
        || (journal.before.entries.is_empty() && journal.owned.get(&1) != Some(&entry(1)?))
    {
        return Err("Journal does not match a fresh, non-overwriting creation plan.".into());
    }
    for (slot, value) in &journal.owned {
        entry_shape(*slot, value)?;
        if (*slot != journal.target && value != &entry(*slot)?)
            || journal.before.entries.contains_key(slot)
        {
            return Err("Journal cannot overwrite pre-existing descriptors.".into());
        }
    }
    Ok(())
}
pub fn restore(backend: &mut impl Backend, journal: &Journal) -> Result<(), String> {
    if journal.committed {
        return Err(
            "Provisioning is committed; preserve the persistent device and inspect its layout."
                .into(),
        );
    }
    validate_journal(journal)?;
    backend.safe_to_restore()?;
    let actual = backend.snapshot()?;
    if actual.unexpected_parameters
        || actual.ancillary_fingerprint != journal.before.ancillary_fingerprint
        || actual.published.iter().any(|s| s.status != Status::Free)
    {
        return Err(
            "External configuration or active feeder conflicts with recovery; preserving it."
                .into(),
        );
    }
    for (slot, value) in &actual.entries {
        if let Some(expected) = journal
            .owned
            .get(slot)
            .filter(|_| journal.written.contains(slot))
        {
            // Permit only known partial writes from this journal.
            if value.unknown
                || value
                    .descriptor
                    .as_ref()
                    .is_some_and(|v| Some(v) != expected.descriptor.as_ref())
                || value.size.is_some_and(|s| Some(s) != expected.size)
            {
                return Err(
                    "Owned descriptor changed externally; recovery will not overwrite it.".into(),
                );
            }
        } else if journal.before.entries.get(slot) != Some(value) {
            return Err(
                "An unrelated descriptor changed; preserve it for explicit recovery.".into(),
            );
        }
    }
    if journal
        .before
        .entries
        .iter()
        .any(|(slot, value)| actual.entries.get(slot) != Some(value))
    {
        return Err("A pre-existing descriptor changed; preserving current settings.".into());
    }
    if journal.written.is_empty() && same_config(&actual, &journal.before) {
        return Ok(()); // A cancelled preflight must not restart unrelated slots.
    }
    for slot in journal.written.iter().rev() {
        // Retire durable ownership even when an external tool already removed the key.
        backend.remove(*slot)?;
    }
    backend.refresh()?;
    let restored = backend.snapshot()?;
    qualified(&restored)?;
    if !same_config(&restored, &journal.before) {
        return Err("Original configuration could not be verified.".into());
    }
    Ok(())
}
#[cfg(test)]
#[path = "provision_tests.rs"]
pub(crate) mod tests;
