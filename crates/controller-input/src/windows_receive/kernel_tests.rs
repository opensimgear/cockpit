//! Real Windows kernel I/O on local test pipes, never on a HID device.
use super::tests::SERIAL;
use super::*;
use windows_sys::Win32::System::Pipes::*;

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn server(path: &[u16]) -> Handle {
    let pipe = unsafe {
        CreateNamedPipeW(
            path.as_ptr(),
            PIPE_ACCESS_OUTBOUND,
            PIPE_TYPE_BYTE | PIPE_WAIT,
            1,
            65536,
            65536,
            1000,
            null(),
        )
    };
    assert_ne!(
        pipe,
        INVALID_HANDLE_VALUE,
        "{}",
        std::io::Error::last_os_error()
    );
    Handle(pipe)
}
fn selection(path: String) -> Selection {
    Selection {
        path,
        vendor_id: 1,
        product_id: 1,
        version_number: 1,
        usage_page: 0xff00,
        usage: 1,
        input_report_bytes: 64,
        feature_report_bytes: 16,
    }
}
fn pipe() -> (Session, Handle, String) {
    let name = format!(r"\\.\pipe\osg-hid-receive-{}", uuid::Uuid::new_v4());
    let path: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let server = server(&path);
    let client = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            null_mut(),
        )
    };
    assert_ne!(
        client,
        INVALID_HANDLE_VALUE,
        "{}",
        std::io::Error::last_os_error()
    );
    // The client has already connected, so this synchronous server-side call
    // returns immediately (normally ERROR_PIPE_CONNECTED).
    let connected = unsafe { ConnectNamedPipe(server.0, null_mut()) };
    assert!(connected != 0 || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED);
    SLOTS.fetch_add(1, Ordering::AcqRel);
    let file = File {
        handle: client,
        path: name.clone(),
        pp: 0,
        _permit: Permit,
        io: &io::NATIVE,
    };
    (
        Session {
            file: Arc::new(file),
            stream: None,
            get: None,
        },
        server,
        name,
    )
}
fn write(server: HANDLE, bytes: &[u8]) {
    let mut count = 0;
    assert_ne!(
        unsafe {
            WriteFile(
                server,
                bytes.as_ptr(),
                bytes.len() as u32,
                &mut count,
                null_mut(),
            )
        },
        0
    );
    assert_eq!(count as usize, bytes.len());
}

#[test]
fn real_overlapped_pipe_reads_preserve_frames_timeout_and_kernel_cancellation() {
    let _serial = SERIAL.lock().unwrap();
    let (mut receiver, server, path) = pipe();
    assert!(receiver.poll(64, Duration::ZERO).unwrap().is_none());
    write(server.0, &[3, 0x81, 0x7f]);
    assert_eq!(
        receiver.poll(64, Duration::from_secs(1)).unwrap(),
        Some(vec![3, 0x81, 0x7f])
    );
    let writer = server.0 as usize;
    let delayed = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(10));
        write(writer as HANDLE, &[0xfe]);
    });
    assert_eq!(
        receiver.poll(64, Duration::from_secs(1)).unwrap(),
        Some(vec![0xfe])
    );
    delayed.join().unwrap();
    for kind in [ReportKind::Input, ReportKind::Feature, ReportKind::Output] {
        let mut pending = Pending::new(16).unwrap();
        pending.bytes[0] = 3;
        // A named pipe has no HID GET controls. This exercises the actual
        // DeviceIoControl error path without sending anything to a device.
        assert!((io::NATIVE.start)(&receiver.file, &mut pending, Some(kind)).is_err());
    }
    assert!(receiver.poll(64, Duration::ZERO).unwrap().is_none());
    (io::NATIVE.cancel)(&receiver.file, receiver.stream.as_ref().unwrap());
    assert!(wait(
        &receiver.file,
        receiver.stream.as_mut().unwrap(),
        Duration::from_secs(1)
    )
    .is_err());
    drop(receiver);
    drop(server);
    for _ in 0..100 {
        reap();
        if !RETIRED
            .lock()
            .unwrap()
            .iter()
            .any(|entry| entry.file.path == path)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!RETIRED
        .lock()
        .unwrap()
        .iter()
        .any(|entry| entry.file.path == path));
}

#[test]
fn real_open_rejects_non_hid_handles_and_failed_wait_is_not_a_timeout() {
    let _serial = SERIAL.lock().unwrap();
    let name = format!(r"\\.\pipe\osg-hid-metadata-{}", uuid::Uuid::new_v4());
    let path: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let _server = server(&path);
    // Opening a readable path is insufficient: live HidD attributes must pass.
    assert!(Session::open(&selection(name)).is_err());
    let mut pending = Pending::new(8).unwrap();
    unsafe {
        CloseHandle(pending.overlap.hEvent);
    }
    pending.overlap.hEvent = null_mut();
    assert!((io::NATIVE.wait_event)(&pending, Duration::ZERO).is_err());
}
