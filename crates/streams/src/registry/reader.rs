use super::{Cancellation, RegistryError, Shared, Topic};
use crate::{Subscription, WireFrame};
use serde_json::Value;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Weak,
};

pub(super) struct Lease {
    broker: Weak<Shared>,
    id: u64,
    cancellation: Cancellation,
    closed: AtomicBool,
}

impl Lease {
    pub(super) fn new(broker: Weak<Shared>, id: u64, cancellation: Cancellation) -> Self {
        Self {
            broker,
            id,
            cancellation,
            closed: AtomicBool::new(false),
        }
    }

    pub(super) fn close(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(broker) = self.broker.upgrade() {
            broker.lock().readers.remove(&self.id);
        }
        self.cancellation.cancel();
    }
}

#[derive(Clone)]
pub struct ReaderCancellation {
    lease: Arc<Lease>,
}
impl ReaderCancellation {
    pub fn cancel(&self) {
        self.lease.close();
    }
}

pub struct Reader {
    topic: Arc<Topic>,
    subscription: Subscription<Value, Value>,
    lease: Arc<Lease>,
}

impl Reader {
    pub(super) fn new(
        topic: Arc<Topic>,
        subscription: Subscription<Value, Value>,
        lease: Arc<Lease>,
    ) -> Self {
        Self {
            topic,
            subscription,
            lease,
        }
    }
    pub fn recv(&mut self) -> Result<WireFrame, RegistryError> {
        Ok(self.topic.identity.frame(self.subscription.recv()?))
    }
    pub fn try_recv(&mut self) -> Result<Option<WireFrame>, RegistryError> {
        Ok(self
            .subscription
            .try_recv()?
            .map(|delivery| self.topic.identity.frame(delivery)))
    }
    pub fn cancellation_handle(&self) -> ReaderCancellation {
        ReaderCancellation {
            lease: self.lease.clone(),
        }
    }
    pub fn close(&self) {
        self.lease.close();
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.close();
    }
}
