//! One cancellable acquisition owner; readers only dequeue committed datagrams.
use super::{Config, Network};
use std::{
    collections::{BTreeMap, VecDeque},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, Weak},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

const MAX_SUBSCRIBERS: usize = 64;
const MAX_PENDING_FRAMES: usize = 256;
const MAX_PENDING_BYTES: usize = 1024 * 1024;
const OVERFLOW: &str = "DCS export subscriber queue overflowed; acquire a fresh baseline.";

pub(super) enum CommandSubmission {
    Sent,
    DecodePending,
}

impl CommandSubmission {
    pub(super) fn require_sent(self) -> Result<(), String> {
        match self {
            Self::Sent => Ok(()),
            Self::DecodePending => Err("DCS decode has pending native data; command was not submitted.".into()),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CapturedDatagram {
    pub bytes: Arc<[u8]>,
    pub captured_at: Instant,
    pub producer_id: String,
    pub sequence: u64,
}

#[derive(Default)]
struct Pending {
    frames: VecDeque<CapturedDatagram>,
    bytes: usize,
}
struct State {
    subscribers: BTreeMap<u64, Result<Pending, String>>,
    next_subscriber: u64,
    sequence: u64,
    terminal: Option<String>,
}
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    producer_id: String,
    origin: Instant,
    stop: CancellationToken,
    #[cfg(test)]
    receive_waits: std::sync::atomic::AtomicUsize,
}
impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
    fn subscribe(&self) -> Result<u64, String> {
        let mut state = self.lock();
        if let Some(error) = &state.terminal {
            return Err(error.clone());
        }
        if state.subscribers.len() >= MAX_SUBSCRIBERS {
            return Err("DCS export subscriber limit reached.".into());
        }
        state.next_subscriber = state
            .next_subscriber
            .checked_add(1)
            .ok_or("DCS export subscriber counter exhausted.")?;
        let id = state.next_subscriber;
        state.subscribers.insert(id, Ok(Pending::default()));
        Ok(id)
    }
    fn fail(&self, error: String) {
        let mut state = self.lock();
        Self::fail_locked(&mut state, error);
        self.stop.cancel();
        self.changed.notify_all();
    }
    fn fail_locked(state: &mut State, error: String) {
        state.terminal = Some(error.clone());
        for pending in state.subscribers.values_mut() {
            *pending = Err(error.clone());
        }
    }
    fn capture(&self, bytes: &[u8], captured_at: Instant) -> Result<(), ()> {
        // The command path shares the state lock. Copy the UDP payload before
        // taking it, then commit sequence and subscriber continuity together.
        let bytes: Arc<[u8]> = bytes.into();
        let length = bytes.len();
        let mut state = self.lock();
        if state.terminal.is_some() {
            return Err(());
        }
        let Some(sequence) = state.sequence.checked_add(1) else {
            Self::fail_locked(&mut state, "DCS export capture counter exhausted.".into());
            self.stop.cancel();
            self.changed.notify_all();
            return Err(());
        };
        state.sequence = sequence;
        let frame = CapturedDatagram {
            bytes,
            captured_at,
            producer_id: self.producer_id.clone(),
            sequence,
        };
        for pending in state.subscribers.values_mut() {
            if let Ok(queue) = pending {
                if queue.frames.len() >= MAX_PENDING_FRAMES
                    || queue.bytes.saturating_add(length) > MAX_PENDING_BYTES
                {
                    // Dropping the queue withdraws every partial baseline permanently.
                    *pending = Err(OVERFLOW.into());
                } else {
                    queue.bytes += length;
                    queue.frames.push_back(frame.clone());
                }
            }
        }
        self.changed.notify_all();
        Ok(())
    }
    fn pop(state: &mut State, subscriber: u64) -> Result<Option<CapturedDatagram>, String> {
        let queue = state
            .subscribers
            .get_mut(&subscriber)
            .ok_or("DCS export subscriber closed.")?
            .as_mut()
            .map_err(|error| error.clone())?;
        let frame = queue.frames.pop_front();
        if let Some(frame) = &frame {
            queue.bytes -= frame.bytes.len();
        }
        Ok(frame)
    }
}

// The worker retains Shared, never Owner. The last transport cancels and joins it.
struct Owner {
    shared: Arc<Shared>,
    address: SocketAddr,
    stop: CancellationToken,
    worker: Mutex<Option<JoinHandle<()>>>,
}
impl Owner {
    fn open(config: &Network) -> Result<(Self, u64), String> {
        Self::start(config, |_| {})
    }
    fn start(
        config: &Network,
        before_start: impl FnOnce(SocketAddr),
    ) -> Result<(Self, u64), String> {
        let receiver = socket2::Socket::new(
            socket2::Domain::IPV4,
            socket2::Type::DGRAM,
            Some(socket2::Protocol::UDP),
        )
        .map_err(|e| e.to_string())?;
        receiver
            .set_reuse_address(true)
            .map_err(|e| e.to_string())?;
        receiver
            .bind(&SocketAddrV4::new(config.listen_address, config.listen_port).into())
            .map_err(|e| format!("Cannot listen for DCS-BIOS export: {e}"))?;
        if let Some(group) = config.multicast_group {
            receiver
                .join_multicast_v4(&group, &config.multicast_interface)
                .map_err(|e| format!("Cannot join DCS-BIOS multicast: {e}"))?;
        }
        receiver.set_nonblocking(true).map_err(|e| e.to_string())?;
        let receiver: UdpSocket = receiver.into();
        let address = receiver.local_addr().map_err(|e| e.to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .map_err(|e| e.to_string())?;
        let socket = {
            let _entered = runtime.enter();
            tokio::net::UdpSocket::from_std(receiver).map_err(|e| e.to_string())?
        };
        let stop = CancellationToken::new();
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                subscribers: BTreeMap::new(),
                next_subscriber: 0,
                sequence: 0,
                terminal: None,
            }),
            changed: Condvar::new(),
            producer_id: format!("dcs-bios:{}", uuid::Uuid::new_v4()),
            origin: Instant::now(),
            stop: stop.clone(),
            #[cfg(test)]
            receive_waits: std::sync::atomic::AtomicUsize::new(0),
        });
        // Install the first queue before acquisition can drain the bound socket.
        let subscriber = shared.subscribe()?;
        before_start(address);
        let worker_shared = shared.clone();
        let worker_stop = stop.clone();
        let network = config.clone();
        let worker = thread::Builder::new()
            .name("dcs-bios-receive".into())
            .spawn(move || runtime.block_on(acquire(socket, worker_shared, worker_stop, network)))
            .map_err(|e| e.to_string())?;
        Ok((
            Self {
                shared,
                address,
                stop,
                worker: Mutex::new(Some(worker)),
            },
            subscriber,
        ))
    }
    fn shutdown(&self) {
        self.stop.cancel();
        // Never join with the shared state locked: cancellation may be racing
        // a capture commit. Registry replacement serializes bind after this join.
        let mut worker = self.worker.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(worker) = worker.take() {
            let _ = worker.join();
        }
    }
    fn overlaps(&self, config: &Network) -> bool {
        config.listen_port != 0
            && self.address.port() == config.listen_port
            && (self.address.ip() == std::net::IpAddr::V4(config.listen_address)
                || self.address.ip().is_unspecified()
                || config.listen_address.is_unspecified())
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.shutdown();
    }
}

async fn acquire(
    socket: tokio::net::UdpSocket,
    shared: Arc<Shared>,
    stop: CancellationToken,
    network: Network,
) {
    let mut bytes = vec![0; 65_536];
    loop {
        #[cfg(test)]
        {
            let _state = shared.lock();
            shared
                .receive_waits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            shared.changed.notify_all();
        }
        // Idle waits only on native readiness or cancellation, with no timer.
        let received = tokio::select! {
            biased;
            _ = stop.cancelled() => return,
            received = socket.recv_from(&mut bytes) => received,
        };
        // Native time is recorded at acquisition, before any consumer lock/read.
        let captured_at = Instant::now();
        match received {
            Ok(_) if stop.is_cancelled() => return,
            Ok((count, source)) if count > 0 && network.accepts_export_source(source.ip()) => {
                if shared.capture(&bytes[..count], captured_at).is_err() {
                    return;
                }
            }
            Ok(_) => {}
            Err(error) => {
                shared.fail(format!("DCS-BIOS export receive failed: {error}"));
                return;
            }
        }
    }
}

type Registry = Mutex<BTreeMap<String, Weak<Owner>>>;
static RECEIVERS: OnceLock<Registry> = OnceLock::new();

pub(crate) fn acquisition_key(config: &Network) -> String {
    // Include the effective accepted-source set, not command routing/freshness.
    let mut accepted = vec![config.command_host];
    if config.command_host.is_loopback()
        && config.multicast_group.is_some()
        && !config.multicast_interface.is_unspecified()
    {
        accepted.push(config.multicast_interface);
    }
    accepted.sort();
    accepted.dedup();
    format!(
        "{}:{}:{:?}:{}:{accepted:?}",
        config.listen_address,
        config.listen_port,
        config.multicast_group,
        config.multicast_interface
    )
}

pub struct Transport {
    receiver: Arc<Owner>,
    subscriber: u64,
    sender: UdpSocket,
    command: SocketAddrV4,
}
impl Transport {
    pub fn open(config: &Config) -> Result<Self, String> {
        Self::open_network(&config.network())
    }
    pub(crate) fn open_network(config: &Network) -> Result<Self, String> {
        let sender = UdpSocket::bind(SocketAddrV4::new(
            if config.command_host.is_loopback() {
                Ipv4Addr::LOCALHOST
            } else {
                Ipv4Addr::UNSPECIFIED
            },
            0,
        ))
        .map_err(|e| e.to_string())?;
        sender.set_nonblocking(true).map_err(|e| e.to_string())?;
        let mut receivers = RECEIVERS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        receivers.retain(|_, receiver| receiver.strong_count() > 0);
        let key = acquisition_key(config);
        let mut retired = Vec::new();
        for (other_key, receiver) in receivers.iter() {
            let Some(owner) = receiver.upgrade() else {
                continue;
            };
            if !owner.overlaps(config) {
                continue;
            }
            let failed = owner.shared.lock().terminal.is_some();
            if failed {
                owner.shutdown();
                retired.push(other_key.clone());
            } else if *other_key != key {
                return Err(
                    "DCS export endpoint already has a different acquisition policy.".into(),
                );
            }
        }
        for key in retired {
            receivers.remove(&key);
        }
        let receiver = if config.listen_port != 0 {
            receivers.get(&key).and_then(Weak::upgrade)
        } else {
            None
        };
        let receiver = receiver.filter(|receiver| {
            let failed = receiver.shared.lock().terminal.is_some();
            if failed {
                receiver.shutdown();
            }
            !failed
        });
        let (receiver, subscriber) = match receiver {
            Some(receiver) => {
                let subscriber = receiver.shared.subscribe()?;
                (receiver, subscriber)
            }
            None => {
                let (owner, subscriber) = Owner::open(config)?;
                let receiver = Arc::new(owner);
                if config.listen_port != 0 {
                    receivers.insert(key, Arc::downgrade(&receiver));
                }
                (receiver, subscriber)
            }
        };
        Ok(Self {
            receiver,
            subscriber,
            sender,
            command: SocketAddrV4::new(config.command_host, config.command_port),
        })
    }
    pub fn local_addr(&self) -> Result<SocketAddr, String> {
        Ok(self.receiver.address)
    }
    pub fn receive(&self) -> Result<Option<Vec<u8>>, String> {
        self.receive_captured()
            .map(|frame| frame.map(|frame| frame.bytes.to_vec()))
    }
    pub(crate) fn receive_captured(&self) -> Result<Option<CapturedDatagram>, String> {
        Shared::pop(&mut self.receiver.shared.lock(), self.subscriber)
    }
}
impl Drop for Transport {
    fn drop(&mut self) {
        self.receiver
            .shared
            .lock()
            .subscribers
            .remove(&self.subscriber);
    }
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;

#[path = "transport_client.rs"]
mod client;
