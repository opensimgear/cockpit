use super::*;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

fn limits() -> Limits {
    Limits {
        max_subscribers: 2,
        max_snapshot_bytes: 1024,
        max_frame_bytes: 64,
        max_batch_bytes: 64,
        max_pending_frames: 2,
        max_pending_bytes: 64,
        max_batch_frames: 8,
    }
}

fn stream() -> Stream<u64, String> {
    Stream::with_limits(1, 0, 1, "test-clock", limits()).unwrap()
}

#[test]
fn concurrent_subscribe_and_publish_capture_one_atomic_revision_boundary() {
    let roomy_limits = Limits {
        max_pending_frames: 1200,
        max_pending_bytes: 64 * 1024,
        max_batch_bytes: 64 * 1024,
        ..limits()
    };
    let journal = Arc::new(Stream::with_limits(1, 0, 1, "test-clock", roomy_limits).unwrap());
    let barrier = Arc::new(Barrier::new(2));
    let writer = {
        let journal = journal.clone();
        let barrier = barrier.clone();
        thread::spawn(move || {
            barrier.wait();
            for value in 1..=1000 {
                journal
                    .publish(1, value, value.to_string(), value, "test-clock")
                    .unwrap();
            }
        })
    };
    barrier.wait();
    let mut subscription = journal.subscribe().unwrap();
    writer.join().unwrap();

    let Some(Delivery::Snapshot(snapshot)) = subscription.try_recv().unwrap() else {
        panic!("subscription must deliver its atomic snapshot first");
    };
    assert!(snapshot.value <= 1000);
    let mut expected_value = snapshot.value + 1;
    while let Some(delivery) = subscription.try_recv().unwrap() {
        match delivery {
            Delivery::Event(event) => {
                assert_eq!(event.generation, snapshot.generation);
                assert_eq!(event.revision, expected_value + 1);
                assert_eq!(event.sequence, expected_value);
                assert_eq!(event.value.parse::<u64>().unwrap(), expected_value);
                expected_value += 1;
            }
            Delivery::Loss(loss) => panic!("roomy reader lost at revision {}", loss.revision),
            Delivery::Snapshot(_) => panic!("unexpected second snapshot"),
        }
    }
    assert_eq!(expected_value, 1001);
}

#[test]
fn fast_and_slow_subscribers_are_independent_and_resync_is_a_hard_boundary() {
    let journal = stream();
    let mut fast = journal.subscribe().unwrap();
    let mut slow = journal.subscribe().unwrap();
    assert!(matches!(
        fast.try_recv().unwrap(),
        Some(Delivery::Snapshot(_))
    ));
    assert!(matches!(
        slow.try_recv().unwrap(),
        Some(Delivery::Snapshot(_))
    ));

    for value in 1..=6 {
        journal
            .publish(1, value, value.to_string(), value, "test-clock")
            .unwrap();
        assert!(matches!(fast.try_recv().unwrap(), Some(Delivery::Event(_))));
    }
    assert!(matches!(
        slow.try_recv().unwrap(),
        Some(Delivery::Loss(Loss {
            reason: LossReason::Overflow,
            ..
        }))
    ));
    assert_eq!(slow.try_recv().unwrap(), None);

    journal.resynchronize(1, 6, 7, "test-clock").unwrap();
    journal
        .publish(1, 7, "after-baseline".into(), 8, "test-clock")
        .unwrap();
    assert!(matches!(
        slow.try_recv().unwrap(),
        Some(Delivery::Loss(Loss {
            reason: LossReason::BaselineReset,
            ..
        }))
    ));
    assert!(matches!(
        slow.try_recv().unwrap(),
        Some(Delivery::Snapshot(Snapshot { value: 6, .. }))
    ));
    assert!(
        matches!(slow.try_recv().unwrap(), Some(Delivery::Event(Event { value, .. })) if value == "after-baseline")
    );
}

#[test]
fn frame_and_byte_limits_reserve_loss_and_clear_pending_effects() {
    let by_frames = stream();
    let mut frame_reader = by_frames.subscribe().unwrap();
    frame_reader.try_recv().unwrap();
    for value in 0..3 {
        by_frames
            .publish(1, value, "x".into(), value, "test-clock")
            .unwrap();
    }
    assert_eq!(
        frame_reader.try_recv().unwrap(),
        Some(Delivery::Loss(Loss {
            generation: 1,
            revision: 4,
            reason: LossReason::Overflow,
        }))
    );
    assert_eq!(frame_reader.try_recv().unwrap(), None);

    let byte_limits = Limits {
        max_pending_bytes: 5,
        ..limits()
    };
    let by_bytes: Stream<u64, String> =
        Stream::with_limits(1, 0, 1, "test-clock", byte_limits).unwrap();
    let mut byte_reader = by_bytes.subscribe().unwrap();
    byte_reader.try_recv().unwrap();
    by_bytes.publish(1, 1, "a".into(), 2, "test-clock").unwrap(); // 3 JSON bytes
    by_bytes.publish(1, 2, "b".into(), 3, "test-clock").unwrap(); // exceeds 5
    assert!(matches!(
        byte_reader.try_recv().unwrap(),
        Some(Delivery::Loss(Loss {
            reason: LossReason::Overflow,
            ..
        }))
    ));
    assert_eq!(byte_reader.try_recv().unwrap(), None);
}

#[test]
fn dropping_subscription_reclaims_capacity_and_generation_change_fences_old_data() {
    let journal = stream();
    let first = journal.subscribe().unwrap();
    let second = journal.subscribe().unwrap();
    assert!(matches!(
        journal.subscribe(),
        Err(StreamError::SubscriberLimit)
    ));
    drop(first);
    let mut replacement = journal.subscribe().unwrap();
    drop(second);

    journal.replace_generation(2, 10, 2, "test-clock").unwrap();
    assert!(matches!(
        replacement.try_recv().unwrap(),
        Some(Delivery::Loss(Loss {
            reason: LossReason::GenerationReplaced,
            generation: 2,
            ..
        }))
    ));
    assert!(matches!(
        replacement.try_recv().unwrap(),
        Some(Delivery::Snapshot(Snapshot {
            generation: 2,
            value: 10,
            ..
        }))
    ));
    assert_eq!(
        journal.publish(1, 11, "old".into(), 3, "test-clock"),
        Err(PublishError::StaleGeneration)
    );
    assert_eq!(
        journal.resynchronize(1, 12, 4, "test-clock"),
        Err(PublishError::StaleGeneration)
    );
    journal
        .publish(2, 11, "new".into(), 3, "test-clock")
        .unwrap();
    assert!(matches!(
        replacement.try_recv().unwrap(),
        Some(Delivery::Event(Event {
            generation: 2,
            sequence: 1,
            ..
        }))
    ));
}

#[test]
fn close_discards_pending_baselines_and_wakes_receivers_with_closed_loss() {
    let journal = stream();
    let mut subscription = journal.subscribe().unwrap();
    journal.close();
    assert!(matches!(
        subscription.recv().unwrap(),
        Delivery::Loss(Loss {
            reason: LossReason::Closed,
            ..
        })
    ));
    assert_eq!(subscription.recv(), Err(StreamError::Closed));
    assert_eq!(
        journal.publish(1, 2, "after-close".into(), 2, "test-clock"),
        Err(PublishError::Closed)
    );
}

#[test]
fn invalid_or_oversized_frames_do_not_mutate_state_or_subscriber_queues() {
    let journal = stream();
    let mut subscriber = journal.subscribe().unwrap();
    let revision = journal.revision();
    assert_eq!(
        journal.publish(1, 1, "x".repeat(100), 2, "test-clock"),
        Err(PublishError::FrameTooLarge)
    );
    assert_eq!(journal.revision(), revision);
    assert!(matches!(
        subscriber.try_recv().unwrap(),
        Some(Delivery::Snapshot(Snapshot { value: 0, .. }))
    ));
    assert_eq!(subscriber.try_recv().unwrap(), None);

    assert_eq!(
        journal.publish(1, 1, "ok".into(), 2, "bad clock"),
        Err(PublishError::InvalidClockId)
    );
    assert_eq!(journal.revision(), revision);
    assert_eq!(subscriber.try_recv().unwrap(), None);

    let tiny = Stream::<String, String>::with_limits(
        1,
        "x".repeat(100),
        1,
        "test-clock",
        Limits {
            max_snapshot_bytes: 4,
            ..limits()
        },
    );
    assert!(matches!(tiny, Err(PublishError::SnapshotTooLarge)));
}

#[test]
fn publish_batch_is_atomic_when_later_frame_is_invalid() {
    let journal = stream();
    let mut subscriber = journal.subscribe().unwrap();
    subscriber.try_recv().unwrap();
    let revision = journal.revision();
    assert_eq!(
        journal.publish_batch(
            5,
            1,
            vec![
                ("valid".to_string(), 2, "test-clock".to_string()),
                ("z".repeat(100), 3, "test-clock".to_string()),
            ],
        ),
        Err(PublishError::FrameTooLarge)
    );
    assert_eq!(journal.revision(), revision);
    assert_eq!(subscriber.try_recv().unwrap(), None);
}

#[test]
fn oversized_batch_is_rejected_before_revision_or_subscriber_mutation() {
    let small_batch = Limits {
        max_batch_bytes: 5,
        ..limits()
    };
    let journal: Stream<u64, String> =
        Stream::with_limits(1, 0, 1, "test-clock", small_batch).unwrap();
    let mut subscriber = journal.subscribe().unwrap();
    subscriber.try_recv().unwrap();
    let revision = journal.revision();
    assert_eq!(
        journal.publish_batch(
            1,
            3,
            vec![
                ("a".to_owned(), 2, "test-clock".to_owned()),
                ("b".to_owned(), 3, "test-clock".to_owned()),
            ],
        ),
        Err(PublishError::BatchTooLarge)
    );
    assert_eq!(journal.revision(), revision);
    assert_eq!(subscriber.try_recv().unwrap(), None);
}

#[test]
fn wire_neutral_fixtures_preserve_boundaries_ordered_deltas_and_exact_counters() {
    let baseline: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/snapshot_then_updates.json")).unwrap();
    assert_eq!(baseline["snapshot"]["revision"], "41");
    assert_eq!(baseline["updates"][0]["revision"], "42");
    assert_eq!(baseline["snapshot"]["continuity"], "baseline");

    let recovery: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/overflow_recovery.json")).unwrap();
    let deliveries = recovery["deliveries"].as_array().unwrap();
    assert_eq!(deliveries[0]["kind"], "loss");
    assert_eq!(deliveries[1]["kind"], "snapshot");
    assert_eq!(deliveries[2]["kind"], "event");
    assert!(deliveries[0]["revision"].as_str().is_some());

    let relative: serde_json::Value = serde_json::from_str(include_str!(
        "../tests/fixtures/ordered_relative_values.json"
    ))
    .unwrap();
    let events = relative["events"].as_array().unwrap();
    assert_eq!(events[0]["payload"]["encoderDelta"], -2);
    assert_eq!(events[1]["payload"]["encoderDelta"], 3);
    assert_eq!(events[0]["sequence"], "1");
    assert_eq!(events[1]["sequence"], "2");
    let journal: Stream<i64, i64> = Stream::with_limits(9, 0, 1, "test-clock", limits()).unwrap();
    let mut subscription = journal.subscribe().unwrap();
    assert!(matches!(
        subscription.try_recv().unwrap(),
        Some(Delivery::Snapshot(_))
    ));
    let deltas = events
        .iter()
        .map(|event| {
            (
                event["payload"]["encoderDelta"].as_i64().unwrap(),
                event["capturedUs"]
                    .as_str()
                    .unwrap()
                    .parse::<u64>()
                    .unwrap(),
                "test-clock".to_owned(),
            )
        })
        .collect();
    journal.publish_batch(9, 1, deltas).unwrap();
    for expected in [-2, 3] {
        assert!(matches!(
            subscription.try_recv().unwrap(),
            Some(Delivery::Event(Event { value, .. })) if value == expected
        ));
    }
    assert_eq!(subscription.try_recv().unwrap(), None);

    let exact: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/exact_u64.json")).unwrap();
    for key in [
        "generation",
        "sequence",
        "revision",
        "capturedUs",
        "nativeUnsignedValue",
    ] {
        assert!(
            exact[key].as_str().is_some(),
            "{key} must remain an exact decimal string"
        );
    }
    assert_eq!(
        exact["generation"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap(),
        u64::MAX
    );
}

#[test]
fn blocking_receiver_wakes_on_publish_and_cancellation() {
    let journal = stream();
    let mut first = journal.subscribe().unwrap();
    first.try_recv().unwrap();
    let publisher = journal.clone();
    let (sent, received) = std::sync::mpsc::channel();
    let waiter = thread::spawn(move || {
        sent.send(first.recv()).unwrap();
    });
    thread::sleep(Duration::from_millis(10));
    publisher
        .publish(1, 1, "wake".into(), 2, "test-clock")
        .unwrap();
    assert!(
        matches!(received.recv_timeout(Duration::from_secs(1)).unwrap(), Ok(Delivery::Event(Event { value, .. })) if value == "wake")
    );
    waiter.join().unwrap();

    let mut second = journal.subscribe().unwrap();
    second.try_recv().unwrap();
    let cancellation = second.cancellation_handle();
    let (sent, received) = std::sync::mpsc::channel();
    let waiter = thread::spawn(move || sent.send(second.recv()).unwrap());
    thread::sleep(Duration::from_millis(10));
    cancellation.cancel();
    assert_eq!(
        received.recv_timeout(Duration::from_secs(1)).unwrap(),
        Err(StreamError::Closed)
    );
    waiter.join().unwrap();
}

#[path = "tests/delayed_decode.rs"]
mod delayed_decode;

#[path = "tests/decode_failure.rs"]
mod decode_failure;
