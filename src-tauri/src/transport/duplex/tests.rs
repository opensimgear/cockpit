use super::*;
use std::{io::{Read, Write}, sync::mpsc, time::Duration};

fn receive_exact(receiver: &mut dyn BlockingReceiver, bytes: &mut [u8]) {
    let mut offset = 0;
    while offset < bytes.len() {
        let count = receiver.receive(&mut bytes[offset..]).unwrap();assert!(count > 0);offset += count;
    }
}
fn cancellation(mut parts: DuplexParts) {
    parts.cancel.cancel();parts.cancel.cancel();
    assert!(parts.receiver.receive(&mut [0]).is_err());
    assert!(parts.writer.write_all(b"late", Instant::now() + Duration::from_secs(1)).is_err());
}
fn cancel_blocked(parts: DuplexParts) {
    let DuplexParts { mut receiver, writer, cancel } = parts;
    let (sent, received) = mpsc::channel();let (entered, ready) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        entered.send(()).unwrap();let _ = sent.send(receiver.receive(&mut [0]));
    });
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    cancel.cancel();
    let result = received.recv_timeout(Duration::from_secs(2));
    if result.is_ok() { thread.join().unwrap(); }
    assert!(result.expect("native receive did not wake on cancellation").is_err());
    drop(writer);
}
fn independent(mut parts: DuplexParts, mut peer: impl Read + Write + Send + 'static) {
    let (sent, received) = mpsc::channel();
    let mut receiver = parts.receiver;
    let reader = std::thread::spawn(move || {
        let mut bytes = [0; 3];receive_exact(receiver.as_mut(), &mut bytes);assert!(sent.send((bytes, receiver)).is_ok());
    });
    // RX is waiting for the peer, while the separate writer can complete TX.
    parts.writer.write_all(b"out", Instant::now() + Duration::from_secs(2)).unwrap();
    let mut outbound = [0; 3];peer.read_exact(&mut outbound).unwrap();assert_eq!(&outbound, b"out");
    peer.write_all(b"in!").unwrap();
    let result = received.recv_timeout(Duration::from_secs(2));parts.cancel.cancel();
    let (inbound, mut receiver) = result.unwrap();reader.join().unwrap();assert_eq!(&inbound, b"in!");
    assert!(receiver.receive(&mut [0]).is_err(), "cancellation between reads was lost");
}

#[test]
fn tcp_cancel_before_receive_and_blocked_receive_is_persistent() {
    let (parts, _peer) = sockets::tcp_fixture();cancellation(parts);
    let (parts, _peer) = sockets::tcp_fixture();cancel_blocked(parts);
}
#[test]
fn tcp_input_and_output_are_independent_and_cancel_between_reads() {
    let (parts, peer) = sockets::tcp_fixture();independent(parts, peer);
}
#[test]
fn socket_writes_use_the_actual_deadline_when_the_peer_stops_reading() {
    let (mut parts, _peer) = sockets::saturated_tcp_fixture();
    assert!(parts.writer.write_all(b"not sent", Instant::now() - Duration::from_millis(1)).is_err());
    let started = Instant::now();
    let result = parts.writer.write_all(&vec![0; 1024 * 1024], started + Duration::from_millis(40));
    parts.cancel.cancel();
    let error = result.expect_err("a saturated socket write ignored its deadline");
    assert!(error.contains("deadline"), "saturated write failed outside the deadline path: {error}");
    assert!(started.elapsed() < Duration::from_secs(2));
}
#[test]
fn unadvertised_local_endpoint_is_rejected_before_connect() {
    let port = PortInfo { path:"tcp:127.0.0.1:1/00112233445566778899aabb".into(),
        serial_number:Some("00112233445566778899aabb".into()), vendor_id:None, product_id:None,
        manufacturer:None, friendly_name:None };
    assert!(open(&port).is_err());
    let mut invalid=port;invalid.path="../not-a-serial-device".into();
    assert!(open(&invalid).is_err());
}
#[cfg(unix)]
#[test]
fn unix_sockets_cancel_without_read_timeouts_and_support_duplex() {
    let (parts, _peer) = sockets::unix_fixture();cancellation(parts);
    let (parts, _peer) = sockets::unix_fixture();cancel_blocked(parts);
    let (parts, peer) = sockets::unix_fixture();independent(parts, peer);
}
#[cfg(unix)]
#[test]
fn native_tty_readiness_wait_has_persistent_cancel_and_independent_writes() {
    let (parts, _peer) = unix::fixture();cancellation(parts);
    let (parts, _peer) = unix::fixture();cancel_blocked(parts);
    let (parts, peer) = unix::fixture();independent(parts, peer);
}
#[cfg(unix)]
#[test]
fn native_tty_blocked_writes_expire_without_waiting_for_rx() {
    let (mut parts, _peer) = unix::fixture();let started = Instant::now();
    assert!(parts.writer.write_all(&vec![0; 1024 * 1024], started + Duration::from_millis(40)).is_err());
    parts.cancel.cancel();assert!(started.elapsed() < Duration::from_secs(2));
}
