use super::runtime::Inner;
use osg_streams::{Broker, Publisher};
use serde_json::Value;
use std::{collections::BTreeMap, sync::{atomic::Ordering, Arc}};

pub(super) const MAX_DETAILS: usize = 64;
pub(super) struct Slot {
    pub id: u64,
    pub leases: usize,
    pub ready: Option<Result<(), String>>,
    pub publisher: Option<Publisher>,
    pub last: Option<Value>,
}
#[derive(Default)]
pub(super) struct Registry {
    pub slots: BTreeMap<String, Slot>,
    pub next_id: u64,
    pub ready: Option<Result<(), String>>,
}

impl Registry {
    pub fn acquire(&mut self, game_id: &str) -> Result<u64, String> {
        if !self.slots.contains_key(game_id) {
            if self.slots.len() >= MAX_DETAILS { return Err("Game detail query capacity is full.".into()); }
            let id = self.next_id.checked_add(1).ok_or("Game detail query counter exhausted.")?;
            self.next_id = id;
            self.slots.insert(game_id.into(), Slot { id, leases: 0, ready: None, publisher: None, last: None });
        }
        let slot = self.slots.get_mut(game_id).expect("admitted detail");
        if let Some(Err(error)) = &slot.ready { return Err(error.clone()); }
        if slot.leases >= 64 { return Err("Game detail subscription capacity is full.".into()); }
        slot.leases += 1;
        Ok(slot.id)
    }

    pub fn release(&mut self, game_id: &str, id: u64) {
        if let Some(slot) = self.slots.get_mut(game_id).filter(|slot| slot.id == id) {
            slot.leases = slot.leases.saturating_sub(1);
        }
    }

    pub fn prune(&mut self, broker: &Broker) -> bool {
        // Keep admission and exact idle retirement under one registry lock.
        let idle = self.slots.iter().filter(|(_, slot)| slot.leases == 0
            && slot.publisher.as_ref().is_none_or(|publisher| broker.retire_if_idle(publisher)))
            .map(|(game_id, _)| game_id.clone()).collect::<Vec<_>>();
        let changed = !idle.is_empty();
        for game_id in idle { self.slots.remove(&game_id); }
        changed
    }
}

pub(super) struct Task {
    pub game_id: String,
    pub id: u64,
    pub publisher: Option<Publisher>,
    pub last: Option<Value>,
}

pub(super) fn tasks(inner: &Inner, broker: &Broker) -> (Vec<Task>, bool) {
    let mut registry = inner.lock();
    let retired = registry.prune(broker);
    let tasks = registry.slots.iter().filter(|(_, slot)| !matches!(slot.ready, Some(Err(_))))
        .map(|(game_id, slot)| Task { game_id: game_id.clone(), id: slot.id,
            publisher: slot.publisher.clone(), last: slot.last.clone() }).collect();
    (tasks, retired)
}

pub(super) fn retire_missing(inner: &Inner, broker: &Broker, task: &Task) {
    let publisher = {
        let mut registry = inner.lock();
        registry.slots.get_mut(&task.game_id).filter(|slot| slot.id == task.id).and_then(|slot| {
            slot.ready = Some(Err("Game detail source retired.".into()));
            slot.last = None;
            slot.publisher.take()
        })
    };
    if let Some(publisher) = publisher { broker.retire(&publisher); }
    inner.changed.notify_all();
}

pub(super) fn accept(inner: &Inner, task: &Task, publisher: Publisher, payload: Value) -> bool {
    let mut registry = inner.lock();
    let Some(slot) = registry.slots.get_mut(&task.game_id).filter(|slot| slot.id == task.id) else { return false; };
    slot.publisher = Some(publisher);
    slot.last = Some(payload);
    slot.ready = Some(Ok(()));
    inner.changed.notify_all();
    true
}

pub(super) struct Lease { inner: Arc<Inner>, game_id: String, id: u64 }
impl Lease {
    pub fn acquire(inner: &Arc<Inner>, game_id: &str) -> Result<Self, String> {
        let mut registry = inner.lock();
        if inner.closed.load(Ordering::Acquire) { return Err("Game read models are closed.".into()); }
        let id = registry.acquire(game_id)?;
        drop(registry);
        inner.signal.wake.notify();
        Ok(Self { inner: inner.clone(), game_id: game_id.into(), id })
    }

    pub fn wait_ready(&self) -> Result<(), String> {
        let mut registry = self.inner.lock();
        loop {
            if self.inner.closed.load(Ordering::Acquire) {
                return Err(registry.ready.as_ref().and_then(|ready| ready.as_ref().err()).cloned()
                    .unwrap_or_else(|| "Game read models are closed.".into()));
            }
            let slot = registry.slots.get(&self.game_id).filter(|slot| slot.id == self.id)
                .ok_or("Game detail source retired during subscription.")?;
            if let Some(ready) = &slot.ready { return ready.clone(); }
            registry = self.inner.changed.wait(registry).unwrap_or_else(|error| error.into_inner());
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.inner.lock().release(&self.game_id, self.id);
        self.inner.signal.wake.notify();
    }
}
