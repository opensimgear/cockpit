use super::*;
use std::{
    io::Write,
    os::fd::FromRawFd,
    sync::{
        atomic::{AtomicBool, Ordering},
        Condvar,
    },
};
static SERIAL: Mutex<()> = Mutex::new(());
fn pipe() -> (File, File) {
    let mut fds = [0; 2];
    assert_eq!(
        unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) },
        0
    );
    unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) }
}
fn key() -> PathBuf {
    PathBuf::from(format!("fixture-{}", uuid::Uuid::new_v4()))
}
fn device(calls: Arc<dyn Calls>) -> Device {
    let (read, _write) = pipe();
    Device::from_file(read, key(), calls).unwrap()
}
struct Fake {
    reply: Mutex<Vec<u8>>,
    returned: Mutex<Option<usize>>,
    fail: Mutex<Option<i32>>,
    held: AtomicBool,
    entered: AtomicBool,
    gate: Mutex<bool>,
    wake: Condvar,
    requests: Mutex<Vec<(u32, Vec<u8>)>>,
    descriptor_size: Mutex<i32>,
    changed: AtomicBool,
    events: Mutex<std::collections::VecDeque<std::io::Result<i16>>>,
    reads: Mutex<std::collections::VecDeque<std::io::Result<Vec<u8>>>>,
}
impl Fake {
    fn new(reply: Vec<u8>) -> Self {
        Self {
            reply: Mutex::new(reply),
            returned: Mutex::new(None),
            fail: Mutex::new(None),
            held: AtomicBool::new(false),
            entered: AtomicBool::new(false),
            gate: Mutex::new(false),
            wake: Condvar::new(),
            requests: Mutex::new(vec![]),
            descriptor_size: Mutex::new(3),
            changed: AtomicBool::new(false),
            events: Mutex::new(Default::default()),
            reads: Mutex::new(Default::default()),
        }
    }
    fn release(&self) {
        *self.gate.lock().unwrap() = true;
        self.wake.notify_all();
    }
}
impl Calls for Fake {
    fn ioctl(&self, _: i32, request: u32, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.requests
            .lock()
            .unwrap()
            .push((request, bytes.to_vec()));
        if let Some(code) = *self.fail.lock().unwrap() {
            return Err(std::io::Error::from_raw_os_error(code));
        }
        match request & 0xff {
            1 => {
                bytes.copy_from_slice(&self.descriptor_size.lock().unwrap().to_ne_bytes());
                Ok(0)
            }
            2 => {
                bytes[4..7].copy_from_slice(&[5, 1, 0xc0]);
                if self.changed.load(Ordering::Acquire) {
                    bytes[..4].copy_from_slice(&2u32.to_ne_bytes());
                }
                Ok(0)
            }
            3 => {
                bytes[..4].copy_from_slice(&3u32.to_ne_bytes());
                bytes[4..6].copy_from_slice(&0xfedcu16.to_ne_bytes());
                bytes[6..8].copy_from_slice(&0x8123u16.to_ne_bytes());
                Ok(0)
            }
            4 | 5 | 8 => {
                bytes[..5].copy_from_slice(b"name\0");
                Ok(5)
            }
            7 | 10 => {
                let reply = self.reply.lock().unwrap().clone();
                self.entered.store(true, Ordering::Release);
                if self.held.load(Ordering::Acquire) {
                    let mut gate = self.gate.lock().unwrap();
                    while !*gate {
                        gate = self.wake.wait(gate).unwrap();
                    }
                }
                let copied = reply.len().min(bytes.len());
                bytes[..copied].copy_from_slice(&reply[..copied]);
                Ok(self.returned.lock().unwrap().unwrap_or(reply.len()))
            }
            _ => panic!("No SET/output ioctl is allowed: {request:#x}"),
        }
    }
    fn poll(&self, _: i32, _: i32) -> std::io::Result<i16> {
        self.events.lock().unwrap().pop_front().unwrap_or(Ok(0))
    }
    fn read(&self, _: i32, bytes: &mut [u8]) -> std::io::Result<usize> {
        let report = self.reads.lock().unwrap().pop_front().unwrap()?;
        let copied = report.len().min(bytes.len());
        bytes[..copied].copy_from_slice(&report[..copied]);
        Ok(report.len())
    }
}

#[test]
fn kernel_pipe_is_read_only_nonblocking_and_preserves_short_unknown_and_repeated_deliveries() {
    let _serial = SERIAL.lock().unwrap();
    let (read, mut write) = pipe();
    let mut device = Device::from_file(read, key(), Arc::new(native::Native)).unwrap();
    let flags = unsafe { libc::fcntl(device.file().as_raw_fd(), libc::F_GETFL) };
    assert_eq!(flags & libc::O_ACCMODE, libc::O_RDONLY);
    assert_ne!(flags & libc::O_NONBLOCK, 0);
    assert_ne!(
        unsafe { libc::fcntl(device.file().as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
        0
    );
    assert_eq!(device.poll(64, Duration::ZERO), Receive::Timeout);
    for report in [vec![0xfe], vec![3, 0x81, 0x7f], vec![3, 0x81, 0x7f]] {
        write.write_all(&report).unwrap();
        assert_eq!(
            device.poll(64, Duration::from_millis(20)),
            Receive::Report(report)
        );
    }
    write.write_all(&[1, 2, 3, 4, 5]).unwrap();
    assert!(matches!(
        device.poll(4, Duration::ZERO),
        Receive::Error(IoError {
            kind: ErrorKind::Capacity,
            ..
        })
    ));
    assert_eq!(device.metadata().unwrap_err().kind, ErrorKind::Unsupported);
    assert_eq!(
        device
            .get(
                "native-unsupported",
                GetKind::Feature,
                3,
                16,
                Duration::from_secs(1)
            )
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    drop(write);
    let mut eof = [0; 8];
    assert_eq!(
        native::Native
            .read(device.file().as_raw_fd(), &mut eof)
            .unwrap(),
        0
    );
    assert!(native::Native.read(-1, &mut eof).is_err());
    assert_eq!(device.poll(64, Duration::from_millis(20)), Receive::Removed);
}

#[test]
fn metadata_and_gets_use_actual_fd_bytes_without_synthetic_zero_framing() {
    let _serial = SERIAL.lock().unwrap();
    let fake = Arc::new(Fake::new(vec![3, 0x81, 0x7f]));
    let mut device = device(fake.clone());
    let metadata = device.metadata().unwrap();
    assert_eq!(metadata.descriptor, vec![5, 1, 0xc0]);
    assert_eq!(metadata.vendor_id, 0xfedc);
    assert_eq!(metadata.product_id, 0x8123);
    assert_eq!(metadata.name.as_deref(), Some("name"));
    let report = device
        .get(
            "feature-exact",
            GetKind::Feature,
            3,
            8,
            Duration::from_secs(1),
        )
        .unwrap();
    assert_eq!(report.request_id, "feature-exact");
    assert_eq!(report.bytes, vec![3, 0x81, 0x7f]);
    *fake.reply.lock().unwrap() = vec![0x81, 0x7f];
    assert_eq!(
        device
            .get(
                "unnumbered-input",
                GetKind::Input,
                0,
                3,
                Duration::from_secs(1)
            )
            .unwrap()
            .bytes,
        vec![0x81, 0x7f]
    );
    let requests = fake.requests.lock().unwrap();
    let get = requests
        .iter()
        .filter(|(request, _)| matches!(request & 0xff, 7 | 10))
        .collect::<Vec<_>>();
    assert_eq!(get[0].1[0], 3);
    assert_eq!(get[0].1.len(), 8);
    assert_eq!(get[1].1[0], 0);
    assert_eq!(get[1].1.len(), 3);
}

#[test]
fn timeout_retains_fd_and_fences_same_interface_until_old_get_worker_finishes() {
    let _serial = SERIAL.lock().unwrap();
    let fake = Arc::new(Fake::new(vec![3, 1]));
    fake.held.store(true, Ordering::Release);
    let (read, _write) = pipe();
    let path = key();
    let mut selected = Device::from_file(read, path.clone(), fake.clone()).unwrap();
    assert_eq!(
        selected
            .get("expired", GetKind::Input, 3, 8, Duration::ZERO)
            .unwrap_err()
            .kind,
        ErrorKind::Timeout
    );
    let end = Instant::now() + Duration::from_secs(1);
    while !fake.entered.load(Ordering::Acquire) {
        assert!(Instant::now() < end);
        std::thread::yield_now();
    }
    assert_eq!(
        selected
            .get("later", GetKind::Feature, 3, 8, Duration::ZERO)
            .unwrap_err()
            .kind,
        ErrorKind::Busy
    );
    let weak = Arc::downgrade(&selected.lease);
    drop(selected);
    let (replacement, _) = pipe();
    assert!(matches!(
        Device::from_file(replacement, path.clone(), fake.clone()),
        Err(IoError {
            kind: ErrorKind::Busy,
            ..
        })
    ));
    fake.release();
    let end = Instant::now() + Duration::from_secs(1);
    while weak.upgrade().is_some() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
    let (replacement, _) = pipe();
    assert!(Device::from_file(replacement, path, fake).is_ok());
}

#[test]
fn input_bounds_eof_and_transport_errors_remain_distinct() {
    let _serial = SERIAL.lock().unwrap();
    let fake = Arc::new(Fake::new(vec![]));
    let mut device = device(fake.clone());
    fake.events.lock().unwrap().push_back(Ok(libc::POLLIN));
    fake.reads.lock().unwrap().push_back(Ok(vec![]));
    assert!(matches!(
        device.poll(64, Duration::ZERO),
        Receive::Error(IoError {
            kind: ErrorKind::Eof,
            ..
        })
    ));
    fake.events.lock().unwrap().push_back(Ok(libc::POLLERR));
    assert!(matches!(
        device.poll(64, Duration::ZERO),
        Receive::Error(IoError {
            kind: ErrorKind::Io,
            ..
        })
    ));
    fake.events
        .lock()
        .unwrap()
        .push_back(Err(std::io::Error::from_raw_os_error(libc::EACCES)));
    assert!(matches!(
        device.poll(64, Duration::ZERO),
        Receive::Error(IoError {
            kind: ErrorKind::AccessDenied,
            ..
        })
    ));
    assert!(matches!(
        device.poll(0, Duration::ZERO),
        Receive::Error(IoError {
            kind: ErrorKind::Capacity,
            ..
        })
    ));
    assert!(matches!(
        device.poll(MAX_REPORT_BYTES + 1, Duration::ZERO),
        Receive::Error(IoError {
            kind: ErrorKind::Capacity,
            ..
        })
    ));
    assert!(matches!(
        device.poll(64, Duration::from_secs(3)),
        Receive::Error(IoError {
            kind: ErrorKind::InvalidRequest,
            ..
        })
    ));
    for (code, kind) in [
        (libc::ENODEV, ErrorKind::Removed),
        (libc::EIO, ErrorKind::Io),
        (libc::ENOTTY, ErrorKind::Unsupported),
    ] {
        assert_eq!(
            os("fixture", std::io::Error::from_raw_os_error(code)).kind,
            kind
        );
    }
}

#[test]
fn malformed_metadata_get_responses_and_requests_fail_without_output_ioctls() {
    let _serial = SERIAL.lock().unwrap();
    let fake = Arc::new(Fake::new(vec![4, 1]));
    let mut device = device(fake.clone());
    assert_eq!(
        device
            .get("wrong-id", GetKind::Feature, 3, 8, Duration::from_secs(1))
            .unwrap_err()
            .kind,
        ErrorKind::Io
    );
    *fake.returned.lock().unwrap() = Some(0);
    assert!(device
        .get("empty", GetKind::Feature, 3, 8, Duration::from_secs(1))
        .is_err());
    *fake.returned.lock().unwrap() = Some(9);
    assert!(device
        .get("overrun", GetKind::Feature, 3, 8, Duration::from_secs(1))
        .is_err());
    assert!(device
        .get("", GetKind::Feature, 3, 8, Duration::ZERO)
        .is_err());
    assert!(device
        .get("deadline", GetKind::Feature, 3, 8, Duration::from_secs(3))
        .is_err());
    assert!(device
        .get(
            "size",
            GetKind::Feature,
            3,
            MAX_REPORT_BYTES + 1,
            Duration::ZERO
        )
        .is_err());
    *fake.descriptor_size.lock().unwrap() = 4097;
    assert_eq!(device.metadata().unwrap_err().kind, ErrorKind::Capacity);
    *fake.descriptor_size.lock().unwrap() = 3;
    fake.changed.store(true, Ordering::Release);
    assert_eq!(device.metadata().unwrap_err().kind, ErrorKind::Removed);
    let directory = tempfile::tempdir().unwrap();
    let ordinary = directory.path().join("hidraw0");
    std::fs::write(&ordinary, []).unwrap();
    assert!(Device::open(&ordinary).is_err());
}

#[test]
fn interrupted_poll_and_nonblocking_read_race_respect_one_deadline() {
    let _serial = SERIAL.lock().unwrap();
    let fake = Arc::new(Fake::new(vec![]));
    let mut device = device(fake.clone());
    fake.events.lock().unwrap().extend([
        Err(std::io::Error::from_raw_os_error(libc::EINTR)),
        Ok(libc::POLLIN),
        Ok(libc::POLLIN),
    ]);
    fake.reads.lock().unwrap().extend([
        Err(std::io::Error::from_raw_os_error(libc::EAGAIN)),
        Ok(vec![0xfe]),
    ]);
    assert_eq!(
        device.poll(64, Duration::from_millis(20)),
        Receive::Report(vec![0xfe])
    );
    fake.events
        .lock()
        .unwrap()
        .push_back(Err(std::io::Error::from_raw_os_error(libc::EINTR)));
    assert_eq!(device.poll(64, Duration::ZERO), Receive::Timeout);
}

#[test]
fn session_capacity_and_duplicate_interface_admission_are_bounded() {
    let _serial = SERIAL.lock().unwrap();
    let fake = Arc::new(Fake::new(vec![]));
    let mut sessions = vec![];
    for _ in 0..MAX_DEVICES {
        sessions.push(device(fake.clone()));
    }
    let (read, _) = pipe();
    assert!(matches!(
        Device::from_file(read, key(), fake.clone()),
        Err(IoError {
            kind: ErrorKind::Capacity,
            ..
        })
    ));
    drop(sessions);
    assert!(device(fake).metadata().is_ok());
}
