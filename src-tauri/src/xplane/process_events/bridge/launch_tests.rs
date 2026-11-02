use super::*;
use std::sync::{atomic::AtomicUsize, mpsc, Mutex};

static SERIAL: Mutex<()> = Mutex::new(());
fn request() -> Request {
    let identity = super::super::security::current().unwrap();
    Request {
        nonce: "a".repeat(64),
        parent_pid: identity.pid,
        parent_epoch: identity.epoch,
    }
}
unsafe fn text(pointer: *const u16) -> String {
    assert!(!pointer.is_null());
    let mut length = 0;
    while unsafe { *pointer.add(length) } != 0 {
        length += 1;
        assert!(length <= 32768);
    }
    String::from_utf16(unsafe { std::slice::from_raw_parts(pointer, length) }).unwrap()
}
fn transfer_current_process() -> RawHandle {
    let process = Handle::open(std::process::id()).unwrap();
    let raw = process.raw();
    std::mem::forget(process);
    raw
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetProcessId(process: RawHandle) -> u32;
}

#[test]
fn shell_boundary_uses_fixed_self_executable_and_canonical_hidden_runas_request() {
    let _serial = SERIAL.lock().unwrap();
    let request = request();
    let raw = transfer_current_process();
    let returned = with_shell(&request, |info| {
        assert_eq!(info.size as usize, std::mem::size_of::<ShellInfo>());
        assert_eq!(
            info.size,
            if std::mem::size_of::<usize>() == 8 {
                112
            } else {
                60
            }
        );
        assert_eq!(info.mask, 0x40 | 0x100 | 0x400);
        assert_eq!(info.show, 0);
        assert_eq!(unsafe { text(info.verb) }, "runas");
        assert_eq!(
            unsafe { text(info.file) },
            std::env::current_exe()
                .unwrap()
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap()
        );
        assert_eq!(unsafe { text(info.parameters) }, request.arguments());
        assert!(info.window.is_null() && info.directory.is_null() && info.id_list.is_null());
        assert!(
            info.class.is_null()
                && info.key.is_null()
                && info.icon.is_null()
                && info.process.is_null()
        );
        assert_eq!(info.hot_key, 0);
        info.process = raw;
        1
    })
    .unwrap();
    assert_eq!(returned.raw(), raw);
    assert_eq!(
        returned.identity(std::process::id()).unwrap().pid,
        std::process::id()
    );
    drop(returned);
    assert_eq!(
        unsafe { GetProcessId(raw) },
        0,
        "the returned kernel handle must close with its sole owner"
    );
}

#[test]
fn malformed_requests_never_reach_the_os_adapter() {
    let _serial = SERIAL.lock().unwrap();
    let mut invalid = request();
    invalid.nonce = "x; arbitrary.exe".into();
    assert!(with_shell(&invalid, |_| panic!("invalid nonce must not launch")).is_err());
    invalid = request();
    invalid.parent_pid = 0;
    assert!(with_shell(&invalid, |_| panic!("invalid PID must not launch")).is_err());
    invalid = request();
    invalid.parent_epoch = 0;
    assert!(with_shell(&invalid, |_| panic!("invalid epoch must not launch")).is_err());
}

#[test]
fn false_and_null_outcomes_fail_closed_and_close_any_returned_handle() {
    let _serial = SERIAL.lock().unwrap();
    let request = request();
    assert!(with_shell(&request, |_| 0)
        .err()
        .unwrap()
        .contains("declined"));
    assert!(with_shell(&request, |_| 1).err().unwrap().contains("exact"));
    let raw = transfer_current_process();
    assert!(with_shell(&request, |info| {
        info.process = raw;
        0
    })
    .is_err());
    assert_eq!(unsafe { GetProcessId(raw) }, 0);
}

#[test]
fn nested_and_concurrent_launches_are_rejected_until_the_os_call_returns() {
    let _serial = SERIAL.lock().unwrap();
    let request = request();
    let worker_request = request.clone();
    let (entered_sender, entered) = mpsc::sync_channel(1);
    let (release, release_receiver) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        with_shell(&worker_request, |_| {
            assert!(with_shell(&worker_request, |_| panic!(
                "nested launch must not reach OS"
            ))
            .err()
            .unwrap()
            .contains("still pending"));
            entered_sender.send(()).unwrap();
            release_receiver
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
            0
        })
        .map(|_| ())
    });
    entered
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    assert!(
        with_shell(&request, |_| panic!("concurrent launch must not reach OS"))
            .err()
            .unwrap()
            .contains("still pending")
    );
    release.send(()).unwrap();
    assert!(worker.join().unwrap().is_err());
    let calls = AtomicUsize::new(0);
    assert!(with_shell(&request, |_| {
        calls.fetch_add(1, Ordering::Relaxed);
        0
    })
    .is_err());
    assert_eq!(
        calls.load(Ordering::Relaxed),
        1,
        "the permit must release after OS return"
    );
}

#[test]
fn an_unwinding_adapter_releases_the_in_flight_permit() {
    let _serial = SERIAL.lock().unwrap();
    let request = request();
    assert!(std::panic::catch_unwind(|| {
        let _ = with_shell(&request, |_| panic!("injected adapter panic"));
    })
    .is_err());
    let calls = AtomicUsize::new(0);
    assert!(with_shell(&request, |_| {
        calls.fetch_add(1, Ordering::Relaxed);
        0
    })
    .is_err());
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}
