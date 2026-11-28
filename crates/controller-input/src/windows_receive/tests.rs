use super::*;
use windows_sys::Win32::Foundation::*;

fn selected() -> Selection {
    Selection {
        path: "\\\\?\\hid#vid_1209&pid_0001#selected-interface".into(),
        vendor_id: 0x1209,
        product_id: 1,
        version_number: 1,
        usage_page: 0xff00,
        usage: 1,
        input_report_bytes: 64,
        feature_report_bytes: 16,
    }
}
#[test]
fn selected_vendor_collection_is_readable_but_system_classes_are_metadata_only() {
    assert!(policy::selection(&selected()).is_ok());
    for (page, usage) in [
        (1, 1),
        (1, 2),
        (1, 6),
        (1, 7),
        (1, 0x80),
        (7, 1),
        (0x0d, 1),
        (0x0d, 2),
        (0x0d, 4),
        (0x0d, 5),
    ] {
        let mut selection = selected();
        selection.usage_page = page;
        selection.usage = usage;
        assert_eq!(
            policy::selection(&selection).unwrap_err().kind,
            ErrorKind::MetadataOnly
        );
    }
    let mut gamepad = selected();
    gamepad.usage_page = 1;
    gamepad.usage = 5;
    assert!(policy::selection(&gamepad).is_ok());
    for (page, usage) in [(1, 0x82), (1, 0x0c), (0x0c, 1), (0x20, 0x40), (0x84, 4)] {
        assert!(!metadata_only(page, usage));
    }
}
#[test]
fn invalid_paths_and_oversized_reports_never_reach_native_open() {
    for path in [String::new(), "bad\0path".into(), "x".repeat(8193)] {
        let mut selection = selected();
        selection.path = path;
        assert_eq!(
            policy::selection(&selection).unwrap_err().kind,
            ErrorKind::InvalidRequest
        );
    }
    let mut selection = selected();
    selection.input_report_bytes = MAX_REPORT_BYTES + 1;
    assert_eq!(
        policy::selection(&selection).unwrap_err().kind,
        ErrorKind::Capacity
    );
}
#[test]
fn explicit_get_requires_live_kind_id_evidence_request_identity_and_caller_bound() {
    assert!(policy::get("request-a", 3, 64, MAX_DEADLINE, 64, &[3]).is_ok());
    assert!(policy::get("request-a", 0, 16, MAX_DEADLINE, 16, &[0]).is_ok());
    for ids in [vec![], vec![1, 2]] {
        assert_eq!(
            policy::get("request", 3, 64, MAX_DEADLINE, 64, &ids)
                .unwrap_err()
                .kind,
            ErrorKind::Unsupported
        );
    }
    assert_eq!(
        policy::get("request", 3, 63, MAX_DEADLINE, 64, &[3])
            .unwrap_err()
            .kind,
        ErrorKind::InvalidRequest
    );
    assert_eq!(
        policy::get("request", 3, MAX_REPORT_BYTES + 1, MAX_DEADLINE, 64, &[3])
            .unwrap_err()
            .kind,
        ErrorKind::InvalidRequest
    );
    assert_eq!(
        policy::get("", 3, 64, MAX_DEADLINE, 64, &[3])
            .unwrap_err()
            .kind,
        ErrorKind::InvalidRequest
    );
    assert!(policy::get("bad\nrequest", 3, 64, MAX_DEADLINE, 64, &[3]).is_err());
    assert!(policy::get(&"x".repeat(81), 3, 64, MAX_DEADLINE, 64, &[3]).is_err());
    assert_eq!(
        policy::get("request", 3, 64, Duration::from_millis(2001), 64, &[3])
            .unwrap_err()
            .kind,
        ErrorKind::InvalidRequest
    );
}
#[test]
fn get_preserves_exact_short_per_id_bytes_with_type_level_maximum() {
    // Windows caps lengths are type-level maxima; different IDs may be shorter.
    assert!(policy::completed_get(3, 64, &[3, 0x81, 0x7f]).is_ok());
    assert!(policy::completed_get(0, 64, &[0, 0x81, 0x7f]).is_ok());
    assert!(policy::completed_get(3, 64, &[]).is_err());
    assert!(policy::completed_get(3, 64, &[3]).is_err());
    assert!(policy::completed_get(3, 64, &[4, 1]).is_err());
    assert!(policy::completed_get(3, 2, &[3, 1, 2]).is_err());
}
#[test]
fn native_failures_distinguish_removal_authority_and_unsupported_get() {
    for code in [
        ERROR_DEVICE_NOT_CONNECTED,
        ERROR_NO_SUCH_DEVICE,
        ERROR_INVALID_HANDLE,
    ] {
        assert_eq!(policy::os(code, "read").kind, ErrorKind::Removed);
    }
    assert_eq!(
        policy::os(ERROR_ACCESS_DENIED, "open").kind,
        ErrorKind::AccessDenied
    );
    assert_eq!(
        policy::os(ERROR_NOT_SUPPORTED, "GET").kind,
        ErrorKind::Unsupported
    );
    assert_eq!(policy::os(ERROR_CRC, "read").kind, ErrorKind::Os);
    let error = policy::os(ERROR_CRC, "read");
    assert_eq!(error.code, Some(ERROR_CRC));
    assert!(serde_json::to_string(&Receive::Error(error))
        .unwrap()
        .contains("os"));
}
