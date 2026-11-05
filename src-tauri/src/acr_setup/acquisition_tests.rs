use super::*;

#[test]
fn native_monitor_denial_remains_an_access_problem_without_claiming_a_layout_failure() {
    let fixture = ProbeHarness::new();
    let platform = ProbePlatform::running(fixture.process.clone());
    for message in [
        "Cannot subscribe to native game process notifications (ROOT\\CIMV2): WMI event query: 0x80041003. Windows denied process-event access.",
        "Native permission denied.",
    ] {
        let proof = probe_acquired(&fixture.game, &platform, &fixture.fingerprint,
            || false, |_, _| {}, |_| Err(message.into()), |_| Ok(vec![]), || 0,
            || panic!("Permission denial cannot begin telemetry sampling"),
        ).unwrap();
        assert_eq!(proof["status"], "access-denied");
        assert_eq!(proof["nativeVerified"], false);
        assert_eq!(proof["diagnostics"][0], message);
    }
}

#[test]
fn cancellation_during_monitor_startup_is_not_published_as_a_layout_failure() {
    let fixture = ProbeHarness::new();
    let platform = ProbePlatform {
        selected: Some(fixture.process.clone()),
        lifetime: Default::default(),
    };
    let cancelled = Cell::new(false);
    let result = probe_acquired(
        &fixture.game, &platform, &fixture.fingerprint,
        || cancelled.get(), |_, _| {},
        |_| {
            cancelled.set(true);
            Err("Native process monitoring wait was cancelled.".into())
        },
        |_| Ok(vec![]), || 0,
        || panic!("A cancelled setup must not wait for telemetry"),
    );
    assert!(result.unwrap_err().contains("cancelled"));
}
