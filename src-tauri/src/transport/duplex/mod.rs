//! Standalone USB duplex I/O. Receivers wait for native input or cancellation;
//! only writes have operation deadlines. No read timeout/retry cadence exists.
use crate::transport::PortInfo;
use std::{sync::Arc, time::Instant};
mod sockets;

pub(crate) use osg_companion_serial::native_duplex::{BlockingReceiver, CancelIo, DeadlineWriter, DuplexParts};
pub(crate) fn open(port: &PortInfo) -> Result<DuplexParts, String> {
    if port.path.starts_with("unix:") || port.path.starts_with("tcp:") { return sockets::open(port); }
    if !crate::transport::allowed_path(&port.path) { return Err("Invalid USB serial path.".into()); }
    osg_companion_serial::native_duplex::open(&port.path, 115_200)
}
const CANCELLED: &str = "USB duplex connection was cancelled.";
const WRITE_TIMEOUT: &str = "USB duplex write exceeded its deadline; its result is unknown.";

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) fn tcp_fixture()->(DuplexParts,std::net::TcpStream){sockets::tcp_fixture()}
