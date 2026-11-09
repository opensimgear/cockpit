use super::*;
use crate::dcs_bios::IntegerOutput;
use std::{
    net::{Ipv4Addr, UdpSocket},
    sync::{atomic::AtomicUsize, mpsc},
    time::Duration,
};

fn socket() -> UdpSocket {
    UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap()
}

fn send_packet(sender: &UdpSocket, source: &Source, aircraft: &[u8; 8], counter: u16) {
    let mut packet = vec![0x55; 4];
    packet.extend([0, 0, 8, 0]);
    packet.extend(aircraft);
    packet.extend([254, 255, 2, 0]);
    packet.extend(counter.to_le_bytes());
    sender.send_to(&packet, source.local_addr().unwrap()).unwrap();
}

fn fixture() -> (Source, UdpSocket, UdpSocket) {
    let sender = socket();
    let destination = socket();
    destination.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    let network = Network {
        command_host: Ipv4Addr::LOCALHOST,
        command_port: destination.local_addr().unwrap().port(),
        listen_address: Ipv4Addr::LOCALHOST,
        listen_port: 0,
        multicast_group: None,
        multicast_interface: Ipv4Addr::LOCALHOST,
        freshness_ms: 100,
    };
    let spec = Spec {
        counter: IntegerOutput::parse(&serde_json::json!({"type":"integer","address":65534,
            "mask":255,"shift_by":0,"max_value":255})).unwrap(),
        aircraft_address: 0,
        aircraft_length: 8,
        freshness_ms: 2000,
    };
    let source = Source::open(&network, spec).unwrap();
    let mut observer = source.subscribe().unwrap();
    for counter in [1, 2] {
        send_packet(&sender, &source, b"Jet     ", counter);
        loop {
            let snapshot = match observer.recv_timeout(Duration::from_secs(2)).unwrap().unwrap() {
                Delivery::Snapshot(snapshot) => snapshot.value,
                Delivery::Event(event) => event.value,
                Delivery::Loss(loss) => {
                    assert_eq!(loss.reason, LossReason::GenerationReplaced);
                    continue;
                }
            };
            if snapshot.counter == Some(counter) { break; }
        }
    }
    assert!(source.snapshot().unwrap().fresh_at(Instant::now()));
    (source, sender, destination)
}

fn hold_commit(source: &Source, sender: &UdpSocket, aircraft: &[u8; 8], counter: u16) -> mpsc::Sender<()> {
    let (arrived, arrival) = mpsc::channel();
    let (resume, resumed) = mpsc::channel();
    *source.owner.shared.before_commit.lock().unwrap() = Some((arrived, resumed));
    send_packet(sender, source, aircraft, counter);
    arrival.recv_timeout(Duration::from_secs(2)).unwrap();
    resume
}

fn wait_for_command(source: &Source) {
    let limit = Instant::now() + Duration::from_secs(1);
    while source.owner.shared.command_waits.load(Ordering::Relaxed) == 0 {
        assert!(Instant::now() < limit, "Command did not wait for the decoder commit");
        thread::yield_now();
    }
}

#[test]
fn queued_command_wakes_on_a_duplicate_raw_commit_and_revalidates_once_before_send() {
    let (source, sender, destination) = fixture();
    let resume = hold_commit(&source, &sender, b"Jet     ", 2);
    let command = source.command_sender();
    let worker = thread::spawn(move || {
        let mut validations = 0;
        let admissions = AtomicUsize::new(0);
        command.send::<String>("TEST", "1", Some("Jet"), Instant::now() + Duration::from_secs(2),
            || { validations += 1; Ok(()) },
            || { admissions.fetch_add(1, Ordering::Relaxed); Ok(()) }).unwrap();
        (validations, admissions.load(Ordering::Relaxed))
    });
    wait_for_command(&source);
    let mut bytes = [0; 64];
    assert!(destination.recv(&mut bytes).is_err());
    resume.send(()).unwrap();
    assert_eq!(worker.join().unwrap(), (2, 1));
    let count = destination.recv(&mut bytes).unwrap();
    assert_eq!(&bytes[..count], b"TEST 1\n");
    assert!(destination.recv(&mut bytes).is_err());
}

#[test]
fn queued_command_cannot_send_after_deadline_while_the_decoder_is_paused() {
    let (source, sender, destination) = fixture();
    let resume = hold_commit(&source, &sender, b"Jet     ", 3);
    let error = source.command_sender().send::<String>("TEST", "1", Some("Jet"),
        Instant::now() + Duration::from_millis(30), || Ok(()), || panic!("Expired command reached send admission")).unwrap_err();
    assert!(error.contains("expired before dispatch"));
    resume.send(()).unwrap();
    assert!(destination.recv(&mut [0; 64]).is_err());
}

#[test]
fn queued_command_rechecks_aircraft_after_the_pending_report_commits() {
    let (source, sender, destination) = fixture();
    let resume = hold_commit(&source, &sender, b"Other   ", 3);
    let command = source.command_sender();
    let worker = thread::spawn(move || command.send::<String>("TEST", "1", Some("Jet"),
        Instant::now() + Duration::from_secs(2), || Ok(()), || panic!("Old aircraft reached send admission")));
    wait_for_command(&source);
    resume.send(()).unwrap();
    assert!(worker.join().unwrap().unwrap_err().contains("DCS aircraft changed"));
    assert!(destination.recv(&mut [0; 64]).is_err());
}
