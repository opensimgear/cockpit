//! One canonical decoder owner; client reads only consume its native journal.
use super::{Snapshot, Spec, State};
use crate::dcs_bios::{transport::{acquisition_key, CommandSubmission}, Network, Transport};
use osg_streams::{
    Delivery, LossReason, NativeLimits, NativeStream, NativeSubscription, NativeWake, NativeWatch,
};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex, MutexGuard, OnceLock, Weak,
    },
    thread::{self, JoinHandle},
    time::Instant,
};

const LIMITS: NativeLimits = NativeLimits {
    max_subscribers: 64,
    max_snapshot_bytes: 4 * 1024 * 1024,
    max_frame_bytes: 4 * 1024 * 1024,
    max_pending_frames: 6,
    max_pending_bytes: 24 * 1024 * 1024,
};

// Includes all 65,536 native byte marks, raw memory and bounded metadata.
const _: () =
    assert!(super::CapturedMemory::max_retained_bytes() + 128 * 1024 <= LIMITS.max_frame_bytes);

struct Committed {
    snapshot: Snapshot,
    processed_head: u64,
    terminal: Option<String>,
}
struct Shared {
    committed: Mutex<Committed>,
    processed: Condvar,
    journal: NativeStream<Snapshot, Snapshot>,
    stop: AtomicBool,
    origin: Instant,
    clock_id: String,
    #[cfg(test)]
    waits: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    command_waits: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    before_commit: Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
}
impl Shared {
    fn lock(&self) -> MutexGuard<'_, Committed> {
        self.committed.lock().unwrap_or_else(|p| p.into_inner())
    }
    fn commit(&self, snapshots: Vec<Snapshot>, processed_head: Option<u64>) -> Result<(), String> {
        let mut current = self.lock();
        // A report can produce several resets/frames. Publish its whole result
        // and processed cursor under one command-authority guard.
        for snapshot in snapshots {
            self.publish_locked(&mut current, snapshot)?;
        }
        if let Some(head) = processed_head {
            current.processed_head = head;
            drop(current);
            self.processed.notify_all();
        }
        Ok(())
    }
    fn publish_locked(&self, current: &mut Committed, snapshot: Snapshot) -> Result<(), String> {
        let captured = snapshot.captured_at().unwrap_or_else(Instant::now);
        let captured_us = u64::try_from(
            captured
                .checked_duration_since(self.origin)
                .ok_or("DCS decode capture predates its native clock.")?
                .as_micros(),
        )
        .map_err(|_| "DCS decode clock exhausted.")?;
        if snapshot.epoch != current.snapshot.epoch {
            self.journal.replace_generation(
                snapshot.epoch,
                snapshot.clone(),
                captured_us,
                self.clock_id.clone(),
            )
        } else {
            self.journal.publish(
                snapshot.epoch,
                snapshot.clone(),
                snapshot.clone(),
                captured_us,
                self.clock_id.clone(),
            )
        }
        .map_err(|error| format!("DCS decode publication failed: {error:?}"))?;
        current.snapshot = snapshot;
        Ok(())
    }
    fn retire(&self, state: &mut State, error: String) {
        let mut current = self.lock();
        // Invalidation clears bytes even when an exhausted epoch cannot advance.
        let _ = state.invalidate(&error);
        current.snapshot = state.snapshot();
        current.terminal = Some(error);
        self.journal.close();
        drop(current);
        self.processed.notify_all();
    }
}

// The worker owns Shared and raw Transport, never Owner. A passive journal
// subscription cannot keep the decoder thread or bound UDP socket alive.
struct Owner {
    shared: Arc<Shared>,
    transport: Weak<Transport>,
    address: SocketAddr,
    worker: Mutex<Option<JoinHandle<()>>>,
}
impl Owner {
    fn open(network: &Network, spec: Spec) -> Result<Self, String> {
        let transport = Arc::new(Transport::open_network(network)?);
        let origin = transport.capture_origin();
        let address = transport.local_addr()?;
        let state = State::new(spec, format!("dcs-decode:{}", uuid::Uuid::new_v4()));
        let snapshot = state.snapshot();
        let clock_id = uuid::Uuid::new_v4().to_string();
        let journal = NativeStream::new(
            snapshot.epoch,
            snapshot.clone(),
            0,
            clock_id.clone(),
            LIMITS,
        )
        .map_err(|error| format!("{error:?}"))?;
        let shared = Arc::new(Shared {
            committed: Mutex::new(Committed {
                snapshot,
                processed_head: 0,
                terminal: None,
            }),
            processed: Condvar::new(),
            journal,
            stop: AtomicBool::new(false),
            origin,
            clock_id,
            #[cfg(test)]
            waits: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            command_waits: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            before_commit: Mutex::new(None),
        });
        let raw = Arc::downgrade(&transport);
        let worker_shared = shared.clone();
        let worker = thread::Builder::new()
            .name("dcs-bios-decode".into())
            .spawn(move || run(state, worker_shared, transport))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            shared,
            transport: raw,
            address,
            worker: Mutex::new(Some(worker)),
        })
    }
    fn shutdown(&self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(raw) = self.transport.upgrade() {
            raw.interrupt_capture();
        }
        if let Some(worker) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = worker.join();
        }
        self.shared.journal.close();
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run(mut state: State, shared: Arc<Shared>, transport: Arc<Transport>) {
    loop {
        if shared.stop.load(Ordering::Acquire) {
            return;
        }
        #[cfg(test)]
        shared.waits.fetch_add(1, Ordering::Relaxed);
        let report = transport.wait_captured_until(state.snapshot().deadline());
        if shared.stop.load(Ordering::Acquire) {
            return;
        }
        let (publications, processed_head) = match report {
            Ok(Some(report)) => (state.feed(&report, Instant::now()), Some(report.sequence)),
            Ok(None) => (
                state
                    .expire(Instant::now())
                    .map(|snapshot| snapshot.into_iter().collect()),
                None,
            ),
            Err(error) => (Err(error), None),
        };
        #[cfg(test)]
        {
            let gate = shared.before_commit.lock().unwrap().take();
            if let Some((arrived, resume)) = gate {
                let _ = arrived.send(());
                let _ = resume.recv();
            }
        }
        let result = publications.and_then(|snapshots| shared.commit(snapshots, processed_head));
        if let Err(error) = result {
            shared.retire(&mut state, error);
            return;
        }
    }
}

type Registry = Mutex<BTreeMap<String, Weak<Owner>>>;
static SOURCES: OnceLock<Registry> = OnceLock::new();

struct Client {
    reader: NativeSubscription<Snapshot, Snapshot>,
    last: Option<Snapshot>,
    error: Option<String>,
}
impl Client {
    fn fail(&mut self, error: String) -> String {
        self.last = None;
        self.error = Some(error.clone());
        error
    }
    fn drain(&mut self) -> Result<Snapshot, String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        loop {
            match self.reader.try_recv() {
                Ok(Some(Delivery::Snapshot(snapshot))) => self.last = Some(snapshot.value),
                Ok(Some(Delivery::Event(event))) => self.last = Some(event.value),
                Ok(Some(Delivery::Loss(loss))) if loss.reason == LossReason::GenerationReplaced => {
                    self.last = None;
                }
                Ok(Some(Delivery::Loss(loss))) => {
                    return Err(self.fail(format!(
                        "DCS decode subscriber lost continuity: {:?}.",
                        loss.reason
                    )));
                }
                Err(error) => {
                    return Err(self.fail(format!("DCS decode subscriber failed: {error:?}")))
                }
                Ok(None) => {
                    return self
                        .last
                        .clone()
                        .ok_or_else(|| "DCS decode baseline unavailable.".into())
                }
            }
        }
    }
}

pub(crate) struct Source {
    owner: Arc<Owner>,
    client: Arc<Mutex<Client>>,
    command_port: u16,
}
pub(crate) struct CommandSender {
    owner: Weak<Owner>,
    client: Weak<Mutex<Client>>,
    command_port: u16,
}
impl CommandSender {
    pub(crate) fn send<E: From<String>>(
        &self, command: &str, argument: &str, expected: Option<&str>,
        deadline: Instant,
        mut validate: impl FnMut() -> Result<(), E>,
        admit: impl Fn() -> Result<(), String>,
    ) -> Result<(), E> {
        let source = Source {
            owner: self.owner.upgrade().ok_or_else(|| "The DCS command reader retired.".to_owned())?,
            client: self.client.upgrade().ok_or_else(|| "The DCS command reader retired.".to_owned())?,
            command_port: self.command_port,
        };
        let shared = &source.owner.shared;
        let check_deadline = || -> Result<(), String> {
            if Instant::now() >= deadline {
                return Err("The queued DCS command expired before dispatch.".into());
            }
            Ok(())
        };
        loop {
            check_deadline()?;
            validate()?;
            let current = shared.lock();
            match source.send_control_attempt(&current, command, argument, expected, || {
                check_deadline()?;
                admit()
            })? {
                CommandSubmission::Sent => return Ok(()),
                CommandSubmission::DecodePending => {
                    // No datagram or admission callback has run. Release the
                    // decoder lock so it can commit, then recheck authority.
                    let head = current.processed_head;
                    #[cfg(test)]
                    shared.command_waits.fetch_add(1, Ordering::Relaxed);
                    drop(shared.processed.wait_timeout_while(
                        current, deadline.saturating_duration_since(Instant::now()),
                        |state| state.processed_head == head && state.terminal.is_none(),
                    ).unwrap_or_else(|p| p.into_inner()));
                }
            }
        }
    }
}
impl Source {
    pub fn open(network: &Network, spec: Spec) -> Result<Self, String> {
        let mut registry = SOURCES
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        registry.retain(|_, owner| owner.strong_count() > 0);
        let key = format!("{}:{}", acquisition_key(network), spec.key());
        let existing = (network.listen_port != 0)
            .then(|| registry.get(&key).and_then(Weak::upgrade))
            .flatten();
        let owner = match existing {
            Some(owner) if owner.shared.lock().terminal.is_none() => owner,
            Some(owner) => {
                owner.shutdown();
                Arc::new(Owner::open(network, spec)?)
            }
            None => Arc::new(Owner::open(network, spec)?),
        };
        let reader = owner
            .shared
            .journal
            .subscribe()
            .map_err(|error| format!("{error:?}"))?;
        if network.listen_port != 0 {
            registry.insert(key, Arc::downgrade(&owner));
        }
        Ok(Self {
            owner,
            client: Arc::new(Mutex::new(Client {
                reader,
                last: None,
                error: None,
            })),
            command_port: network.command_port,
        })
    }
    pub fn snapshot(&self) -> Result<Snapshot, String> {
        // Keep this lock order for reads, publication, and command authority.
        let owner = self.owner.shared.lock();
        let mut client = self.client.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(error) = &owner.terminal {
            return Err(client.fail(error.clone()));
        }
        client.drain()
    }
    pub fn local_addr(&self) -> Result<SocketAddr, String> {
        Ok(self.owner.address)
    }
    pub fn subscribe(&self) -> Result<NativeSubscription<Snapshot, Snapshot>, String> {
        let current = self.owner.shared.lock();
        if let Some(error) = &current.terminal {
            return Err(error.clone());
        }
        self.owner
            .shared
            .journal
            .subscribe()
            .map_err(|error| format!("{error:?}"))
    }
    pub(crate) fn watch(
        &self,
        wake: Arc<NativeWake>,
    ) -> Result<NativeWatch<Snapshot, Snapshot>, String> {
        let current = self.owner.shared.lock();
        if let Some(error) = &current.terminal {
            return Err(error.clone());
        }
        self.owner
            .shared
            .journal
            .watch(wake)
            .map_err(|error| format!("{error:?}"))
    }
    pub fn send_control(&self, command: &str, argument: &str) -> Result<(), String> {
        self.send_control_for_aircraft(command, argument, None)
    }
    pub fn send_control_for_aircraft(
        &self,
        command: &str,
        argument: &str,
        expected: Option<&str>,
    ) -> Result<(), String> {
        self.send_control_attempt(&self.owner.shared.lock(), command, argument, expected, || Ok(()))?.require_sent()
    }
    pub(crate) fn command_sender(&self) -> CommandSender {
        CommandSender { owner: Arc::downgrade(&self.owner), client: Arc::downgrade(&self.client),
            command_port: self.command_port }
    }
    fn send_control_attempt(
        &self, current: &Committed, command: &str, argument: &str, expected: Option<&str>,
        admit: impl FnOnce() -> Result<(), String>,
    ) -> Result<CommandSubmission, String> {
        let client = self.client.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(error) = &current.terminal {
            return Err(error.clone());
        }
        if let Some(error) = &client.error {
            return Err(error.clone());
        }
        if expected.is_some_and(|aircraft| current.snapshot.aircraft.as_deref() != Some(aircraft)) {
            return Err(
                "DCS aircraft changed from the captured game context; command was not submitted."
                    .into(),
            );
        }
        if !client.reader.is_continuous()
            || !client.last.as_ref().is_some_and(|last| {
                last.epoch == current.snapshot.epoch && last.fresh_at(Instant::now())
            })
            || !current.snapshot.fresh_at(Instant::now())
        {
            return Err("DCS decode command requires a fresh, continuous baseline.".into());
        }
        self.owner
            .transport
            .upgrade()
            .ok_or("DCS decode transport closed.")?
            .send_control_port_after(command, argument, self.command_port, current.processed_head, admit)
    }
}

#[cfg(test)]
#[path = "source_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "command_wait_tests.rs"]
mod command_wait_tests;
