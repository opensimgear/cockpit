use super::*;

fn finished<T: Send + 'static>(worker: &mut Worker<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(result) = worker.poll() { return result.unwrap(); }
        assert!(Instant::now() < deadline, "Feedback worker did not finish");
        std::thread::yield_now();
    }
}

#[test]
fn blocked_worker_is_nonblocking_and_coalesces_all_new_snapshots() {
    let mut worker = Worker::default();
    let (entered, waiting) = mpsc::channel();
    let (release, held) = mpsc::channel();
    assert!(worker.start(move |cancelled| {
        entered.send(()).unwrap();
        held.recv_timeout(Duration::from_secs(2)).unwrap();
        !cancelled.load(Ordering::Acquire)
    }).unwrap());
    waiting.recv_timeout(Duration::from_secs(1)).unwrap();
    let began = Instant::now();
    assert!(worker.poll().is_none());
    for _ in 0..100 { assert!(!worker.start(|_| panic!("Busy worker spawned another thread")).unwrap()); }
    assert!(began.elapsed() < Duration::from_millis(100));
    worker.cancel();
    assert!(!worker.start(|_| true).unwrap(), "Cancellation freed a still-blocked worker slot");
    release.send(()).unwrap();
    assert!(!finished(&mut worker));
    assert!(worker.start(|_| true).unwrap());
    assert!(finished(&mut worker));
}

#[test]
fn dropping_the_adapter_worker_revokes_its_inflight_job() {
    let mut worker = Worker::default();
    let (entered, waiting) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let (observed, result) = mpsc::channel();
    worker.start(move |cancelled| {
        entered.send(()).unwrap();
        held.recv_timeout(Duration::from_secs(2)).unwrap();
        observed.send(cancelled.load(Ordering::Acquire)).unwrap();
    }).unwrap();
    waiting.recv_timeout(Duration::from_secs(1)).unwrap();
    drop(worker);
    release.send(()).unwrap();
    assert!(result.recv_timeout(Duration::from_secs(1)).unwrap());
}

#[test]
fn disconnected_worker_reports_a_failure_and_releases_the_slot() {
    let mut worker: Worker<()> = Worker::default();
    worker.start(|_| panic!("Injected feedback worker panic")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(result) = worker.poll() {
            assert!(result.unwrap_err().contains("ended"));
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(worker.start(|_| ()).unwrap());
    finished(&mut worker);
}
