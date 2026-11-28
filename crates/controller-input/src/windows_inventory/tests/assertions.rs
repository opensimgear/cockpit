use super::*;

#[test]
fn every_present_interface_is_retained_even_restricted_and_same_vid_pid() {
    let inventory = run("normal", 3);
    assert_eq!(inventory.interfaces.len(), 3);
    assert!(!inventory.truncated && inventory.failures.is_empty());
    assert_eq!(
        inventory.interfaces[0].path.as_deref(),
        Some("\\\\?\\hid#keyboard")
    );
    assert_eq!(
        inventory.interfaces[1].path.as_deref(),
        Some("\\\\?\\hid#vendor")
    );
    for row in &inventory.interfaces[..2] {
        assert!(row.metadata_opened);
        assert_eq!(
            row.attributes.as_ref().unwrap(),
            &Attributes {
                vendor_id: 0x1111,
                product_id: 0x2222,
                version_number: 0x3333
            }
        );
        assert_eq!(row.product.as_deref(), Some("A"));
        assert!(
            row.caps.is_none()
                && row
                    .failures
                    .iter()
                    .any(|failure| failure.stage == "preparsed-data")
        );
    }
    let restricted = &inventory.interfaces[2];
    assert!(
        !restricted.metadata_opened && restricted.attributes.is_none() && restricted.caps.is_none()
    );
    assert_eq!(restricted.failures[0].stage, "metadata-open");
    assert_eq!(
        restricted.failures[0].native_code,
        Some(ERROR_ACCESS_DENIED)
    );
    STATE.with(|state| {
        let state = state.borrow();
        assert_eq!((state.opened, state.closed, state.destroyed), (2, 2, 1));
        assert!(!state.bad_access);
    });
    let wire = serde_json::to_value(inventory).unwrap();
    assert!(wire["interfaces"][0].get("metadataOpened").is_some());
}

#[test]
fn detail_failures_have_no_invented_identity_and_do_not_hide_other_rows() {
    for mode in [
        "detail-query-failed",
        "detail-size-small",
        "detail-size-big",
        "detail-size-odd",
        "detail-failed",
        "detail-raced",
        "detail-unterminated",
        "detail-empty",
        "detail-unicode",
    ] {
        let inventory = run(mode, 2);
        assert_eq!(inventory.interfaces.len(), 2, "{mode}");
        for row in inventory.interfaces {
            assert!(
                row.path.is_none() && !row.metadata_opened && !row.failures.is_empty(),
                "{mode}"
            );
        }
        STATE.with(|state| assert_eq!((state.borrow().opened, state.borrow().destroyed), (0, 1)));
    }
}

#[test]
fn independent_metadata_failures_keep_identity_and_successful_metadata() {
    for mode in [
        "attrs-failed",
        "string-failed",
        "string-unterminated",
        "string-unicode",
        "null-preparsed",
    ] {
        let inventory = run(mode, 1);
        let row = &inventory.interfaces[0];
        assert!(row.path.is_some() && row.metadata_opened && !row.failures.is_empty());
        if mode == "attrs-failed" {
            assert!(row.attributes.is_none());
            assert_eq!(row.product.as_deref(), Some("A"));
        }
        if mode.starts_with("string") {
            assert!(row.attributes.is_some() && row.product.is_none());
        }
        if mode == "null-preparsed" {
            assert!(row
                .failures
                .iter()
                .any(|failure| failure.stage == "preparsed-data" && failure.native_code.is_none()));
        }
        STATE.with(|state| assert_eq!(state.borrow().closed, 1));
    }
    let empty = run("string-empty", 1);
    assert!(empty.interfaces[0].product.is_none());
}

#[test]
fn global_failures_bounds_duplicates_and_cleanup_are_explicit() {
    reset("set-failed", 1);
    let failure = unsafe { enumerate_with(&api()).err().unwrap() };
    assert_eq!(failure.stage, "interface-set");
    STATE.with(|state| assert_eq!(state.borrow().destroyed, 0));
    let empty = run("normal", 0);
    assert!(empty.interfaces.is_empty() && !empty.truncated);
    let interrupted = run("enum-failed", 3);
    assert_eq!(interrupted.interfaces.len(), 1);
    assert!(interrupted.truncated);
    assert_eq!(interrupted.failures[0].stage, "interface-enumeration");
    let bound = run("normal", MAX_INTERFACES + 1);
    assert_eq!(bound.interfaces.len(), MAX_INTERFACES as usize);
    assert!(bound.truncated);
    assert!(bound
        .failures
        .iter()
        .any(|failure| failure.stage == "interface-capacity"));
    let exact = run("normal", MAX_INTERFACES);
    assert!(!exact.truncated);
    let duplicate = run("duplicate", 2);
    assert!(duplicate.truncated);
    assert_eq!(duplicate.interfaces.len(), 2);
    assert!(duplicate.interfaces[1]
        .failures
        .iter()
        .any(|failure| failure.stage == "interface-identity"));
    let cleanup = run("cleanup-failed", 1);
    assert!(cleanup
        .failures
        .iter()
        .any(|failure| failure.stage == "interface-set-release"));
    assert!(cleanup.interfaces[0]
        .failures
        .iter()
        .any(|failure| failure.stage == "metadata-close"));
}

#[test]
fn unwind_releases_preparsed_metadata_handle_and_device_set() {
    reset("projection-panic", 1);
    assert!(std::panic::catch_unwind(|| unsafe { enumerate_with(&api()) }).is_err());
    STATE.with(|state| {
        let state = state.borrow();
        assert_eq!(
            (state.snapshot, state.freed, state.closed, state.destroyed),
            (1, 1, 1, 1)
        );
    });
}

#[test]
fn invalid_selected_paths_are_rejected_before_any_native_open() {
    for path in ["".to_owned(), "a\0b".into(), "a".repeat(4094)] {
        assert!(inspect(&path).is_err());
    }
}

#[test]
fn parsed_projection_is_retained_and_aggregate_capacity_is_explicit() {
    let inventory = run("preparsed-success", 1);
    assert!(inventory.interfaces[0].caps.is_some());
    STATE.with(|state| {
        let state = state.borrow();
        assert_eq!((state.snapshot, state.freed, state.closed), (1, 1, 1));
    });
    reset("preparsed-success", 1);
    let mut row = Interface::new(0);
    row.path = Some(path_for(0, false));
    unsafe {
        metadata(
            &api(),
            &mut row,
            &mut CapsBudget {
                records: MAX_CAP_RECORDS,
                bytes: 1,
            },
        );
    }
    assert!(
        row.caps.is_none()
            && row
                .failures
                .iter()
                .any(|failure| failure.stage == "caps-capacity")
    );
    STATE.with(|state| assert_eq!((state.borrow().freed, state.borrow().closed), (1, 1)));
    reset("preparsed-success", 1);
    let mut row = Interface::new(0);
    row.path = Some(path_for(0, false));
    unsafe {
        metadata(
            &api(),
            &mut row,
            &mut CapsBudget {
                records: 0,
                bytes: MAX_CAP_BYTES,
            },
        );
    }
    assert!(
        row.caps.is_none()
            && row
                .failures
                .iter()
                .any(|failure| failure.stage == "caps-capacity")
    );
    STATE.with(|state| assert_eq!((state.borrow().snapshot, state.borrow().closed), (0, 1)));
    reset("preparsed-records", 1);
    let mut row = Interface::new(0);
    row.path = Some(path_for(0, false));
    unsafe {
        metadata(
            &api(),
            &mut row,
            &mut CapsBudget {
                records: 2,
                bytes: MAX_CAP_BYTES,
            },
        );
    }
    assert!(
        row.caps.is_none()
            && row
                .failures
                .iter()
                .any(|failure| failure.stage == "caps-capacity")
    );
    STATE.with(|state| assert_eq!((state.borrow().freed, state.borrow().closed), (1, 1)));
}
