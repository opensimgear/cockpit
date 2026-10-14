use super::*;
fn slot() -> Slot {
    Slot {
        id: 1,
        status: Status::Free,
        owner_pid: None,
        buttons: 8,
        axes: vec![],
        continuous_hats: 0,
        discrete_hats: 0,
        force_feedback: false,
    }
}
#[test]
fn identity_filters_and_bounded_native_strings_cannot_broaden_refresh() {
    let id = "ROOT\\HIDCLASS\\0000";
    assert!(device_identity(id));
    for invalid in [
        "USB\\HIDCLASS\\0000",
        "ROOT\\HIDCLASS\\0000\\EXTRA",
        "ROOT\\HIDCLASS\\xyz1",
        "ROOT\\OTHER\\0000",
    ] {
        assert!(!device_identity(invalid));
    }
    let encoded: Vec<_> = id.encode_utf16().chain([0, 0]).collect();
    assert_eq!(multi_string(&encoded).unwrap(), [id]);
    assert!(multi_string(&[0, 0]).unwrap().is_empty());
    for invalid in [
        vec![0],
        vec![1, 0, 2],
        vec![0, 1, 0, 0],
        vec![0; 4097],
        vec![0xd800, 0, 0],
        vec![65; 4096],
    ] {
        assert!(multi_string(&invalid).is_err());
    }
    let oversized: Vec<_> = "A".repeat(257).encode_utf16().chain([0, 0]).collect();
    assert!(multi_string(&oversized).is_err());
    let many: Vec<_> = "A\0".repeat(17).encode_utf16().chain([0]).collect();
    assert!(multi_string(&many).is_err());
    assert_eq!(
        service_text(REG_SZ, &[118, 74, 111, 121, 0], 10).unwrap(),
        "vJoy"
    );
    for (kind, bytes, data) in [
        (3, 2, vec![0]),
        (1, 1, vec![0]),
        (1, 4, vec![0]),
        (1, 2, vec![65]),
        (1, 4, vec![0, 0]),
        (1, 4, vec![0xd800, 0]),
    ] {
        assert!(service_text(kind, &data, bytes).is_err());
    }
    assert!(select(vec![], |_| unreachable!()).is_err());
    assert!(select(vec![id.into(), id.into()], |_| unreachable!()).is_err());
    assert!(select(vec!["USB\\OTHER".into()], |_| unreachable!()).is_err());
    assert!(select(vec![id.into()], |_| Ok(("Other".into(), true))).is_err());
    assert!(select(vec![id.into()], |_| Ok(("vjoy".into(), false))).is_err());
    assert!(select(vec![id.into()], |_| Err("lost device".into())).is_err());
    assert_eq!(
        select(vec![id.into()], |_| Ok(("vjoy".into(), true))).unwrap(),
        id
    );
}
#[test]
fn read_only_native_lookup_and_busy_preflight_never_launch_a_refresh() {
    assert!(checked(CR_SUCCESS).is_ok());
    assert!(checked(CR_BUFFER_SMALL).is_err());
    let ids = native_ids().unwrap();
    if !ids.is_empty() {
        // Inspect only: no SDK acquisition and no PnP command.
        let _ = device();
    }
    assert!(native_service("ROOT\\OSG_TEST_NONEXISTENT\\FFFF").is_err());
    assert!(system_program().unwrap().is_file());
    let mut busy = slot();
    busy.status = Status::Busy;
    for slots in [vec![], vec![busy], vec![slot(); 17]] {
        assert!(refresh_with(
            slots,
            || panic!("must not resolve"),
            |_| panic!("must not run")
        )
        .is_err());
    }
    assert!(refresh_with(vec![slot()], || Err("lookup failed".into()), |_| panic!()).is_err());
    assert!(refresh_with(vec![slot()], || Ok("USB\\OTHER".into()), |_| panic!()).is_err());
    let id = "ROOT\\HIDCLASS\\0000";
    assert!(refresh_with(
        vec![slot()],
        || Ok(id.into()),
        |actual| {
            assert_eq!(actual, id);
            Ok(())
        }
    )
    .is_ok());
    assert!(refresh_with(
        vec![slot()],
        || Ok(id.into()),
        |_| Err("restart failed".into())
    )
    .is_err());
}
#[test]
fn real_child_completion_failure_and_deadline_are_bounded_without_pnp_commands() {
    let python = std::env::var("OSG_TEST_PYTHON").unwrap_or_else(|_| "python".into());
    assert!(bounded(
        Path::new(&python),
        &["-c".into(), "pass".into()],
        Duration::from_secs(2)
    )
    .is_ok());
    assert!(bounded(
        Path::new(&python),
        &["-c".into(), "raise SystemExit(7)".into()],
        Duration::from_secs(2)
    )
    .is_err());
    let start = Instant::now();
    assert!(bounded(
        Path::new(&python),
        &["-c".into(), "import time;time.sleep(30)".into()],
        Duration::from_millis(50)
    )
    .unwrap_err()
    .contains("deadline"));
    assert!(start.elapsed() < Duration::from_secs(3));
    let absent = tempfile::tempdir().unwrap();
    assert!(bounded(
        &absent.path().join("missing.exe"),
        &[],
        Duration::from_secs(1)
    )
    .is_err());
}
