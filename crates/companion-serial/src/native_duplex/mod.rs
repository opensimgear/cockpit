//! Native serial reads wait for data or cancellation. Writes use deadlines.
use std::{sync::Arc, time::Instant};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

pub trait BlockingReceiver: Send {
    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, String>;
}
pub trait DeadlineWriter: Send {
    fn write_all(&mut self, bytes: &[u8], deadline: Instant) -> Result<(), String>;
    fn control_lines(&mut self, dtr: bool, rts: bool) -> Result<(), String>;
}
pub trait CancelIo: Send + Sync {
    fn cancel(&self);
}
pub struct DuplexParts {
    pub receiver: Box<dyn BlockingReceiver>,
    pub writer: Box<dyn DeadlineWriter>,
    pub cancel: Arc<dyn CancelIo>,
}

/// Open a serial path validated by the caller against its device inventory.
pub fn open(path: &str, baud: u32) -> Result<DuplexParts, String> {
    #[cfg(windows)]
    { windows::open(path, baud) }
    #[cfg(unix)]
    { unix::open(path, baud) }
    #[cfg(not(any(windows, unix)))]
    { Err("Native USB duplex I/O is unavailable on this platform.".into()) }
}

pub(super) const FRAMING_ERROR: &str = "USB serial receive reported a framing, parity or overrun error.";
const CANCELLED: &str = "USB duplex connection was cancelled.";
const WRITE_TIMEOUT: &str = "USB duplex write exceeded its deadline; its result is unknown.";
