use super::*;
use std::{sync::mpsc, time::Duration};

#[test]
fn admission_clock_shares_journal_epoch_without_reading_capture_state() {
    let service = ControllerService::new(None, false);
    let clock = service.capture_clock();
    assert_eq!(clock.id().to_string(), service.registry.lock().unwrap().session);
    let before = service.monotonic_us();
    let acquired = std::time::Instant::now();
    let page = service.stream_page("", 0);
    let after = service.monotonic_us();
    let captured = clock.captured_us(acquired).unwrap();
    assert!(before <= captured && captured <= after);
    assert!(before > 0 && before <= page.captured_us && page.captured_us <= after);
    let same = service.stream_page(&page.session, page.cursor);
    assert_eq!(page.cursor, same.cursor, "Clock reads do not publish input evidence.");
}

#[test]
fn admission_clock_remains_available_while_registry_is_locked() {
    let service = Arc::new(ControllerService::new(None, false));
    let held = service.registry.lock().unwrap();
    let worker_service = service.clone();
    let (send, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || send.send(worker_service.monotonic_us()).unwrap());
    // Hold the mutex until after receipt: a registry-backed clock cannot pass.
    let result = receive.recv_timeout(Duration::from_secs(2));
    drop(held);
    worker.join().unwrap();
    assert!(result.unwrap() > 0);
}
