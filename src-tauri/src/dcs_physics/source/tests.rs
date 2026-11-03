use super::*;
use crate::dcs_physics::tests::packet;
use std::sync::Arc;

fn expired_view() -> SourceView {
    let mut session = Session::new();
    let clock = Instant::now() - Duration::from_millis(FRESHNESS_MS + 1);
    for (sequence, time) in [(1, 1.0), (2, 1.05)] {
        session
            .feed(packet(sequence, time).to_string().as_bytes(), clock, 0)
            .unwrap();
    }
    let snapshot = session.snapshot(0);
    let journal =
        Journal::new(snapshot.epoch, snapshot.clone(), 0, "test-clock", LIMITS).unwrap();
    SourceView {
        shared: Arc::new(Shared {
            state: Mutex::new(State {
                session,
                snapshot,
                journal,
            }),
            committed: Condvar::new(),
            clock,
            clock_id: "test-clock".into(),
        }),
    }
}

#[test]
fn late_subscription_waits_for_native_expiry_without_renewing_report_capture() {
    let view = expired_view();
    let waiting = view.clone();
    let (sent, received) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || sent.send(waiting.subscribe()).unwrap());
    assert!(matches!(
        received.recv_timeout(Duration::from_millis(20)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    {
        let mut state = view.shared.lock();
        state.session.expire();
        view.shared.commit(&mut state).unwrap();
    }
    let mut reader = received
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
    let osg_streams::Delivery::Snapshot(snapshot) = reader.try_recv().unwrap().unwrap() else {
        panic!("committed expired baseline expected");
    };
    assert!(!snapshot.value.fresh);
    assert_eq!(snapshot.value.sequence, Some(2));
    assert!(snapshot.value.values.is_empty());
    assert!(snapshot.captured_us >= (FRESHNESS_MS + 1) * 1000);
    assert!(snapshot.value.age_ms.unwrap() > FRESHNESS_MS);
}

#[test]
fn late_subscription_times_out_if_expiry_owner_cannot_run() {
    let view = expired_view();
    let started = Instant::now();
    assert!(matches!(view.subscribe(), Err(StreamError::InvalidTimeout)));
    assert!(started.elapsed() < Duration::from_secs(2));
    // Waiting did not publish or change the source's capture.
    assert!(view.shared.lock().snapshot.fresh);
}

#[test]
fn mutex_contention_does_not_move_the_packet_capture_instant_or_make_it_fresh() {
    let clock = Instant::now();
    let mut session = Session::new();
    session
        .feed(packet(1, 1.0).to_string().as_bytes(), clock, 0)
        .unwrap();
    let snapshot = session.snapshot(0);
    let journal =
        Journal::new(snapshot.epoch, snapshot.clone(), 0, "held-lock", LIMITS).unwrap();
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            session,
            snapshot,
            journal,
        }),
        committed: Condvar::new(),
        clock,
        clock_id: "held-lock".into(),
    });
    let held = shared.lock();
    let worker_shared = shared.clone();
    let (captured_tx, captured_rx) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        let captured_at = Instant::now();
        captured_tx.send(captured_at).unwrap();
        let capture_ms = worker_shared.elapsed_ms(captured_at);
        let mut state = worker_shared.lock();
        state
            .session
            .feed(
                packet(2, 1.05).to_string().as_bytes(),
                captured_at,
                capture_ms,
            )
            .unwrap();
        worker_shared.commit(&mut state).unwrap();
        captured_at
    });
    let captured_at = captured_rx.recv().unwrap();
    thread::sleep(Duration::from_millis(FRESHNESS_MS + 50));
    drop(held);
    assert_eq!(worker.join().unwrap(), captured_at);

    let view = SourceView { shared };
    let delayed = view.snapshot();
    assert_eq!(delayed.sequence, Some(2));
    assert_eq!(delayed.captured_at, Some(captured_at));
    assert!(delayed.age_ms.unwrap() > FRESHNESS_MS);
    assert!(!delayed.fresh);
    assert_eq!(delayed.status, "stale");
    assert!(delayed.values.is_empty());
}

fn wait_for_exit(source: &Source) {
    let until = Instant::now() + Duration::from_secs(3);
    while !source.is_finished() {
        assert!(
            Instant::now() < until,
            "publication failure must stop reception"
        );
        thread::sleep(Duration::from_millis(5));
    }
    let snapshot = source.snapshot();
    assert!(!snapshot.fresh);
    assert!(snapshot.values.is_empty());
    assert!(snapshot
        .error
        .as_deref()
        .unwrap()
        .contains("publication failed"));
    assert!(source.subscribe().is_err());
    UdpSocket::bind(source.local_addr()).expect("failed publisher must release the socket");
}

#[test]
fn failed_report_publication_releases_socket_without_dropping_source() {
    // Exercise both ordinary publish and generation replacement failure.
    for sequence in [2, 3] {
        let source = Source::bind_loopback_ephemeral().unwrap();
        let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let mut reader = source.subscribe().unwrap();
        reader.try_recv().unwrap();
        sender
            .send_to(packet(1, 1.0).to_string().as_bytes(), source.local_addr())
            .unwrap();
        reader
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
        source.view.shared.lock().journal.close();
        sender
            .send_to(
                packet(sequence, 1.05).to_string().as_bytes(),
                source.local_addr(),
            )
            .unwrap();
        wait_for_exit(&source);
    }
}

#[test]
fn failed_expiry_publication_releases_socket_without_dropping_source() {
    let source = Source::bind_loopback_ephemeral().unwrap();
    let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let mut reader = source.subscribe().unwrap();
    reader.try_recv().unwrap();
    sender
        .send_to(packet(1, 1.0).to_string().as_bytes(), source.local_addr())
        .unwrap();
    reader
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
        .unwrap();
    source.view.shared.lock().journal.close();
    wait_for_exit(&source);
}
