use super::*;

#[path = "transport_authority_tests.rs"]
mod authority;

fn network(port: u16) -> Network {
    Network {
        command_host: Ipv4Addr::LOCALHOST,
        command_port: 7778,
        listen_address: Ipv4Addr::LOCALHOST,
        listen_port: port,
        multicast_group: None,
        multicast_interface: Ipv4Addr::LOCALHOST,
        freshness_ms: 1500,
    }
}
fn peer() -> UdpSocket {
    UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap()
}
fn pair() -> (Transport, Transport) {
    let reserve = peer();
    let port = reserve.local_addr().unwrap().port();
    drop(reserve);
    let first = Transport::open_network(&network(port)).unwrap();
    let mut config = network(port);
    config.freshness_ms = 500;
    config.command_port = 7788;
    let second = Transport::open_network(&config).unwrap();
    assert!(Arc::ptr_eq(&first.receiver, &second.receiver));
    (first, second)
}
fn wait_sequence(transport: &Transport, sequence: u64) {
    let shared = &transport.receiver.shared;
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut state = shared.lock();
    while state.sequence < sequence && state.terminal.is_none() {
        let remaining = deadline.checked_duration_since(Instant::now()).unwrap();
        (state, _) = shared.changed.wait_timeout(state, remaining).unwrap();
    }
    assert_eq!(state.sequence, sequence);
}

#[test]
fn owned_capture_precedes_reads_and_preserves_order_time_and_independent_queues() {
    let (first, second) = pair();
    let peer = peer();
    for bytes in [b"first".as_slice(), b"second".as_slice()] {
        peer.send_to(bytes, first.local_addr().unwrap()).unwrap();
    }
    // Observe acquisition without calling either passive read API.
    wait_sequence(&first, 2);
    let acquired_before = Instant::now();
    std::thread::sleep(Duration::from_millis(20));
    for (sequence, bytes) in [(1, b"first".as_slice()), (2, b"second".as_slice())] {
        let left = first.receive_captured().unwrap().unwrap();
        let right = second.wait_captured(Duration::ZERO).unwrap().unwrap();
        assert_eq!(left.bytes.as_ref(), bytes);
        assert_eq!(right.bytes.as_ref(), bytes);
        assert_eq!(left.sequence, sequence);
        assert_eq!(right.sequence, sequence);
        assert_eq!(left.producer_id, right.producer_id);
        assert_eq!(left.captured_at, right.captured_at);
        assert!(left.captured_at <= acquired_before);
        assert!(left.captured_at.elapsed() >= Duration::from_millis(20));
    }
    assert!(first.receive().unwrap().is_none());
    assert!(second.receive().unwrap().is_none());
}

#[test]
fn no_replay_for_late_subscription_and_ephemeral_requests_are_independent() {
    let (first, second) = pair();
    peer().send_to(b"old", first.local_addr().unwrap()).unwrap();
    wait_sequence(&first, 1);
    let late = Transport::open_network(&network(first.local_addr().unwrap().port())).unwrap();
    assert!(late.receive().unwrap().is_none());
    assert_eq!(first.receive().unwrap().unwrap(), b"old");
    assert_eq!(second.receive().unwrap().unwrap(), b"old");
    let ephemeral_a = Transport::open_network(&network(0)).unwrap();
    let ephemeral_b = Transport::open_network(&network(0)).unwrap();
    assert_ne!(
        ephemeral_a.local_addr().unwrap(),
        ephemeral_b.local_addr().unwrap()
    );
    assert!(!Arc::ptr_eq(&ephemeral_a.receiver, &ephemeral_b.receiver));
}

#[test]
fn last_owner_drop_cancels_idle_receive_and_releases_socket_promptly() {
    let (first, second) = pair();
    let address = first.local_addr().unwrap();
    let weak = Arc::downgrade(&first.receiver);
    drop(first);
    assert!(weak.upgrade().is_some());
    let started = Instant::now();
    drop(second);
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(weak.upgrade().is_none());
    let rebound = UdpSocket::bind(address).unwrap();
    assert_eq!(rebound.local_addr().unwrap(), address);
}

#[test]
fn bounded_frame_queues_fail_permanently_without_affecting_active_reader() {
    let (slow, active) = pair();
    for index in 0..=MAX_PENDING_FRAMES {
        slow.receiver.shared.capture(b"x", Instant::now()).unwrap();
        assert_eq!(
            active.receive_captured().unwrap().unwrap().sequence,
            index as u64 + 1
        );
    }
    assert_eq!(slow.receive().unwrap_err(), OVERFLOW);
    assert_eq!(slow.receive().unwrap_err(), OVERFLOW);
    slow.receiver
        .shared
        .capture(b"after", Instant::now())
        .unwrap();
    assert_eq!(slow.receive().unwrap_err(), OVERFLOW);
    assert_eq!(active.receive().unwrap().unwrap(), b"after");
    let state = slow.receiver.shared.lock();
    assert!(state.subscribers[&slow.subscriber].is_err());
}

#[test]
fn bounded_bytes_overflow_before_frame_limit_and_pending_bytes_recover_on_read() {
    let transport = Transport::open_network(&network(0)).unwrap();
    let bytes = vec![42; 65_000];
    transport
        .receiver
        .shared
        .capture(&bytes, Instant::now())
        .unwrap();
    assert_eq!(transport.receive().unwrap().unwrap(), bytes);
    assert_eq!(
        transport.receiver.shared.lock().subscribers[&transport.subscriber]
            .as_ref()
            .unwrap()
            .bytes,
        0
    );
    for _ in 0..MAX_PENDING_BYTES / bytes.len() {
        transport
            .receiver
            .shared
            .capture(&bytes, Instant::now())
            .unwrap();
    }
    assert!(transport.receiver.shared.lock().subscribers[&transport.subscriber].is_ok());
    transport
        .receiver
        .shared
        .capture(&bytes, Instant::now())
        .unwrap();
    assert_eq!(
        transport.wait_captured(Duration::ZERO).unwrap_err(),
        OVERFLOW
    );
}

#[test]
fn subscriber_limit_and_counter_exhaustion_fail_closed() {
    let transport = Transport::open_network(&network(0)).unwrap();
    let shared = &transport.receiver.shared;
    for _ in 1..MAX_SUBSCRIBERS {
        shared.subscribe().unwrap();
    }
    assert!(shared.subscribe().unwrap_err().contains("limit"));
    let mut state = shared.lock();
    state.subscribers.clear();
    state.next_subscriber = u64::MAX;
    drop(state);
    assert!(shared
        .subscribe()
        .unwrap_err()
        .contains("counter exhausted"));
    assert!(shared
        .subscribe()
        .unwrap_err()
        .contains("counter exhausted"));
    assert!(shared.lock().subscribers.is_empty());
}

#[test]
fn capture_counter_exhaustion_withdraws_pending_data_and_stops_socket_owner() {
    let transport = Transport::open_network(&network(0)).unwrap();
    let address = transport.local_addr().unwrap();
    transport
        .receiver
        .shared
        .capture(b"pending", Instant::now())
        .unwrap();
    transport.receiver.shared.lock().sequence = u64::MAX;
    peer().send_to(b"overflow", address).unwrap();
    // Wait through the receiver event boundary, without pumping the socket.
    let shared = &transport.receiver.shared;
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut state = shared.lock();
    while state.terminal.is_none() {
        let remaining = deadline.checked_duration_since(Instant::now()).unwrap();
        (state, _) = shared.changed.wait_timeout(state, remaining).unwrap();
    }
    assert_eq!(state.sequence, u64::MAX);
    drop(state);
    assert!(transport
        .receive()
        .unwrap_err()
        .contains("counter exhausted"));
    // Join proves the owner exited and released its socket while the reader remains.
    // The worker does not retain the lifecycle owner in an Arc cycle.
    transport.receiver.shutdown();
    assert!(UdpSocket::bind(address).is_ok());
}

#[test]
fn idle_acquisition_waits_for_readiness_without_polling() {
    let transport = Transport::open_network(&network(0)).unwrap();
    let shared = &transport.receiver.shared;
    let mut state = shared.lock();
    let deadline = Instant::now() + Duration::from_secs(2);
    while shared
        .receive_waits
        .load(std::sync::atomic::Ordering::Relaxed)
        == 0
    {
        (state, _) = shared
            .changed
            .wait_timeout(
                state,
                deadline.checked_duration_since(Instant::now()).unwrap(),
            )
            .unwrap();
    }
    drop(state);
    assert!(transport
        .wait_captured(Duration::from_millis(40))
        .unwrap()
        .is_none());
    assert_eq!(
        shared
            .receive_waits
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    peer()
        .send_to(b"wake", transport.local_addr().unwrap())
        .unwrap();
    assert_eq!(
        transport
            .wait_captured(Duration::from_secs(2))
            .unwrap()
            .unwrap()
            .bytes
            .as_ref(),
        b"wake"
    );
    let mut state = shared.lock();
    while shared
        .receive_waits
        .load(std::sync::atomic::Ordering::Relaxed)
        < 2
    {
        (state, _) = shared
            .changed
            .wait_timeout(
                state,
                deadline.checked_duration_since(Instant::now()).unwrap(),
            )
            .unwrap();
    }
    drop(state);
    assert!(transport
        .wait_captured(Duration::from_millis(40))
        .unwrap()
        .is_none());
    assert_eq!(
        shared
            .receive_waits
            .load(std::sync::atomic::Ordering::Relaxed),
        2
    );
}

#[test]
fn terminal_failure_wakes_waiters_and_withdraws_pending_data() {
    let transport = Transport::open_network(&network(0)).unwrap();
    let shared = transport.receiver.shared.clone();
    std::thread::scope(|scope| {
        let waiter = scope.spawn(|| transport.wait_captured(Duration::from_secs(10)));
        shared.fail("injected receive failure".into());
        assert_eq!(
            waiter.join().unwrap().unwrap_err(),
            "injected receive failure"
        );
    });
    assert!(shared.capture(b"late", Instant::now()).is_err());
    assert_eq!(transport.receive().unwrap_err(), "injected receive failure");
    assert_eq!(shared.subscribe().unwrap_err(), "injected receive failure");
}

#[test]
fn source_filter_is_an_acquisition_property_and_commands_keep_their_destination() {
    let base = network(5000);
    let mut changed = base.clone();
    changed.freshness_ms = 100;
    changed.command_port = 7788;
    assert_eq!(acquisition_key(&base), acquisition_key(&changed));
    changed.command_host = Ipv4Addr::new(127, 0, 0, 2);
    assert_ne!(acquisition_key(&base), acquisition_key(&changed));
    changed = base.clone();
    changed.multicast_group = Some(Ipv4Addr::new(239, 255, 50, 10));
    assert_ne!(acquisition_key(&base), acquisition_key(&changed));

    let destination = peer();
    destination
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    changed.listen_port = 0;
    changed.command_port = destination.local_addr().unwrap().port();
    let transport = Transport::open_network(&changed).unwrap();
    transport.send_control("TEST", "INC").unwrap();
    let mut bytes = [0; 128];
    let count = destination.recv(&mut bytes).unwrap();
    assert_eq!(&bytes[..count], b"TEST INC\n");
    for (command, argument) in [
        ("", "1"),
        ("BAD\n", "1"),
        ("TEST", "65536"),
        ("TEST", "-65536"),
        ("TEST", "INC\n"),
        ("TEST", "1234567"),
    ] {
        assert!(transport.send_control(command, argument).is_err());
    }
    assert!(transport.send("UFC_MASTER_CAUTION TOGGLE\n").is_err());
}
