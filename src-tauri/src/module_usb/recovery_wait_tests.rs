use super::*;

#[test]
fn recovery_wait_preserves_the_deadline_and_checks_shutdown_even_at_zero() {
    let cancel = Arc::new(AtomicBool::new(false));
    let worker = Worker::new(Box::new(FakeFactory(Arc::default())), timing(), cancel.clone());
    worker.wake.bind_thread(thread::current()).unwrap();
    worker.wake.notify();
    let duration = Duration::from_millis(30);
    let start = Instant::now();
    assert_eq!(worker.wait(duration), Ok(()));
    assert!(start.elapsed() >= duration);

    cancel.store(true, Ordering::Release);
    assert_eq!(worker.wait(Duration::ZERO), Err(CLOSED.into()));
}

#[test]
fn shutdown_wakes_recovery_before_the_hardware_deadline() {
    let cancel = Arc::new(AtomicBool::new(false));
    let worker = Worker::new(Box::new(FakeFactory(Arc::default())), timing(), cancel.clone());
    let wake = worker.wake.clone();
    let (ready, started) = mpsc::channel();
    let (finish, result) = mpsc::channel();
    let thread = thread::spawn(move || {
        worker.wake.bind_thread(thread::current()).unwrap();
        ready.send(()).unwrap();
        let outcome = worker.wait(Duration::from_secs(5));
        finish.send(outcome).unwrap();
    });

    started.recv_timeout(Duration::from_secs(1)).unwrap();
    wake.notify();
    let unrelated = result.recv_timeout(Duration::from_millis(50));
    cancel.store(true, Ordering::Release);
    wake.notify();
    assert_eq!(unrelated, Err(mpsc::RecvTimeoutError::Timeout));
    assert_eq!(result.recv_timeout(Duration::from_secs(1)).unwrap(), Err(CLOSED.into()));
    thread.join().unwrap();
}
