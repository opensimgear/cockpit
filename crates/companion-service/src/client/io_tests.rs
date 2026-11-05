use super::*;
use std::{
    io::Read,
    net::{TcpListener, TcpStream},
    sync::Arc,
    thread,
    time::Duration,
};

fn event_pair() -> (DeadlineTcp, Arc<Cancel>, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (server, _) = listener.accept().unwrap();
    let (io, cancel) = DeadlineTcp::event(server).unwrap();
    (io, cancel, client)
}

#[test]
fn retirement_closes_only_scopes_at_or_before_the_epoch() {
    let registry = Registry::default();
    let old = registry.scope(40, || 40).unwrap();
    let current = registry.scope(41, || 41).unwrap();

    registry.retire(40);

    assert!(old.is_closed());
    assert!(!current.is_closed());
    // A late retirement of the old generation cannot reach a newer scope.
    registry.retire(40);
    assert!(!current.is_closed());
}

#[test]
fn scope_registration_is_epoch_checked_and_limited() {
    let registry = Registry::default();
    assert!(registry.scope(7, || 8).is_err());

    let mut scopes = Vec::new();
    for epoch in 0..32 {
        scopes.push(registry.scope(epoch, || epoch).unwrap());
    }
    assert!(registry.scope(32, || 32).is_err());
    drop(scopes.remove(0));
    assert!(registry.scope(32, || 32).is_ok());
}

#[test]
fn shutdown_closes_all_scopes_and_rejects_new_admission() {
    let registry = Registry::default();
    let first = registry.scope(1, || 1).unwrap();
    let second = registry.scope(2, || 2).unwrap();

    registry.shutdown();

    assert!(first.is_closed());
    assert!(second.is_closed());
    assert!(registry.scope(3, || 3).is_err());
}

#[test]
fn scope_close_cancels_registered_event_io_and_wakes_its_read() {
    let scope = IoScope::default();
    let (mut io, cancel, _client) = event_pair();
    let _registration = scope.register(cancel).unwrap();
    let reader = thread::spawn(move || {
        let mut byte = [0];
        io.read(&mut byte)
    });

    scope.close();

    let error = reader.join().unwrap().unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::ConnectionAborted);
    assert!(scope.is_closed());
}

#[test]
fn registration_after_close_cancels_new_event_io() {
    let scope = IoScope::default();
    scope.close();
    let (mut io, cancel, _client) = event_pair();
    let reader = thread::spawn(move || {
        let mut byte = [0];
        io.read(&mut byte)
    });

    assert!(scope.register(cancel).is_err());
    let error = reader.join().unwrap().unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::ConnectionAborted);
}

#[test]
fn scope_wait_is_interrupted_by_close() {
    let scope = IoScope::default();
    let waiting = scope.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let waiter = thread::spawn(move || {
        waiting.bind_thread().unwrap();
        let observed = waiting.observed();
        ready_tx.send(()).unwrap();
        waiting.wait(observed, Duration::from_secs(5))
    });
    ready_rx.recv().unwrap();

    scope.close();

    assert!(waiter.join().unwrap());
}

#[test]
fn scope_caps_registered_controls() {
    let scope = IoScope::default();
    let mut registrations = Vec::new();
    let mut clients = Vec::new();
    for _ in 0..16 {
        let (_io, cancel, client) = event_pair();
        registrations.push(scope.register(cancel).unwrap());
        clients.push(client);
    }
    let (_io, cancel, client) = event_pair();
    assert!(scope.register(cancel).is_err());
    clients.push(client);
    assert_eq!(registrations.len(), 16);
}
