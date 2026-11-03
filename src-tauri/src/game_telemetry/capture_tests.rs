use super::*;

#[test]
fn owner_requires_a_generation_and_sequences_never_wrap() {
    let mut owner = CaptureOwner::new();
    assert!(owner.capture(Instant::now()).is_err());
    assert_eq!(owner.begin_generation().unwrap(), 1);
    let first = owner.capture(Instant::now()).unwrap();
    let second = owner.capture(Instant::now()).unwrap();
    assert_eq!((first.sequence, second.sequence), (1, 2));

    owner.sequence = u64::MAX;
    assert!(owner.capture(Instant::now()).is_err());
    owner.generation = u64::MAX;
    assert!(owner.begin_generation().is_err());
    assert_eq!(owner.generation, u64::MAX);
}

#[test]
fn capture_sets_require_distinct_nonzero_page_sequences_and_checked_time() {
    let mut owner = CaptureOwner::new();
    owner.begin_generation().unwrap();
    let now = Instant::now();
    let page = |sequence, captured_at| PageCapture {
        sequence,
        captured_at,
    };
    let valid = CaptureSet::new(
        &owner,
        page(1, now - Duration::from_millis(10)),
        page(2, now - Duration::from_millis(5)),
        page(3, now),
    )
    .unwrap();
    assert!(valid.current_at(now, 10));
    assert!(!valid.current_at(now, 9));
    assert!(!valid.current_at(now - Duration::from_millis(1), 10));
    assert!(CaptureSet::new(&owner, page(1, now), page(1, now), page(2, now)).is_err());
    assert!(CaptureSet::new(&owner, page(0, now), page(1, now), page(2, now)).is_err());
    owner.producer_id = uuid::Uuid::nil().to_string();
    assert!(CaptureSet::new(&owner, page(1, now), page(2, now), page(3, now)).is_err());
    owner.producer_id = uuid::Uuid::new_v4().to_string();
    let same_tick = CaptureSet::new(&owner, page(4, now), page(5, now), page(6, now)).unwrap();
    assert!(
        same_tick.valid(),
        "sequence order distinguishes captures in one clock tick"
    );
}
