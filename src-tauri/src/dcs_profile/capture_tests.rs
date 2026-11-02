#[test]
fn delayed_bios_capture_keeps_native_age_and_expired_backlogs_cannot_revive() {
    let fixture = Fixture::new();
    let mut session = ExportSession::new(&fixture.compiled());
    let observed = std::time::Instant::now();
    let report = |sequence, counter, age_ms| crate::dcs_bios::CapturedDatagram {
        producer_id: "capture-1".into(), sequence,
        captured_at: observed - Duration::from_millis(age_ms),
        bytes: frame(Some("A-10C"), Some(b"123.450 "), counter).into(),
    };
    session.feed_captured(&report(1, 1, 250), 10_000, observed);
    assert_eq!(session.snapshot(10_000).age_ms, Some(250));
    assert!(session.snapshot(10_000).fresh);
    session.feed_captured(&report(2, 2, session.freshness_ms + 1), 10_000, observed);
    assert!(!session.snapshot(10_000).fresh);
    assert!(session.snapshot(10_000).values.is_empty());
    // The same guard works when the consumer's mock clock is smaller than age.
    session.feed_captured(&report(3, 3, session.freshness_ms + 1), 0, observed);
    assert!(!session.snapshot(0).fresh);
    // An otherwise fresh native report cannot be assigned a fabricated zero
    // timestamp when the consumer clock cannot represent its acquisition.
    session.feed_captured(&report(4, 4, 250), 100, observed);
    assert!(!session.snapshot(100).fresh);
}
#[test]
fn bios_capture_replacement_gap_and_invalid_clock_require_a_new_baseline() {
    let fixture = Fixture::new();
    let mut session = ExportSession::new(&fixture.compiled());
    let observed = std::time::Instant::now();
    let mut report = crate::dcs_bios::CapturedDatagram {
        producer_id: "capture-1".into(), sequence: 1, captured_at: observed,
        bytes: frame(Some("A-10C"), Some(b"123.450 "), 1).into(),
    };
    session.feed_captured(&report, 100, observed);
    let first = session.snapshot(100);
    assert!(first.fresh);
    report.bytes = frame(Some("A-10C"), Some(b"999.999 "), 2).into();
    session.feed_captured(&report, 200, observed);
    assert_eq!(session.snapshot(200).counter, Some(1));
    assert_eq!(session.snapshot(200).age_ms, Some(100));
    report.producer_id = "capture-2".into();
    session.feed_captured(&report, 200, observed);
    assert!(session.snapshot(200).epoch > first.epoch);
    let replacement = session.snapshot(200).epoch;
    report.sequence = 3;
    session.feed_captured(&report, 250, observed);
    assert!(session.snapshot(250).epoch > replacement);
    report.sequence = 4;
    report.captured_at = observed + Duration::from_millis(1);
    session.feed_captured(&report, 300, observed);
    assert!(!session.snapshot(300).fresh);
    report.sequence = 0;
    report.captured_at = observed;
    session.feed_captured(&report, 300, observed);
    assert!(!session.snapshot(300).fresh);
}
