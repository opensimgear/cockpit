use super::*;

fn pair(event: bool) -> (DeadlineTcp, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let accepted = listener.accept().unwrap().0;
    let mut stream = if event { DeadlineTcp::event(accepted).unwrap().0 } else { DeadlineTcp::new(accepted) };
    stream.upgraded();
    (stream, peer)
}

#[test]
fn quiet_read_uses_its_actual_deadline_instead_of_an_old_poll_timeout() {
    for event in [false,true] {
    let (mut stream, _peer) = pair(event);
    stream.set_read_timeout(Some(Duration::from_millis(10))).unwrap();
    let started = Instant::now();
    stream.set_read_deadline(Some(started + Duration::from_millis(180))).unwrap();
    let error = stream.read(&mut [0]).unwrap_err();
    assert!(matches!(error.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock));
    assert!(started.elapsed() >= Duration::from_millis(150));
    }
}

#[test]
fn partial_reads_cannot_extend_the_absolute_message_budget() {
    for event in [false,true] {
    let (mut stream, mut peer) = pair(event);
    stream.set_read_deadline(Some(Instant::now() + Duration::from_millis(100))).unwrap();
    let sender = thread::spawn(move || {
        for _ in 0..8 { thread::sleep(Duration::from_millis(30)); if peer.write_all(&[1]).is_err() { break; } }
    });
    let started = Instant::now();
    assert!(stream.read_exact(&mut [0;8]).is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
    sender.join().unwrap();
    }
}

#[test]
fn expired_read_deadlines_leave_event_writes_valid_and_can_be_cleared() {
    for event in [false,true] {
    let (mut stream, mut peer) = pair(event);
    assert!(stream.set_read_deadline(Some(Instant::now() - Duration::from_secs(1))).is_err());
    assert!(stream.read(&mut [0]).is_err());
    stream.write_all(&[42]).unwrap();stream.flush().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    let mut byte = [0]; peer.read_exact(&mut byte).unwrap(); assert_eq!(byte,[42]);
    stream.set_read_deadline(None).unwrap();
    if let Transport::Blocking(io) = &stream.transport { assert_eq!(io.read_timeout().unwrap(),None); }
    peer.write_all(&[7]).unwrap();stream.read_exact(&mut byte).unwrap();assert_eq!(byte,[7]);
    }
}
