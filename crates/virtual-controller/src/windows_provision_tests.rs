use super::*;
use crate::{
    provision,
    registry::wide,
    vjoy::{Axis, Status, AXIS_USAGES},
};
use std::{cell::Cell, rc::Rc};

struct Fixture {
    root: String,
    backend: NativeBackend,
    refreshes: Rc<Cell<usize>>,
}
fn slot(id: u8) -> Slot {
    Slot {
        id,
        status: Status::Free,
        owner_pid: None,
        buttons: 8,
        continuous_hats: 0,
        discrete_hats: 0,
        force_feedback: false,
        axes: AXIS_USAGES
            .iter()
            .map(|usage| Axis {
                usage: *usage,
                min: 0,
                max: 32767,
            })
            .collect(),
    }
}
fn expected(id: u8) -> Entry {
    Entry {
        descriptor: Some(provision::descriptor(id).unwrap()),
        size: Some(123),
        unknown: false,
    }
}
impl Fixture {
    fn new() -> Self {
        let root = format!("Software\\OpenSimGearTest\\{}", uuid::Uuid::new_v4());
        let parameters = format!("{root}\\Parameters");
        let journals = format!("{root}\\Journals");
        Key::create(HKEY_CURRENT_USER, &parameters, None).unwrap();
        Key::create(HKEY_CURRENT_USER, &journals, None).unwrap();
        let refreshes = Rc::new(Cell::new(0));
        let counter = refreshes.clone();
        let inspect_path = parameters.clone();
        let backend = NativeBackend {
            hive: HKEY_CURRENT_USER,
            parameters,
            journals,
            operation: uuid::Uuid::new_v4().to_string(),
            protected: false,
            ancillary: vec![(HKEY_CURRENT_USER, format!("{root}\\OEM"))],
            inspect: Box::new(move || {
                let key = Key::open(HKEY_CURRENT_USER, &inspect_path, KEY_READ, None)?.unwrap();
                let (entries, _) = entries(&key)?;
                Ok(if entries.is_empty() {
                    vec![slot(1)]
                } else {
                    entries.keys().map(|id| slot(*id)).collect()
                })
            }),
            refresh: Box::new(move || {
                counter.set(counter.get() + 1);
                Ok(())
            }),
        };
        Self {
            root,
            backend,
            refreshes,
        }
    }
    fn plan(&mut self) -> Journal {
        let mut record = provision::plan(self.backend.snapshot().unwrap(), 2).unwrap();
        record.id = self.backend.operation.clone();
        record
    }
    fn device(&self, id: u8, value: &Entry) -> Key {
        let (key, created) = Key::create(
            HKEY_CURRENT_USER,
            &format!("{}\\Device{id:02}", self.backend.parameters),
            None,
        )
        .unwrap();
        assert!(created);
        if let Some(bytes) = &value.descriptor {
            key.write("HidReportDescriptor", REG_BINARY, bytes).unwrap();
        }
        if let Some(size) = value.size {
            key.write("HidReportDescriptorSize", REG_DWORD, &size.to_le_bytes())
                .unwrap();
        }
        key
    }
    fn transactions(&self) -> bool {
        let tx = Transaction::new().unwrap();
        match Key::open(
            HKEY_CURRENT_USER,
            &self.backend.parameters,
            KEY_READ,
            Some(&tx),
        ) {
            Ok(Some(_)) => true,
            Err(error) if error.contains("os error 6801") => {
                eprintln!("UNQUALIFIED: isolated test hive cannot transact: {error}");
                false
            }
            result => panic!(
                "Unexpected transaction result: {}",
                result.err().unwrap_or_default()
            ),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(self.root.starts_with("Software\\OpenSimGearTest\\"));
        assert!(uuid::Uuid::parse_str(self.root.rsplit('\\').next().unwrap()).is_ok());
        unsafe {
            RegDeleteTreeW(HKEY_CURRENT_USER, wide(&self.root).unwrap().as_ptr());
        }
    }
}
#[test]
fn snapshot_preserves_raw_unknown_fields_and_fingerprints_ancillary_settings() {
    let mut f = Fixture::new();
    let initial = f.backend.snapshot().unwrap();
    let plan = f.plan();
    f.backend.journal(&plan).unwrap();
    assert!(initial.entries.is_empty());
    assert!(!initial.unexpected_parameters);
    assert_eq!(initial.published.len(), 1);
    let params = Key::open(
        f.backend.hive,
        &f.backend.parameters,
        KEY_READ | KEY_WRITE,
        None,
    )
    .unwrap()
    .unwrap();
    let (wdf, _) = Key::create(params.0, "Wdf", None).unwrap();
    wdf.write("Opaque", REG_BINARY, &[1, 2, 3]).unwrap();
    let changed = f.backend.snapshot().unwrap();
    assert_ne!(changed.ancillary_fingerprint, initial.ancillary_fingerprint);
    let (oem, _) = Key::create(HKEY_CURRENT_USER, &format!("{}\\OEM", f.root), None).unwrap();
    oem.write("External", REG_DWORD, &4u32.to_le_bytes())
        .unwrap();
    assert_ne!(
        f.backend.snapshot().unwrap().ancillary_fingerprint,
        changed.ancillary_fingerprint
    );
    let device = f.device(1, &expected(1));
    assert_eq!(f.backend.snapshot().unwrap().entries[&1], expected(1));
    device.write("External", REG_BINARY, &[9]).unwrap();
    assert!(f.backend.snapshot().unwrap().entries[&1].unknown);
    let second = f.device(
        2,
        &Entry {
            descriptor: None,
            size: None,
            unknown: false,
        },
    );
    second
        .write("HidReportDescriptor", REG_SZ, &[0, 0])
        .unwrap();
    second
        .write("HidReportDescriptorSize", REG_BINARY, &[3])
        .unwrap();
    let snapshot = f.backend.snapshot().unwrap();
    assert!(snapshot.entries[&2].unknown);
    assert_eq!(snapshot.entries[&2].size, None);
    Key::create(params.0, "Device3", None).unwrap();
    assert!(f.backend.snapshot().unwrap().unexpected_parameters);
    params
        .write("Unexpected", REG_DWORD, &0u32.to_le_bytes())
        .unwrap();
    assert!(f.backend.snapshot().unwrap().unexpected_parameters);
    f.backend.refresh().unwrap();
    assert_eq!(f.refreshes.get(), 1);
    f.backend.inspect = Box::new(|| Err("inspection failed".into()));
    assert!(f
        .backend
        .snapshot()
        .unwrap_err()
        .contains("inspection failed"));
    f.backend.parameters.push_str("\\Missing");
    assert!(f.backend.snapshot().unwrap_err().contains("missing"));
}
#[test]
fn journal_rejects_identity_forgery_claims_and_external_changes() {
    let mut f = Fixture::new();
    assert!(f.backend.read_journal().is_err());
    let mut record = f.plan();
    let id = record.id.clone();
    record.id = "bad".into();
    assert!(f.backend.journal(&record).is_err());
    record.id = uuid::Uuid::new_v4().to_string();
    assert!(f.backend.journal(&record).is_err());
    record.id = id;
    record.schema = 2;
    assert!(f.backend.journal(&record).is_err());
    record.schema = 1;
    record.written.insert(1);
    assert!(f.backend.journal(&record).is_err());
    record.written.clear();
    record.committed = true;
    assert!(f.backend.journal(&record).is_err());
    record.committed = false;
    f.backend.journal(&record).unwrap();
    f.backend.journal(&record).unwrap();
    assert_eq!(f.backend.read_journal().unwrap().id, record.id);
    let mut altered = record.clone();
    altered.target = 3;
    assert!(f.backend.journal(&altered).is_err());
    let mut altered = record.clone();
    altered.before.ancillary_fingerprint = "0".repeat(64);
    assert!(f.backend.journal(&altered).is_err());
    let mut altered = record.clone();
    altered.written.insert(1);
    assert!(f.backend.journal(&altered).is_err());
    record.written = record.owned.keys().copied().collect();
    let stored = Key::open(
        f.backend.hive,
        &f.backend.journal_path(),
        KEY_READ | KEY_WRITE,
        None,
    )
    .unwrap()
    .unwrap();
    NativeBackend::write_record(&stored, &record).unwrap();
    record.committed = true;
    f.backend.journal(&record).unwrap();
    record.committed = false;
    assert!(f.backend.journal(&record).is_err());
    let journal = Key::open(
        f.backend.hive,
        &f.backend.journal_path(),
        KEY_READ | KEY_WRITE,
        None,
    )
    .unwrap()
    .unwrap();
    journal.write("Journal", REG_SZ, &[]).unwrap();
    assert!(f.backend.read_journal().unwrap_err().contains("type"));
    journal.write("Journal", REG_BINARY, b"invalid").unwrap();
    assert!(f.backend.journal(&record).is_err());
    let missing = Key::create(HKEY_CURRENT_USER, &format!("{}\\Empty", f.root), None)
        .unwrap()
        .0;
    assert!(NativeBackend::read_record(&missing).is_err());
    assert!(operation_id("not-a-uuid").is_err());
    assert!(operation_id(&uuid::Uuid::new_v4().to_string().to_uppercase()).is_err());
    let native = NativeBackend::open(
        &uuid::Uuid::new_v4().to_string(),
        Box::new(|| Ok(vec![])),
        Box::new(|| Ok(())),
    )
    .unwrap();
    assert_eq!(native.parameters, PARAMETERS);
    assert_eq!(native.ancillary.len(), 4);
    assert!(NativeBackend::native("invalid").is_err());
    assert!(NativeBackend::native(&uuid::Uuid::new_v4().to_string()).is_ok());
    assert!(NativeBackend::open("invalid", Box::new(|| Ok(vec![])), Box::new(|| Ok(()))).is_err());
}
#[test]
fn descriptor_and_ownership_are_atomic_and_recovery_never_deletes_recreated_keys() {
    let mut f = Fixture::new();
    let mut record = f.plan();
    f.backend.journal(&record).unwrap();
    if !f.transactions() {
        let error = f.backend.create(1, &expected(1)).err().unwrap();
        assert!(!error.owned);
        assert!(f.backend.snapshot().unwrap().entries.is_empty());
        assert!(f.backend.read_journal().unwrap().written.is_empty());
        assert!(f.backend.remove(1).is_err());
        return;
    }
    assert!(f.backend.create(3, &expected(3)).is_err());
    let mut wrong = expected(1);
    wrong.unknown = true;
    assert!(f.backend.create(1, &wrong).is_err());
    f.backend
        .create(1, &expected(1))
        .unwrap_or_else(|e| panic!("{}", e.message));
    record.written.insert(1);
    f.backend.journal(&record).unwrap();
    assert!(f.backend.read_journal().unwrap().written.contains(&1));
    assert_eq!(f.backend.snapshot().unwrap().entries[&1], expected(1));
    assert!(f.backend.create(1, &expected(1)).is_err());
    let external = f.device(2, &expected(2));
    let collision = f.backend.create(2, &expected(2)).err().unwrap();
    assert!(!collision.owned);
    assert_eq!(entry(&external).unwrap(), expected(2));
    assert!(f.backend.remove(2).is_err());
    let owned = Key::open(
        f.backend.hive,
        &format!("{}\\Device01", f.backend.parameters),
        KEY_READ | KEY_WRITE,
        None,
    )
    .unwrap()
    .unwrap();
    owned
        .write("HidReportDescriptorSize", REG_DWORD, &999u32.to_le_bytes())
        .unwrap();
    assert!(f
        .backend
        .remove(1)
        .unwrap_err()
        .contains("changed externally"));
    owned
        .write("HidReportDescriptorSize", REG_DWORD, &123u32.to_le_bytes())
        .unwrap();
    drop(owned);
    f.backend.remove(1).unwrap();
    assert!(!f.backend.read_journal().unwrap().written.contains(&1));
    assert!(!f.backend.snapshot().unwrap().entries.contains_key(&1));
    f.device(1, &expected(1));
    assert!(f.backend.remove(1).is_err());
    assert_eq!(f.backend.snapshot().unwrap().entries[&1], expected(1));
}

#[test]
fn production_plan_keeps_the_helper_operation_identity_and_fails_closed_without_transactions() {
    let mut f = Fixture::new();
    if f.transactions() {
        let record = f.backend.provision(2).unwrap();
        assert_eq!(record.id, f.backend.operation);
        assert!(record.committed);
        assert_eq!(record.written, [1, 2].into_iter().collect());
        assert_eq!(f.backend.snapshot().unwrap().entries.len(), 2);
    } else {
        let error = f.backend.provision(2).unwrap_err();
        assert!(error.contains("6801"));
        assert!(f.backend.snapshot().unwrap().entries.is_empty());
        assert!(f.backend.read_journal().unwrap().written.is_empty());
    }
}

#[test]
fn native_backend_refuses_unprotected_existing_journal_roots_and_records() {
    let mut f = Fixture::new();
    let record = f.plan();
    f.backend.protected = true;
    assert!(f.backend.prepare_journals().is_err());
    assert!(f.backend.journal(&record).is_err());
    assert!(
        Key::open(f.backend.hive, &f.backend.journal_path(), KEY_READ, None)
            .unwrap()
            .is_none()
    );
    f.backend.protected = false;
    f.backend.journal(&record).unwrap();
    f.backend.protected = true;
    assert!(f.backend.read_journal().is_err());
    assert!(f.backend.journal(&record).is_err());
    assert!(f.backend.snapshot().unwrap().entries.is_empty());
}

#[test]
fn protected_journals_use_admin_ownership_when_the_test_token_can_assign_it() {
    let mut f = Fixture::new();
    let protected_path = format!("{}\\Protected", f.root);
    match crate::journal_security::protected(f.backend.hive, &protected_path) {
        Ok((key, created)) => {
            assert!(created);
            crate::journal_security::verify_protected(&key).unwrap();
        }
        Err(error) if error.contains("Windows error 5") || error.contains("Windows error 1307") => {
            eprintln!("UNQUALIFIED: this test token cannot assign the admin owner: {error}");
            assert!(Key::open(f.backend.hive, &protected_path, KEY_READ, None)
                .unwrap()
                .is_none());
            return;
        }
        Err(error) => panic!("{error}"),
    }
    f.backend.journals = protected_path;
    f.backend.protected = true;
    f.backend.prepare_journals().unwrap();
    let record = f.plan();
    f.backend.journal(&record).unwrap();
    assert_eq!(f.backend.read_journal().unwrap().id, record.id);
    f.backend.journal(&record).unwrap();
    if f.transactions() {
        f.backend
            .create(1, &expected(1))
            .unwrap_or_else(|e| panic!("{}", e.message));
        f.backend.remove(1).unwrap();
        assert!(f.backend.read_journal().unwrap().written.is_empty());
    }
}

#[test]
fn durable_refresh_fence_survives_failure_and_blocks_every_recovery_mutation() {
    let mut f = Fixture::new();
    assert!(f.backend.refresh().is_err()); // No journal, no launch.
    let record = f.plan();
    f.backend.journal(&record).unwrap();
    f.backend.refresh = Box::new(|| {
        Err(crate::windows_refresh::Failure {
            message: "child exit unknown".into(),
            quiescent: false,
        })
    });
    assert!(f.backend.refresh().unwrap_err().contains("unknown"));
    let stored = Key::open(
        f.backend.hive,
        &f.backend.journal_path(),
        KEY_READ | KEY_WRITE,
        None,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        stored.value("RefreshPending").unwrap().unwrap().data,
        1u32.to_le_bytes()
    );
    f.backend.refresh = Box::new(|| panic!("uncertain operation must not launch another child"));
    assert!(f.backend.safe_to_restore().is_err());
    assert!(provision::restore(&mut f.backend, &record)
        .unwrap_err()
        .contains("uncertain"));
    assert!(f.backend.refresh().is_err());
    assert!(f.backend.journal(&record).is_err());
    assert!(f.backend.snapshot().unwrap().entries.is_empty());
    // Corrupt guards also refuse recovery. Only this isolated fixture writes them.
    stored.write("RefreshPending", REG_BINARY, &[0]).unwrap();
    assert!(f.backend.safe_to_restore().is_err());
    stored
        .write("RefreshPending", REG_DWORD, &0u32.to_le_bytes())
        .unwrap();
    f.backend.safe_to_restore().unwrap();
    f.backend.refresh = Box::new(|| Err("confirmed child failure".into()));
    assert!(f.backend.refresh().unwrap_err().contains("confirmed"));
    f.backend.safe_to_restore().unwrap();
    f.backend.refresh = Box::new(|| Ok(()));
    f.backend.refresh().unwrap();
    f.backend.safe_to_restore().unwrap();
}
