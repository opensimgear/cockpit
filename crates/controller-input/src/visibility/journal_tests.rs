use super::super::policy::{
    tests::{capture, selected, state},
    Policy,
};
use super::*;
struct Driver {
    state: State,
    calls: usize,
    fail: Option<usize>,
    after_write: bool,
}
impl Backend for Driver {
    fn read(&mut self) -> Result<State, String> {
        Ok(self.state.clone())
    }
    fn change(&mut self, before: &State, after: &State) -> Result<(), String> {
        assert_eq!(&self.state, before);
        self.calls += 1;
        if self.fail == Some(self.calls) && !self.after_write {
            return Err("injected write failure".into());
        }
        self.state = after.clone();
        if self.fail == Some(self.calls) {
            return Err("lost acknowledgement".into());
        }
        Ok(())
    }
}
fn driver() -> Driver {
    Driver {
        state: state(),
        calls: 0,
        fail: None,
        after_write: false,
    }
}
fn plan() -> Plan {
    Plan::build(state(), selected(Policy::HideAll), capture()).unwrap()
}
#[test]
fn durable_session_restores_owned_properties_and_prevents_competing_owners() {
    let temp = crate::test_directory();
    let path = temp.path().join("visibility.json");
    assert!(inspect(&path).unwrap().is_none());
    assert!(!path.exists());
    let mut store = Store::open(path.clone()).unwrap();
    assert!(Store::open(path.clone()).is_err());
    let mut driver = driver();
    let record = apply(&mut driver, &mut store, plan(), || Ok(())).unwrap();
    assert_eq!(driver.state, record.plan.after);
    assert!(!inspect(&path).unwrap().unwrap().clean);
    assert!(store.begin(plan()).is_err());
    assert!(restore(&mut driver, &mut store, "stale").is_err());
    let clean = restore(&mut driver, &mut store, &record.id).unwrap();
    assert!(clean.clean);
    assert_eq!(driver.state, state());
    let calls = driver.calls;
    restore(&mut driver, &mut store, &record.id).unwrap();
    assert_eq!(driver.calls, calls);
    drop(store);
    assert!(Store::open(path).is_ok());
}
#[test]
fn every_failed_or_unacknowledged_step_and_capture_loss_restores() {
    for after_write in [false, true] {
        for fail in 1..=3 {
            let temp = crate::test_directory();
            let mut store = Store::open(temp.path().join("v.json")).unwrap();
            let mut driver = driver();
            driver.fail = Some(fail);
            driver.after_write = after_write;
            assert!(apply(&mut driver, &mut store, plan(), || Ok(())).is_err());
            assert_eq!(driver.state, state());
            assert!(store.read().unwrap().unwrap().clean);
        }
    }
    for checkpoint in 1..=5 {
        let temp = crate::test_directory();
        let mut store = Store::open(temp.path().join("v.json")).unwrap();
        let mut driver = driver();
        let mut checks = 0;
        assert!(apply(&mut driver, &mut store, plan(), || {
            checks += 1;
            if checks == checkpoint {
                Err("source generation changed".into())
            } else {
                Ok(())
            }
        })
        .is_err());
        assert_eq!(driver.state, state());
    }
}
#[test]
fn interrupted_pending_write_has_explicit_restart_recovery() {
    for happened in [false, true] {
        let temp = crate::test_directory();
        let path = temp.path().join("v.json");
        let mut store = Store::open(path.clone()).unwrap();
        let mut record = store.begin(plan()).unwrap();
        record.pending = Some(1);
        store.write(&record).unwrap();
        let mut driver = driver();
        if happened {
            driver.state = record.plan.steps().unwrap()[1].clone();
        }
        drop(store);
        let mut restarted = Store::open(path).unwrap();
        assert!(!restarted.read().unwrap().unwrap().clean);
        assert!(
            restore(&mut driver, &mut restarted, &record.id)
                .unwrap()
                .clean
        );
        assert_eq!(driver.state, state());
    }
}
#[test]
fn external_policy_or_journal_edits_are_never_overwritten() {
    let temp = crate::test_directory();
    let path = temp.path().join("v.json");
    let mut store = Store::open(path.clone()).unwrap();
    let mut driver = driver();
    let record = apply(&mut driver, &mut store, plan(), || Ok(())).unwrap();
    driver.state.hidden.push(r"HID\EXTERNAL\2".into());
    let changed = driver.state.clone();
    let calls = driver.calls;
    assert!(restore(&mut driver, &mut store, &record.id).is_err());
    assert_eq!(driver.state, changed);
    assert_eq!(driver.calls, calls);
    sql_write(&path, b"external edit").unwrap();
    assert!(store.read().is_err());
    assert!(store.write(&record).is_err());
    assert_eq!(sql_read(&path).unwrap(), b"external edit");
}
#[test]
fn malformed_records_pending_conflicts_and_invalid_files_refuse_recovery() {
    let temp = crate::test_directory();
    let path = temp.path().join("v.json");
    let mut store = Store::open(path.clone()).unwrap();
    let record = store.begin(plan()).unwrap();
    let mut bad = record.clone();
    bad.id = "invalid".into();
    assert!(bad.validate().is_err());
    let mut bad = record.clone();
    bad.position = 99;
    assert!(bad.validate().is_err());
    let mut bad = record.clone();
    bad.pending = Some(2);
    assert!(bad.validate().is_err());
    let mut bad = record.clone();
    bad.clean = true;
    bad.pending = Some(1);
    assert!(bad.validate().is_err());
    let mut pending = record;
    pending.pending = Some(1);
    store.write(&pending).unwrap();
    let mut driver = driver();
    driver.state.inverse = true;
    assert!(restore(&mut driver, &mut store, &pending.id).is_err());
    assert_eq!(driver.calls, 0);
    drop(store);
    sql_write(&path, b"{}").unwrap();
    assert!(Store::open(path.clone()).is_err());
    sql_write(&path, vec![0; MAX_RECORD as usize + 1]).unwrap();
    assert!(inspect(&path).is_err());
    assert!(inspect(temp.path()).is_err());
}
#[cfg(unix)]
#[test]
fn symlink_journal_and_lock_are_rejected() {
    let temp = crate::test_directory();
    let real = temp.path().join("real");
    std::fs::write(&real, b"{}").unwrap();
    let path = temp.path().join("v.json");
    std::os::unix::fs::symlink(&real, &path).unwrap();
    assert!(inspect(&path).is_err());
    let other = temp.path().join("other.json");
    std::os::unix::fs::symlink(&real, other.with_extension("lock")).unwrap();
    assert!(Store::open(other).is_err());
}

#[test]
fn process_exit_keeps_pending_intent_and_releases_file_ownership() {
    const VARIABLE: &str = "OSG_TEST_VISIBILITY_CRASH_JOURNAL";
    if let Some(path) = std::env::var_os(VARIABLE) {
        let mut store = Store::open(path.into()).unwrap();
        let mut record = store.begin(plan()).unwrap();
        record.pending = Some(1);
        store.write(&record).unwrap();
        std::process::exit(0); // Bypass Store/driver/session destructors deliberately.
    }
    let temp = crate::test_directory();
    let path = temp.path().join("v.json");
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "visibility::journal::tests::process_exit_keeps_pending_intent_and_releases_file_ownership"])
        .env(VARIABLE, &path).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("child deadline");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let mut store = Store::open(path).unwrap();
    let record = store.read().unwrap().unwrap();
    assert_eq!(record.pending, Some(1));
    let mut driver = driver();
    driver.state = record.plan.steps().unwrap()[1].clone();
    assert!(restore(&mut driver, &mut store, &record.id).unwrap().clean);
    assert_eq!(driver.state, state());
}

fn sql_read(path:&std::path::Path)->Result<Vec<u8>,String>{osg_persistence::test_support::read_evidence(path,"recovery-visibility")}
fn sql_write(path:&std::path::Path,data:impl AsRef<[u8]>)->Result<(),String>{osg_persistence::test_support::write_evidence(path,"recovery-visibility",Some(data.as_ref()))}
