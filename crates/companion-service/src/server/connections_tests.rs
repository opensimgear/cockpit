use super::*;
use std::sync::{mpsc, Barrier};

fn pair() -> (DeadlineTcp, Arc<socket_io::Cancel>, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (stream, cancel) = DeadlineTcp::event(listener.accept().unwrap().0).unwrap();
    (stream, cancel, peer)
}

#[test]
fn idle_read_has_no_periodic_timeout() {
    let (mut stream, _cancel, mut peer) = pair();
    stream.upgraded();
    let (done, result) = mpsc::channel();
    let worker = thread::spawn(move || {
        let started = Instant::now();
        let mut byte = [0];
        let read = stream.read_exact(&mut byte);
        done.send((read, byte, started.elapsed())).unwrap();
    });
    thread::sleep(Duration::from_millis(180));
    peer.write_all(&[42]).unwrap();
    let (read, byte, elapsed) = result.recv_timeout(Duration::from_secs(1)).unwrap();
    read.unwrap();
    assert_eq!(byte, [42]);
    assert!(elapsed >= Duration::from_millis(150), "idle read returned early: {elapsed:?}");
    worker.join().unwrap();
}

#[test]
fn cancellation_interrupts_an_idle_read_with_terminal_error() {
    let (mut stream, cancel, _peer) = pair();
    stream.upgraded();
    let connections = Connections::default();
    connections.register("peer", cancel).unwrap();
    let (entered, ready) = mpsc::channel();
    let (done, result) = mpsc::channel();
    let worker = thread::spawn(move || {
        entered.send(()).unwrap();
        done.send(stream.read(&mut [0])).unwrap();
    });
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(result.recv_timeout(Duration::from_millis(40)), Err(mpsc::RecvTimeoutError::Timeout)));
    connections.close();
    let error = result.recv_timeout(Duration::from_secs(1)).unwrap().unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::ConnectionAborted);
    worker.join().unwrap();
}

#[test]
fn racing_registration_is_cancelled_and_cannot_affect_a_fresh_owner() {
    for _ in 0..32 {
        let (mut stream, cancel, _peer) = pair();
        stream.upgraded();
        let connections = Arc::new(Connections::default());
        let registry = connections.clone();
        let barrier = Arc::new(Barrier::new(2));
        let start = barrier.clone();
        let worker = thread::spawn(move || {
            start.wait();
            registry.register("peer", cancel)
        });
        barrier.wait();
        connections.close();
        let registration = worker.join().unwrap();
        if let Err(error) = registration {
            assert_eq!(error.kind(), std::io::ErrorKind::ConnectionAborted);
        }
        let error = stream.read(&mut [0]).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionAborted);
        let (mut late_stream, late_cancel, _late_peer) = pair();
        late_stream.upgraded();
        let error = connections.register("late", late_cancel).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionAborted);
        let error = late_stream.read(&mut [0]).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionAborted);
    }

    let (mut fresh_stream, fresh_cancel, mut fresh_peer) = pair();
    fresh_stream.upgraded();
    let fresh = Connections::default();
    fresh.register("peer", fresh_cancel).unwrap();
    fresh_peer.write_all(&[7]).unwrap();
    let mut byte = [0];
    fresh_stream.read_exact(&mut byte).unwrap();
    assert_eq!(byte, [7]);
    fresh.close();
}

#[test]
fn cancellation_controls_have_bounded_admission_and_unique_ids() {
    let connections = Connections::default();
    let mut owners = Vec::new();
    for id in 0..16 {
        let (stream, cancel, peer) = pair();
        connections.register(&id.to_string(), cancel.clone()).unwrap();
        owners.push((stream, cancel, peer));
    }
    let (_extra_stream, extra_cancel, _extra_peer) = pair();
    assert!(connections.register("extra", extra_cancel).is_err());
    let (_duplicate_stream, duplicate_cancel, _duplicate_peer) = pair();
    assert!(connections.register("0", duplicate_cancel).is_err());

    connections.remove("0");
    let (_replacement_stream, replacement_cancel, _replacement_peer) = pair();
    connections.register("replacement", replacement_cancel).unwrap();
    connections.close();
    assert!(connections.0.lock().unwrap().peers.is_empty());
    let error = owners[1].0.read(&mut [0]).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::ConnectionAborted);
}
