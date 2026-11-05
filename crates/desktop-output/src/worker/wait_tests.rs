use super::*;
use std::{sync::{mpsc, Barrier}, thread};

fn receive_item<T>(waiter: &mut NativeWait, receiver: &Receiver<T>) -> T {
    let deadline = Instant::now() + Duration::from_millis(400);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "native queue wake exceeded its bounded test deadline");
        match waiter.receive(receiver, remaining).unwrap() {
            Ok(value) => {
                assert!(Instant::now() < deadline, "queued frame arrived only after the fallback timeout");
                return value;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("queue disconnected: {error}"),
        }
    }
}

#[test]
fn queued_frames_are_ordered_even_when_event_notifications_coalesce() {
    let mut waiter = NativeWait::new().unwrap();
    let wake = waiter.waker();
    let (send, receive) = mpsc::sync_channel(4);
    for value in 1..=3 { send.send(value).unwrap(); wake.notify(); }
    for value in 1..=3 { assert_eq!(receive_item(&mut waiter, &receive), value); }
    assert!(matches!(waiter.receive(&receive, Duration::from_millis(2)).unwrap(), Err(RecvTimeoutError::Timeout)));
    drop(send);
    assert!(matches!(waiter.receive(&receive, Duration::from_millis(2)).unwrap(), Err(RecvTimeoutError::Disconnected)));
}

#[test]
fn reset_check_wait_races_do_not_lose_queue_notifications() {
    let mut waiter = NativeWait::new().unwrap();
    let wake = waiter.waker();
    let (send, receive) = mpsc::sync_channel(1);
    let (start, started) = mpsc::sync_channel(0);
    let producer = thread::spawn(move || {
        for value in 0..100 {
            started.recv().unwrap();
            send.send(value).unwrap();
            wake.notify();
        }
    });
    for value in 0..100 {
        start.send(()).unwrap();
        assert_eq!(receive_item(&mut waiter, &receive), value);
    }
    producer.join().unwrap();
}

#[test]
fn timer_expiration_and_rearm_do_not_reuse_an_old_signal() {
    let mut waiter = NativeWait::new().unwrap();
    let (_send, receive) = mpsc::sync_channel::<u8>(1);
    for millis in [2, 15] {
        let before = Instant::now();
        assert!(matches!(waiter.receive(&receive, Duration::from_millis(millis)).unwrap(), Err(RecvTimeoutError::Timeout)));
        assert!(before.elapsed() >= Duration::from_millis(millis - 1));
        assert!(before.elapsed() < Duration::from_millis(500));
    }
    assert!(matches!(waiter.receive(&receive, Duration::ZERO).unwrap(), Err(RecvTimeoutError::Timeout)));
}

#[test]
fn queue_notification_interrupts_a_long_native_deadline() {
    let mut waiter = NativeWait::new().unwrap();
    let wake = waiter.waker();
    let (send, receive) = mpsc::sync_channel(1);
    let ready = Arc::new(Barrier::new(2));
    let producer_ready = ready.clone();
    let producer = thread::spawn(move || {
        producer_ready.wait();
        thread::sleep(Duration::from_millis(10));
        send.send(42).unwrap();
        wake.notify();
    });
    ready.wait();
    let before = Instant::now();
    assert_eq!(receive_item(&mut waiter, &receive), 42);
    assert!(before.elapsed() < Duration::from_millis(300));
    producer.join().unwrap();
}

#[test]
fn failure_only_notifications_wake_without_a_protocol_frame() {
    let mut waiter = NativeWait::new().unwrap();
    let wake = waiter.waker();
    let (_send, receive) = mpsc::sync_channel::<u8>(1);
    let producer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(10));
        wake.notify();
    });
    let before = Instant::now();
    assert!(matches!(waiter.receive(&receive, Duration::from_millis(400)).unwrap(), Err(RecvTimeoutError::Timeout)));
    assert!(before.elapsed() < Duration::from_millis(300));
    producer.join().unwrap();
    // A failure notification sent before the reset is retained too.
    waiter.waker().notify();
    let before = Instant::now();
    assert!(matches!(waiter.receive(&receive, Duration::from_millis(400)).unwrap(), Err(RecvTimeoutError::Timeout)));
    assert!(before.elapsed() < Duration::from_millis(300));
}

#[test]
fn failed_event_notification_is_explicit_and_relative_due_times_round_up() {
    let mut waiter = NativeWait::new().unwrap();
    let (_send, receive) = mpsc::sync_channel::<u8>(1);
    waiter.event.0.failed.store(true, Ordering::Release);
    assert!(waiter.receive(&receive, Duration::from_millis(5)).unwrap_err().contains("notification failed"));
    assert_eq!(relative_due(Duration::ZERO), -1);
    assert_eq!(relative_due(Duration::from_nanos(100)), -1);
    assert_eq!(relative_due(Duration::from_nanos(101)), -2);
    assert_eq!(relative_due(Duration::MAX), -i64::MAX);
}
