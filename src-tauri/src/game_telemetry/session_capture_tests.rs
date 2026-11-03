use super::*;
use crate::game_telemetry::CaptureOwner;

fn sample() -> RawSample {
    RawSample {
        source: SourceIdentity {
            id: "fixture".into(),
            epoch: 1,
        },
        physics: include_bytes!("../../../tests/fixtures/acr/live.physics.bin").to_vec(),
        graphics: include_bytes!("../../../tests/fixtures/acr/live.graphics.bin").to_vec(),
        statics: include_bytes!("../../../tests/fixtures/acr/live.static.bin").to_vec(),
        capture: None,
    }
}
fn advance(sample: &mut RawSample, packet: u32) {
    sample.physics[..4].copy_from_slice(&packet.to_le_bytes());
}

#[test]
fn reset_and_remap_cannot_reuse_or_backdate_native_baselines() {
    for reset in [false, true] {
        let base = Instant::now();
        let mut owner = CaptureOwner::new();
        owner.begin_generation().unwrap();
        let mut session = Session::new(100).unwrap();
        let first = native_sample(&mut owner, 80, base);
        session.feed_at(&first, 0, base);
        let second = native_sample(&mut owner, 81, base + std::time::Duration::from_millis(1));
        assert!(
            session
                .feed_at(&second, 1, base + std::time::Duration::from_millis(1))
                .fresh
        );
        let mut invalid = if reset {
            session.reset();
            second.clone()
        } else {
            owner.begin_generation().unwrap();
            native_sample(&mut owner, 82, base)
        };
        advance(&mut invalid, 82);
        let rejected = session.feed_at(&invalid, 2, base + std::time::Duration::from_millis(2));
        assert_eq!(rejected.status, "error");
        assert!(!rejected.fresh);
        assert!(rejected.capture.is_none());
    }
}
fn native_sample(owner: &mut CaptureOwner, packet: u32, at: Instant) -> RawSample {
    let mut sample = sample();
    advance(&mut sample, packet);
    let physics = owner
        .capture(at - std::time::Duration::from_nanos(2))
        .unwrap();
    let graphics = owner
        .capture(at - std::time::Duration::from_nanos(1))
        .unwrap();
    let statics = owner.capture(at).unwrap();
    sample.capture = Some(CaptureSet::new(owner, physics, graphics, statics).unwrap());
    sample
}

#[test]
fn native_capture_is_committed_with_values_and_duplicate_packets_do_not_renew_it() {
    let base = Instant::now();
    let mut owner = CaptureOwner::new();
    owner.begin_generation().unwrap();
    let mut session = Session::new(100).unwrap();
    let first = native_sample(&mut owner, 20, base);
    assert!(!session.feed_at(&first, 0, base).fresh);
    let second = native_sample(&mut owner, 21, base + std::time::Duration::from_millis(5));
    let committed = second.capture.clone().unwrap();
    assert!(
        session
            .feed_at(&second, 5, base + std::time::Duration::from_millis(5))
            .fresh
    );
    assert_eq!(
        session
            .snapshot_at(5, base + std::time::Duration::from_millis(5))
            .capture,
        Some(committed.clone())
    );

    let duplicate = native_sample(&mut owner, 21, base + std::time::Duration::from_millis(20));
    let snapshot = session.feed_at(&duplicate, 20, base + std::time::Duration::from_millis(20));
    assert!(snapshot.fresh);
    assert_eq!(snapshot.capture, Some(committed));
    assert_eq!(snapshot.age_ms, Some(15));
}

#[test]
fn initial_native_baseline_is_a_replay_fence_but_not_published_capture() {
    let base = Instant::now();
    let mut owner = CaptureOwner::new();
    owner.begin_generation().unwrap();
    let mut session = Session::new(100).unwrap();
    let baseline = native_sample(&mut owner, 25, base);
    let witness = baseline.capture.clone().unwrap();
    let snapshot = session.feed_at(&baseline, 0, base);
    assert!(!snapshot.fresh);
    assert!(snapshot.capture.is_none());

    let mut replay = baseline.clone();
    advance(&mut replay, 26);
    let rejected = session.feed_at(&replay, 1, base + std::time::Duration::from_millis(1));
    assert_eq!(rejected.status, "error");
    assert!(rejected.capture.is_none());
    assert_eq!(witness.generation, 1);
}

#[test]
fn native_capture_rejects_future_or_delayed_samples_and_expires_from_original_instants() {
    let base = Instant::now();
    let mut owner = CaptureOwner::new();
    owner.begin_generation().unwrap();
    let mut session = Session::new(100).unwrap();
    let first = native_sample(&mut owner, 30, base);
    session.feed_at(&first, 0, base);
    let second = native_sample(&mut owner, 31, base + std::time::Duration::from_millis(1));
    session.feed_at(&second, 1, base + std::time::Duration::from_millis(1));
    assert!(
        session
            .snapshot_at(50_000, base + std::time::Duration::from_millis(20))
            .fresh
    );
    assert!(
        session
            .snapshot_at(0, base + std::time::Duration::from_millis(30))
            .fresh
    );
    let retained = session.snapshot_at(1, base + std::time::Duration::from_millis(1));
    assert_eq!(retained.age_ms, Some(0));
    let expired_evidence = session.snapshot_at(2, base + std::time::Duration::from_millis(102));
    assert!(!expired_evidence.fresh);
    assert_eq!(expired_evidence.age_ms, Some(101));
    let expired = session.tick_at(50_000, base + std::time::Duration::from_millis(102));
    assert!(!expired.fresh);
    assert_eq!(
        expired.packet_id,
        Some(31),
        "expiry preserves the packet replay fence"
    );
    assert!(expired.capture.is_none());
    assert_eq!(
        expired.age_ms, None,
        "expired native proof never falls back to receipt age"
    );
    let duplicate = native_sample(&mut owner, 31, base + std::time::Duration::from_millis(103));
    let frozen = session.feed_at(&duplicate, 0, base + std::time::Duration::from_millis(103));
    assert!(
        !frozen.fresh,
        "the duplicate packet cannot rearm an expired native baseline"
    );

    let future = native_sample(&mut owner, 32, base + std::time::Duration::from_millis(200));
    let rejected = session.feed_at(&future, 2, base + std::time::Duration::from_millis(102));
    assert_eq!(rejected.status, "error");
    assert!(rejected.capture.is_none());
    let delayed = native_sample(&mut owner, 33, base);
    let rejected = session.feed_at(&delayed, 3, base + std::time::Duration::from_millis(104));
    assert_eq!(rejected.status, "error");
    assert!(rejected.capture.is_none());
}

#[test]
fn advancing_packets_reject_reused_or_regressed_native_page_evidence() {
    for regress_time in [false, true] {
        let base = Instant::now();
        let mut owner = CaptureOwner::new();
        owner.begin_generation().unwrap();
        let mut session = Session::new(100).unwrap();
        let first = native_sample(&mut owner, 50, base);
        session.feed_at(&first, 0, base);
        let committed = native_sample(&mut owner, 51, base + std::time::Duration::from_millis(1));
        session.feed_at(&committed, 1, base + std::time::Duration::from_millis(1));
        let previous = committed.capture.unwrap();
        let observed = base + std::time::Duration::from_millis(2);
        let mut candidate = native_sample(&mut owner, 52, observed);
        let capture = candidate.capture.as_mut().unwrap();
        if regress_time {
            capture.physics.captured_at =
                previous.physics.captured_at - std::time::Duration::from_nanos(1);
        } else {
            capture.physics.sequence = previous.physics.sequence;
        }
        assert!(capture.valid());
        let rejected = session.feed_at(&candidate, 2, observed);
        assert_eq!(rejected.status, "error");
        assert!(rejected.capture.is_none());
    }

    let base = Instant::now();
    let mut owner = CaptureOwner::new();
    owner.begin_generation().unwrap();
    let mut session = Session::new(100).unwrap();
    let first = native_sample(&mut owner, 60, base);
    session.feed_at(&first, 0, base);
    let committed = native_sample(&mut owner, 61, base + std::time::Duration::from_millis(1));
    session.feed_at(&committed, 1, base + std::time::Duration::from_millis(1));
    let previous = committed.capture.unwrap();
    let observed = base + std::time::Duration::from_millis(2);
    let mut same_tick = native_sample(&mut owner, 62, observed);
    let capture = same_tick.capture.as_mut().unwrap();
    capture.physics.captured_at = previous.physics.captured_at;
    capture.graphics.captured_at = previous.graphics.captured_at;
    capture.statics.captured_at = previous.statics.captured_at;
    assert!(capture.valid());
    assert!(session.feed_at(&same_tick, 2, observed).fresh);
}

#[test]
fn remapped_native_generation_rebases_before_accepting_new_page_evidence() {
    let base = Instant::now();
    let mut owner = CaptureOwner::new();
    owner.begin_generation().unwrap();
    let mut session = Session::new(100).unwrap();
    let first = native_sample(&mut owner, 40, base);
    session.feed_at(&first, 0, base);
    let active = native_sample(&mut owner, 41, base + std::time::Duration::from_millis(1));
    assert!(
        session
            .feed_at(&active, 1, base + std::time::Duration::from_millis(1))
            .fresh
    );

    owner.begin_generation().unwrap();
    let replacement = native_sample(&mut owner, 42, base + std::time::Duration::from_millis(2));
    let baseline = session.feed_at(&replacement, 2, base + std::time::Duration::from_millis(2));
    assert!(!baseline.fresh);
    assert!(baseline.capture.is_none());
    let mut baseline_replay = replacement.clone();
    advance(&mut baseline_replay, 43);
    let rejected = session.feed_at(
        &baseline_replay,
        3,
        base + std::time::Duration::from_millis(3),
    );
    assert_eq!(rejected.status, "error");

    let mut old_generation = active.clone();
    advance(&mut old_generation, 44);
    let rejected = session.feed_at(
        &old_generation,
        4,
        base + std::time::Duration::from_millis(4),
    );
    assert_eq!(
        rejected.status, "error",
        "a lower generation cannot become a new baseline"
    );

    let next = native_sample(&mut owner, 43, base + std::time::Duration::from_millis(3));
    let committed = session.feed_at(&next, 5, base + std::time::Duration::from_millis(5));
    assert!(committed.fresh);
    assert_eq!(committed.capture.unwrap().generation, 2);
}

#[test]
fn session_generation_exhaustion_clears_authority_and_never_rearms() {
    let base = Instant::now();
    let mut owner = CaptureOwner::new();
    owner.begin_generation().unwrap();
    let mut session = Session::new(100).unwrap();
    let first = native_sample(&mut owner, 70, base);
    session.feed_at(&first, 0, base);
    let active = native_sample(&mut owner, 71, base + std::time::Duration::from_millis(1));
    assert!(
        session
            .feed_at(&active, 1, base + std::time::Duration::from_millis(1))
            .fresh
    );

    session.epoch = u64::MAX;
    session.reset();
    let exhausted = session.snapshot_at(2, base + std::time::Duration::from_millis(2));
    assert!(!exhausted.fresh);
    assert!(exhausted.capture.is_none());
    assert_eq!(exhausted.epoch, u64::MAX);
    assert_eq!(exhausted.status, "error");

    let next = native_sample(&mut owner, 72, base + std::time::Duration::from_millis(2));
    let still_exhausted = session.feed_at(&next, 2, base + std::time::Duration::from_millis(2));
    assert!(!still_exhausted.fresh);
    assert!(still_exhausted.capture.is_none());
}
