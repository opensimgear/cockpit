//! One cancellable receive owner. Read handles retain only committed state/history.
use super::{Session, Snapshot, FRESHNESS_MS, MAX_PACKET, PORT};
use osg_streams::{
    NativeLimits, NativeStream, NativeSubscription, NativeWake, NativeWatch, StreamError,
};
#[cfg(test)]
use std::net::SocketAddr;
use std::{
    net::{Ipv4Addr, UdpSocket},
    sync::{Arc, Condvar, Mutex, MutexGuard, Weak},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

type Journal = NativeStream<Snapshot, Snapshot>;
pub(crate) type Reader = NativeSubscription<Snapshot, Snapshot>;
pub(super) const LIMITS: NativeLimits = NativeLimits {
    max_subscribers: 64,
    max_snapshot_bytes: 128 * 1024,
    max_frame_bytes: 128 * 1024,
    max_pending_frames: 32,
    max_pending_bytes: 2 * 1024 * 1024,
};
struct State {
    session: Session,
    snapshot: Snapshot,
    journal: Journal,
    vehicle_wake: Weak<NativeWake>,
    closed: bool,
}
struct Shared {
    state: Mutex<State>,
    committed: Condvar,
    clock: Instant,
    clock_id: String,
}
impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn now_ms(&self) -> u64 {
        u64::try_from(self.clock.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
    fn elapsed_ms(&self, at: Instant) -> u64 {
        u64::try_from(at.saturating_duration_since(self.clock).as_millis()).unwrap_or(u64::MAX)
    }
    fn now_us(&self) -> u64 {
        u64::try_from(self.clock.elapsed().as_micros()).unwrap_or(u64::MAX)
    }
    // The owner lock makes cache mutation, stream commit and registration one
    // boundary. No runtime, transport or consumer locks are acquired here.
    fn commit(&self, state: &mut State) -> Result<(), ()> {
        let snapshot = state.session.snapshot(self.now_ms());
        let captured_us = self.now_us();
        let result = if snapshot.epoch != state.journal.generation() {
            state
                .journal
                .replace_generation(
                    snapshot.epoch,
                    snapshot.clone(),
                    captured_us,
                    &self.clock_id,
                )
                .map(|_| ())
        } else {
            state
                .journal
                .publish(
                    snapshot.epoch,
                    snapshot.clone(),
                    snapshot.clone(),
                    captured_us,
                    &self.clock_id,
                )
                .map(|_| ())
        };
        state.snapshot = snapshot;
        if result.is_err() {
            state
                .session
                .invalidate("DCS ownship observation publication failed.");
            state.snapshot = state.session.snapshot(self.now_ms());
            state.closed = true;
            state.journal.close();
        }
        self.committed.notify_all();
        if let Some(wake) = state.vehicle_wake.upgrade() {
            wake.notify();
        }
        result.map_err(|_| ())
    }
}

/// Cloning an observer cannot retain a socket or delay source shutdown.
#[derive(Clone)]
pub(crate) struct SourceView {
    shared: Arc<Shared>,
}
impl SourceView {
    pub(crate) fn snapshot(&self) -> Snapshot {
        let state = self.shared.lock();
        let mut snapshot = state.snapshot.clone();
        snapshot.age_ms = state.session.captured_at().map(|at| {
            u64::try_from(Instant::now().saturating_duration_since(at).as_millis())
                .unwrap_or(u64::MAX)
        });
        // Defend passive readers if the OS has not yet scheduled the owner's
        // deadline wake. This projection never renews or publishes evidence.
        if snapshot.age_ms.is_some_and(|age| age > FRESHNESS_MS) {
            snapshot.fresh = false;
            snapshot.values.clear();
            if snapshot.status != "stopped" {
                snapshot.status = "stale".into();
            }
        }
        snapshot
    }
    pub(crate) fn subscribe(&self) -> Result<Reader, StreamError> {
        let mut state = self.shared.lock();
        let until = Instant::now() + Duration::from_secs(1);
        // A late registration must not capture expired live evidence while the
        // worker is waiting to acquire this lock for its deadline withdrawal.
        // Wait for that native commit instead of authoring a consumer event.
        while state.snapshot.fresh
            && state.session.captured_at().is_some_and(|captured_at| {
                Instant::now().saturating_duration_since(captured_at)
                    > Duration::from_millis(FRESHNESS_MS)
            })
        {
            let Some(remaining) = until.checked_duration_since(Instant::now()) else {
                return Err(StreamError::InvalidTimeout);
            };
            (state, _) = self
                .shared
                .committed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|e| e.into_inner());
        }
        state.journal.subscribe()
    }
    /// Register a passive commit notification without projecting a snapshot
    /// or renewing evidence. Consumers still read through `snapshot()`.
    pub(crate) fn watch(
        &self,
        wake: Arc<NativeWake>,
    ) -> Result<NativeWatch<Snapshot, Snapshot>, StreamError> {
        let state = self.shared.lock();
        state.journal.watch(wake)
    }
}

/// Sole lifecycle owner. Only the worker owns the receive socket.
pub(crate) struct Source {
    view: SourceView,
    stop: CancellationToken,
    worker: Option<JoinHandle<()>>,
    #[cfg(test)]
    address: SocketAddr,
}
impl Source {
    pub(crate) fn open() -> Result<Self, String> {
        Self::bind(PORT)
    }
    fn bind(port: u16) -> Result<Self, String> {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, port)).map_err(|e| {
            format!("Cannot receive local DCS ownship telemetry on port {port}: {e}")
        })?;
        socket2::SockRef::from(&socket)
            .set_recv_buffer_size(256 * 1024)
            .map_err(|e| e.to_string())?;
        #[cfg(test)]
        let address = socket.local_addr().map_err(|e| e.to_string())?;
        socket.set_nonblocking(true).map_err(|e| e.to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|e| e.to_string())?;
        let socket = {
            let _entered = runtime.enter();
            tokio::net::UdpSocket::from_std(socket).map_err(|e| e.to_string())?
        };
        let session = Session::new();
        let snapshot = session.snapshot(0);
        let clock_id = format!("dcs-ownship:{}", uuid::Uuid::new_v4());
        let journal = Journal::new(snapshot.epoch, snapshot.clone(), 0, &clock_id, LIMITS)
            .map_err(|e| format!("Cannot initialize DCS ownship observations: {e:?}"))?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                session,
                snapshot,
                journal,
                vehicle_wake: Weak::new(),
                closed: false,
            }),
            committed: Condvar::new(),
            clock: Instant::now(),
            clock_id,
        });
        let stop = CancellationToken::new();
        let worker_shared = shared.clone();
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name("dcs-ownship-receive".into())
            .spawn(move || runtime.block_on(receive(socket, worker_shared, worker_stop)))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            view: SourceView { shared },
            stop,
            worker: Some(worker),
            #[cfg(test)]
            address,
        })
    }
    pub(crate) fn view(&self) -> SourceView {
        self.view.clone()
    }
    pub(crate) fn snapshot(&self) -> Snapshot {
        self.view.snapshot()
    }
    pub(crate) fn subscribe(&self) -> Result<Reader, StreamError> {
        self.view.subscribe()
    }
    /// One internal vehicle owner shares the capture without a journal lease.
    pub(crate) fn watch_vehicle_commits(&self, wake: &Arc<NativeWake>) {
        let mut state = self.view.shared.lock();
        state.vehicle_wake = Arc::downgrade(wake);
        wake.notify();
    }
    pub(crate) fn is_closed(&self) -> bool {
        self.is_finished() || self.view.shared.lock().closed
    }
    pub(crate) fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(JoinHandle::is_finished)
    }
    #[cfg(test)]
    pub(crate) fn bind_loopback_ephemeral() -> Result<Self, String> {
        Self::bind(0)
    }
    #[cfg(test)]
    pub(crate) fn local_addr(&self) -> SocketAddr {
        self.address
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

async fn receive(socket: tokio::net::UdpSocket, shared: Arc<Shared>, stop: CancellationToken) {
    let mut bytes = [0; MAX_PACKET + 1];
    let mut deadline: Option<Instant> = None;
    let failure = loop {
        // Idle has no timer. Only a source report authors a freshness deadline;
        // cancellation wakes the executor directly and cannot fail like UDP I/O.
        let expires = async {
            match deadline {
                Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await,
                None => std::future::pending::<()>().await,
            }
        };
        let received = tokio::select! {
            biased;
            _ = stop.cancelled() => break None,
            _ = expires => {
                let mut state = shared.lock();
                state.session.expire();
                if shared.commit(&mut state).is_err() {
                    return;
                }
                deadline = None;
                continue;
            }
            received = socket.recv_from(&mut bytes) => received,
        };
        match received {
            Ok(_) if stop.is_cancelled() => break None,
            Ok((count, peer)) => {
                // Capture transport arrival before lock contention or parsing.
                let captured_at = Instant::now();
                if !peer.ip().is_loopback() {
                    continue;
                }
                let mut state = shared.lock();
                let capture_ms = shared.elapsed_ms(captured_at);
                // A report can arrive concurrently with expiry. Commit the
                // withdrawal before admitting its fresh advancing baseline.
                if deadline.is_some_and(|at| captured_at >= at) {
                    state.session.expire();
                    if shared.commit(&mut state).is_err() {
                        return;
                    }
                    deadline = None;
                }
                match state.session.feed(&bytes[..count], captured_at, capture_ms) {
                    Ok(false) => {} // replay/retired sessions never renew a lease
                    Ok(true) => {
                        deadline = Some(captured_at + Duration::from_millis(FRESHNESS_MS + 1));
                        if shared.commit(&mut state).is_err() {
                            return;
                        }
                    }
                    Err(_) => {
                        if shared.commit(&mut state).is_err() {
                            return;
                        }
                    }
                }
            }
            Err(error) if matches!(error.kind(), std::io::ErrorKind::Interrupted) => {}
            Err(error) => break Some(error.to_string()),
        }
    };
    let mut state = shared.lock();
    state.closed = true;
    state
        .session
        .invalidate(failure.as_deref().unwrap_or("DCS ownship source closed."));
    let _ = shared.commit(&mut state);
    state.journal.close();
}

#[cfg(test)]
mod tests;
