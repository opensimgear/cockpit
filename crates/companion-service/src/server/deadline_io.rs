use super::*;
enum Transport { Blocking(TcpStream), Event(socket_io::SocketIo) }

/// Client blocking I/O and server event I/O share absolute handshake and read
/// budgets. Read expiry never poisons event-only writes.
pub struct DeadlineTcp {
    transport: Transport, deadline: Option<Instant>, read_deadline: Option<Instant>,
    write_deadline: Option<Instant>,
}
pub(crate) trait ReadDeadline {
    fn set_read_deadline(&mut self, deadline: Option<Instant>) -> std::io::Result<()>;
}
impl DeadlineTcp {
    pub fn new(inner: TcpStream) -> Self {
        Self { transport:Transport::Blocking(inner), deadline:Some(Instant::now()+Duration::from_secs(3)),
            read_deadline:None, write_deadline:None }
    }
    pub(super) fn event(inner: TcpStream) -> std::io::Result<(Self, Arc<socket_io::Cancel>)> {
        let deadline = Some(Instant::now()+Duration::from_secs(3));
        let (io, cancel) = socket_io::SocketIo::new(inner)?;
        Ok((Self { transport:Transport::Event(io), deadline, read_deadline:None, write_deadline:None }, cancel))
    }
    pub(super) fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        match &self.transport { Transport::Blocking(io)=>io.peer_addr(), Transport::Event(io)=>io.peer_addr() }
    }
    pub fn upgraded(&mut self) { self.deadline = None; }
    pub fn set_read_timeout(&self, value: Option<Duration>) -> std::io::Result<()> {
        match &self.transport { Transport::Blocking(io)=>io.set_read_timeout(value), Transport::Event(_)=>Ok(()) }
    }
    fn remaining(deadline: Instant) -> std::io::Result<Duration> {
        deadline.checked_duration_since(Instant::now()).filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::TimedOut,"Companion I/O deadline exceeded"))
    }
    fn check(&self) -> std::io::Result<()> { self.deadline.map(Self::remaining).transpose().map(|_|()) }
}
impl ReadDeadline for DeadlineTcp {
    fn set_read_deadline(&mut self, deadline: Option<Instant>) -> std::io::Result<()> {
        self.read_deadline = deadline;
        let timeout = self.deadline.into_iter().chain(deadline).min().map(Self::remaining).transpose()?;
        if let Transport::Blocking(io) = &self.transport { io.set_read_timeout(timeout)?; }
        Ok(())
    }
}
impl Read for DeadlineTcp {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let deadline = self.deadline.into_iter().chain(self.read_deadline).min();
        match &mut self.transport {
            Transport::Blocking(io)=>{
                if let Some(deadline)=deadline { io.set_read_timeout(Some(Self::remaining(deadline)?))?; }
                io.read(buffer)
            },
            Transport::Event(io)=>io.read(buffer,deadline),
        }
    }
}
impl Write for DeadlineTcp {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.check()?;
        match &mut self.transport {
            Transport::Blocking(io)=>io.write(bytes),
            Transport::Event(io)=>{
                let deadline = *self.write_deadline.get_or_insert_with(||Instant::now()+Duration::from_millis(500));
                io.write(bytes,Some(self.deadline.map_or(deadline,|handshake|handshake.min(deadline))))
            },
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.check()?;
        match &mut self.transport {
            Transport::Blocking(io)=>io.flush(),
            Transport::Event(io)=>{
                if let Some(deadline)=self.write_deadline { Self::remaining(deadline)?; }
                io.flush()?;self.write_deadline=None;Ok(())
            },
        }
    }
}
impl ReadDeadline for crate::browser::Replay<BudgetIo<StreamOwned<ServerConnection, DeadlineTcp>>> {
    fn set_read_deadline(&mut self, deadline: Option<Instant>) -> std::io::Result<()> {
        self.stream.inner.sock.set_read_deadline(deadline)
    }
}
#[cfg(test)]
#[path = "deadline_io_tests.rs"]
mod tests;
