use super::*;
use std::{io, ptr, sync::{atomic::{AtomicBool, Ordering}, Weak}};
use windows_sys::Win32::{
    Devices::Communication::{ClearCommError, EscapeCommFunction, GetCommState, SetCommState, SetCommTimeouts,
        CLRDTR, CLRRTS, COMMTIMEOUTS, COMSTAT, DCB, NOPARITY, ONESTOPBIT, SETDTR, SETRTS},
    Foundation::{CloseHandle, GetLastError, ERROR_IO_PENDING, HANDLE, INVALID_HANDLE_VALUE,
        GENERIC_READ, GENERIC_WRITE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Storage::FileSystem::{CreateFileW, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OVERLAPPED, OPEN_EXISTING},
    System::{IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED}, Threading::{CreateEventW, SetEvent, WaitForMultipleObjects}},
};

struct Handle(HANDLE);
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
fn error(context: &str) -> String { format!("{context}: {}", io::Error::last_os_error()) }
fn event() -> Result<Handle, String> {
    let handle = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
    if handle.is_null() { Err(error("Could not create USB I/O event")) } else { Ok(Handle(handle)) }
}
struct Shared { port: Handle, stop: Handle, stopped: AtomicBool }
impl Shared {
    fn check(&self) -> Result<(), String> {
        if self.stopped.load(Ordering::Acquire) { Err(CANCELLED.into()) } else { Ok(()) }
    }
    fn wait(&self, operation: *mut OVERLAPPED, completed: HANDLE, deadline: Option<Instant>) -> Result<u32, String> {
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
            let outcome = unsafe { WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, timeout) };
            if outcome == WAIT_OBJECT_0 || self.stopped.load(Ordering::Acquire) {
                self.cancel_operation(operation);return Err(CANCELLED.into());
            }
            if outcome == WAIT_OBJECT_0 + 1 {
                let mut count = 0;
                if unsafe { GetOverlappedResult(self.port.0, operation, &mut count, 0) } == 0 {
                    return Err(error("USB overlapped I/O failed"));
                }
                return Ok(count);
            }
            if outcome == WAIT_TIMEOUT { continue; } // Recompute the actual deadline after clamping.
            let failure = if outcome == WAIT_FAILED { error("USB native I/O wait failed") }
                else { "USB native I/O wait returned an invalid result.".into() };
            self.cancel_operation(operation);return Err(failure);
        }
    }
    fn cancel_operation(&self, operation: *mut OVERLAPPED) {
        unsafe {
            // ERROR_NOT_FOUND may mean normal completion won the race. In all
            // cases observe terminal completion before freeing OVERLAPPED/buffer.
            CancelIoEx(self.port.0, operation);
            let mut ignored = 0;GetOverlappedResult(self.port.0, operation, &mut ignored, 1);
        }
    }
    fn read(&self, bytes: &mut [u8]) -> Result<usize, String> {
        self.check()?;
        let complete = event()?;
        let mut operation: OVERLAPPED = unsafe { std::mem::zeroed() };operation.hEvent = complete.0;
        // The kernel retains this pointer until terminal completion. Do not
        // create Rust references to its asynchronously updated fields meanwhile.
        let operation = ptr::addr_of_mut!(operation);
        let mut count = 0;
        let result = unsafe { ReadFile(self.port.0, bytes.as_mut_ptr(), bytes.len().min(u32::MAX as usize) as u32,
            &mut count, operation) };
        if result == 0 && unsafe { GetLastError() } != ERROR_IO_PENDING { return Err(error("USB overlapped read failed")); }
        self.wait(operation, complete.0, None).map(|count| count as usize)
    }
    fn write(&self, bytes: &[u8], deadline: Instant) -> Result<usize, String> {
        self.check()?;
        if Instant::now() >= deadline { return Err(WRITE_TIMEOUT.into()); }
        let complete = event()?;
        let mut operation: OVERLAPPED = unsafe { std::mem::zeroed() };operation.hEvent = complete.0;
        let operation = ptr::addr_of_mut!(operation);
        let mut count = 0;
        let result = unsafe { WriteFile(self.port.0, bytes.as_ptr(), bytes.len().min(u32::MAX as usize) as u32,
            &mut count, operation) };
        if result == 0 && unsafe { GetLastError() } != ERROR_IO_PENDING { return Err(error("USB overlapped write failed; its result is unknown")); }
        self.wait(operation, complete.0, Some(deadline)).map(|count| count as usize)
    }
}
impl Drop for Shared {
    fn drop(&mut self) { unsafe { EscapeCommFunction(self.port.0, CLRDTR);EscapeCommFunction(self.port.0, CLRRTS); } }
}
struct Cancel(Weak<Shared>);
impl CancelIo for Cancel {
    fn cancel(&self) { if let Some(shared) = self.0.upgrade() {
        shared.stopped.store(true, Ordering::Release);
        unsafe { SetEvent(shared.stop.0); }
    } }
}
struct Receiver(Arc<Shared>);
impl BlockingReceiver for Receiver {
    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, String> {
        self.0.check()?;if buffer.is_empty() { return Ok(0); }
        // With timeout-free COM reads, requesting one byte guarantees immediate
        // delivery on arrival rather than waiting to fill a large caller buffer.
        let count = self.0.read(&mut buffer[..1])?;
        if count == 0 || buffer.len() == 1 { return Ok(count); }
        let mut status: COMSTAT = unsafe { std::mem::zeroed() };let mut errors = 0;
        if unsafe { ClearCommError(self.0.port.0, &mut errors, &mut status) } == 0 { return Err(error("Could not inspect received USB bytes")); }
        if errors != 0 { return Err("USB serial receive reported a framing, parity or overrun error.".into()); }
        let ready = (status.cbInQue as usize).min(buffer.len() - 1);
        if ready == 0 { self.0.check()?;return Ok(1); }
        self.0.read(&mut buffer[1..1 + ready]).map(|count| count + 1)
    }
}
struct Writer(Arc<Shared>);
impl DeadlineWriter for Writer {
    fn write_all(&mut self, mut bytes: &[u8], deadline: Instant) -> Result<(), String> {
        while !bytes.is_empty() {
            let count = self.0.write(bytes, deadline)?;
            if count == 0 { return Err("USB serial write made no progress; its result is unknown.".into()); }
            bytes = &bytes[count..];
        }
        self.0.check()
    }
    fn control_lines(&mut self, dtr: bool, rts: bool) -> Result<(), String> {
        self.0.check()?;
        if unsafe { EscapeCommFunction(self.0.port.0, if dtr { SETDTR } else { CLRDTR }) } == 0
            || unsafe { EscapeCommFunction(self.0.port.0, if rts { SETRTS } else { CLRRTS }) } == 0
        { return Err(error("Could not set USB control lines")); }
        Ok(())
    }
}
pub(super) fn open(path: &str, baud: u32) -> Result<DuplexParts, String> {
    let path = if path.starts_with('\\') { path.to_owned() } else { format!("\\\\.\\{}", path) };
    let path = path.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let port = unsafe { CreateFileW(path.as_ptr(), GENERIC_READ | GENERIC_WRITE, 0, ptr::null(), OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED, ptr::null_mut()) };
    if port == INVALID_HANDLE_VALUE { return Err(error("Could not open USB duplex port")); }
    let port = Handle(port);
    let mut dcb: DCB = unsafe { std::mem::zeroed() };dcb.DCBlength = std::mem::size_of::<DCB>() as u32;
    if unsafe { GetCommState(port.0, &mut dcb) } == 0 { return Err(error("Could not read USB serial configuration")); }
    dcb.BaudRate = baud;dcb.ByteSize = 8;dcb.Parity = NOPARITY;dcb.StopBits = ONESTOPBIT;
    dcb._bitfield = 1; // Binary; no parity, software/hardware flow, DTR or RTS handshake.
    if unsafe { SetCommState(port.0, &dcb) } == 0 { return Err(error("Could not configure USB serial 8N1")); }
    let timeouts: COMMTIMEOUTS = unsafe { std::mem::zeroed() };
    if unsafe { SetCommTimeouts(port.0, &timeouts) } == 0 { return Err(error("Could not disable USB serial polling timeouts")); }
    let shared = Arc::new(Shared { port, stop: event()?, stopped: AtomicBool::new(false) });
    Ok(DuplexParts { receiver: Box::new(Receiver(shared.clone())), writer: Box::new(Writer(shared.clone())),
        cancel: Arc::new(Cancel(Arc::downgrade(&shared))) })
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
