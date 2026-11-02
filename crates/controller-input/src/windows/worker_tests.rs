use super::*;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn cleanup_failure_preserves_primary_error_and_reports_each_failed_resource() {
    // Failed startup has no registered Raw Input ownership to remove. Invalid
    // resources exercise the real Windows cleanup errors without touching peers.
    let missing = wide(&format!("MissingControllerClass-{}",uuid::Uuid::new_v4()));
    let error = unsafe { cleanup_window(null_mut(), &missing, GetModuleHandleW(null()), false) }.unwrap();
    assert!(error.contains("Destroy controller input window"));
    assert!(error.contains("Unregister controller input window"));
    let combined = append_cleanup("Capture registration failed".into(),Some(error));
    assert!(combined.starts_with("Capture registration failed; cleanup failed:"));
    assert_eq!(append_cleanup("Capture failed".into(),None),"Capture failed");
}

#[test]
fn idle_message_wait_blocks_until_shutdown_is_signaled() {
    let wake = Arc::new(Wake::new().unwrap());
    let worker_wake = wake.clone();
    let (sent, received) = mpsc::channel();
    std::thread::spawn(move || sent.send(wait_for_work(&worker_wake)).unwrap());

    assert!(received.recv_timeout(Duration::from_millis(40)).is_err());
    wake.signal().unwrap();
    assert_eq!(
        received
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
        WaitOutcome::Wake
    );
}

#[test]
fn device_notifications_are_the_only_rescan_trigger_after_startup() {
    assert!(is_device_change(WM_INPUT_DEVICE_CHANGE));
    assert!(!is_device_change(WM_INPUT));
    assert!(!is_device_change(WM_INPUT + 1));
}

#[test]
fn repeated_startup_and_shutdown_wakes_idle_worker_promptly() {
    for _ in 0..8 {
        let wake = Arc::new(Wake::new().unwrap());
        let shared = Arc::new(Mutex::new(Registry::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_wake = wake.clone();
        let worker_stop = stop.clone();
        let worker_shared = shared.clone();
        let worker = std::thread::spawn(move || run(worker_shared, worker_stop, worker_wake));

        // Startup enumeration is complete once the worker publishes ready.
        for _ in 0..100 {
            if shared.lock().unwrap().backend_status == "ready" {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        stop.store(true, Ordering::Release);
        wake.signal().unwrap();
        assert!(worker.join().unwrap().is_ok());
        assert_eq!(shared.lock().unwrap().backend_status, "ready");
    }
}
