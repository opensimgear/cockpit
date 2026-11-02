#[test]
fn independent_catalogue_projections_share_immutable_native_state_without_renewing_it() {
    use crate::dcs_bios::{decoded::State, CapturedDatagram};
    let fixture = Fixture::new();
    let compiled = fixture.compiled();
    let mut first = ExportSession::new(&compiled);
    let mut second = ExportSession::new(&compiled);
    let mut native = State::new(first.decode_spec().unwrap(), "native-source".into());
    let captured = Instant::now() - Duration::from_millis(20);
    for counter in 1..=2 {
        native.feed(&CapturedDatagram { producer_id: "transport".into(), sequence: counter,
            captured_at: captured + Duration::from_millis(counter),
            bytes: frame(Some("A-10C"), Some(b"123.450 "), counter as u8).into(),
        }, Instant::now()).unwrap();
    }
    let raw = native.snapshot();
    first.accept_shared(raw.clone()).unwrap();
    second.accept_shared(raw.clone()).unwrap();
    assert!(std::sync::Arc::ptr_eq(&first.shared.as_ref().unwrap().memory,
        &second.shared.as_ref().unwrap().memory));
    assert!(first.snapshot(0).fresh);
    assert_eq!(first.snapshot(0).values, second.snapshot(u64::MAX).values);
    assert_eq!(first.shared.as_ref().unwrap().captured_at, raw.captured_at);
    let generation = first.snapshot(0).epoch;
    first.accept_shared(raw.clone()).unwrap();
    assert_eq!(first.snapshot(0).epoch, generation);
    let mut replacement = raw;
    replacement.producer_id = "replacement".into();
    first.accept_shared(replacement.clone()).unwrap();
    assert_eq!(first.snapshot(0).epoch, generation + 1);
    replacement.captured_at = Some(Instant::now() - Duration::from_millis(replacement.freshness_ms + 1));
    first.accept_shared(replacement).unwrap();
    assert!(!first.snapshot(0).fresh);
    assert!(first.snapshot(0).values.is_empty());
    assert!(second.snapshot(0).fresh, "A projection cannot mutate another source view.");
}
