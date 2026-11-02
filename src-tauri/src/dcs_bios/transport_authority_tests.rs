use super::*;

#[test]
fn loss_revokes_command_authority_before_reader_polls_but_healthy_sibling_can_send() {
    let destination = peer();
    destination
        .set_read_timeout(Some(Duration::from_millis(40)))
        .unwrap();
    let (mut slow, mut healthy) = pair();
    slow.command = match destination.local_addr().unwrap() {
        SocketAddr::V4(at) => at,
        _ => unreachable!(),
    };
    healthy.command = slow.command;
    for _ in 0..=MAX_PENDING_FRAMES {
        slow.receiver.shared.capture(b"x", Instant::now()).unwrap();
        healthy.receive().unwrap().unwrap();
    }
    // No receive call on slow has exposed its loss yet.
    assert_eq!(slow.send("UFC_MASTER_CAUTION 1\n").unwrap_err(), OVERFLOW);
    assert_eq!(slow.send_control("TEST", "1").unwrap_err(), OVERFLOW);
    healthy.send_control("TEST", "2").unwrap();
    let mut bytes = [0; 128];
    let count = destination.recv(&mut bytes).unwrap();
    assert_eq!(&bytes[..count], b"TEST 2\n");
    assert!(destination.recv(&mut bytes).is_err());
    slow.receiver.shared.fail("terminal loss".into());
    assert_eq!(
        healthy.send_control("TEST", "3").unwrap_err(),
        "terminal loss"
    );
    assert_eq!(slow.send_control("TEST", "4").unwrap_err(), "terminal loss");
    assert!(destination.recv(&mut bytes).is_err());
}

#[test]
fn failed_fixed_endpoint_reopens_with_new_producer_while_old_clients_remain_failed() {
    let destination = peer();
    destination
        .set_read_timeout(Some(Duration::from_millis(40)))
        .unwrap();
    let (mut old, mut sibling) = pair();
    let address = old.local_addr().unwrap();
    let mut config = network(address.port());
    config.command_port = destination.local_addr().unwrap().port();
    old.command = SocketAddrV4::new(config.command_host, config.command_port);
    sibling.command = old.command;
    let old_producer = old.receiver.shared.producer_id.clone();
    old.receiver.shared.fail("endpoint failed".into());
    assert!(old.receiver.stop.is_cancelled());
    let started = Instant::now();
    let fresh = Transport::open_network(&config).unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(fresh.local_addr().unwrap(), address);
    assert_ne!(fresh.receiver.shared.producer_id, old_producer);
    assert!(!Arc::ptr_eq(&old.receiver, &fresh.receiver));
    assert!(old.receiver.worker.lock().unwrap().is_none());
    peer().send_to(b"new generation", address).unwrap();
    let report = fresh
        .wait_captured(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(report.bytes.as_ref(), b"new generation");
    assert_eq!(report.sequence, 1);
    assert_eq!(old.receive().unwrap_err(), "endpoint failed");
    assert_eq!(sibling.receive().unwrap_err(), "endpoint failed");
    assert_eq!(
        old.send_control("TEST", "1").unwrap_err(),
        "endpoint failed"
    );
    assert_eq!(
        sibling.send_control("TEST", "2").unwrap_err(),
        "endpoint failed"
    );
    fresh.send_control("TEST", "3").unwrap();
    let mut bytes = [0; 128];
    let count = destination.recv(&mut bytes).unwrap();
    assert_eq!(&bytes[..count], b"TEST 3\n");
    assert!(destination.recv(&mut bytes).is_err());
}

#[test]
fn first_subscription_captures_packet_queued_before_worker_starts() {
    let (owner, subscriber) = Owner::start(&network(0), |address| {
        // This executes after bind and initial subscription, before thread spawn.
        peer().send_to(b"startup", address).unwrap();
    })
    .unwrap();
    let transport = Transport {
        receiver: Arc::new(owner),
        subscriber,
        sender: peer(),
        command: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 7778),
    };
    let report = transport
        .wait_captured(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(report.bytes.as_ref(), b"startup");
    assert_eq!(report.sequence, 1);
}

#[test]
fn overlapping_live_policies_are_rejected_and_terminal_conflicts_are_released() {
    let reserve = peer();
    let port = reserve.local_addr().unwrap().port();
    drop(reserve);
    let mut wildcard = network(port);
    wildcard.listen_address = Ipv4Addr::UNSPECIFIED;
    let original = Transport::open_network(&wildcard).unwrap();
    let mut concrete = network(port);
    concrete.command_host = Ipv4Addr::new(127, 0, 0, 2);
    let rejected = Transport::open_network(&concrete);
    assert!(rejected
        .err()
        .unwrap()
        .contains("different acquisition policy"));
    let same_policy = Transport::open_network(&wildcard).unwrap();
    assert!(Arc::ptr_eq(&original.receiver, &same_policy.receiver));
    let mut other_source = wildcard.clone();
    other_source.command_host = concrete.command_host;
    assert!(Transport::open_network(&other_source)
        .err()
        .unwrap()
        .contains("different acquisition policy"));
    original.receiver.shared.fail("retire wildcard".into());
    let fresh = Transport::open_network(&concrete).unwrap();
    assert_eq!(
        fresh.local_addr().unwrap(),
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
    );
    assert!(original.receiver.worker.lock().unwrap().is_none());
    assert_ne!(
        original.receiver.shared.producer_id,
        fresh.receiver.shared.producer_id
    );
    assert_eq!(original.receive().unwrap_err(), "retire wildcard");
    let export = UdpSocket::bind((concrete.command_host, 0)).unwrap();
    export
        .send_to(b"replacement", fresh.local_addr().unwrap())
        .unwrap();
    assert_eq!(
        fresh
            .wait_captured(Duration::from_secs(2))
            .unwrap()
            .unwrap()
            .bytes
            .as_ref(),
        b"replacement"
    );
}

#[test]
fn disjoint_concrete_bind_addresses_can_acquire_independently_on_same_port() {
    let reserve = peer();
    let port = reserve.local_addr().unwrap().port();
    drop(reserve);
    let first = Transport::open_network(&network(port)).unwrap();
    let mut other = network(port);
    other.listen_address = Ipv4Addr::new(127, 0, 0, 2);
    let second = Transport::open_network(&other).unwrap();
    assert!(!Arc::ptr_eq(&first.receiver, &second.receiver));
    let export = peer();
    export.send_to(b"one", first.local_addr().unwrap()).unwrap();
    export
        .send_to(b"two", second.local_addr().unwrap())
        .unwrap();
    assert_eq!(
        first
            .wait_captured(Duration::from_secs(2))
            .unwrap()
            .unwrap()
            .bytes
            .as_ref(),
        b"one"
    );
    assert_eq!(
        second
            .wait_captured(Duration::from_secs(2))
            .unwrap()
            .unwrap()
            .bytes
            .as_ref(),
        b"two"
    );
}
