//! One readiness owner for accepts, committed deadline changes and Stop.
use super::*;
use mio::{Events, Interest, Poll, Token, Waker};

const SOCKET: Token = Token(0);
const CHANGE: Token = Token(1);
const ACCEPT_BATCH: usize = 16;

pub(super) struct Readiness {
    poll: Poll,
    listener: mio::net::TcpListener,
    events: Events,
    wake: Arc<Waker>,
}

impl Readiness {
    pub(super) fn new(listener: TcpListener, hub: &Hub) -> Result<Self, String> {
        let poll = Poll::new().map_err(|error| error.to_string())?;
        let mut listener = mio::net::TcpListener::from_std(listener);
        poll.registry().register(&mut listener, SOCKET, Interest::READABLE)
            .map_err(|error| error.to_string())?;
        let wake = Arc::new(Waker::new(poll.registry(), CHANGE).map_err(|error| error.to_string())?);
        hub.observe_deadlines(wake.clone())?;
        Ok(Self { poll, listener, events: Events::with_capacity(8), wake })
    }

    pub(super) fn wake(&self) -> Arc<Waker> { self.wake.clone() }

    fn wait(&mut self, timeout: Option<Duration>) -> std::io::Result<bool> {
        self.poll.poll(&mut self.events, timeout)?;
        Ok(self.events.iter().any(|event| event.token() == SOCKET))
    }
}

struct PeerFinished {
    active: Arc<AtomicUsize>, wake: Arc<Waker>,
    connections: Arc<connections::Connections>, hub: Hub, id: String,
}
impl Drop for PeerFinished {
    fn drop(&mut self) {
        self.connections.remove(&self.id);
        self.hub.disconnect_connection(&self.id, self.hub.now_ms());
        self.active.fetch_sub(1, Ordering::SeqCst);
        let _ = self.wake.wake();
    }
}

pub(super) fn run(
    mut readiness: Readiness, hub: Hub, tls: Arc<rustls::ServerConfig>,
    stop: Arc<AtomicBool>, active: Arc<AtomicUsize>, attempts: Arc<Mutex<Attempts>>,
    browser_origin: String, browser_assets: &'static [crate::browser::BrowserAsset],
    failure: Arc<Mutex<Option<String>>>,
    connections: Arc<connections::Connections>,
) {
    let mut workers: Vec<JoinHandle<()>> = Vec::new();
    let mut ready = true;
    let result = (|| -> std::io::Result<()> {
        while !stop.load(Ordering::SeqCst) {
            hub.tick(hub.now_ms());
            let mut index = 0;
            while index < workers.len() {
                if workers[index].is_finished() { let _ = workers.swap_remove(index).join(); }
                else { index += 1; }
            }
            if ready {
                // Drain edge-triggered readiness, yielding to Stop and expiry
                // after each bounded batch even during an incoming flood.
                for _ in 0..ACCEPT_BATCH {
                    if stop.load(Ordering::SeqCst) { break; }
                    let stream = match readiness.listener.accept() {
                        Ok((stream, _)) => TcpStream::from(stream),
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => { ready = false; break; }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) => return Err(error),
                    };
                    if active.load(Ordering::SeqCst) >= 16 { continue; }
                    let (stream, cancel) = DeadlineTcp::event(stream)?;
                    let id = uuid::Uuid::new_v4().to_string();
                    if let Err(error) = connections.register(&id, cancel) {
                        if stop.load(Ordering::SeqCst) { break; }
                        return Err(error);
                    }
                    active.fetch_add(1, Ordering::SeqCst);
                    let finished = PeerFinished { active: active.clone(), wake: readiness.wake(),
                        connections: connections.clone(), hub: hub.clone(), id: id.clone() };
                    let hub = hub.clone();
                    let tls = tls.clone();
                    let stop = stop.clone();
                    let attempts = attempts.clone();
                    let origin = browser_origin.clone();
                    let worker = thread::Builder::new().name("companion-peer".into()).spawn(move || {
                        let _finished = finished;
                        let _result = serve_connection(stream, tls, &hub, &id, &stop, &attempts, &origin, browser_assets);
                        #[cfg(test)]
                        if let Err(error) = _result { eprintln!("companion peer ended: {error}"); }
                    })?;
                    workers.push(worker);
                }
            }
            if ready { continue; }
            let timeout = hub.next_deadline_ms()
                .map(|deadline| Duration::from_millis(deadline.saturating_sub(hub.now_ms())));
            match readiness.wait(timeout) {
                Ok(socket_ready) => ready = socket_ready,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {},
                Err(error) => return Err(error),
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        *failure.lock().unwrap_or_else(|p| p.into_inner()) = Some(format!("Companion listener stopped: {error}"));
    }
    // A failed listener cannot leave peer authority or parked feedback readers
    // alive. Disconnect wakes those readers before their workers are joined.
    stop.store(true, Ordering::SeqCst);
    connections.close();
    hub.disconnect_all();
    for worker in workers { let _ = worker.join(); }
    hub.tick(hub.now_ms());
}

#[cfg(test)]
#[path = "listener_tests.rs"]
mod tests;
