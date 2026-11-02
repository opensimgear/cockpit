use super::*;
use std::{sync::mpsc, thread, time::Duration};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Bytes(usize);
impl RetainedBytes for Bytes {
    fn retained_bytes(&self) -> usize {
        self.0
    }
}

fn limits() -> NativeLimits {
    NativeLimits {
        max_subscribers: 3,
        max_snapshot_bytes: 8,
        max_frame_bytes: 4,
        max_pending_frames: 2,
        max_pending_bytes: 6,
    }
}
fn stream() -> NativeStream<Bytes, Bytes> {
    NativeStream::new(1, Bytes(8), 10, "clock", limits()).unwrap()
}
fn consume_snapshot(reader: &mut NativeSubscription<Bytes, Bytes>) {
    assert!(matches!(reader.try_recv(), Ok(Some(Delivery::Snapshot(_)))));
}
fn loss(reader: &mut NativeSubscription<Bytes, Bytes>, reason: LossReason) {
    assert!(matches!(reader.try_recv(), Ok(Some(Delivery::Loss(value))) if value.reason == reason));
}

#[test]
fn native_arc_identity_without_serialization() {
    // This type deliberately implements neither Serialize nor Deserialize.
    #[derive(Clone)]
    struct Memory(Arc<[u8; 128 * 1024]>);
    impl RetainedBytes for Memory {
        fn retained_bytes(&self) -> usize {
            self.0.len()
        }
    }
    let memory = Arc::new([7; 128 * 1024]);
    let budget = NativeLimits {
        max_snapshot_bytes: memory.len(),
        max_frame_bytes: memory.len(),
        max_pending_bytes: memory.len(),
        ..limits()
    };
    let stream = NativeStream::new(1, Memory(memory.clone()), 1, "native", budget).unwrap();
    let mut first = stream.subscribe().unwrap();
    let mut second = stream.subscribe().unwrap();
    for reader in [&mut first, &mut second] {
        assert!(!reader.is_continuous());
        let Delivery::Snapshot(snapshot) = reader.recv().unwrap() else {
            panic!()
        };
        assert!(Arc::ptr_eq(&snapshot.value.0, &memory));
        assert!(reader.is_continuous());
    }
    stream
        .publish(
            1,
            Memory(memory.clone()),
            Memory(memory.clone()),
            2,
            "native",
        )
        .unwrap();
    for reader in [&mut first, &mut second] {
        let Delivery::Event(event) = reader.recv().unwrap() else {
            panic!()
        };
        assert!(Arc::ptr_eq(&event.value.0, &memory));
        assert_eq!((event.sequence, event.revision), (1, 2));
    }
}

#[test]
fn exact_limits_and_slow_reader_are_independent() {
    let stream = stream();
    let mut slow = stream.subscribe().unwrap();
    let mut fast = stream.subscribe().unwrap();
    consume_snapshot(&mut fast);
    stream.publish(1, Bytes(4), Bytes(8), 11, "clock").unwrap();
    assert!(matches!(fast.recv(), Ok(Delivery::Event(_))));
    stream.publish(1, Bytes(2), Bytes(8), 12, "clock").unwrap();
    assert!(matches!(fast.recv(), Ok(Delivery::Event(_))));
    assert_eq!(slow.shared.lock().subscribers[&slow.id].pending_bytes, 6);
    stream.publish(1, Bytes(1), Bytes(8), 13, "clock").unwrap();
    loss(&mut slow, LossReason::Overflow);
    assert!(!slow.is_continuous());
    assert!(slow.try_recv().unwrap().is_none());
    assert_eq!(slow.recv(), Err(StreamError::Closed));
    assert_eq!(slow.recv_timeout(Duration::ZERO), Err(StreamError::Closed));
    assert!(fast.is_continuous());
    assert!(matches!(fast.recv(), Ok(Delivery::Event(_))));
    let mut recovered = stream.subscribe().unwrap();
    let Delivery::Snapshot(baseline) = recovered.recv().unwrap() else {
        panic!()
    };
    assert_eq!(baseline.revision, 4);
    assert!(recovered.is_continuous());
    stream.replace_generation(2, Bytes(8), 14, "clock").unwrap();
    assert!(!slow.is_continuous());
    assert!(slow.try_recv().unwrap().is_none());
}

#[test]
fn byte_overflow_clears_partial_history_and_accounting() {
    let stream = stream();
    let mut reader = stream.subscribe().unwrap();
    consume_snapshot(&mut reader);
    stream.publish(1, Bytes(4), Bytes(8), 11, "clock").unwrap();
    assert!(matches!(reader.recv(), Ok(Delivery::Event(_))));
    assert_eq!(
        reader.shared.lock().subscribers[&reader.id].pending_bytes,
        0
    );
    stream.publish(1, Bytes(4), Bytes(8), 12, "clock").unwrap();
    stream.publish(1, Bytes(3), Bytes(8), 13, "clock").unwrap();
    loss(&mut reader, LossReason::Overflow);
    let state = reader.shared.lock();
    assert!(state.subscribers[&reader.id].events.is_empty());
    assert_eq!(state.subscribers[&reader.id].pending_bytes, 0);
}

#[test]
fn generation_loss_precedes_baseline_and_restores_continuity() {
    let stream = stream();
    let mut reader = stream.subscribe().unwrap();
    consume_snapshot(&mut reader);
    stream.publish(1, Bytes(1), Bytes(7), 11, "clock").unwrap();
    assert_eq!(stream.replace_generation(2, Bytes(6), 12, "other"), Ok(3));
    assert!(!reader.is_continuous());
    stream.publish(2, Bytes(1), Bytes(5), 13, "other").unwrap();
    loss(&mut reader, LossReason::GenerationReplaced);
    assert!(!reader.is_continuous());
    let Delivery::Snapshot(baseline) = reader.recv().unwrap() else {
        panic!()
    };
    assert_eq!(
        (baseline.generation, baseline.revision, baseline.value.0),
        (2, 3, 6)
    );
    assert!(reader.is_continuous());
    let Delivery::Event(event) = reader.recv().unwrap() else {
        panic!()
    };
    assert_eq!(
        (event.generation, event.sequence, event.revision),
        (2, 1, 4)
    );
    assert_eq!((stream.generation(), stream.revision()), (2, 4));
    stream.replace_generation(3, Bytes(1), 14, "clock").unwrap();
    stream.replace_generation(4, Bytes(2), 15, "clock").unwrap();
    loss(&mut reader, LossReason::GenerationReplaced);
    let Delivery::Snapshot(baseline) = reader.recv().unwrap() else {
        panic!()
    };
    assert_eq!(baseline.generation, 4);
}

#[test]
fn validation_and_rejected_publications_are_atomic() {
    for clock in ["", "white space", "é", &"x".repeat(129)] {
        assert!(matches!(
            NativeStream::<Bytes, Bytes>::new(1, Bytes(1), 1, clock, limits()),
            Err(PublishError::InvalidClockId)
        ));
    }
    assert!(matches!(
        NativeStream::<Bytes, Bytes>::new(0, Bytes(1), 1, "clock", limits()),
        Err(PublishError::InvalidGeneration)
    ));
    assert!(matches!(
        NativeStream::<Bytes, Bytes>::new(1, Bytes(9), 1, "clock", limits()),
        Err(PublishError::SnapshotTooLarge)
    ));
    for field in 0..5 {
        let mut budget = limits();
        match field {
            0 => budget.max_subscribers = 0,
            1 => budget.max_snapshot_bytes = 0,
            2 => budget.max_frame_bytes = 0,
            3 => budget.max_pending_frames = 0,
            _ => budget.max_pending_bytes = 0,
        }
        assert!(matches!(
            NativeStream::<Bytes, Bytes>::new(1, Bytes(1), 1, "clock", budget),
            Err(PublishError::Serialization(_))
        ));
    }
    let stream = stream();
    assert_eq!(
        stream.publish(1, Bytes(5), Bytes(8), 1, "clock"),
        Err(PublishError::FrameTooLarge)
    );
    assert_eq!(
        stream.publish(1, Bytes(4), Bytes(9), 1, "clock"),
        Err(PublishError::SnapshotTooLarge)
    );
    assert_eq!(
        stream.publish(2, Bytes(4), Bytes(8), 1, "clock"),
        Err(PublishError::StaleGeneration)
    );
    assert_eq!(
        stream.publish(1, Bytes(4), Bytes(8), 1, ""),
        Err(PublishError::InvalidClockId)
    );
    assert_eq!(
        stream.replace_generation(1, Bytes(8), 1, "clock"),
        Err(PublishError::StaleGeneration)
    );
    assert_eq!(
        stream.replace_generation(2, Bytes(9), 1, "clock"),
        Err(PublishError::SnapshotTooLarge)
    );
    assert_eq!(
        stream.replace_generation(2, Bytes(8), 1, ""),
        Err(PublishError::InvalidClockId)
    );
    assert_eq!((stream.generation(), stream.revision()), (1, 1));
}

#[test]
fn checked_counters_and_subscriber_capacity() {
    let stream = stream();
    let first = stream.subscribe().unwrap();
    let second = stream.subscribe().unwrap();
    let third = stream.subscribe().unwrap();
    assert!(matches!(
        stream.subscribe(),
        Err(StreamError::SubscriberLimit)
    ));
    drop(first);
    let replacement = stream.subscribe().unwrap();
    drop((second, third, replacement));
    stream.shared.lock().next_subscriber = u64::MAX;
    assert!(matches!(
        stream.subscribe(),
        Err(StreamError::Serialization(_))
    ));
    {
        let mut state = stream.shared.lock();
        Arc::make_mut(&mut state.snapshot).revision = u64::MAX;
    }
    assert_eq!(
        stream.publish(1, Bytes(1), Bytes(1), 1, "clock"),
        Err(PublishError::CounterExhausted)
    );
    assert_eq!(
        stream.replace_generation(2, Bytes(1), 1, "clock"),
        Err(PublishError::CounterExhausted)
    );
    {
        let mut state = stream.shared.lock();
        Arc::make_mut(&mut state.snapshot).revision = 1;
        state.sequence = u64::MAX;
    }
    assert_eq!(
        stream.publish(1, Bytes(1), Bytes(1), 1, "clock"),
        Err(PublishError::CounterExhausted)
    );
    assert_eq!(stream.revision(), 1);
}

#[test]
fn byte_counter_overflow_is_explicit_loss() {
    let budget = NativeLimits {
        max_frame_bytes: usize::MAX,
        max_pending_bytes: usize::MAX,
        ..limits()
    };
    let stream = NativeStream::new(1, Bytes(1), 1, "clock", budget).unwrap();
    let mut reader = stream.subscribe().unwrap();
    stream
        .publish(1, Bytes(usize::MAX), Bytes(1), 1, "clock")
        .unwrap();
    stream.publish(1, Bytes(1), Bytes(1), 1, "clock").unwrap();
    loss(&mut reader, LossReason::Overflow);
}

#[test]
fn blocking_wake_timeout_cancel_and_close() {
    let stream = stream();
    let mut reader = stream.subscribe().unwrap();
    consume_snapshot(&mut reader);
    assert_eq!(reader.recv_timeout(Duration::ZERO), Ok(None));
    assert_eq!(
        reader.recv_timeout(Duration::MAX),
        Err(StreamError::InvalidTimeout)
    );
    assert_eq!(reader.recv_timeout(Duration::from_millis(1)), Ok(None));
    let (ready, waiting) = mpsc::channel();
    let worker = thread::spawn(move || {
        ready.send(()).unwrap();
        assert!(matches!(reader.recv(), Ok(Delivery::Event(_))));
        reader
    });
    waiting.recv().unwrap();
    stream.publish(1, Bytes(1), Bytes(1), 1, "clock").unwrap();
    let mut reader = worker.join().unwrap();
    let cancel = reader.cancellation_handle();
    let worker = thread::spawn(move || reader.recv_timeout(Duration::from_secs(30)));
    cancel.cancel();
    assert_eq!(worker.join().unwrap(), Err(StreamError::Closed));
    let mut reader = stream.subscribe().unwrap();
    consume_snapshot(&mut reader);
    let closer = stream.clone();
    let worker = thread::spawn(move || {
        closer.close();
        closer.close();
    });
    assert!(matches!(
        reader.recv(),
        Ok(Delivery::Loss(Loss {
            reason: LossReason::Closed,
            ..
        }))
    ));
    worker.join().unwrap();
    assert!(!reader.is_continuous());
    assert_eq!(reader.recv(), Err(StreamError::Closed));
    assert!(matches!(stream.subscribe(), Err(StreamError::Closed)));
    assert_eq!(
        stream.publish(1, Bytes(1), Bytes(1), 1, "clock"),
        Err(PublishError::Closed)
    );
    assert_eq!(
        stream.replace_generation(2, Bytes(1), 1, "clock"),
        Err(PublishError::Closed)
    );
    cancel.cancel();
}

#[test]
fn close_discards_baseline_and_pending_event_immediately() {
    let stream = stream();
    let mut reader = stream.subscribe().unwrap();
    stream.publish(1, Bytes(1), Bytes(1), 1, "clock").unwrap();
    stream.close();
    loss(&mut reader, LossReason::Closed);
    assert!(reader.try_recv().unwrap().is_none());
    let cancel = reader.cancellation_handle();
    cancel.cancel();
    assert!(!reader.is_continuous());
    assert_eq!(reader.try_recv(), Err(StreamError::Closed));
}
