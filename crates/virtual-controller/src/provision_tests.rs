use super::*;
use crate::vjoy::Axis;
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
struct Fake {
    current: Snapshot,
    journals: Vec<Journal>,
    fail: &'static str,
    refreshes: usize,
    attempts: usize,
    removed: Vec<u8>,
}
impl Fake {
    fn new() -> Self {
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
        if self.fail == "refresh" && self.refreshes == 1 {
            return Err("refresh failure".into());
        }
        self.current.published = if self.current.entries.is_empty() {
            vec![slot(1)]
        } else {
            self.current.entries.keys().map(|id| slot(*id)).collect()
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
