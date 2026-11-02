use super::*;
use crate::dcs_bios::IntegerOutput;
use serde_json::json;
use std::{
    net::{Ipv4Addr, UdpSocket},
    time::Duration,
};

fn peer() -> UdpSocket {
    UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap()
}
fn network(port: u16) -> Network {
    Network {
        command_host: Ipv4Addr::LOCALHOST,
        command_port: 7778,
        listen_address: Ipv4Addr::LOCALHOST,
        listen_port: port,
        multicast_group: None,
        multicast_interface: Ipv4Addr::LOCALHOST,
        freshness_ms: 100,
    }
}
fn spec(ttl: u64) -> Spec {
    Spec {
        counter: IntegerOutput::parse(&json!({"type":"integer","address":65534,
            "mask":255,"shift_by":0,"max_value":255}))
        .unwrap(),
        aircraft_address: 0,
        aircraft_length: 8,
        freshness_ms: ttl,
    }
}
fn fixed() -> Network {
    let reserve = peer();
    network(reserve.local_addr().unwrap().port())
}
fn wire(name: Option<&str>, value: Option<u16>, counter: u16) -> Vec<u8> {
    fn block(bytes: &mut Vec<u8>, at: u16, data: &[u8]) {
        bytes.extend(at.to_le_bytes());
        bytes.extend((data.len() as u16).to_le_bytes());
        bytes.extend(data);
    }
    let mut bytes = vec![0x55; 4];
    if let Some(name) = name {
        let mut header = [b' '; 8];
        header[..name.len()].copy_from_slice(name.as_bytes());
        block(&mut bytes, 0, &header);
    }
    if let Some(value) = value {
        block(&mut bytes, 8192, &value.to_le_bytes());
    }
    block(&mut bytes, 65534, &counter.to_le_bytes());
    bytes
}
fn next(reader: &mut NativeSubscription<Snapshot, Snapshot>) -> Delivery<Snapshot, Snapshot> {
    reader
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .expect("native publication")
}
fn until(
    reader: &mut NativeSubscription<Snapshot, Snapshot>,
    predicate: impl Fn(&Snapshot) -> bool,
) -> Snapshot {
    loop {
        let snapshot = match next(reader) {
            Delivery::Snapshot(snapshot) => snapshot.value,
            Delivery::Event(event) => event.value,
            Delivery::Loss(loss) => {
                assert_eq!(loss.reason, LossReason::GenerationReplaced);
                continue;
            }
        };
        if predicate(&snapshot) {
            return snapshot;
        }
    }
}
fn send(
    source: &Source,
    reader: &mut NativeSubscription<Snapshot, Snapshot>,
    name: Option<&str>,
    value: Option<u16>,
    counter: u16,
) -> Snapshot {
    peer()
        .send_to(&wire(name, value, counter), source.local_addr().unwrap())
        .unwrap();
    until(reader, |snapshot| snapshot.counter == Some(counter))
}
fn live(source: &Source, reader: &mut NativeSubscription<Snapshot, Snapshot>) -> Snapshot {
    send(source, reader, Some("Jet"), Some(11), 1);
    let snapshot = send(source, reader, None, None, 2);
    assert_eq!(snapshot.status, "live");
    snapshot
}
fn wait_processed(source: &Source, head: u64) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while source.owner.shared.lock().processed_head < head {
        assert!(Instant::now() < deadline, "decoder commit deadline");
        thread::yield_now();
    }
}

#[test]
fn fixed_clients_share_one_autonomous_decoder_and_native_memory() {
    let config = fixed();
    let a = Source::open(&config, spec(1000)).unwrap();
    let mut routed = config.clone();
    routed.command_port = 7788;
    let b = Source::open(&routed, spec(1000)).unwrap();
    assert!(Arc::ptr_eq(&a.owner, &b.owner));
    let mut observer = a.subscribe().unwrap();
    let initial = until(&mut observer, |_| true);
    let current = live(&a, &mut observer);
    // Decoder progress occurred without any Source snapshot consumer pumping it.
    let left = a.snapshot().unwrap();
    let right = b.snapshot().unwrap();
    assert_eq!(left.producer_id, right.producer_id);
    assert_eq!(left.counter, Some(2));
    assert!(Arc::ptr_eq(&left.memory, &right.memory));
    assert!(Arc::ptr_eq(&left.memory, &current.memory));
    assert_ne!(left.epoch, initial.epoch);
    let c = Source::open(&config, spec(1500)).unwrap();
    assert!(!Arc::ptr_eq(&a.owner, &c.owner));
    let zero_a = Source::open(&network(0), spec(1000)).unwrap();
    let zero_b = Source::open(&network(0), spec(1000)).unwrap();
    assert_ne!(zero_a.local_addr().unwrap(), zero_b.local_addr().unwrap());
    assert_ne!(
        zero_a.snapshot().unwrap().producer_id,
        zero_b.snapshot().unwrap().producer_id
    );
}

#[test]
fn expiry_publishes_without_reads_and_duplicate_does_not_renew_capture() {
    let source = Source::open(&network(0), spec(100)).unwrap();
    let mut observer = source.subscribe().unwrap();
    let current = live(&source, &mut observer);
    peer()
        .send_to(&wire(None, Some(99), 2), source.local_addr().unwrap())
        .unwrap();
    wait_processed(&source, 3);
    let duplicate = source.snapshot().unwrap();
    assert_eq!(duplicate.captured_at, current.captured_at);
    assert!(Arc::ptr_eq(&duplicate.memory, &current.memory));
    let stale = until(&mut observer, |snapshot| snapshot.status == "stale");
    assert!(stale.epoch > current.epoch);
    assert!(stale.memory.valid.iter().all(|valid| !valid));
    assert!(source.send_control("TEST", "1").is_err());
    assert_eq!(source.snapshot().unwrap().status, "stale");
}

#[test]
fn subscriber_overflow_revokes_command_before_polling_and_healthy_client_routes_commands() {
    let destination_a = peer();
    let destination_b = peer();
    destination_a
        .set_read_timeout(Some(Duration::from_millis(30)))
        .unwrap();
    destination_b
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut config = fixed();
    config.command_port = destination_a.local_addr().unwrap().port();
    let slow = Source::open(&config, spec(1000)).unwrap();
    config.command_port = destination_b.local_addr().unwrap().port();
    let healthy = Source::open(&config, spec(1000)).unwrap();
    let mut observer = healthy.subscribe().unwrap();
    live(&healthy, &mut observer);
    slow.snapshot().unwrap();
    healthy.snapshot().unwrap();
    for counter in 3..=10 {
        send(&healthy, &mut observer, None, Some(counter), counter);
        healthy.snapshot().unwrap();
    }
    assert!(slow.send_control("TEST", "1").is_err());
    assert!(healthy.send_control_for_aircraft("TEST", "999", Some("Other")).unwrap_err().contains("aircraft changed"));
    healthy.send_control("TEST", "2").unwrap();
    let mut bytes = [0; 128];
    let count = destination_b.recv(&mut bytes).unwrap();
    assert_eq!(&bytes[..count], b"TEST 2\n");
    assert!(destination_a.recv(&mut bytes).is_err());
    assert!(slow.snapshot().unwrap_err().contains("Overflow"));
    assert!(slow.snapshot().is_err());
    send(&healthy, &mut observer, Some("Other"), Some(9), 11);
    // Native replacement baseline has not yet been accepted by this client.
    assert!(healthy.send_control("TEST", "3").is_err());
    healthy.snapshot().unwrap();
    send(&healthy, &mut observer, None, Some(10), 12);
    healthy.snapshot().unwrap();
    healthy.send_control("TEST", "4").unwrap();
    let count = destination_b.recv(&mut bytes).unwrap();
    assert_eq!(&bytes[..count], b"TEST 4\n");
}

#[test]
fn terminal_source_replaces_owner_and_old_clients_remain_failed() {
    let config = fixed();
    let old = Source::open(&config, spec(1000)).unwrap();
    let address = old.local_addr().unwrap();
    let old_id = old.snapshot().unwrap().producer_id;
    let mut observer = old.subscribe().unwrap();
    until(&mut observer, |_| true);
    old.owner.transport.upgrade().unwrap().interrupt_capture();
    match next(&mut observer) {
        Delivery::Loss(loss) => assert_eq!(loss.reason, LossReason::Closed),
        _ => panic!("terminal loss expected"),
    }
    let fresh = Source::open(&config, spec(1000)).unwrap();
    assert_eq!(fresh.local_addr().unwrap(), address);
    assert_ne!(fresh.snapshot().unwrap().producer_id, old_id);
    assert!(old.snapshot().is_err());
    assert!(old.send_control("TEST", "1").is_err());
    let mut current = fresh.subscribe().unwrap();
    live(&fresh, &mut current);
    assert_eq!(fresh.snapshot().unwrap().status, "live");
}

#[test]
fn passive_observer_cannot_retain_socket_and_last_drop_is_prompt() {
    let source = Source::open(&network(0), spec(1000)).unwrap();
    let address = source.local_addr().unwrap();
    let mut observer = source.subscribe().unwrap();
    until(&mut observer, |_| true);
    let owner = Arc::downgrade(&source.owner);
    let started = Instant::now();
    drop(source);
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(owner.upgrade().is_none());
    let rebound = UdpSocket::bind(address).unwrap();
    assert_eq!(rebound.local_addr().unwrap(), address);
    match next(&mut observer) {
        Delivery::Loss(loss) => assert_eq!(loss.reason, LossReason::Closed),
        _ => panic!("closed observer expected"),
    }
}

#[test]
fn idle_decode_wait_has_no_periodic_wakeups() {
    let source = Source::open(&network(0), spec(1000)).unwrap();
    let started = Instant::now();
    while source.owner.shared.waits.load(Ordering::Relaxed) == 0 {
        assert!(started.elapsed() < Duration::from_secs(1));
        thread::yield_now();
    }
    thread::sleep(Duration::from_millis(60));
    assert_eq!(source.owner.shared.waits.load(Ordering::Relaxed), 1);
    source.snapshot().unwrap();
    assert_eq!(source.owner.shared.waits.load(Ordering::Relaxed), 1);
}

#[test]
fn oversized_decoded_batch_retires_without_publishing_any_partial_values() {
    let source = Source::open(&network(0), spec(1000)).unwrap();
    let mut observer = source.subscribe().unwrap();
    live(&source, &mut observer);
    source.snapshot().unwrap();
    let packet: Vec<u8> = (3..=19).flat_map(|counter| wire(None, Some(counter), counter)).collect();
    peer().send_to(&packet, source.local_addr().unwrap()).unwrap();
    match next(&mut observer) {
        Delivery::Loss(loss) => assert_eq!(loss.reason, LossReason::Closed),
        _ => panic!("An oversized atomic batch cannot publish a valid-looking prefix."),
    }
    assert!(source.snapshot().unwrap_err().contains("16-frame bound"));
    assert!(source.send_control("TEST", "1").is_err());
    assert!(source.owner.shared.lock().snapshot.memory.valid.iter().all(|valid| !valid));
}

#[test]
fn acquired_uncommitted_reports_block_commands_and_frozen_duplicates_advance_only_processed_head() {
    fn pause_commit(
        source: &Source,
    ) -> (std::sync::mpsc::Receiver<()>, std::sync::mpsc::Sender<()>) {
        let (arrived, arrival) = std::sync::mpsc::channel();
        let (resume, resumed) = std::sync::mpsc::channel();
        *source.owner.shared.before_commit.lock().unwrap() = Some((arrived, resumed));
        (arrival, resume)
    }
    let destination = peer();
    destination
        .set_read_timeout(Some(Duration::from_millis(30)))
        .unwrap();
    let mut config = network(0);
    config.command_port = destination.local_addr().unwrap().port();
    let source = Source::open(&config, spec(1000)).unwrap();
    let mut observer = source.subscribe().unwrap();
    live(&source, &mut observer);
    source.snapshot().unwrap();
    let (arrived, resume) = pause_commit(&source);
    peer()
        .send_to(&wire(None, Some(12), 3), source.local_addr().unwrap())
        .unwrap();
    arrived.recv_timeout(Duration::from_secs(2)).unwrap();
    // The raw report is already dequeued and parsed, but still uncommitted.
    // Reads remain passive and command dispatch cannot trust the old cache.
    assert_eq!(source.snapshot().unwrap().counter, Some(2));
    assert!(source
        .send_control("TEST", "1")
        .unwrap_err()
        .contains("pending native data"));
    let mut bytes = [0; 128];
    assert!(destination.recv(&mut bytes).is_err());
    resume.send(()).unwrap();
    until(&mut observer, |snapshot| snapshot.counter == Some(3));
    let accepted = source.snapshot().unwrap();
    source.send_control("TEST", "2").unwrap();
    let count = destination.recv(&mut bytes).unwrap();
    assert_eq!(&bytes[..count], b"TEST 2\n");

    let (arrived, resume) = pause_commit(&source);
    peer()
        .send_to(
            &wire(Some("Other"), Some(99), 3),
            source.local_addr().unwrap(),
        )
        .unwrap();
    arrived.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(source
        .send_control("TEST", "3")
        .unwrap_err()
        .contains("pending native data"));
    assert!(destination.recv(&mut bytes).is_err());
    resume.send(()).unwrap();
    wait_processed(&source, 4);
    // A duplicate has no journal publication and no accepted capture renewal,
    // yet its completed raw cursor must permit a later healthy submission.
    assert!(observer.try_recv().unwrap().is_none());
    let duplicate = source.snapshot().unwrap();
    assert_eq!(duplicate.capture_sequence, accepted.capture_sequence);
    assert_eq!(duplicate.captured_at, accepted.captured_at);
    assert_eq!(duplicate.deadline(), accepted.deadline());
    assert!(Arc::ptr_eq(&duplicate.memory, &accepted.memory));
    source.send_control("TEST", "4").unwrap();
    let count = destination.recv(&mut bytes).unwrap();
    assert_eq!(&bytes[..count], b"TEST 4\n");
}
