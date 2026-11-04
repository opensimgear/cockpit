use super::*;
use serialport::SerialPort;
use std::{io::{self, Write}, os::{fd::{AsRawFd, RawFd}, unix::net::UnixStream},
    sync::{atomic::{AtomicBool, Ordering}, Mutex, Weak}};

struct Shared {
    serial: Mutex<serialport::TTYPort>,
    fd: RawFd,
    stop_read: UnixStream,
    stop_write: UnixStream,
    stopped: AtomicBool,
}
impl Shared {
    fn check(&self) -> Result<(), String> {
        if self.stopped.load(Ordering::Acquire) { Err(CANCELLED.into()) } else { Ok(()) }
    }
    fn wait(&self, write: bool, deadline: Option<Instant>) -> Result<(), String> {
        loop {
            self.check()?;
            let timeout = match deadline {
                None => -1,
                Some(at) => {
                    let remaining = at.checked_duration_since(Instant::now()).ok_or(WRITE_TIMEOUT)?;
                    if remaining.is_zero() { return Err(WRITE_TIMEOUT.into()); }
                    remaining.as_millis().saturating_add(u128::from(remaining.subsec_nanos() % 1_000_000 != 0))
                        .min(i32::MAX as u128) as i32
                }
            };
            let mut descriptors = [libc::pollfd { fd: self.stop_read.as_raw_fd(), events: libc::POLLIN, revents: 0 },
                libc::pollfd { fd: self.fd, events: if write { libc::POLLOUT } else { libc::POLLIN }, revents: 0 }];
            let result = unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as libc::nfds_t, timeout) };
            self.check()?;
            if result < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted { continue; }
                return Err(format!("USB native readiness wait failed: {error}"));
            }
            if result == 0 { continue; } // Recompute the real write deadline after clamping/EINTR.
            if descriptors[0].revents != 0 { return Err(CANCELLED.into()); }
            let ready = descriptors[1].revents;
            // A final readable frame may accompany hangup; drain it before
            // reporting transport loss. Cancellation above still wins.
            if !write && ready & libc::POLLIN != 0 { return Ok(()); }
            if ready & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 { return Err("USB serial endpoint disconnected.".into()); }
            if ready & descriptors[1].events != 0 { return Ok(()); }
        }
    }
}
impl Drop for Shared {
    fn drop(&mut self) {
        let serial = self.serial.get_mut().unwrap_or_else(|p| p.into_inner());
        let _ = serial.write_data_terminal_ready(false);let _ = serial.write_request_to_send(false);
    }
}
struct Cancel(Weak<Shared>);
impl CancelIo for Cancel {
    fn cancel(&self) { if let Some(shared) = self.0.upgrade() {
        if !shared.stopped.swap(true, Ordering::AcqRel) {
            // One byte stays readable forever; both RX and TX waits see it.
            let mut signal = &shared.stop_write;let _ = signal.write_all(&[1]);
        }
    } }
}
struct Receiver(Arc<Shared>);
impl BlockingReceiver for Receiver {
    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, String> {
        self.0.check()?;if buffer.is_empty() { return Ok(0); }
        loop {
            self.0.wait(false, None)?;
            let count = unsafe { libc::read(self.0.fd, buffer.as_mut_ptr().cast(), buffer.len()) };
            self.0.check()?;
            if count >= 0 { return Ok(count as usize); }
            let error = io::Error::last_os_error();
            if matches!(error.kind(), io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock) { continue; }
            return Err(format!("USB serial receive failed: {error}"));
        }
    }
}
struct Writer(Arc<Shared>);
impl DeadlineWriter for Writer {
    fn write_all(&mut self, mut bytes: &[u8], deadline: Instant) -> Result<(), String> {
        while !bytes.is_empty() {
            self.0.wait(true, Some(deadline))?;
            let count = unsafe { libc::write(self.0.fd, bytes.as_ptr().cast(), bytes.len()) };
            self.0.check()?;
            if count > 0 { bytes = &bytes[count as usize..];continue; }
            if count == 0 { return Err("USB serial write made no progress; its result is unknown.".into()); }
            let error = io::Error::last_os_error();
            if matches!(error.kind(), io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock) { continue; }
            return Err(format!("USB serial write failed; its result is unknown: {error}"));
        }
        self.0.check()
    }
    fn control_lines(&mut self, dtr: bool, rts: bool) -> Result<(), String> {
        self.0.check()?;
        let mut serial = self.0.serial.lock().unwrap_or_else(|p| p.into_inner());
        serial.write_data_terminal_ready(dtr).and_then(|_| serial.write_request_to_send(rts)).map_err(|error| error.to_string())
    }
}
fn parts(serial: serialport::TTYPort) -> Result<DuplexParts, String> {
    let fd = serial.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(format!("Could not configure USB readiness I/O: {}", io::Error::last_os_error()));
    }
    let (stop_read, stop_write) = UnixStream::pair().map_err(|error| error.to_string())?;
    let shared = Arc::new(Shared { serial: Mutex::new(serial), fd, stop_read, stop_write, stopped: AtomicBool::new(false) });
    Ok(DuplexParts { receiver: Box::new(Receiver(shared.clone())), writer: Box::new(Writer(shared.clone())),
        cancel: Arc::new(Cancel(Arc::downgrade(&shared))) })
}
pub(super) fn open(path: &str, baud: u32) -> Result<DuplexParts, String> {
    let serial = serialport::new(path, baud).data_bits(serialport::DataBits::Eight)
        .parity(serialport::Parity::None).stop_bits(serialport::StopBits::One).flow_control(serialport::FlowControl::None)
        .open_native().map_err(|error| error.to_string())?;
    parts(serial)
}

#[cfg(test)]
pub(super) fn fixture() -> (DuplexParts, serialport::TTYPort) {
    let (serial, peer) = serialport::TTYPort::pair().unwrap();
    (parts(serial).unwrap(), peer)
}

#[cfg(test)]
#[path = "unix_tests.rs"]
mod tests;
