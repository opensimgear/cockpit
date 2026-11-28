use super::tests::{finish, reset, session, Finish, MOCK, SERIAL};
use super::*;

fn selected() -> Selection {
    Selection {
        path: "fake-verified-interface".into(),
        vendor_id: 0x1209,
        product_id: 1,
        version_number: 1,
        usage_page: 0xff00,
        usage: 1,
        input_report_bytes: 64,
        feature_report_bytes: 16,
    }
}
fn snapshot() -> Snapshot {
    let mut snapshot = Snapshot::unavailable("fake kernel fixture");
    snapshot.available = true;
    snapshot.reports = [
        (ReportKind::Input, 64, vec![3]),
        (ReportKind::Feature, 16, vec![0, 3]),
        (ReportKind::Output, 8, vec![9]),
    ]
    .into_iter()
    .map(|(kind, byte_length, report_ids)| windows_caps::Report {
        kind,
        byte_length,
        report_ids,
        data_indices: 0,
        declared_button_caps: 0,
        declared_value_caps: 0,
        button_caps: vec![],
        value_caps: vec![],
        complete: true,
    })
    .collect();
    snapshot
}

#[test]
fn public_receiver_keeps_stream_evidence_and_enforces_get_authority() {
    let _serial = SERIAL.lock().unwrap();
    reset();
    let selection = selected();
    let mut receiver = Receiver::validated(&selection, session(), snapshot());
    assert_eq!(receiver.input_ids, vec![3]);
    assert!(matches!(
        receiver.poll(Duration::from_secs(3)),
        Receive::Error(_)
    ));
    assert_eq!(receiver.poll(Duration::ZERO), Receive::Timeout);
    for wire in [vec![3, 0x81, 0x7f], vec![0xfe], vec![0x77, 1]] {
        finish(Finish::Data(wire.clone(), wire.len()));
        assert_eq!(
            receiver.poll(Duration::ZERO),
            Receive::Report(wire.clone()),
            "Unknown IDs and short positive frames remain evidence"
        );
        let projected = receiver.samples(ReportKind::Input, &wire);
        assert_eq!(projected.raw, wire);
        assert!(!projected.failures.is_empty());
    }
    finish(Finish::Error(ERROR_CRC));
    assert!(matches!(receiver.poll(Duration::ZERO), Receive::Error(_)));
    finish(Finish::Error(ERROR_DEVICE_NOT_CONNECTED));
    assert_eq!(receiver.poll(Duration::ZERO), Receive::Removed);
    finish(Finish::Error(ERROR_OPERATION_ABORTED));
    drop(receiver);

    let mut receiver = Receiver::validated(&selection, session(), snapshot());
    assert_eq!(
        receiver
            .get("request", ReportKind::Output, 9, 64, Duration::ZERO)
            .unwrap_err()
            .kind,
        ErrorKind::InvalidRequest
    );
    assert_eq!(
        receiver
            .get("request", ReportKind::Input, 4, 64, Duration::ZERO)
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    finish(Finish::Data(vec![3, 1, 2], 3));
    assert_eq!(
        receiver
            .get("exact-request", ReportKind::Input, 3, 64, Duration::ZERO)
            .unwrap(),
        GetReport {
            request_id: "exact-request".into(),
            bytes: vec![3, 1, 2]
        }
    );
    finish(Finish::Data(vec![0, 8, 9], 2));
    assert_eq!(
        receiver
            .get("zero", ReportKind::Feature, 0, 16, Duration::ZERO)
            .unwrap()
            .bytes,
        vec![0, 8, 9]
    );
    finish(Finish::Data(vec![4, 1], 2));
    assert!(receiver
        .get("wrong-id", ReportKind::Feature, 3, 16, Duration::ZERO)
        .is_err());
    finish(Finish::Data(vec![], 0));
    assert!(receiver
        .get("empty", ReportKind::Feature, 3, 16, Duration::ZERO)
        .is_err());
    finish(Finish::Data(vec![3, 1], 17));
    assert!(receiver
        .get("overrun", ReportKind::Feature, 3, 16, Duration::ZERO)
        .is_err());
    drop(receiver);

    for incomplete in [false, true] {
        let mut facts = snapshot();
        if incomplete {
            facts.reports[0].complete = false;
        } else {
            facts.available = false;
        }
        let mut no_input = selection.clone();
        no_input.input_report_bytes = 0;
        let mut receiver = Receiver::validated(&no_input, session(), facts);
        assert!(receiver.input_ids.is_empty());
        assert!(matches!(
            receiver.poll(Duration::ZERO),
            Receive::Error(ReceiveError {
                kind: ErrorKind::Unsupported,
                ..
            })
        ));
        assert_eq!(
            receiver
                .get("denied", ReportKind::Input, 3, 64, Duration::ZERO)
                .unwrap_err()
                .kind,
            ErrorKind::Unsupported
        );
    }
    let mut restricted = selection.clone();
    restricted.usage_page = 1;
    restricted.usage = 6;
    assert!(matches!(
        open(&restricted),
        Err(ReceiveError {
            kind: ErrorKind::MetadataOnly,
            ..
        })
    ));
    let mut absent = selection;
    absent.path = format!("\\\\?\\hid#osg-nonexistent-{}", uuid::Uuid::new_v4());
    assert!(open(&absent).is_err());
}

static OPEN_STEP: AtomicUsize = AtomicUsize::new(0);
unsafe extern "system" fn fake_open(
    _: *const u16,
    access: u32,
    share: u32,
    _: *const windows_sys::Win32::Security::SECURITY_ATTRIBUTES,
    disposition: u32,
    flags: u32,
    _: HANDLE,
) -> HANDLE {
    assert_eq!(access, GENERIC_READ);
    assert_eq!(share, FILE_SHARE_READ | FILE_SHARE_WRITE);
    assert_eq!(disposition, OPEN_EXISTING);
    assert_eq!(flags, FILE_FLAG_OVERLAPPED);
    if OPEN_STEP.load(Ordering::Acquire) == 1 {
        SetLastError(ERROR_ACCESS_DENIED);
        return INVALID_HANDLE_VALUE;
    }
    CreateEventW(null(), 1, 0, null())
}
unsafe extern "system" fn fake_attributes(_: HANDLE, attributes: *mut HIDD_ATTRIBUTES) -> bool {
    assert_eq!((*attributes).Size, size_of::<HIDD_ATTRIBUTES>() as u32);
    if OPEN_STEP.load(Ordering::Acquire) == 2 {
        SetLastError(ERROR_ACCESS_DENIED);
        return false;
    }
    (*attributes).VendorID = 0x1209;
    (*attributes).ProductID = 1;
    (*attributes).VersionNumber = if OPEN_STEP.load(Ordering::Acquire) == 3 {
        2
    } else {
        1
    };
    true
}
unsafe extern "system" fn fake_preparsed(_: HANDLE, pp: *mut isize) -> bool {
    if OPEN_STEP.load(Ordering::Acquire) == 4 {
        SetLastError(ERROR_NOT_SUPPORTED);
        return false;
    }
    *pp = 0;
    true // No fabricated pointer is ever passed to HidP/HidD_FreePreparsedData.
}
unsafe extern "system" fn fake_caps(_: isize, caps: *mut HIDP_CAPS) -> i32 {
    if OPEN_STEP.load(Ordering::Acquire) == 5 {
        return HIDP_STATUS_INVALID_PREPARSED_DATA;
    }
    (*caps).UsagePage = 0xff00;
    (*caps).Usage = 1;
    (*caps).InputReportByteLength = 64;
    (*caps).FeatureReportByteLength = 16;
    if OPEN_STEP.load(Ordering::Acquire) == 6 {
        (*caps).Usage = 2;
    }
    if OPEN_STEP.load(Ordering::Acquire) == 7 {
        (*caps).FeatureReportByteLength = 32;
    }
    HIDP_STATUS_SUCCESS
}
unsafe fn fake_snapshot(_: isize) -> Snapshot {
    snapshot()
}
static OPEN: OpenApi = OpenApi {
    open: fake_open,
    attributes: fake_attributes,
    preparsed: fake_preparsed,
    caps: fake_caps,
    snapshot: fake_snapshot,
    io: &MOCK,
};

#[test]
fn opened_identity_and_live_caps_are_verified_and_retired_same_path_is_fenced() {
    let _serial = SERIAL.lock().unwrap();
    reset();
    let selected = selected();
    for step in 1..=7 {
        OPEN_STEP.store(step, Ordering::Release);
        assert!(Session::open_with(&selected, &OPEN).is_err());
    }
    OPEN_STEP.store(0, Ordering::Release);
    let (mut receiver, facts) = Session::open_with(&selected, &OPEN).unwrap();
    assert!(facts.available);
    assert_eq!(receiver.preparsed(), 0);
    assert_eq!(
        receiver
            .get(ReportKind::Feature, 3, 16, Duration::ZERO)
            .unwrap_err()
            .kind,
        ErrorKind::Timeout
    );
    drop(receiver);
    let mut replacement = selected.clone();
    replacement.path = selected.path.to_ascii_uppercase();
    assert!(matches!(
        Session::open_with(&replacement, &OPEN),
        Err(ReceiveError {
            kind: ErrorKind::Busy,
            ..
        })
    ));
    finish(Finish::Error(ERROR_OPERATION_ABORTED));
    let (new_receiver, _) = Session::open_with(&selected, &OPEN).unwrap();
    drop(new_receiver);
    // Eight outstanding/retired sessions consume the bound before another open.
    let sessions: Vec<_> = (0..SESSION_LIMIT)
        .map(|_| Session::open_with(&selected, &OPEN).unwrap().0)
        .collect();
    assert!(matches!(
        Session::open_with(&selected, &OPEN),
        Err(ReceiveError {
            kind: ErrorKind::Capacity,
            ..
        })
    ));
    drop(sessions);
}
