//! Passive delivery sessions. The producer never executes a transport callback.
pub(crate) mod commands;
mod service;
mod service_output;
pub use service_output::ServiceOutput;
#[cfg(test)]
mod tests;

use osg_streams::{Broker, ReaderCancellation, WireFrame};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex, Weak,
    },
    time::Instant,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Delivery {
    pub subscription_id: String,
    pub delivery: String,
    pub frame: WireFrame,
}

struct Session {
    client: String,
    cancel: ReaderCancellation,
    acknowledge: mpsc::SyncSender<()>,
    awaiting: Option<String>,
}

struct Inner {
    broker: Broker,
    sessions: Mutex<BTreeMap<String, Session>>,
    origin: Instant,
    closed: AtomicBool,
    workers: Arc<AtomicUsize>,
    reader_wake: Mutex<Vec<Weak<osg_streams::NativeWake>>>,
}

struct WorkerSlot(Arc<AtomicUsize>, Vec<Weak<osg_streams::NativeWake>>);
impl Drop for WorkerSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
        for wake in self.1.iter().filter_map(Weak::upgrade) {wake.notify();}
    }
}

#[derive(Clone)]
pub struct Runtime {
    inner: Arc<Inner>,
}

impl crate::gateways::GatewayManager {
    pub fn observation_runtime(&self) -> Runtime {
        self.observations.clone()
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                broker: Broker::new(uuid::Uuid::new_v4().to_string())
                    .expect("Generated observation session is a valid identity"),
                sessions: Mutex::new(BTreeMap::new()),
                origin: Instant::now(),
                closed: AtomicBool::new(false),
                workers: Arc::new(AtomicUsize::new(0)),
                reader_wake: Mutex::new(Vec::new()),
            }),
        }
    }

    pub(crate) fn watch_reader_lifecycle(&self, wake: Arc<osg_streams::NativeWake>) {
        let mut wakes=self.inner.reader_wake.lock().unwrap_or_else(|p|p.into_inner());
        wakes.retain(|wake|wake.strong_count()>0);
        let weak=Arc::downgrade(&wake);
        if wakes.len()<8&&!wakes.iter().any(|old|old.ptr_eq(&weak)){wakes.push(weak);}
    }
    pub(crate) fn broker(&self) -> Broker {
        self.inner.broker.clone()
    }

    pub(crate) fn captured_us(&self) -> u64 {
        u64::try_from(self.inner.origin.elapsed().as_micros()).unwrap_or(u64::MAX)
    }

    /// Send one record, then wait for the exact receipt before draining again.
    /// A blocked window/pipe can retain only one in-flight record per session.
    pub fn subscribe(
        &self,
        client: &str,
        audience: &str,
        topic: &str,
        send: impl Fn(Delivery) -> Result<(), String> + Send + 'static,
    ) -> Result<String, String> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err("Observation runtime is closed.".into());
        }
        self.inner
            .workers
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 64).then_some(count + 1)
            })
            .map_err(|_| "Observation delivery worker limit reached.")?;
        let slot = WorkerSlot(self.inner.workers.clone(), self.inner.reader_wake.lock().unwrap_or_else(|p|p.into_inner()).clone());
        let reader = self
            .inner
            .broker
            .subscribe(client, audience, topic)
            .map_err(|error| error.to_string())?;
        let token = uuid::Uuid::new_v4().to_string();
        let (acknowledge, receipts) = mpsc::sync_channel(1);
        let cancel = reader.cancellation_handle();
        let mut sessions = self
            .inner
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if self.inner.closed.load(Ordering::Acquire) {
            return Err("Observation runtime is closed.".into());
        }
        sessions.insert(
            token.clone(),
            Session {
                client: client.into(),
                cancel,
                acknowledge,
                awaiting: None,
            },
        );
        drop(sessions);
        let (weak, id) = (Arc::downgrade(&self.inner), token.clone());
        if let Err(error) = std::thread::Builder::new()
            .name("observation-delivery".into())
            .spawn(move || {
                let _slot = slot;
                let mut reader = reader;
                let mut ordinal = 0u64;
                while let Ok(frame) = reader.recv() {
                    let Some(next) = ordinal.checked_add(1) else {
                        break;
                    };
                    ordinal = next;
                    let receipt = ordinal.to_string();
                    let Some(inner) = weak.upgrade() else {
                        break;
                    };
                    {
                        let mut sessions = inner.sessions.lock().unwrap_or_else(|p| p.into_inner());
                        let Some(session) = sessions.get_mut(&id) else {
                            break;
                        };
                        session.awaiting = Some(receipt.clone());
                    }
                    drop(inner);
                    if send(Delivery {
                        subscription_id: id.clone(),
                        delivery: receipt,
                        frame,
                    })
                    .is_err()
                    {
                        break;
                    }
                    if receipts.recv().is_err() {
                        break;
                    }
                }
                release(&weak, &id);
            })
        {
            self.unsubscribe(client, &token)?;
            return Err(format!("Cannot start observation delivery: {error}"));
        }
        Ok(token)
    }

    pub fn acknowledge(&self, client: &str, token: &str, delivery: &str) -> Result<(), String> {
        let mut sessions = self
            .inner
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let session = sessions
            .get_mut(token)
            .filter(|entry| entry.client == client)
            .ok_or("This observation subscription is not owned by this client.")?;
        if session.awaiting.as_deref() != Some(delivery) {
            return Err("Observation receipt is stale or out of order.".into());
        }
        session
            .acknowledge
            .try_send(())
            .map_err(|_| "Observation delivery is closed or already acknowledged.")?;
        session.awaiting = None;
        Ok(())
    }

    pub fn unsubscribe(&self, client: &str, token: &str) -> Result<(), String> {
        let removed = {
            let mut sessions = self
                .inner
                .sessions
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if sessions
                .get(token)
                .is_some_and(|entry| entry.client != client)
            {
                return Err("This observation subscription is not owned by this client.".into());
            }
            sessions.remove(token)
        };
        if let Some(session) = removed {
            session.cancel.cancel();
        }
        Ok(())
    }

    pub fn close_client(&self, client: &str) {
        let removed = {
            let mut sessions = self
                .inner
                .sessions
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let ids = sessions
                .iter()
                .filter(|(_, entry)| entry.client == client)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            ids.into_iter()
                .filter_map(|id| sessions.remove(&id))
                .collect::<Vec<_>>()
        };
        for session in removed {
            session.cancel.cancel();
        }
        self.inner.broker.close_client(client);
    }

    pub fn close(&self) {
        self.inner.closed.store(true, Ordering::Release);
        let removed = std::mem::take(
            &mut *self
                .inner
                .sessions
                .lock()
                .unwrap_or_else(|p| p.into_inner()),
        );
        for (_, session) in removed {
            session.cancel.cancel();
        }
        self.inner.broker.close();
    }
}

fn release(shared: &Weak<Inner>, token: &str) {
    if let Some(inner) = shared.upgrade() {
        let removed = inner
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(token);
        if let Some(session) = removed {
            session.cancel.cancel();
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        let sessions = self.sessions.get_mut().unwrap_or_else(|p| p.into_inner());
        for (_, session) in std::mem::take(sessions) {
            session.cancel.cancel();
        }
        self.broker.close();
    }
}
