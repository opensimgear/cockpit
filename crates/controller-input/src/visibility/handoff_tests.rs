use super::*;
use crate::visibility::{
    journal::{self, Backend, Store},
    policy::{Policy, Selection},
};
struct Driver(State);
impl Backend for Driver {
    fn read(&mut self) -> Result<State, String> {
        Ok(self.0.clone())
    }
    fn change(&mut self, expected: &State, desired: &State) -> Result<(), String> {
        if &self.0 != expected {
            return Err("External policy".into());
        }
        self.0 = desired.clone();
        Ok(())
    }
}
fn original() -> State {
    State {
        active: false,
        inverse: false,
        applications: vec![r"\Device\Volume\Existing.exe".into()],
        hidden: vec![],
    }
}
fn plan(before: State) -> Plan {
    Plan::build(
        before,
        vec![Selection {
            instance: r"HID\EXACT\1".into(),
            policy: Policy::HideAll,
        }],
        r"\Device\Volume\Cockpit.exe".into(),
    )
    .unwrap()
}
fn status(record: &Record) -> Status {
    Status {
        job_id: uuid::Uuid::new_v4().to_string(),
        active: true,
        phase: "active".into(),
        message: String::new(),
        session_id: Some(record.id.clone()),
    }
}
#[test]
fn real_journal_handoff_reverses_original_before_new_hide_and_stop_restores_original() {
    let root = crate::test_directory();
    let mut driver = Driver(original());
    let mut old = Store::open(root.path().join("old.json")).unwrap();
    let record = journal::apply(&mut driver, &mut old, plan(original()), || Ok(())).unwrap();
    let mut status = status(&record);
    let proof = Proof::capture(
        &uuid::Uuid::new_v4().to_string(),
        &record,
        &status,
        &driver.0,
    )
    .unwrap();
    let new_plan = plan(proof.plan.before.clone());
    assert_ne!(new_plan.before, driver.0);
    assert_eq!(new_plan.before, original());
    let mut next = Store::open(root.path().join("new.json")).unwrap();
    assert!(journal::apply(&mut driver, &mut next, new_plan.clone(), || Ok(())).is_err());
    let restored = journal::restore(&mut driver, &mut old, &record.id).unwrap();
    status.active = false;
    status.phase = "restored".into();
    proof
        .validate_restored(&restored, &status, &driver.0)
        .unwrap();
    let incoming = journal::apply(&mut driver, &mut next, new_plan, || Ok(())).unwrap();
    assert_eq!(incoming.plan.before, original());
    journal::restore(&mut driver, &mut next, &incoming.id).unwrap();
    assert_eq!(driver.0, original());
}
#[test]
fn partial_dirty_changed_owner_and_external_driver_state_never_prove_active() {
    let root = crate::test_directory();
    let mut driver = Driver(original());
    let mut store = Store::open(root.path().join("owner.json")).unwrap();
    let record = journal::apply(&mut driver, &mut store, plan(original()), || Ok(())).unwrap();
    let status = status(&record);
    let proof = Proof::capture(
        &uuid::Uuid::new_v4().to_string(),
        &record,
        &status,
        &driver.0,
    )
    .unwrap();
    let mut altered = record.clone();
    altered.position = 0;
    assert!(proof.validate_active(&altered, &status, &driver.0).is_err());
    altered.pending = Some(1);
    assert!(proof.validate_active(&altered, &status, &driver.0).is_err());
    altered = record.clone();
    altered.restoring = true;
    assert!(proof.validate_active(&altered, &status, &driver.0).is_err());
    altered.id = uuid::Uuid::new_v4().to_string();
    assert!(proof.validate_active(&altered, &status, &driver.0).is_err());
    let mut wrong = status.clone();
    wrong.active = false;
    assert!(proof.validate_active(&record, &wrong, &driver.0).is_err());
    wrong = status.clone();
    wrong.job_id = uuid::Uuid::new_v4().to_string();
    assert!(proof.validate_active(&record, &wrong, &driver.0).is_err());
    wrong = status.clone();
    wrong.session_id = None;
    assert!(proof.validate_active(&record, &wrong, &driver.0).is_err());
    driver
        .0
        .applications
        .push(r"\Device\Volume\External.exe".into());
    let external = driver.0.clone();
    // Metadata admission deliberately cannot claim a new driver observation.
    proof.validate_metadata_active(&record, &status).unwrap();
    assert!(proof.validate_active(&record, &status, &driver.0).is_err());
    assert!(journal::restore(&mut driver, &mut store, &record.id).is_err());
    assert_eq!(driver.0, external);
}
#[test]
fn restored_proof_rejects_other_journal_or_worker_and_original_state_edits() {
    let root = crate::test_directory();
    let mut driver = Driver(original());
    let mut store = Store::open(root.path().join("owner.json")).unwrap();
    let record = journal::apply(&mut driver, &mut store, plan(original()), || Ok(())).unwrap();
    let mut status = status(&record);
    let proof = Proof::capture(
        &uuid::Uuid::new_v4().to_string(),
        &record,
        &status,
        &driver.0,
    )
    .unwrap();
    assert!(proof
        .validate_restored(&record, &status, &driver.0)
        .is_err());
    let clean = journal::restore(&mut driver, &mut store, &record.id).unwrap();
    status.active = false;
    status.phase = "restored".into();
    proof.validate_restored(&clean, &status, &driver.0).unwrap();
    let mut changed = clean.clone();
    changed.id = uuid::Uuid::new_v4().to_string();
    assert!(proof
        .validate_restored(&changed, &status, &driver.0)
        .is_err());
    let mut wrong = status.clone();
    wrong.job_id = uuid::Uuid::new_v4().to_string();
    assert!(proof.validate_restored(&clean, &wrong, &driver.0).is_err());
    driver.0.active = true;
    assert!(proof.validate_restored(&clean, &status, &driver.0).is_err());
}
#[test]
fn proof_wire_is_strict_bounded_and_cannot_relabel_an_original_plan() {
    let root = crate::test_directory();
    let mut driver = Driver(original());
    let mut store = Store::open(root.path().join("owner.json")).unwrap();
    let record = journal::apply(&mut driver, &mut store, plan(original()), || Ok(())).unwrap();
    let status = status(&record);
    let proof = Proof::capture(
        &uuid::Uuid::new_v4().to_string(),
        &record,
        &status,
        &driver.0,
    )
    .unwrap();
    let value = serde_json::to_value(&proof).unwrap();
    assert_eq!(
        serde_json::from_value::<Proof>(value.clone()).unwrap(),
        proof
    );
    let mut unknown = value;
    unknown["browserApproved"] = true.into();
    assert!(serde_json::from_value::<Proof>(unknown).is_err());
    for field in ["owner", "jobId", "sessionId"] {
        let mut bad = serde_json::to_value(&proof).unwrap();
        bad[field] = "../owner".into();
        assert!(serde_json::from_value::<Proof>(bad)
            .unwrap()
            .validate()
            .is_err());
    }
    let mut bad = proof.clone();
    bad.schema_version = 2;
    assert!(bad.validate().is_err());
    bad = proof.clone();
    bad.record_hash = "sha256:not-a-hash".into();
    assert!(bad.validate().is_err());
    bad = proof.clone();
    bad.plan.before.applications.push("x".repeat(131_072));
    assert!(bad.validate().is_err());
    bad = proof.clone();
    bad.plan = plan(record.plan.after.clone());
    assert!(bad.validate_active(&record, &status, &driver.0).is_err());
}
