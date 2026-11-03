//! Standalone USB duplex I/O. Receivers wait for native input or cancellation;
//! only writes have operation deadlines. No read timeout/retry cadence exists.
use crate::transport::PortInfo;
use std::{sync::Arc, time::Instant};
#[cfg(windows)]
mod windows;
#[cfg(unix)]
mod unix;
mod sockets;

pub(crate) trait BlockingReceiver: Send {
    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, String>;
}
pub(crate) trait DeadlineWriter: Send {
    fn write_all(&mut self, bytes: &[u8], deadline: Instant) -> Result<(), String>;
    fn control_lines(&mut self, dtr: bool, rts: bool) -> Result<(), String>;
}
pub(crate) trait CancelIo: Send + Sync { fn cancel(&self); }
pub(crate) struct DuplexParts {
    pub receiver: Box<dyn BlockingReceiver>,
    pub writer: Box<dyn DeadlineWriter>,
    pub cancel: Arc<dyn CancelIo>,
}
pub(crate) fn open(port: &PortInfo) -> Result<DuplexParts, String> {
    if port.path.starts_with("unix:") || port.path.starts_with("tcp:") { return sockets::open(port); }
    if !crate::transport::allowed_path(&port.path) { return Err("Invalid USB serial path.".into()); }
    #[cfg(windows)]
    { windows::open(port) }
    #[cfg(unix)]
    { unix::open(port) }
    #[cfg(not(any(windows, unix)))]
    { Err("Native USB duplex I/O is unavailable on this platform.".into()) }
}
const CANCELLED: &str = "USB duplex connection was cancelled.";
const WRITE_TIMEOUT: &str = "USB duplex write exceeded its deadline; its result is unknown.";

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) fn tcp_fixture()->(DuplexParts,std::net::TcpStream){sockets::tcp_fixture()}
