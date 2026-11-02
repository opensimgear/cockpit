use super::*;

#[test]
fn bootstrap_accepts_selected_pid_but_rejects_competitors_and_invalid_ids() {
    assert!(started(42, 42).is_ok());
    assert!(started(42, 43).is_err());
    assert!(started(42, 0).is_err());
}

#[tokio::test]
async fn failure_wait_is_passive_and_retains_the_terminal_reason() {
    let (sender, mut receiver) = watch::channel::<Option<String>>(None);
    sender.send_replace(Some("selected process exited".into()));
    assert_eq!(wait_failure(&mut receiver).await, "selected process exited");
    assert_eq!(wait_failure(&mut receiver).await, "selected process exited");
    let (sender, mut receiver) = watch::channel(None);
    drop(sender);
    assert!(wait_failure(&mut receiver).await.contains("unexpectedly"));
}

#[tokio::test]
async fn portable_in_memory_monitor_retains_revocation_for_registry_observers() {
    let (monitor, observed) = test_monitor();
    assert!(monitor.error().is_none());
    monitor.cancel();
    assert!(observed.borrow().as_ref().unwrap().contains("disabled"));
    assert!(monitor.failed().await.contains("disabled"));
}

#[test]
fn elevated_facade_rejects_invalid_and_cancelled_requests_without_launching() {
    let invalid = ProcessIdentity {
        pid: 0,
        epoch: 0,
        path: "invalid".into(),
    };
    assert!(Monitor::start_elevated(&invalid).is_err());
    let (_sender, receiver) = watch::channel(true);
    assert!(Monitor::start_elevated_with_cancel(&invalid, receiver).is_err());
    assert!(
        helper_main().is_none(),
        "ordinary test invocation must not dispatch the helper"
    );
}

#[cfg(windows)]
#[test]
fn queued_bootstrap_competitor_cannot_be_hidden_by_selected_start() {
    let (inbox, receiver) = windows::test_inbox(4);
    inbox.push(windows::Event::Started(42));
    inbox.push(windows::Event::Started(43));
    assert!(windows::test_drain(&inbox, &receiver, 42).is_err());
}

#[cfg(windows)]
#[test]
fn overflow_and_subscription_completion_are_terminal() {
    let (inbox, receiver) = windows::test_inbox(1);
    inbox.push(windows::Event::Started(42));
    inbox.push(windows::Event::Started(42));
    assert!(windows::test_drain(&inbox, &receiver, 42)
        .unwrap_err()
        .contains("overflow"));
    // Draining cannot recover evidence already lost.
    assert!(windows::test_drain(&inbox, &receiver, 42).is_err());
    let (inbox, receiver) = windows::test_inbox(2);
    inbox.push(windows::Event::Failed("subscription completed".into()));
    assert_eq!(
        windows::test_drain(&inbox, &receiver, 42).unwrap_err(),
        "subscription completed"
    );
}

#[cfg(windows)]
#[test]
fn retained_current_process_handle_validates_epoch_and_image() {
    let expected = native::Handle::current_identity();
    assert!(native::Handle::process(&expected).is_ok());
    let mut wrong = expected.clone();
    wrong.epoch += 1;
    assert!(native::Handle::process(&wrong).is_err());
    wrong = expected;
    wrong.path.push_str(".different");
    assert!(native::Handle::process(&wrong).is_err());
}

#[cfg(windows)]
#[tokio::test]
async fn explicit_cancel_revokes_an_arc_shared_monitor_immediately() {
    fn assert_send_sync<T: Send + Sync>() {}
    fn assert_send<T: Send>(_: T) {}
    assert_send_sync::<Monitor>();
    let (sender, failure) = watch::channel(None);
    let monitor = std::sync::Arc::new(Monitor {
        failure,
        _worker: Backend::Test(sender),
    });
    let observer = monitor.clone();
    assert_send(observer.failed());
    monitor.cancel();
    assert!(observer.error().unwrap().contains("disabled"));
    assert!(observer.failed().await.contains("disabled"));
}

#[cfg(windows)]
#[test]
#[ignore = "Requires the native Windows WMI service and local event-subscription permissions."]
fn native_wmi_subscriptions_register_and_cancel() {
    windows::test_subscription().unwrap();
}

#[cfg(windows)]
#[test]
fn cancelling_a_native_wait_needs_no_timer_or_process_exit() {
    let stop = std::sync::Arc::new(native::Handle::event().unwrap());
    let wake = native::Handle::event().unwrap();
    let process = native::Handle::process(&native::Handle::current_identity()).unwrap();
    let waiter_stop = stop.clone();
    let waiter = std::thread::spawn(move || native::wait(&waiter_stop, &wake, &process));
    stop.signal();
    assert_eq!(waiter.join().unwrap().unwrap(), 0);
}

#[cfg(windows)]
#[test]
#[ignore = "Creates a temporary native PowerShell process to verify the retained exit handle."]
fn retained_child_handle_signals_its_exact_exit() {
    let mut child = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 60",
        ])
        .spawn()
        .unwrap();
    let expected = native::Handle::identity_for_pid(child.id());
    let retained = native::Handle::process(&expected).unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    let stop = native::Handle::event().unwrap();
    let wake = native::Handle::event().unwrap();
    assert_eq!(native::wait(&stop, &wake, &retained).unwrap(), 2);
    assert!(retained.running().is_err());
}
