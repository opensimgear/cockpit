use super::*;
use std::net::TcpStream;
#[cfg(unix)]
use std::{net::SocketAddr,time::Duration};
#[cfg(not(windows))]
use std::{io::{self, Read, Write}, net::Shutdown, sync::{atomic::{AtomicBool, Ordering}, Weak}};
#[cfg(all(test, windows))]
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(windows)]
#[path = "windows_sockets.rs"]
mod native;

enum Socket { Tcp(TcpStream), #[cfg(unix)] Unix(UnixStream) }
#[cfg(not(windows))]
impl Socket {
    fn clone_socket(&self) -> io::Result<Self> { match self {
        Self::Tcp(stream) => stream.try_clone().map(Self::Tcp),
        #[cfg(unix)] Self::Unix(stream) => stream.try_clone().map(Self::Unix),
    } }
    fn shutdown(&self) { let _ = match self {
        Self::Tcp(stream) => stream.shutdown(Shutdown::Both),
        #[cfg(unix)] Self::Unix(stream) => stream.shutdown(Shutdown::Both),
    }; }
    fn read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> { match self {
        Self::Tcp(stream) => stream.set_read_timeout(timeout),
        #[cfg(unix)] Self::Unix(stream) => stream.set_read_timeout(timeout),
    } }
    fn write_timeout(&self, timeout: Duration) -> io::Result<()> { match self {
        Self::Tcp(stream) => stream.set_write_timeout(Some(timeout)),
        #[cfg(unix)] Self::Unix(stream) => stream.set_write_timeout(Some(timeout)),
    } }
}
#[cfg(not(windows))]
impl Read for Socket {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> { match self {
        Self::Tcp(stream) => stream.read(bytes),
        #[cfg(unix)] Self::Unix(stream) => stream.read(bytes),
    } }
}
#[cfg(not(windows))]
impl Write for Socket {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> { match self {
        Self::Tcp(stream) => stream.write(bytes),
        #[cfg(unix)] Self::Unix(stream) => stream.write(bytes),
    } }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}
#[cfg(not(windows))]
struct Shared { socket: Socket, stopped: AtomicBool }
#[cfg(not(windows))]
struct Cancel(Weak<Shared>);
#[cfg(not(windows))]
impl CancelIo for Cancel {
    fn cancel(&self) { if let Some(shared) = self.0.upgrade() {
        if !shared.stopped.swap(true, Ordering::AcqRel) { shared.socket.shutdown(); }
    } }
}
#[cfg(not(windows))]
struct Receiver { socket: Socket, shared: Arc<Shared> }
#[cfg(not(windows))]
impl BlockingReceiver for Receiver {
    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, String> {
        loop {
            if self.shared.stopped.load(Ordering::Acquire) { return Err(CANCELLED.into()); }
            match self.socket.read(buffer) {
                Ok(_) if self.shared.stopped.load(Ordering::Acquire) => return Err(CANCELLED.into()),
                Ok(count) => return Ok(count),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(format!("USB socket receive failed: {error}")),
            }
        }
    }
}
#[cfg(not(windows))]
struct Writer { socket: Socket, shared: Arc<Shared> }
#[cfg(not(windows))]
impl DeadlineWriter for Writer {
    fn write_all(&mut self, mut bytes: &[u8], deadline: Instant) -> Result<(), String> {
        while !bytes.is_empty() {
            if self.shared.stopped.load(Ordering::Acquire) { return Err(CANCELLED.into()); }
            let remaining = deadline.checked_duration_since(Instant::now()).ok_or(WRITE_TIMEOUT)?;
            if remaining.is_zero() { return Err(WRITE_TIMEOUT.into()); }
            self.socket.write_timeout(remaining).map_err(|error| error.to_string())?;
            match self.socket.write(bytes) {
                Ok(0) => return Err("USB socket closed during write; its result is unknown.".into()),
                Ok(count) => bytes = &bytes[count..],
                Err(error) if matches!(error.kind(), io::ErrorKind::Interrupted | io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => continue,
                Err(error) => return Err(format!("USB socket write failed; its result is unknown: {error}")),
            }
        }
        if self.shared.stopped.load(Ordering::Acquire) { Err(CANCELLED.into()) } else { Ok(()) }
    }
    fn control_lines(&mut self, _: bool, _: bool) -> Result<(), String> { Ok(()) }
}
fn parts(socket: Socket) -> Result<DuplexParts, String> {
    #[cfg(windows)]
    { let Socket::Tcp(socket) = socket;native::parts(socket) }
    #[cfg(not(windows))]
    {
    socket.read_timeout(None).map_err(|error| error.to_string())?;
    let receiver = socket.clone_socket().map_err(|error| error.to_string())?;
    let writer = socket.clone_socket().map_err(|error| error.to_string())?;
    let shared = Arc::new(Shared { socket, stopped: AtomicBool::new(false) });
    Ok(DuplexParts { receiver: Box::new(Receiver { socket: receiver, shared: shared.clone() }),
        writer: Box::new(Writer { socket: writer, shared: shared.clone() }), cancel: Arc::new(Cancel(Arc::downgrade(&shared))) })
    }
}
#[cfg(not(unix))]
pub(super) fn open(_: &PortInfo) -> Result<DuplexParts, String> {
    Err("Local gateway advertisements are unavailable on this platform.".into())
}
#[cfg(unix)]
pub(super) fn open(port: &PortInfo) -> Result<DuplexParts, String> {
    // Match the existing local transport's namespace, private advertisement and
    // identity validation before opening either socket kind.
    if !crate::local_gateway::allowed_path(&port.path) || !crate::local_gateway::ports().iter()
        .any(|observed| observed.path == port.path && observed.serial_number == port.serial_number)
    { return Err("The local gateway is no longer validly advertised.".into()); }
    if let Some(endpoint) = port.path.strip_prefix("tcp:127.0.0.1:") {
        let (number, serial) = endpoint.split_once('/').ok_or("Invalid local TCP endpoint.")?;
        if port.serial_number.as_deref() != Some(serial) { return Err("Local TCP identity mismatch.".into()); }
        let address: SocketAddr = format!("127.0.0.1:{number}").parse().map_err(|_| "Invalid local TCP port.")?;
        let socket = TcpStream::connect_timeout(&address, Duration::from_secs(3)).map_err(|error| error.to_string())?;
        socket.set_nodelay(true).map_err(|error| error.to_string())?;
        return parts(Socket::Tcp(socket));
    }
    #[cfg(unix)]
    if let Some(path) = port.path.strip_prefix("unix:") {
        return parts(Socket::Unix(UnixStream::connect(path).map_err(|error| error.to_string())?));
    }
    Err("Unsupported local duplex endpoint.".into())
}

#[cfg(test)]
pub(super) fn tcp_fixture() -> (DuplexParts, TcpStream) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    socket.set_nodelay(true).unwrap();
    let (peer, _) = listener.accept().unwrap();
    socket2::SockRef::from(&socket).set_send_buffer_size(4096).unwrap();
    socket2::SockRef::from(&peer).set_recv_buffer_size(4096).unwrap();
    (parts(Socket::Tcp(socket)).unwrap(), peer)
}

#[cfg(test)]
pub(super) fn saturated_tcp_fixture() -> (DuplexParts, TcpStream) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    socket2::SockRef::from(&listener).set_recv_buffer_size(4096).unwrap();
    let mut socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    socket2::SockRef::from(&socket).set_send_buffer_size(4096).unwrap();
    let (peer, _) = listener.accept().unwrap();
    socket.set_nonblocking(true).unwrap();
    let mut filled = 0;
    loop {
        match socket.write(&[0; 4096]) {
            Ok(0) => panic!("test socket closed before saturation"),
            Ok(count) => {filled += count;assert!(filled < 64 * 1024 * 1024, "test could not establish TCP backpressure");}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) => panic!("test saturation failed: {error}"),
        }
    }
    socket.set_nonblocking(false).unwrap();
    (parts(Socket::Tcp(socket)).unwrap(), peer)
}

#[cfg(test)]
#[cfg(unix)]
pub(super) fn unix_fixture() -> (DuplexParts, UnixStream) {
    let (socket, peer) = UnixStream::pair().unwrap();
    (parts(Socket::Unix(socket)).unwrap(), peer)
}
