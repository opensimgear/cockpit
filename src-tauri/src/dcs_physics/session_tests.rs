use super::*;
use crate::dcs_physics::tests::packet;
use std::time::{Duration, Instant};

#[test]
fn capture_sequence_exhaustion_fails_closed_without_wrapping_or_recovery() {
    let mut session = Session::new();
    session.capture_sequence = u64::MAX - 1;
    session
        .feed(packet(1, 1.0).to_string().as_bytes(), Instant::now(), 0)
        .unwrap();
    assert_eq!(session.snapshot(0).capture_sequence, u64::MAX);
    let last_capture = session.snapshot(0).captured_at;
    assert!(!session
        .feed(packet(1, 1.0).to_string().as_bytes(), Instant::now(), 25)
        .unwrap());
    assert_eq!(session.snapshot(25).capture_sequence, u64::MAX);
    assert_eq!(session.snapshot(25).captured_at, last_capture);

    let exhausted_at = Instant::now();
    assert!(session
        .feed(packet(2, 1.05).to_string().as_bytes(), exhausted_at, 50,)
        .is_err());
    let failed = session.snapshot(50);
    assert_eq!(failed.capture_sequence, u64::MAX);
    assert!(!failed.fresh);
    assert_eq!(failed.status, "error");

    assert!(session
        .feed(
            packet(3, 1.1).to_string().as_bytes(),
            exhausted_at + Duration::from_millis(50),
            100,
        )
        .is_err());
    let still_failed = session.snapshot(100);
    assert_eq!(still_failed.capture_sequence, u64::MAX);
    assert!(!still_failed.fresh);
    assert_eq!(still_failed.status, "error");
}
