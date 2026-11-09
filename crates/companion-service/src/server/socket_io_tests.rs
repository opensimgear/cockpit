use super::*;
use std::sync::mpsc;

fn pair() -> (SocketIo, Arc<Cancel>, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (io, cancel) = SocketIo::new(listener.accept().unwrap().0).unwrap();
    (io, cancel, peer)
}

#[test]
fn cancellation_before_io_is_terminal_instead_of_retried_interruption() {
    let (mut io, cancel, _peer) = pair();
    cancel.cancel();
    assert_eq!(io.read(&mut [0],None).unwrap_err().kind(),std::io::ErrorKind::ConnectionAborted);
    assert_eq!(io.write(&[1],None).unwrap_err().kind(),std::io::ErrorKind::ConnectionAborted);
    assert_eq!(io.flush().unwrap_err().kind(),std::io::ErrorKind::ConnectionAborted);
}

#[test]
fn changing_from_write_to_idle_read_does_not_spin_on_writable_readiness() {
    let (mut io, _cancel, mut peer) = pair();
    assert_eq!(io.write(&[7],Some(Instant::now()+Duration::from_secs(1))).unwrap(),1);
    peer.read_exact(&mut [0]).unwrap();
    let started=Instant::now();
    assert_eq!(io.read(&mut [0],Some(started+Duration::from_millis(180))).unwrap_err().kind(),std::io::ErrorKind::TimedOut);
    assert!(started.elapsed()>=Duration::from_millis(150));
    assert!(io.polls<8,"Idle read kept waking on writable events: {} polls",io.polls);
}

#[test]
fn a_backpressured_write_has_a_deadline_and_can_be_cancelled_directly() {
    let (mut io, cancel, _peer) = pair();
    let deadline = Instant::now() + Duration::from_millis(180);
    assert_eq!(write_until_stopped(&mut io, deadline), std::io::ErrorKind::TimedOut);
    assert!(io.polls > 0, "The writer never waited for socket readiness");

    let (done, result) = mpsc::channel();
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        // A TCP window update can let another write through after the first deadline.
        done.send(write_until_stopped(&mut io, deadline)).unwrap();
    });
    assert_eq!(result.recv_timeout(Duration::from_millis(40)), Err(mpsc::RecvTimeoutError::Timeout));
    cancel.cancel();
    assert_eq!(result.recv_timeout(Duration::from_secs(1)), Ok(std::io::ErrorKind::ConnectionAborted));
    worker.join().unwrap();
}

fn write_until_stopped(io: &mut SocketIo, deadline: Instant) -> std::io::ErrorKind {
    let block = [1; 65_536];
    let mut total = 0;
    loop {
        match io.write(&block, Some(deadline)) {
            Ok(count) => {
                total += count;
                assert!(total < 64 * 1024 * 1024, "Peer never applied backpressure");
            }
            Err(error) => return error.kind(),
        }
    }
}
