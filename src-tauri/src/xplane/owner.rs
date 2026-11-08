//! Shared capture ownership. Consumers only drain committed native deliveries.
use super::*;
use osg_streams::{NativeLimits, NativeStream, NativeSubscription};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread::{self, JoinHandle};
use tokio::sync::{mpsc, Notify};
use tokio_util::sync::CancellationToken;

mod client;
mod worker;
pub(crate) use client::Source;

type Journal = NativeStream<NativeSnapshot, NativeSnapshot>;
type Reader = NativeSubscription<NativeSnapshot, NativeSnapshot>;
type Catalogue = BTreeMap<String, Arc<datarefs::Channel>>;
const LIMITS: NativeLimits = NativeLimits {
    max_subscribers: 64,
    max_snapshot_bytes: 1024 * 1024,
    max_frame_bytes: 1024 * 1024,
    max_pending_frames: 32,
    max_pending_bytes: 8 * 1024 * 1024,
};

struct Shared {
    journal: Journal,
    terminal: Mutex<Option<String>>,
    release: Arc<Notify>,
    origin: Instant,
    clock_id: String,
    active_leases: Arc<std::sync::atomic::AtomicUsize>,
}
impl Shared {
    fn error(&self) -> Option<String> {
        self.terminal
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    fn publish(&self, snapshot: NativeSnapshot, at: Instant) -> Result<(), String> {
        let stamp = u64::try_from(
            at.checked_duration_since(self.origin)
                .ok_or("X-Plane publication predates its native clock.")?
                .as_micros(),
        )
        .map_err(|_| "X-Plane native clock exhausted.")?;
        let result = if snapshot.epoch == self.journal.generation() {
            self.journal
                .publish(
                    snapshot.epoch,
                    snapshot.clone(),
                    snapshot,
                    stamp,
                    &self.clock_id,
                )
                .map(|_| ())
        } else {
            self.journal
                .replace_generation(snapshot.epoch, snapshot, stamp, &self.clock_id)
                .map(|_| ())
        };
        result.map_err(|e| format!("X-Plane publication failed: {e:?}"))
    }
    fn fail(&self, error: String) {
        *self.terminal.lock().unwrap_or_else(|p| p.into_inner()) = Some(error);
        self.journal.close();
    }
}

struct Lease {
    id: String,
    release: Arc<Notify>,
    active: std::sync::atomic::AtomicBool,
    count: Arc<std::sync::atomic::AtomicUsize>,
}
impl Lease {
    fn deactivate(&self) {
        use std::sync::atomic::Ordering;
        if self.active.swap(false, Ordering::AcqRel) {
            self.count.fetch_sub(1, Ordering::AcqRel);
            self.release.notify_one();
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.deactivate();
    }
}
#[derive(Clone)]
struct Selection {
    lease: Weak<Lease>,
    ids: Vec<String>,
}
struct Request {
    lease: Weak<Lease>,
    ids: Vec<String>,
    subscribe: bool,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    reply: std::sync::mpsc::SyncSender<Result<Option<Reader>, String>>,
}

struct Owner {
    shared: Arc<Shared>,
    catalogue: Arc<Catalogue>,
    requests: mpsc::Sender<Request>,
    stop: CancellationToken,
    _worker: JoinHandle<()>,
    address: std::net::SocketAddr,
    monitor: Option<std::sync::Weak<process_events::Monitor>>,
}
impl Owner {
    fn start(
        connection: &Value,
        monitor: Option<Arc<process_events::Monitor>>,
        catalogue: BTreeMap<String, datarefs::Channel>,
    ) -> Result<Self, String> {
        let origin = Instant::now();
        let mut driver = Driver::connect_selected(
            connection,
            vec![],
            vec![true; GUARDS.len() + PATH_BYTES],
            BTreeMap::new(),
            vec![],
            Box::new(|| Ok(())),
            true,
        )?;
        driver
            .plan
            .remove_lease("profile")
            .map_err(|e| e.to_string())?;
        driver
            .plan
            .remove_lease("inspection")
            .map_err(|e| e.to_string())?;
        let address = driver.socket.local_addr().map_err(|e| e.to_string())?;
        let clock_id = format!("xplane:{}", uuid::Uuid::new_v4());
        let mut baseline = Session::new(connection);
        baseline.select_channels(vec![]);
        let (snapshot, _) = baseline.native_snapshot(origin, None, 0, &clock_id, true);
        let journal = Journal::new(snapshot.epoch, snapshot, 0, &clock_id, LIMITS)
            .map_err(|e| format!("{e:?}"))?;
        let shared = Arc::new(Shared {
            journal,
            terminal: Mutex::new(None),
            release: Arc::new(Notify::new()),
            origin,
            clock_id,
            active_leases: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        });
        let catalogue: Arc<Catalogue> = Arc::new(
            catalogue
                .into_iter()
                .map(|(id, channel)| (id, Arc::new(channel)))
                .collect(),
        );
        let (requests, receiver) = mpsc::channel(128);
        let stop = CancellationToken::new();
        let worker_shared = shared.clone();
        let worker_catalogue = catalogue.clone();
        let worker_stop = stop.clone();
        let monitor_view = monitor.as_ref().map(Arc::downgrade);
        let worker = thread::Builder::new()
            .name("xplane-capture".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                match runtime {
                    Ok(runtime) => runtime.block_on(worker::run(
                        driver,
                        monitor,
                        receiver,
                        worker_shared,
                        worker_catalogue,
                        worker_stop,
                    )),
                    Err(error) => worker_shared.fail(error.to_string()),
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            shared,
            catalogue,
            requests,
            stop,
            _worker: worker,
            address,
            monitor: monitor_view,
        })
    }
    fn update(
        &self,
        lease: &Arc<Lease>,
        ids: Vec<String>,
        subscribe: bool,
    ) -> Result<Option<Reader>, String> {
        if let Some(error) = self.shared.error() {
            return Err(error);
        }
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (reply, result) = std::sync::mpsc::sync_channel(1);
        self.requests
            .try_send(Request {
                lease: Arc::downgrade(lease),
                ids,
                subscribe,
                cancelled: cancelled.clone(),
                reply,
            })
            .map_err(|_| "X-Plane selection queue unavailable or full.")?;
        match result.recv_timeout(Duration::from_secs(2)) {
            Ok(result) => result,
            Err(_) => {
                cancelled.store(true, std::sync::atomic::Ordering::Release);
                lease.deactivate();
                Err("X-Plane selection did not commit before its deadline.".into())
            }
        }
    }
    fn shutdown(&self) {
        self.stop.cancel();
        self.shared.journal.close();
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        // Protocol cancellation runs on the capture worker, never the Stop/UI
        // caller. Dropping its JoinHandle detaches only this bounded cleanup.
        self.stop.cancel();
        self.shared.journal.close();
    }
}

#[cfg(test)]
mod tests;
