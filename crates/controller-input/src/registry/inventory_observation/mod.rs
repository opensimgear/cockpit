//! Cached metadata only. Native reports never serialize or enter this worker.
use super::Registry;
use osg_streams::{Broker, NativeWake};
use std::sync::{Arc, Weak};
mod model;
mod queue;
mod worker;
use model::{SelfAccess, Summary, View};
use queue::{Actor, Shared, Update};

pub(super) struct Inventory {
    actor: Option<Actor>,
    epoch: u64,
    latest: Option<Arc<View>>,
    self_access: Arc<SelfAccess>,
    closed: bool,
    resource_wake: Weak<NativeWake>,
}
impl Default for Inventory {
    fn default() -> Self {
        Self { actor: None, epoch: 0, latest: None,
            self_access: Arc::new(crate::visibility::self_access::Status::default().into()), closed: false, resource_wake: Weak::new() }
    }
}
// Speculative metadata edits never publish through a registry clone.
impl Clone for Inventory {
    fn clone(&self) -> Self { Self { self_access: self.self_access.clone(), ..Self::default() } }
}
impl Inventory {
    fn next_epoch(&mut self) -> Result<u64, String> {
        self.epoch = self.epoch.checked_add(1).ok_or("Controller inventory epoch exhausted.")?;
        Ok(self.epoch)
    }
}

pub struct InventoryObservation { shared: Arc<Shared>, epoch: u64 }
impl InventoryObservation {
    pub fn with_subscription<T>(self, action: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        self.shared.wait(self.epoch)?;
        action()
    }
}
impl Drop for InventoryObservation {
    fn drop(&mut self) { self.shared.release(); }
}
pub struct InventoryShutdown(Option<Actor>);
impl InventoryShutdown {
    pub fn shutdown(mut self) { if let Some(actor) = self.0.take() { actor.shutdown(); } }
}
impl Registry {
    pub(crate) fn watch_resource_inventory(&mut self, wake: &Arc<NativeWake>) -> Result<(), String> {
        let inventory = &mut self.inventory_observation;
        if inventory.closed { return Err("Controller inventory worker stopped.".into()); }
        let observer = Arc::downgrade(wake);
        if inventory.resource_wake.strong_count() > 0 && !inventory.resource_wake.ptr_eq(&observer) {
            return Err("Controller resource inventory already has a native observer.".into());
        }
        inventory.resource_wake = observer;
        wake.notify();
        Ok(())
    }

    /// Startup-only immutable diagnostic; this never inspects visibility state.
    pub(crate) fn set_inventory_self_access(&mut self, status: crate::visibility::self_access::Status) {
        self.inventory_observation.self_access = Arc::new(status.into());
    }
    /// Capture and pin under the registry lock; wait and subscribe after release.
    pub fn prepare_inventory_observation(&mut self, broker: &Broker) -> Result<InventoryObservation, String> {
        if self.inventory_observation.closed { return Err("Controller inventory worker stopped.".into()); }
        let view = Arc::new(View::capture(self)?);
        let epoch = self.inventory_observation.next_epoch()?;
        if self.inventory_observation.actor.is_none() {
            self.inventory_observation.actor = Some(Actor::start(broker.clone())?);
        }
        let shared = self.inventory_observation.actor.as_ref().unwrap().shared.clone();
        shared.prepare(Update { view: view.clone(), epoch, captured_us: self.journal.now(), resync: false })?;
        self.inventory_observation.latest = Some(view);
        Ok(InventoryObservation { shared, epoch })
    }
    pub fn take_inventory_shutdown(&mut self) -> InventoryShutdown {
        self.inventory_observation.closed = true;
        if let Some(wake) = self.inventory_observation.resource_wake.upgrade() { wake.notify(); }
        InventoryShutdown(self.inventory_observation.actor.take())
    }
    pub(super) fn inventory_changed(&mut self) {
        if let Some(wake) = self.inventory_observation.resource_wake.upgrade() { wake.notify(); }
        let Some(shared) = self.inventory_observation.actor.as_ref().map(|actor| actor.shared.clone()) else { return; };
        let view = match View::capture(self) {
            Ok(view) => Arc::new(view),
            Err(error) => { shared.fail(error); return; }
        };
        if self.inventory_observation.latest.as_deref() == Some(view.as_ref()) { return; }
        let epoch = match self.inventory_observation.next_epoch() {
            Ok(epoch) => epoch,
            Err(error) => { shared.fail(error); return; }
        };
        shared.capture(Update { view: view.clone(), epoch, captured_us: self.journal.now(), resync: false });
        self.inventory_observation.latest = Some(view);
    }
    /// A first complete report copies only its own fixed metadata row. The
    /// worker merges it with cached inventory after releasing capture ownership.
    pub(super) fn inventory_readiness(&mut self, id: &str) {
        if let Some(wake) = self.inventory_observation.resource_wake.upgrade() { wake.notify(); }
        let Some(shared) = self.inventory_observation.actor.as_ref().map(|actor| actor.shared.clone()) else { return; };
        let Some(source) = self.sources.get(id) else { return; };
        let row = Summary::capture(source);
        let epoch = match self.inventory_observation.next_epoch() {
            Ok(epoch) => epoch,
            Err(error) => { shared.fail(error); return; }
        };
        shared.capture_row(row, epoch, self.journal.now());
        self.inventory_observation.latest = None;
    }
    pub(crate) fn set_backend_state(&mut self, status: &str, diagnostic: String) {
        if self.backend_status == status && self.diagnostic == diagnostic { return; }
        self.backend_status = status.into();
        self.diagnostic = diagnostic;
        self.inventory_changed();
    }
    pub(crate) fn set_backend_status(&mut self, status: &str) {
        if self.backend_status == status { return; }
        self.backend_status = status.into();
        self.inventory_changed();
    }
    pub(crate) fn set_diagnostic(&mut self, diagnostic: String) {
        if self.diagnostic == diagnostic { return; }
        self.diagnostic = diagnostic;
        self.inventory_changed();
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod concurrency_tests;
