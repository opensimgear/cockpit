use super::*;
use std::{sync::Barrier, thread};

#[derive(Clone)]
struct Bytes(usize);
impl RetainedBytes for Bytes {
    fn retained_bytes(&self) -> usize {
        self.0
    }
}
fn stream() -> NativeStream<Bytes, Bytes> {
    NativeStream::new(
        1,
        Bytes(1),
        0,
        "clock",
        NativeLimits {
            max_subscribers: 3,
            max_snapshot_bytes: 2,
            max_frame_bytes: 2,
            max_pending_frames: 1,
            max_pending_bytes: 2,
        },
    )
    .unwrap()
}

#[test]
fn watch_initial_commit_replacement_close_and_drop() {
    let stream = stream();
    let wake = Arc::new(NativeWake::default());
    let watch = stream.watch(wake.clone()).unwrap();
    assert!(!watch.is_closed());
    assert_eq!(wake.observed(), 1);
    stream.publish(1, Bytes(1), Bytes(1), 0, "clock").unwrap();
    assert_eq!(wake.observed(), 2);
    stream.replace_generation(2, Bytes(1), 0, "clock").unwrap();
    assert_eq!(wake.observed(), 3);
    stream.close();
    assert_eq!(wake.observed(), 4);
    assert!(watch.is_closed());
    stream.close();
    assert_eq!(wake.observed(), 4);
    assert!(matches!(
        stream.watch(wake.clone()),
        Err(StreamError::Closed)
    ));
    drop(watch);
    assert!(stream.shared.lock().watchers.is_empty());
}

#[test]
fn shared_capacity_and_nonwrapping_ids_reject_without_notifying() {
    let stream = stream();
    let wake = Arc::new(NativeWake::default());
    let watch = stream.watch(wake.clone()).unwrap();
    let first = stream.subscribe().unwrap();
    let second = stream.subscribe().unwrap();
    assert!(matches!(
        stream.subscribe(),
        Err(StreamError::SubscriberLimit)
    ));
    assert!(matches!(
        stream.watch(wake.clone()),
        Err(StreamError::SubscriberLimit)
    ));
    assert_eq!(wake.observed(), 1);
    drop(watch);
    let watch = stream.watch(wake.clone()).unwrap();
    assert_eq!(wake.observed(), 2);
    drop((first, second, watch));
    stream.shared.lock().next_subscriber = u64::MAX;
    assert!(matches!(
        stream.watch(wake.clone()),
        Err(StreamError::Serialization(_))
    ));
    assert_eq!(wake.observed(), 2);
}

#[test]
fn rejected_transactions_never_notify() {
    let stream = stream();
    let wake = Arc::new(NativeWake::default());
    let _watch = stream.watch(wake.clone()).unwrap();
    assert!(stream.publish(1, Bytes(3), Bytes(1), 0, "clock").is_err());
    assert!(stream.publish(1, Bytes(1), Bytes(3), 0, "clock").is_err());
    assert!(stream.publish(2, Bytes(1), Bytes(1), 0, "clock").is_err());
    assert!(stream.publish(1, Bytes(1), Bytes(1), 0, "").is_err());
    assert!(stream.replace_generation(1, Bytes(1), 0, "clock").is_err());
    assert!(stream.replace_generation(2, Bytes(3), 0, "clock").is_err());
    assert!(stream.replace_generation(2, Bytes(1), 0, "").is_err());
    stream.shared.lock().sequence = u64::MAX;
    assert!(stream.publish(1, Bytes(1), Bytes(1), 0, "clock").is_err());
    Arc::make_mut(&mut stream.shared.lock().snapshot).revision = u64::MAX;
    assert!(stream.publish(1, Bytes(1), Bytes(1), 0, "clock").is_err());
    assert!(stream.replace_generation(2, Bytes(1), 0, "clock").is_err());
    assert_eq!(wake.observed(), 1);
    stream.close();
    assert!(stream.publish(1, Bytes(1), Bytes(1), 0, "clock").is_err());
    assert!(stream.replace_generation(2, Bytes(1), 0, "clock").is_err());
    assert_eq!(wake.observed(), 2);
}

#[test]
fn register_racing_commit_or_close_cannot_miss_current_state() {
    for close in [false, true] {
        for _ in 0..40 {
            let stream = stream();
            let publisher = stream.clone();
            let barrier = Arc::new(Barrier::new(2));
            let worker_barrier = barrier.clone();
            let worker = thread::spawn(move || {
                worker_barrier.wait();
                if close {
                    publisher.close();
                } else {
                    publisher
                        .publish(1, Bytes(1), Bytes(2), 0, "clock")
                        .unwrap();
                }
            });
            let wake = Arc::new(NativeWake::default());
            barrier.wait();
            let watch = stream.watch(wake.clone());
            worker.join().unwrap();
            if close {
                match watch {
                    Ok(watch) => {
                        assert!(watch.is_closed());
                        assert_eq!(wake.observed(), 2);
                    }
                    Err(error) => {
                        assert_eq!(error, StreamError::Closed);
                        assert_eq!(wake.observed(), 0);
                    }
                }
            } else {
                assert!(!watch.unwrap().is_closed());
                assert!((1..=2).contains(&wake.observed()));
                let mut reader = stream.subscribe().unwrap();
                assert!(
                    matches!(reader.try_recv().unwrap(), Some(Delivery::Snapshot(s)) if s.revision == 2)
                );
            }
        }
    }
}

#[test]
fn unbound_or_stalled_watchers_do_not_block_or_queue_payloads() {
    struct NoClone;
    impl Clone for NoClone {
        fn clone(&self) -> Self {
            panic!("watchers must never clone payloads")
        }
    }
    impl RetainedBytes for NoClone {
        fn retained_bytes(&self) -> usize {
            1
        }
    }
    let limits = stream().shared.limits;
    let stream = NativeStream::new(1, NoClone, 0, "clock", limits).unwrap();
    let unbound = Arc::new(NativeWake::default());
    let stalled = Arc::new(NativeWake::default());
    assert!(stalled.bind_thread(thread::current()).is_ok());
    let first = stream.watch(unbound.clone()).unwrap();
    let second = stream.watch(stalled.clone()).unwrap();
    for _ in 0..1000 {
        stream.publish(1, NoClone, NoClone, 0, "clock").unwrap();
    }
    assert_eq!(unbound.observed(), 1001);
    assert_eq!(stalled.observed(), 1001);
    assert!(stream.shared.lock().subscribers.is_empty());
    drop(first);
    stream.publish(1, NoClone, NoClone, 0, "clock").unwrap();
    assert_eq!(unbound.observed(), 1001);
    assert_eq!(stalled.observed(), 1002);
    drop(second);
}
