//! One byte owner; cancellation only sets a flag and wakes its OS wait.
use super::*;
use mio::{Events, Interest, Poll, Token, Waker};
const SOCKET: Token = Token(0);
const STOP: Token = Token(1);

pub(crate) struct Cancel { stopped: AtomicBool, wake: Waker }
impl Cancel {
    pub(crate) fn cancel(&self) {
        self.stopped.store(true, Ordering::Release);
        let _ = self.wake.wake();
    }
    fn check(&self) -> std::io::Result<()> {
        if self.stopped.load(Ordering::Acquire) {
            // Rustls retries Interrupted internally; cancellation is terminal.
            Err(std::io::Error::new(std::io::ErrorKind::ConnectionAborted, "Companion peer stopped"))
        } else { Ok(()) }
    }
}
pub(super) struct SocketIo {
    stream: mio::net::TcpStream, poll: Poll, events: Events,
    reading: bool, cancel: Arc<Cancel>,
    #[cfg(test)]
    polls: usize,
}
impl SocketIo {
    pub(super) fn new(stream: TcpStream) -> std::io::Result<(Self, Arc<Cancel>)> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        let mut stream = mio::net::TcpStream::from_std(stream);
        let poll = Poll::new()?;
        poll.registry().register(&mut stream, SOCKET, Interest::READABLE)?;
        let cancel = Arc::new(Cancel { stopped: AtomicBool::new(false), wake: Waker::new(poll.registry(), STOP)? });
        Ok((Self { stream, poll, events: Events::with_capacity(4), reading:true, cancel:cancel.clone(),
            #[cfg(test)] polls:0 }, cancel))
    }
    pub(super) fn peer_addr(&self) -> std::io::Result<SocketAddr> { self.stream.peer_addr() }
    fn remaining(deadline: Option<Instant>) -> std::io::Result<Option<Duration>> {
        deadline.map(|deadline| deadline.checked_duration_since(Instant::now()).filter(|time| !time.is_zero())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::TimedOut, "Companion I/O deadline exceeded"))).transpose()
    }
    fn arm(&mut self, reading: bool) -> std::io::Result<()> {
        if self.reading != reading {
            self.poll.registry().reregister(&mut self.stream, SOCKET,
                if reading { Interest::READABLE } else { Interest::WRITABLE })?;
            self.reading = reading;
        }
        Ok(())
    }
    fn wait(&mut self, deadline: Option<Instant>) -> std::io::Result<()> {
        loop {
            self.cancel.check()?;
            let timeout = Self::remaining(deadline)?;
            #[cfg(test)]
            { self.polls += 1; }
            match self.poll.poll(&mut self.events, timeout) {
                Ok(()) => {},
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
            self.cancel.check()?;
            Self::remaining(deadline)?;
            if self.events.iter().any(|event| event.token() == SOCKET && (event.is_error()
                || if self.reading { event.is_readable() || event.is_read_closed() }
                   else { event.is_writable() || event.is_write_closed() })) { return Ok(()); }
        }
    }
    pub(super) fn read(&mut self, bytes: &mut [u8], deadline: Option<Instant>) -> std::io::Result<usize> {
        self.arm(true)?;
        loop {
            self.cancel.check()?;
            Self::remaining(deadline)?;
            match self.stream.read(bytes) {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => self.wait(deadline)?,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {},
                result => return result,
            }
        }
    }
    pub(super) fn write(&mut self, bytes: &[u8], deadline: Option<Instant>) -> std::io::Result<usize> {
        self.arm(false)?;
        loop {
            self.cancel.check()?;
            Self::remaining(deadline)?;
            match self.stream.write(bytes) {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => self.wait(deadline)?,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {},
                result => return result,
            }
        }
    }
    pub(super) fn flush(&mut self) -> std::io::Result<()> { self.cancel.check()?; self.stream.flush() }
}
#[cfg(test)]
#[path = "socket_io_tests.rs"]
mod tests;
