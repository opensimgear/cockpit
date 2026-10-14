use super::*;
use crate::vjoy::{Axis, AXIS_USAGES};
fn slot(id: u8) -> Slot {
    Slot {
        id,
        status: Status::Free,
        owner_pid: None,
        buttons: 8,
        axes: AXIS_USAGES
            .iter()
            .map(|u| Axis {
                usage: *u,
                min: 0,
                max: 32767,
            })
            .collect(),
        continuous_hats: 0,
        discrete_hats: 0,
        force_feedback: false,
    }
}
fn initial() -> Snapshot {
    Snapshot {
        entries: BTreeMap::new(),
        published: vec![slot(1)],
        ancillary_fingerprint: "a".repeat(64),
        unexpected_parameters: false,
    }
}
pub(crate) struct Fake {
    current: Snapshot,
    pub(crate) journals: Vec<Journal>,
    fail: &'static str,
    refreshes: usize,
    attempts: usize,
    removed: Vec<u8>,
}
impl Fake {
    pub(crate) fn new() -> Self {
        Self {
            current: initial(),
            journals: vec![],
            fail: "",
            refreshes: 0,
            attempts: 0,
            removed: vec![],
        }
    }
}
impl Backend for Fake {
    fn safe_to_restore(&mut self) -> Result<(), String> {
        if self.fail == "uncertain" {
            Err("refresh child exit unconfirmed".into())
        } else {
            Ok(())
        }
    }
    fn snapshot(&mut self) -> Result<Snapshot, String> {
        Ok(self.current.clone())
    }
    fn journal(&mut self, journal: &Journal) -> Result<(), String> {
        if self.fail == "journal" && self.journals.is_empty() {
            return Err("journal failure".into());
        }
        self.journals.push(journal.clone());
        Ok(())
    }
    fn create(&mut self, id: u8, value: &Entry) -> Result<(), MutationError> {
        self.attempts += 1;
        if self.fail == "race" && id == 2 {
            self.current.entries.insert(id, value.clone());
            return Err(MutationError {
                owned: false,
                message: "external creation".into(),
            });
        }
        if self.current.entries.contains_key(&id) {
            return Err(MutationError {
                owned: false,
                message: "already exists".into(),
            });
        }
        self.current.entries.insert(id, value.clone());
        if self.fail == "partial" && id == 2 {
            self.current.entries.get_mut(&id).unwrap().size = None;
            return Err(MutationError {
                owned: true,
                message: "write failure".into(),
            });
        }
        Ok(())
    }
    fn remove(&mut self, id: u8) -> Result<(), String> {
        self.removed.push(id);
        self.current.entries.remove(&id);
        Ok(())
    }
    fn refresh(&mut self) -> Result<(), String> {
        self.refreshes += 1;
        if ["refresh", "uncertain"].contains(&self.fail) && self.refreshes == 1 {
            return Err("refresh failure".into());
        }
        self.current.published = if self.current.entries.is_empty() {
            vec![slot(1)]
        } else {
            self.current
                .entries
                .iter()
                .map(|(id, entry)| {
                    let shape =
                        Layout::from_descriptor(*id, entry.descriptor.as_deref().unwrap()).unwrap();
                    let mut device = slot(*id);
                    device.buttons = shape.buttons.into();
                    device.continuous_hats = shape.hats;
                    device.axes = device
                        .axes
                        .into_iter()
                        .enumerate()
                        .filter(|(i, _)| shape.axes & (1 << i) != 0)
                        .map(|(_, axis)| axis)
                        .collect();
                    device
                })
                .collect()
        };
        Ok(())
    }
}
#[test]
fn qualified_creation_materializes_defaults_and_retains_persistent_slots() {
    use sha2::{Digest, Sha256};
    assert_eq!(
        format!("{:x}", Sha256::digest(descriptor(1).unwrap())),
        "3ccc044ec270e74cb588c56b1b49c7e9aba74eb878666efca43855d6b1ea1e93"
    );
    let mut backend = Fake::new();
    let committed = create(&mut backend, 2).unwrap();
    assert!(committed.committed);
    assert!(backend.journals[0].written.is_empty());
    assert_eq!(backend.current.entries.len(), 2);
    assert_eq!(backend.current.published.len(), 2);
    assert_eq!(committed.written, BTreeSet::from([1, 2]));
    assert!(restore(&mut backend, &committed).is_err());
    let third = create(&mut backend, 3).unwrap();
    assert_eq!(third.written, BTreeSet::from([3]));
    assert_eq!(backend.current.published.len(), 3);
    assert!(create(&mut backend, 2).is_err());
    assert!(descriptor(0).is_err());
}
#[test]
fn failed_partial_creation_restores_only_owned_entries_and_preserves_oem_state() {
    for fail in ["partial", "refresh", "journal"] {
        let mut backend = Fake::new();
        backend.fail = fail;
        assert!(create(&mut backend, 2).is_err());
        assert!(same_config(&backend.current, &initial()));
        if fail == "journal" {
            assert_eq!(backend.attempts, 0);
        }
    }
    let mut backend = Fake::new();
    backend.fail = "race";
    let error = create(&mut backend, 2).unwrap_err();
    assert!(error.contains("preserve") || error.contains("Recovery"));
    assert!(backend.current.entries.contains_key(&2)); // An identical external descriptor is not ours.
}
#[test]
fn recovery_rejects_external_mutations_malformed_journals_and_busy_owners() {
    let mut backend = Fake::new();
    let mut journal = plan(initial(), 2).unwrap();
    for (id, value) in &journal.owned {
        backend.current.entries.insert(*id, value.clone());
    }
    journal.written = BTreeSet::from([1, 2]);
    backend.current.entries.get_mut(&2).unwrap().unknown = true;
    assert!(restore(&mut backend, &journal).is_err());
    backend.current.entries.get_mut(&2).unwrap().unknown = false;
    backend.current.ancillary_fingerprint = "b".repeat(64);
    assert!(restore(&mut backend, &journal).is_err());
    backend.current.ancillary_fingerprint = "a".repeat(64);
    backend.current.published[0].status = Status::Busy;
    assert!(restore(&mut backend, &journal).is_err());
    backend.current.published[0].status = Status::Free;
    let mut bad = journal.clone();
    bad.schema = 2;
    assert!(restore(&mut backend, &bad).is_err());
    let mut bad = journal.clone();
    bad.owned.get_mut(&2).unwrap().size = Some(1);
    assert!(restore(&mut backend, &bad).is_err());
    restore(&mut backend, &journal).unwrap();
    assert!(same_config(&backend.current, &initial()));
    let mut custom = initial();
    custom.published[0].force_feedback = true;
    assert!(plan(custom, 2).is_err());
    let mut busy = initial();
    busy.published[0].status = Status::Busy;
    assert!(plan(busy, 2).is_err());
    let mut unknown = initial();
    unknown.unexpected_parameters = true;
    assert!(plan(unknown, 2).is_err());
    let mut mismatch = initial();
    mismatch.entries.insert(2, entry(2).unwrap());
    assert!(plan(mismatch, 3).is_err());
}

#[test]
fn missing_owned_keys_retire_ownership_and_stale_plans_cannot_restart() {
    let mut backend = Fake::new();
    let mut journal = plan(initial(), 2).unwrap();
    let id = journal.id.clone();
    assert_eq!(execute(&mut backend, journal.clone()).unwrap().id, id);
    let mut backend = Fake::new();
    journal.written = BTreeSet::from([1, 2]);
    assert!(execute(&mut backend, journal.clone()).is_err());
    restore(&mut backend, &journal).unwrap();
    assert_eq!(backend.removed, [2, 1]);
    journal.committed = true;
    journal.written.clear();
    assert!(validate_journal(&journal).is_err());
    journal.committed = false;
    journal.id = id.to_uppercase();
    assert!(validate_journal(&journal).is_err());
}

#[test]
fn uncertain_refresh_never_rolls_back_or_starts_a_second_refresh() {
    let mut backend = Fake::new();
    backend.fail = "uncertain";
    let error = create(&mut backend, 2).unwrap_err();
    assert!(error.contains("unconfirmed"));
    assert!(backend.removed.is_empty());
    assert_eq!(backend.refreshes, 1);
    assert_eq!(backend.current.entries.len(), 2);
    assert!(!backend.journals.last().unwrap().committed);
}

#[test]
fn cancellation_at_each_checkpoint_restores_owned_changes_without_cancelling_cleanup() {
    for stop_at in 0..7 {
        let mut backend = Fake::new();
        let journal = plan(initial(), 2).unwrap();
        let mut checks = 0;
        let result = execute_controlled(&mut backend, journal, || {
            let cancelled = checks == stop_at;
            checks += 1;
            if cancelled {
                Err("operation cancelled".into())
            } else {
                Ok(())
            }
        });
        assert!(result.unwrap_err().contains("cancelled"));
        assert!(
            same_config(&backend.current, &initial()),
            "checkpoint {stop_at}"
        );
        if stop_at < 3 {
            assert_eq!(backend.refreshes, 0);
        }
    }
    let mut backend = Fake::new();
    let journal = plan(initial(), 2).unwrap();
    assert!(
        execute_controlled(&mut backend, journal, || Ok(()))
            .unwrap()
            .committed
    );
}

#[test]
fn starter_custom_layout_retention_and_cancelled_custom_creation() {
    let mut backend = Fake::new();
    let original = backend.current.clone();
    let planned = plan_layout(original, 2, Layout::STARTER).unwrap();
    execute(&mut backend, planned).unwrap();
    let starter = backend.current.clone();
    assert_eq!(starter.published[1].buttons, 32);
    assert_eq!(starter.published[1].axes.len(), 8);
    assert_eq!(starter.published[1].continuous_hats, 1);
    assert_eq!(starter.entries[&1], entry(1).unwrap());
    let custom = Layout {
        axes: 0x8101,
        buttons: 128,
        hats: 4,
    };
    let planned = plan_layout(starter.clone(), 3, custom).unwrap();
    let mut checks = 0;
    assert!(execute_controlled(&mut backend, planned, || {
        checks += 1;
        if checks == 5 {
            Err("cancel after refresh".into())
        } else {
            Ok(())
        }
    })
    .is_err());
    assert!(same_config(&backend.current, &starter));
    let planned = plan_layout(starter, 3, custom).unwrap();
    let done = execute(&mut backend, planned).unwrap();
    validate_journal(&done).unwrap();
    let mut altered = done;
    altered
        .owned
        .get_mut(&3)
        .unwrap()
        .descriptor
        .as_mut()
        .unwrap()[0] ^= 1;
    assert!(validate_journal(&altered).is_err());
    backend.current.published[2].buttons -= 1;
    assert!(plan(backend.current.clone(), 4).is_err());
}

#[test]
fn journals_cannot_claim_existing_implicit_slots_or_skip_default_preservation() {
    let mut journal = plan(initial(), 2).unwrap();
    journal.target = 1;
    assert!(validate_journal(&journal).is_err());
    let mut journal = plan(initial(), 2).unwrap();
    journal.owned.remove(&1);
    assert!(validate_journal(&journal).is_err());
    let mut journal = plan(initial(), 2).unwrap();
    journal
        .owned
        .insert(1, entry_layout(1, Layout::STARTER).unwrap());
    assert!(validate_journal(&journal).is_err());
    let mut duplicate = initial();
    duplicate.published.push(slot(1));
    assert!(plan(duplicate, 2).is_err());
}
