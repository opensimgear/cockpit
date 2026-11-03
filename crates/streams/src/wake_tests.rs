use super::*;
use std::{
    sync::{mpsc, Arc},
    thread,
};

#[test]
fn notify_without_binding_and_saturation_never_lose_pending_work() {
    let wake = NativeWake::default();
    assert_eq!(wake.observed(), 0);
    wake.notify();
    assert_eq!(wake.observed(), 1);
    assert!(!wake.wait(0, Duration::from_secs(30)));
    wake.epoch.store(u64::MAX - 1, Ordering::Relaxed);
    wake.notify();
    assert_eq!(wake.observed(), u64::MAX);
    assert!(!wake.wait_scheduled(u64::MAX, None));
    wake.notify();
    assert_eq!(wake.observed(), u64::MAX);
    assert!(!wake.wait(u64::MAX, Duration::from_secs(30)));
}

#[test]
fn binding_is_once_only_and_timeout_can_park() {
    let wake = NativeWake::default();
    assert!(wake.bind_thread(thread::current()).is_ok());
    assert!(wake.bind_thread(thread::current()).is_err());
    assert!(wake.wait(0, Duration::ZERO));
    assert!(wake.wait(0, Duration::from_millis(1)));
}

#[test]
fn binding_releases_worker_parked_after_a_prebind_notification() {
    let wake = NativeWake::default();
    let (ready, waiting) = mpsc::channel();
    let (done, finished) = mpsc::channel();
    let worker = thread::spawn(move || {
        // Model the wait boundary after its epoch check: notification arrives
        // before the parent binds the worker, so notify cannot leave a token.
        ready.send(()).unwrap();
        thread::park();
        done.send(()).unwrap();
    });
    waiting.recv_timeout(Duration::from_secs(2)).unwrap();
    wake.notify();
    assert!(finished.recv_timeout(Duration::from_millis(10)).is_err());
    assert!(wake.bind_thread(worker.thread().clone()).is_ok());
    let released = finished.recv_timeout(Duration::from_secs(2));
    // Bound the regression failure too: let the worker exit before asserting.
    worker.thread().unpark();
    worker.join().unwrap();
    assert!(
        released.is_ok(),
        "binding did not release the parked worker"
    );
}

#[test]
fn binding_without_a_notification_does_not_manufacture_a_park_token() {
    let wake = NativeWake::default();
    let (start, started) = mpsc::channel();
    let (done, finished) = mpsc::channel();
    let worker = thread::spawn(move || {
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        thread::park();
        done.send(()).unwrap();
    });
    assert!(wake.bind_thread(worker.thread().clone()).is_ok());
    start.send(()).unwrap();
    let before_notification = finished.recv_timeout(Duration::from_millis(10));
    wake.notify();
    let released = finished.recv_timeout(Duration::from_secs(2));
    worker.thread().unpark();
    worker.join().unwrap();
    assert!(before_notification.is_err(), "binding manufactured a token");
    assert!(released.is_ok(), "notification did not release the worker");
}

#[test]
fn changed_epoch_survives_notification_during_drain_and_consumed_park_token() {
    let wake = Arc::new(NativeWake::default());
    let worker_wake = wake.clone();
    let (ready, observed) = mpsc::channel();
    let (resume, resumed) = mpsc::channel();
    let worker = thread::spawn(move || {
        assert!(worker_wake.bind_thread(thread::current()).is_ok());
        for _ in 0..3 {
            let epoch = worker_wake.observed();
            ready.send(()).unwrap();
            resumed.recv_timeout(Duration::from_secs(2)).unwrap();
            // IPC or other in-poll code may already have consumed the token.
            thread::park_timeout(Duration::ZERO);
            assert!(!worker_wake.wait(epoch, Duration::from_secs(30)));
        }
    });
    for burst in [1, 2, 100] {
        observed.recv_timeout(Duration::from_secs(2)).unwrap();
        for _ in 0..burst {
            wake.notify();
        }
        resume.send(()).unwrap();
    }
    worker.join().unwrap();
}

#[test]
fn unscheduled_wait_is_released_by_committed_notification() {
    let wake = Arc::new(NativeWake::default());
    let worker_wake = wake.clone();
    let (ready, waiting) = mpsc::channel();
    let (done, finished) = mpsc::channel();
    let worker = thread::spawn(move || {
        assert!(worker_wake.bind_thread(thread::current()).is_ok());
        let epoch = worker_wake.observed();
        ready.send(()).unwrap();
        while worker_wake.observed() == epoch {
            worker_wake.wait_scheduled(epoch, None);
        }
        done.send(worker_wake.observed()).unwrap();
    });
    waiting.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(finished.recv_timeout(Duration::from_millis(10)).is_err());
    wake.notify();
    assert_eq!(finished.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
    worker.join().unwrap();
}
