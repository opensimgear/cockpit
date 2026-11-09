use super::{Command, Counter, Delivery, Scheduler};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

/// A transferred queue head stays revocable by scheduler cancellation even
/// when its producer token has not changed.
#[derive(Clone)]
pub struct DispatchPermit(Arc<AtomicBool>);

impl DispatchPermit {
    pub fn valid(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

impl Scheduler {
    pub(super) fn pop_dispatch_head(&mut self, operation_id: Counter) -> Result<Delivery, String> {
        if self.deliveries.front().is_none_or(|head| head.operation_id != operation_id) {
            return Err("Desktop dispatch head no longer matches the queued operation.".into());
        }
        Ok(self.deliveries.pop_front().expect("validated queue head"))
    }

    /// Transfer one FIFO head while retaining the ability to revoke it. The
    /// caller must finish that dispatch before transferring another.
    pub fn take_dispatch_head(&mut self, operation_id: Counter) -> Result<(Delivery, DispatchPermit), String> {
        let delivery = self.pop_dispatch_head(operation_id)?;
        let _ = self.capture(Command::DrainOne { operation_id });
        let permit = DispatchPermit(Arc::new(AtomicBool::new(true)));
        self.dispatch_head = Some((delivery.action_id.clone(), permit.clone()));
        Ok((delivery, permit))
    }

    pub(super) fn revoke_dispatch_head(&mut self, action: &str) {
        if self.dispatch_head.as_ref().is_some_and(|(id, _)| id == action) {
            let (_, permit) = self.dispatch_head.take().expect("matched action");
            permit.0.store(false, Ordering::Release);
        }
    }
}
