use super::*;

#[tokio::test]
async fn closed_worker_channel_has_the_same_cached_and_awaited_failure() {
    let (sender, receiver) = watch::channel(None);
    let monitor = test_monitor_receiver(receiver);
    assert!(monitor.error().is_none());
    let mut observer = monitor.failure_receiver();
    drop(sender);
    assert_eq!(monitor.error().as_deref(), Some(STOPPED));
    assert_eq!(monitor.failed().await, STOPPED);
    assert_eq!(wait_failure(&mut observer).await, STOPPED);
}

#[tokio::test]
async fn terminal_reason_survives_sender_closure_and_receivers_do_not_own_workers() {
    let (sender, receiver) = watch::channel(None);
    let monitor = std::sync::Arc::new(test_monitor_receiver(receiver));
    let weak = std::sync::Arc::downgrade(&monitor);
    let mut observer = monitor.failure_receiver();
    sender.send_replace(Some("selected process exited".into()));
    drop(sender);
    assert_eq!(monitor.error().as_deref(), Some("selected process exited"));
    drop(monitor);
    assert!(weak.upgrade().is_none());
    assert_eq!(wait_failure(&mut observer).await, "selected process exited");
}
