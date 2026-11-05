use super::super::Monitor;
use super::*;
use std::{
    process::{Child, Command, Stdio},
    sync::atomic::AtomicUsize,
    time::{Duration, Instant},
};

struct Lease {
    _inbox: Arc<Inbox>,
    closed: SyncSender<()>,
}
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = self.closed.try_send(());
    }
}
fn current() -> ProcessIdentity {
    Handle::current_identity()
}
fn fixture(expected: ProcessIdentity) -> (Monitor, Arc<Inbox>, Receiver<()>) {
    let (events, receiver) = mpsc::sync_channel(1);
    let (closed, cleanup) = mpsc::sync_channel(1);
    let monitor = Monitor::direct(start_with(
        expected,
        move |inbox, _| {
            events.send(inbox.clone()).unwrap();
            Ok(Lease {
                _inbox: inbox,
                closed,
            })
        },
        |_| Ok(()),
        Duration::from_secs(3),
    ))
    .unwrap();
    (
        monitor,
        receiver.recv_timeout(Duration::from_secs(3)).unwrap(),
        cleanup,
    )
}
fn failed(monitor: &Monitor) -> String {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(3), monitor.failed())
                .await
                .unwrap()
        })
}

#[test]
fn subscribed_events_are_serialized_with_inventory_before_readiness() {
    let expected = current();
    let pid = expected.pid;
    let stage = Arc::new(AtomicUsize::new(0));
    let subscription_stage = stage.clone();
    let inventory_stage = stage.clone();
    let (events, receiver) = mpsc::sync_channel(1);
    let (closed, cleanup) = mpsc::sync_channel(1);
    let monitor = Monitor::direct(start_with(
        expected,
        move |inbox, _| {
            assert_eq!(subscription_stage.fetch_add(1, Ordering::SeqCst), 0);
            inbox.push(Event::Started(pid));
            events.send(inbox.clone()).unwrap();
            Ok(Lease {
                _inbox: inbox,
                closed,
            })
        },
        move |identity| {
            assert_eq!(identity.pid, pid);
            assert_eq!(inventory_stage.fetch_add(1, Ordering::SeqCst), 1);
            Ok(())
        },
        Duration::from_secs(3),
    ))
    .unwrap();
    assert_eq!(stage.load(Ordering::SeqCst), 2);
    assert!(monitor.error().is_none());
    let inbox = receiver.recv().unwrap();
    inbox.push(Event::Started(pid));
    inbox.push(Event::Failed("ordered terminal notification".into()));
    assert_eq!(failed(&monitor), "ordered terminal notification");
    cleanup.recv_timeout(Duration::from_secs(3)).unwrap();
    monitor.cancel();
    assert_eq!(monitor.error().unwrap(), "ordered terminal notification");
}

#[test]
fn a_competing_start_after_ready_invalidates_the_actual_waiting_worker() {
    let expected = current();
    let competitor = expected.pid.checked_add(4).unwrap();
    let (monitor, inbox, cleanup) = fixture(expected);
    inbox.push(Event::Started(competitor));
    assert!(failed(&monitor).contains("Another X-Plane"));
    cleanup.recv_timeout(Duration::from_secs(3)).unwrap();
}

#[test]
fn stop_wakes_native_wait_and_drops_subscription_without_resetting_first_failure() {
    let (monitor, _, cleanup) = fixture(current());
    assert!(monitor.error().is_none());
    monitor.cancel();
    assert!(failed(&monitor).contains("disabled"));
    monitor.cancel();
    cleanup.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(monitor.error().unwrap().contains("disabled"));
    drop(monitor);
}

#[test]
fn dropping_last_monitor_revokes_observers_and_releases_subscription() {
    let (monitor, _, cleanup) = fixture(current());
    let observed = monitor.failure.clone();
    drop(monitor);
    assert!(observed.borrow().as_ref().unwrap().contains("disabled"));
    cleanup.recv_timeout(Duration::from_secs(3)).unwrap();
}

#[test]
fn bootstrap_rejects_subscription_inventory_identity_and_event_loss_errors() {
    let expected = current();
    let error = start_with(
        expected.clone(),
        |_, _| Err::<(), _>("subscription unavailable".into()),
        |_| panic!("no inventory after subscription failure"),
        Duration::from_secs(3),
    )
    .err()
    .unwrap();
    assert_eq!(error, "subscription unavailable");
    let error = start_with(
        expected.clone(),
        |_, _| Ok(()),
        |_| Err("inventory denied".into()),
        Duration::from_secs(3),
    )
    .err()
    .unwrap();
    assert_eq!(error, "inventory denied");
    let mut wrong = expected.clone();
    wrong.epoch += 1;
    assert!(start_with(
        wrong,
        |_, _| Ok(()),
        |_| panic!("no inventory after wrong exact handle identity"),
        Duration::from_secs(3)
    )
    .is_err());
    let pid = expected.pid;
    assert!(start_with(
        expected.clone(),
        move |inbox, _| {
            inbox.push(Event::Started(pid + 4));
            Ok(())
        },
        |_| Ok(()),
        Duration::from_secs(3)
    )
    .err()
    .unwrap()
    .contains("Another X-Plane"));
    let error = start_with(
        expected,
        move |inbox, _| {
            for _ in 0..257 {
                inbox.push(Event::Started(pid));
            }
            Ok(())
        },
        |_| Ok(()),
        Duration::from_secs(3),
    )
    .err()
    .unwrap();
    assert!(error.contains("overflow"));
}

#[test]
fn worker_panics_are_reported_as_terminal_bootstrap_failure() {
    let error = start_with(
        current(),
        |_, _| -> Result<(), String> { panic!("injected subscription panic") },
        |_| Ok(()),
        Duration::from_secs(3),
    )
    .err()
    .unwrap();
    assert!(error.contains("panicked"));
}

#[test]
fn timeout_cancels_a_blocked_rpc_without_waiting_and_prevents_late_inventory() {
    let (release, blocked) = mpsc::sync_channel(1);
    let (closed, cleanup) = mpsc::sync_channel(1);
    let inventory = Arc::new(AtomicBool::new(false));
    let attempted = inventory.clone();
    let started = Instant::now();
    let result = start_with(
        current(),
        move |inbox, cancelled| {
            blocked.recv_timeout(Duration::from_secs(3)).unwrap();
            assert!(cancelled.load(Ordering::Acquire));
            Ok(Lease {
                _inbox: inbox,
                closed,
            })
        },
        move |_| {
            attempted.store(true, Ordering::Release);
            Ok(())
        },
        Duration::from_millis(300),
    );
    assert!(result.err().unwrap().contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(2));
    release.send(()).unwrap();
    cleanup.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(!inventory.load(Ordering::Acquire));
}

#[test]
fn cancelled_startup_checks_before_inventory_and_closed_ready_is_an_error() {
    let stop = Handle::event().unwrap();
    let cancelled = AtomicBool::new(true);
    let (ready, _receiver) = mpsc::sync_channel(1);
    assert!(run_with(
        &current(),
        &stop,
        &cancelled,
        &ready,
        |_, _| Ok(()),
        |_| panic!("cancelled startup cannot inventory")
    )
    .unwrap_err()
    .contains("cancelled"));
    let (ready, receiver) = mpsc::sync_channel(1);
    drop(receiver);
    assert!(run_with(
        &current(),
        &stop,
        &AtomicBool::new(false),
        &ready,
        |_, _| Ok(()),
        |_| Ok(())
    )
    .unwrap_err()
    .contains("cancelled"));
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn exact_child_exit_is_terminal_through_the_native_worker_wait() {
    let name = format!(
        "{}::native_child_fixture",
        module_path!().split_once("::").unwrap().1
    );
    let mut child = ChildGuard(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &name, "--ignored"])
            .env("OSG_DIRECT_PROCESS_CHILD_FIXTURE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let expected = Handle::open(child.0.id())
        .unwrap()
        .identity(child.0.id())
        .unwrap();
    let (monitor, _, cleanup) = fixture(expected);
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    assert!(failed(&monitor).contains("exited"));
    cleanup.recv_timeout(Duration::from_secs(3)).unwrap();
}

#[test]
fn ordinary_user_wmi_capability_and_native_facade_failures_are_explicit() {
    match test_subscription() {
        Ok(()) => eprintln!("Local temporary WMI subscriptions registered and cancelled; no event-delivery qualification claimed."),
        Err(error) => {
            eprintln!("Local WMI capability unavailable: {error}");
            assert!(error.contains("Cannot subscribe") && error.contains("WMI"));
            if error.contains("0x80041003") || error.contains("0x80070005") { assert!(error.contains("explicit native monitoring enable")); }
        }
    }
    let result = Monitor::start(&current(), "x-plane-12");
    match result {
        Ok(monitor) => {
            monitor.cancel();
            assert!(monitor.error().is_some());
        }
        Err(error) => assert!(!error.is_empty()),
    }
}

#[test]
#[ignore = "Only launched as an ordinary child by the direct native handle test."]
fn native_child_fixture() {
    if std::env::var_os("OSG_DIRECT_PROCESS_CHILD_FIXTURE").is_some() {
        std::thread::park();
    }
}
