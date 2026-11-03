//! Winsock cancellation uses a persistent native stop event, not shutdown of a
//! duplicated blocking socket. RX/TX own separate OVERLAPPED completion events.
use super::super::*;
use std::{io, net::TcpStream, os::windows::io::AsRawSocket, ptr,
    sync::{atomic::{AtomicBool, Ordering}, Weak}};
use windows_sys::Win32::{Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Networking::WinSock::{WSAGetLastError, WSAGetOverlappedResult, WSARecv, WSASend, WSABUF, WSA_IO_PENDING},
    System::{IO::{CancelIoEx, OVERLAPPED}, Threading::{CreateEventW, SetEvent, WaitForMultipleObjects}}};

struct Event(HANDLE);
unsafe impl Send for Event {}
unsafe impl Sync for Event {}
impl Drop for Event { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
fn event() -> Result<Event, String> {
    let handle = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
    if handle.is_null() { Err(io::Error::last_os_error().to_string()) } else { Ok(Event(handle)) }
}
fn socket_error(context: &str) -> String {
    format!("{context}: {}", io::Error::from_raw_os_error(unsafe { WSAGetLastError() }))
}
struct Shared { socket: TcpStream, stop: Event, stopped: AtomicBool }
impl Shared {
    fn check(&self) -> Result<(), String> {
        if self.stopped.load(Ordering::Acquire) { Err(CANCELLED.into()) } else { Ok(()) }
    }
    fn cancel_operation(&self, operation: *mut OVERLAPPED) {
        unsafe {
            CancelIoEx(self.socket.as_raw_socket() as HANDLE, operation);
            let mut count = 0;let mut flags = 0;
            WSAGetOverlappedResult(self.socket.as_raw_socket() as usize, operation, &mut count, 1, &mut flags);
        }
    }
    fn wait(&self, operation: *mut OVERLAPPED, completed: HANDLE, deadline: Option<Instant>) -> Result<usize, String> {
        let handles = [self.stop.0, completed];
        loop {
            let timeout = match deadline {
                None => u32::MAX,
                Some(at) => {
                    let Some(remaining) = at.checked_duration_since(Instant::now()) else {
                        self.cancel_operation(operation);return Err(WRITE_TIMEOUT.into());
                    };
                    if remaining.is_zero() { self.cancel_operation(operation);return Err(WRITE_TIMEOUT.into()); }
                    remaining.as_millis().saturating_add(u128::from(remaining.subsec_nanos() % 1_000_000 != 0))
                        .min((u32::MAX - 1) as u128) as u32
                }
            };
            let result = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, timeout) };
            if result == WAIT_OBJECT_0 || self.stopped.load(Ordering::Acquire) {
                self.cancel_operation(operation);return Err(CANCELLED.into());
            }
            if result == WAIT_OBJECT_0 + 1 {
                let mut count = 0;let mut flags = 0;
                if unsafe { WSAGetOverlappedResult(self.socket.as_raw_socket() as usize, operation, &mut count, 0, &mut flags) } == 0 {
                    return Err(socket_error("USB socket overlapped I/O failed"));
                }
                return Ok(count as usize);
            }
            if result == WAIT_TIMEOUT { continue; }
            let failure = format!("USB socket native wait failed: {}", io::Error::last_os_error());
            self.cancel_operation(operation);return Err(failure);
        }
    }
    fn transfer(&self, bytes: *mut u8, length: usize, write: bool, deadline: Option<Instant>) -> Result<usize, String> {
        self.check()?;
        if deadline.is_some_and(|at| Instant::now() >= at) { return Err(WRITE_TIMEOUT.into()); }
        let completed = event()?;
        let mut operation: OVERLAPPED = unsafe { std::mem::zeroed() };operation.hEvent = completed.0;
        let operation = ptr::addr_of_mut!(operation);
        let buffer = WSABUF { len: length.min(u32::MAX as usize) as u32, buf: bytes };
        let mut count = 0;let mut flags = 0;
        let result = unsafe { if write {
            WSASend(self.socket.as_raw_socket() as usize, &buffer, 1, &mut count, 0, operation, None)
        } else {
            WSARecv(self.socket.as_raw_socket() as usize, &buffer, 1, &mut count, &mut flags, operation, None)
        } };
        if result != 0 && unsafe { WSAGetLastError() } != WSA_IO_PENDING {
            return Err(socket_error("USB socket overlapped submission failed"));
        }
        // Buffer and OVERLAPPED storage stay live until wait observes a terminal
        // completion, including when cancellation/deadline races normal success.
        self.wait(operation, completed.0, deadline)
    }
}
struct Cancel(Weak<Shared>);
impl CancelIo for Cancel {
    fn cancel(&self) { if let Some(shared) = self.0.upgrade() {
        shared.stopped.store(true, Ordering::Release);unsafe { SetEvent(shared.stop.0); }
    } }
}
struct Receiver(Arc<Shared>);
impl BlockingReceiver for Receiver {
    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, String> {
        self.0.check()?;if buffer.is_empty() { return Ok(0); }
        self.0.transfer(buffer.as_mut_ptr(), buffer.len(), false, None)
    }
}
struct Writer(Arc<Shared>);
impl DeadlineWriter for Writer {
    fn write_all(&mut self, mut bytes: &[u8], deadline: Instant) -> Result<(), String> {
        while !bytes.is_empty() {
            let count = self.0.transfer(bytes.as_ptr().cast_mut(), bytes.len(), true, Some(deadline))?;
            if count == 0 { return Err("USB socket closed during write; its result is unknown.".into()); }
            bytes = &bytes[count..];
        }
        self.0.check()
    }
    fn control_lines(&mut self, _: bool, _: bool) -> Result<(), String> { self.0.check() }
}
pub(super) fn parts(socket: TcpStream) -> Result<DuplexParts, String> {
    let shared = Arc::new(Shared { socket, stop:event()?, stopped:AtomicBool::new(false) });
    Ok(DuplexParts { receiver:Box::new(Receiver(shared.clone())), writer:Box::new(Writer(shared.clone())),
        cancel:Arc::new(Cancel(Arc::downgrade(&shared))) })
}
