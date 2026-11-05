use super::*;
use std::sync::mpsc;

fn readiness() -> Readiness {
    let hub = Hub::new("readiness-test".into(), Arc::new(crate::NullSourceSink));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    Readiness::new(listener, &hub).unwrap()
}

#[test]
fn a_quiet_listener_waits_without_a_timer_and_is_interrupted_directly() {
    let mut readiness = readiness();
    // Consume observer registration, which deliberately requests an initial
    // deadline drain before the first indefinite wait.
    readiness.wait(Some(Duration::ZERO)).unwrap();
    let wake = readiness.wake();
    let (entered, ready) = mpsc::channel();
    let (done, completed) = mpsc::channel();
    let worker = thread::spawn(move || {
        entered.send(()).unwrap();
        done.send(readiness.wait(None).unwrap()).unwrap();
    });
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(completed.recv_timeout(Duration::from_millis(80)), Err(mpsc::RecvTimeoutError::Timeout)));
    wake.wake().unwrap();
    assert!(!completed.recv_timeout(Duration::from_secs(2)).unwrap(), "A control wake is not socket readiness");
    worker.join().unwrap();
}

#[test]
fn real_connections_wake_the_listener_and_drain_to_would_block() {
    let mut readiness = readiness();
    readiness.wait(Some(Duration::ZERO)).unwrap();
    let address = readiness.listener.local_addr().unwrap();
    let peers = (0..3).map(|_| TcpStream::connect(address).unwrap()).collect::<Vec<_>>();
    assert!(readiness.wait(Some(Duration::from_secs(2))).unwrap());
    let mut accepted = Vec::new();
    loop {
        match readiness.listener.accept() {
            Ok((stream, _)) => accepted.push(stream),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) => panic!("{error}"),
        }
    }
    assert_eq!(accepted.len(), peers.len());
}
