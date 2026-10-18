use super::*;
use std::cell::Cell;
const IMAGE: &str = r"\Device\Volume1\Cockpit.exe";
fn state(inverse: bool) -> State {
    State {
        active: true, inverse,
        applications: vec![r"\Device\Volume2\External.exe".into(), r"\Device\Volume1\Cockpit.exe.backup.exe".into()],
        hidden: vec![r"HID\EXTERNAL\1".into(), r"USB\EXTERNAL\2".into()],
    }
}
struct Fixture {
    state: State,
    reads: usize,
    writes: usize,
    fail_read: usize,
    fail_write: bool,
    mismatch: bool,
}
impl Fixture {
    fn new(state: State) -> Self { Self { state, reads: 0, writes: 0, fail_read: 0, fail_write: false, mismatch: false } }
}
impl Backend for Fixture {
    fn read(&mut self) -> Result<State, String> {
        self.reads += 1;
        if self.reads == self.fail_read { return Err("Injected read failure".into()); }
        Ok(self.state.clone())
    }
    fn change(&mut self, expected: &State, after: &State) -> Result<(), String> {
        assert_eq!(&self.state, expected);
        assert_eq!(&desired(expected, IMAGE).unwrap(), after);
        self.writes += 1;
        if self.fail_write { return Err("Injected write/readback failure".into()); }
        self.state = after.clone();
        if self.mismatch { self.state.active = !self.state.active; }
        Ok(())
    }
}
fn encode(applications: &[String]) -> Vec<u8> {
    applications.iter().flat_map(|value| value.encode_utf16().chain(Some(0)))
        .chain(Some(0)).flat_map(u16::to_le_bytes).collect()
}
#[test]
fn normal_and_inverse_self_access_preserve_external_policy_and_are_idempotent() {
    for inverse in [false, true] {
        for active in [false, true] {
            for listed in [false, true] {
                let mut before = state(inverse);
                before.active = active;
                if listed { before.applications.push(IMAGE.to_uppercase()); }
                let mut driver = Fixture::new(before.clone());
                let guards = Cell::new(0);
                let result = reconcile(&mut driver, IMAGE.into(), || { guards.set(guards.get() + 1); Ok(()) });
                let changed = listed == inverse;
                assert!(result.ready);
                assert_eq!(result.changed, Some(changed));
                assert_eq!(result.status, if changed { "configured" } else { "unchanged" });
                assert_eq!(guards.get(), usize::from(changed));
                assert_eq!(driver.state.active, before.active);
                assert_eq!(driver.state.inverse, before.inverse);
                assert_eq!(driver.state.hidden, before.hidden);
                assert_eq!(&driver.state.applications[..2], &before.applications[..2]);
                assert_eq!(policy::contains(&driver.state.applications, IMAGE), !inverse);
                assert!(driver.state.capture_allowed(IMAGE));
                let stable = driver.state.clone();
                let again = reconcile(&mut driver, IMAGE.into(), || panic!("No-op must not touch recovery"));
                assert_eq!(again.status, "unchanged");
                assert_eq!(again.changed, Some(false));
                assert_eq!(driver.state, stable);
                assert_eq!(driver.writes, usize::from(changed));
            }
        }
    }
}
#[test]
fn pending_recovery_and_invalid_policy_block_before_any_write() {
    let before = state(false);
    let mut driver = Fixture::new(before.clone());
    let blocked = reconcile(&mut driver, IMAGE.into(), || Err("Pending owner".into()));
    assert_eq!(blocked.status, "blocked");
    assert_eq!(blocked.message, "Pending owner");
    assert_eq!(blocked.changed, Some(false));
    assert!(!blocked.ready);
    assert_eq!(driver.state, before);
    assert_eq!(driver.writes, 0);
    for image in ["relative.exe", r"C:\Cockpit.exe"] {
        assert_eq!(reconcile(&mut driver, image.into(), || panic!()).status, "error");
    }
    driver.state.applications.push(driver.state.applications[0].clone());
    assert_eq!(reconcile(&mut driver, IMAGE.into(), || panic!()).status, "error");
    driver.state.applications = (0..256).map(|id|format!(r"\Device\Volume1\External{id}.exe")).collect();
    assert_eq!(reconcile(&mut driver, IMAGE.into(), || panic!()).status, "error");
    assert_eq!(driver.writes, 0);
}
#[test]
fn failed_reads_writes_and_readback_never_claim_ready_or_known_unchanged() {
    for phase in 0..4 {
        let mut driver = Fixture::new(state(false));
        driver.fail_read = if phase == 0 { 1 } else if phase == 2 { 2 } else { 0 };
        driver.fail_write = phase == 1;
        driver.mismatch = phase == 3;
        let result = reconcile(&mut driver, IMAGE.into(), || Ok(()));
        assert_eq!(result.status, "error");
        assert!(!result.ready);
        assert_eq!(result.changed, if phase == 0 { Some(false) } else { None });
        assert!(result.message.contains(if phase == 3 { "readback" } else { "failure" }));
    }
}
#[test]
fn native_write_boundary_accepts_only_the_exact_own_application_delta() {
    for inverse in [false, true] {
        let mut before = state(inverse);
        if inverse { before.applications.push(IMAGE.into()); }
        let after = desired(&before, IMAGE).unwrap();
        let data = encode(&after.applications);
        validate_write(&before, IMAGE, 2049, Some(&data)).unwrap();
        for function in [2048, 2051, 2053, 2055, 0] {
            assert!(validate_write(&before, IMAGE, function, Some(&data)).is_err());
        }
        assert!(validate_write(&before, IMAGE, 2049, None).is_err());
        assert!(validate_write(&before, IMAGE, 2049, Some(&[1])).is_err());
        assert!(validate_write(&before, IMAGE, 2049, Some(&encode(&[IMAGE.into()]))).is_err());
        assert!(validate_write(&after, IMAGE, 2049, Some(&data)).is_err());
    }
    assert_eq!(serde_json::to_value(Status::default()).unwrap()["status"], "not-requested");
}
