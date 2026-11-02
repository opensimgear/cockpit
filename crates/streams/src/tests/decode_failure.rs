use super::*;

#[test]
fn undecodable_event_revokes_continuity_until_a_fresh_baseline() {
    let journal: Stream<u64, f64> = Stream::with_limits(1, 0, 1, "test-clock", limits()).unwrap();
    let mut subscriber = journal.subscribe().unwrap();
    subscriber.try_recv().unwrap();
    assert_eq!(subscriber.recv_timeout(Duration::ZERO), Ok(None));
    assert_eq!(
        subscriber.recv_timeout(Duration::MAX),
        Err(StreamError::InvalidTimeout)
    );

    // JSON encodes non-finite floats as null, which cannot decode as f64.
    // The channel must fault rather than skip that event and continue.
    journal.publish(1, 1, f64::NAN, 2, "test-clock").unwrap();
    assert!(matches!(
        subscriber.try_recv(),
        Err(StreamError::Serialization(_))
    ));
    assert!(matches!(
        subscriber.try_recv().unwrap(),
        Some(Delivery::Loss(Loss {
            reason: LossReason::DecodeFailure,
            ..
        }))
    ));
    journal.publish(1, 2, 2.0, 3, "test-clock").unwrap();
    assert_eq!(subscriber.try_recv().unwrap(), None);

    journal.resynchronize(1, 2, 4, "test-clock").unwrap();
    assert!(matches!(
        subscriber.try_recv().unwrap(),
        Some(Delivery::Loss(Loss {
            reason: LossReason::BaselineReset,
            ..
        }))
    ));
    assert!(matches!(
        subscriber.try_recv().unwrap(),
        Some(Delivery::Snapshot(Snapshot { value: 2, .. }))
    ));
    journal.publish(1, 3, 3.0, 5, "test-clock").unwrap();
    assert!(matches!(
        subscriber.try_recv().unwrap(),
        Some(Delivery::Event(Event { value: 3.0, .. }))
    ));
}

#[test]
fn undecodable_initial_snapshot_does_not_expose_later_events() {
    let journal: Stream<f64, u64> =
        Stream::with_limits(1, f64::NAN, 1, "test-clock", limits()).unwrap();
    let mut subscriber = journal.subscribe().unwrap();
    journal.publish(1, 1.0, 1, 2, "test-clock").unwrap();
    assert!(matches!(
        subscriber.try_recv(),
        Err(StreamError::Serialization(_))
    ));
    assert!(matches!(
        subscriber.try_recv().unwrap(),
        Some(Delivery::Loss(Loss {
            reason: LossReason::DecodeFailure,
            ..
        }))
    ));
    assert_eq!(subscriber.try_recv().unwrap(), None);
}
