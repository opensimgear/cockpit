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
    let mut resources = Resources::open().unwrap();
    assert!(resources.wts && resources.registered);
    assert!(!resources.foreground.is_null() && !resources.desktop.is_null());
    assert!(!resources.hwnd.is_null());
    resources.cleanup().unwrap();
    assert!(!resources.wts && !resources.registered);
    assert!(resources.hwnd.is_null() && resources.foreground.is_null() && resources.desktop.is_null());
    resources.cleanup().unwrap();
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
