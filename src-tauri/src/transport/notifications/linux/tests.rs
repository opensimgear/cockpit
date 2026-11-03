use super::*;
use std::sync::atomic::AtomicUsize;

#[test]
fn stopped_source_never_waits_or_receives() {
    let wake = NativeWake::default();
    run(-1, -1, &AtomicBool::new(true), &wake, || panic!("Closed source cannot read")).unwrap();
    assert_eq!(wake.observed(), 0);
}

#[test]
fn cancellation_interrupts_the_blocking_wait_without_a_timer() {
    let (mut cancel, stopped) = UnixStream::pair().unwrap();
    let (monitor, _sender) = UnixStream::pair().unwrap();
    let worker = thread::spawn(move || run(monitor.as_raw_fd(), stopped.as_raw_fd(),
        &AtomicBool::new(false), &NativeWake::default(), || panic!("Cancellation cannot receive a device")));
    cancel.write_all(&[1]).unwrap();
    worker.join().unwrap().unwrap();
}

#[test]
fn completed_events_coalesce_one_batch_and_receive_loss_is_explicit() {
    let (_cancel, stopped) = UnixStream::pair().unwrap();
    let (monitor, mut sender) = UnixStream::pair().unwrap(); sender.write_all(&[1]).unwrap();
    let closed = AtomicBool::new(false); let count = AtomicUsize::new(0); let wake = NativeWake::default();
    run(monitor.as_raw_fd(), stopped.as_raw_fd(), &closed, &wake, || {
        if count.fetch_add(1, Ordering::AcqRel) + 1 == MAX_BATCH { closed.store(true, Ordering::Release); }
        Ok(true)
    }).unwrap();
    assert_eq!(count.load(Ordering::Acquire), MAX_BATCH);
    assert_eq!(wake.observed(), 1);
    let error = run(monitor.as_raw_fd(), stopped.as_raw_fd(), &AtomicBool::new(false), &wake,
        || Err(io::Error::from_raw_os_error(105))).unwrap_err();
    assert!(error.contains("lost"));
}
