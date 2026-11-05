use super::*;

#[test]
fn raw_queue_retains_original_window_order_and_overflow_is_sticky() {
    INBOX.with(|slot| *slot.borrow_mut() = Inbox::default());
    enqueue(Raw::Foreground(123));
    enqueue(Raw::Foreground(456));
    INBOX.with(|slot| {
        let inbox = slot.borrow();
        assert!(matches!(inbox.queue[0], Raw::Foreground(123)));
        assert!(matches!(inbox.queue[1], Raw::Foreground(456)));
    });
    for _ in 2..RAW_LIMIT {
        enqueue(Raw::DesktopSwitch);
    }
    enqueue(Raw::Session(WTS_SESSION_LOCK as usize));
    let shared = Shared::new();
    let mut active = true;
    assert!(drain(&shared, &mut active)
        .unwrap_err()
        .contains("overflow"));
    enqueue(Raw::Foreground(789));
    INBOX.with(|slot| {
        assert_eq!(slot.borrow().queue.len(), RAW_LIMIT);
        *slot.borrow_mut() = Inbox::default();
    });
}

#[test]
fn noninteractive_or_missing_foreground_does_not_inspect_a_process() {
    assert!(inspect(123usize as HWND, false).foreground.is_none());
    let result = inspect(ptr::null_mut(), true);
    assert!(result.foreground.is_none());
    assert!(result.error.is_none());
}

#[test]
fn ordered_session_records_keep_disconnect_until_a_fresh_connect_observation() {
    INBOX.with(|slot| *slot.borrow_mut() = Inbox::default());
    let shared = Shared::new();
    let mut active = true;
    for reason in [WTS_SESSION_LOCK, WTS_SESSION_LOGOFF, WTS_CONSOLE_DISCONNECT, WTS_REMOTE_DISCONNECT] {
        enqueue(Raw::Session(reason as usize));
        drain(&shared, &mut active).unwrap();
        assert!(!active);
    }
    // These are local owner records, never synthetic global WinEvent/WTS
    // broadcasts that could interfere with another running Cockpit.
    for reason in [WTS_SESSION_UNLOCK, WTS_SESSION_LOGON, WTS_CONSOLE_CONNECT,
        WTS_REMOTE_CONNECT, WTS_SESSION_DESKTOP_READY] {
        enqueue(Raw::Session(reason as usize));
        drain(&shared, &mut active).unwrap();
    }
    enqueue(Raw::Session(usize::MAX));
    enqueue(Raw::DesktopSwitch);
    enqueue(Raw::Foreground(0));
    enqueue(Raw::Foreground(1));
    drain(&shared, &mut active).unwrap();
    assert!(shared.terminal().is_none());
    shared.close();
    enqueue(Raw::DesktopSwitch);
    drain(&shared, &mut active).unwrap();
    INBOX.with(|slot| *slot.borrow_mut() = Inbox::default());
}

#[test]
fn native_resources_are_owned_and_cleanup_is_idempotent() {
    let lifetime = super::super::lifetime::Lifetime::new();
    let mut resources = Resources::open(Some(&lifetime)).unwrap();
    assert!(resources.wts && resources.registered);
    assert!(!resources.foreground.is_null() && !resources.desktop.is_null());
    assert!(!resources.hwnd.is_null());
    resources.cleanup().unwrap();
    assert!(!lifetime.with_live_resources(|| panic!("Stop posted after teardown began")));
    assert!(!lifetime.exited(), "Resource teardown precedes full observer completion");
    assert!(!resources.wts && !resources.registered);
    assert!(resources.hwnd.is_null() && resources.foreground.is_null() && resources.desktop.is_null());
    resources.cleanup().unwrap();
}

#[test]
fn unwinding_native_resources_revokes_stop_posts_before_exit() {
    let lifetime = super::super::lifetime::Lifetime::new();
    let result = std::panic::catch_unwind(|| {
        let _resources = Resources::open(Some(&lifetime)).unwrap();
        panic!("Injected native resource unwind");
    });
    assert!(result.is_err());
    assert!(!lifetime.with_live_resources(|| panic!("Stop posted after native unwind")));
    assert!(!lifetime.exited());
}

#[test]
fn a_failed_native_post_is_a_terminal_wake_failure() {
    // PostQuitMessage affects this dedicated test thread only.
    std::thread::spawn(|| {
        INBOX.with(|slot| *slot.borrow_mut() = Inbox { hwnd: 1, ..Inbox::default() });
        enqueue(Raw::DesktopSwitch);
        let shared = Shared::new();
        assert!(drain(&shared, &mut true).unwrap_err().contains("wake failed"));
    }).join().unwrap();
}

#[test]
fn retained_terminal_owner_replaces_only_after_cleanup_and_cannot_stop_its_replacement() {
    use super::super::{registry::with_available, Registry};
    use crate::Scope;

    let shared = Arc::new(Shared::new());
    let mut old = Owner::open(shared.clone(), Instant::now() + Duration::from_secs(5)).unwrap();
    let registry = Arc::new(Mutex::new(Registry {
        owner: Arc::downgrade(&old), shared: Arc::downgrade(&shared),
    }));
    let (terminal, observed_terminal) = mpsc::sync_channel(1);
    let (release, cleanup) = mpsc::sync_channel(1);
    let cleanup = Mutex::new(cleanup);
    let subscription = shared.subscribe(9001, Scope::DesktopWide, Arc::new(move |snapshot| {
        if snapshot.error.is_some() {
            terminal.send(()).unwrap();
            cleanup.lock().unwrap().recv().unwrap();
        } else {
            panic!("Injected native scope callback failure");
        }
    })).unwrap();
    // A local message drives a real native failure; its terminal callback holds
    // the old Resources alive until the test explicitly permits cleanup.
    assert_ne!(unsafe { PostMessageW(old.hwnd as HWND, WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK as usize, 0) }, 0);
    observed_terminal.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(shared.terminal().is_some() && !shared.lifetime.exited());
    let (notice, waiting) = mpsc::sync_channel(1);
    shared.lifetime.notify_wait(notice);
    let replacing = registry.clone();
    let retired = shared.clone();
    let (finished, answer) = mpsc::sync_channel(1);
    let replacement = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        finished.send(with_available(&replacing, deadline, |slot, owner| {
            assert!(owner.is_none());
            assert!(retired.lifetime.exited(), "Replacement must follow actual native resource cleanup");
            let shared = Arc::new(Shared::new());
            let owner = Owner::open(shared.clone(), deadline)?;
            slot.shared = Arc::downgrade(&shared);
            slot.owner = Arc::downgrade(&owner);
            Ok(owner)
        })).unwrap();
    });
    waiting.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(answer.try_recv().is_err());
    // Acquiring this guard while the replacement waits proves no registry lock
    // spans terminal callbacks or native cleanup.
    let slot = registry.lock().unwrap();
    release.send(()).unwrap();
    shared.lifetime.wait(Instant::now() + Duration::from_secs(2)).unwrap();
    assert!(answer.try_recv().is_err());
    drop(slot);
    let current = answer.recv_timeout(Duration::from_secs(2)).unwrap().unwrap();
    replacement.join().unwrap();
    assert!(!Arc::ptr_eq(&old, &current));

    let (event, observed_event) = mpsc::sync_channel(1);
    let current_subscription = current.shared.subscribe(9002, Scope::DesktopWide,
        Arc::new(move |_| { let _ = event.send(()); })).unwrap();
    // Deterministically model the OS reusing both identifiers after teardown.
    let retired = Arc::get_mut(&mut old).unwrap();
    retired.hwnd = current.hwnd;
    retired.thread_id = current.thread_id;
    drop(old);
    assert_ne!(unsafe { PostMessageW(current.hwnd as HWND, WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK as usize, 0) }, 0);
    observed_event.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(!current.shared.lifetime.exited(), "Retiring an old diagnostic lease cannot close the replacement");
    current.shared.unsubscribe(&current_subscription);
    shared.unsubscribe(&subscription);
    drop(current);
}
