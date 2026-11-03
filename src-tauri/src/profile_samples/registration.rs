use super::*;
use std::time::{Duration, Instant};

pub(crate) struct Pin { inner: Arc<Inner>, topic: String, id: u64 }
impl Runtime {
    /// Called while the coordinator validates the immutable active owner. The
    /// pin precedes release of that guard; subsequent value changes cannot miss it.
    pub(crate) fn pin(&self, plan: Arc<NodePlan>) -> Result<Pin, String> {
        let retired = {
            let mut state = self.signal.inner.lock();
            let idle = state.slots.iter().filter(|(_, slot)| slot.pins == 0
                && slot.publisher.as_ref().is_none_or(|publisher| !publisher.has_readers()))
                .map(|(topic, _)| topic.clone()).collect::<Vec<_>>();
            let retired = idle.into_iter().filter_map(|topic| state.slots.remove(&topic).and_then(|slot| slot.publisher)).collect();
            self.signal.inner.interested.store(state.slots.len(), Ordering::Release);
            retired
        };
        self.signal.inner.retire(retired);
        if self.signal.inner.closed.load(Ordering::Acquire) {
            return Err("Profile sample observations are unavailable.".into());
        }
        let mut state = self.signal.inner.lock();
        if self.signal.inner.closed.load(Ordering::Acquire) {
            return Err("Profile sample observations are unavailable.".into());
        }
        let topic = plan.topic.clone();
        if !state.slots.contains_key(&topic) {
            if state.slots.len() >= MAX_TOPICS
                || state.slots.values().map(|slot| slot.plan.port_count()).sum::<usize>()
                    .saturating_add(plan.port_count()) > MAX_PORTS {
                return Err("Profile sample query capacity is full.".into());
            }
            let id = state.next_id.checked_add(1).ok_or("Profile sample query counter exhausted.")?;
            state.next_id = id;
            state.slots.insert(topic.clone(), Slot { id, plan, pins: 0, ready: None, publisher: None, last: None });
        }
        let slot = state.slots.get_mut(&topic).unwrap();
        if let Some(Err(error)) = &slot.ready { return Err(error.clone()); }
        if slot.pins >= 64 { return Err("Profile sample subscription capacity is full.".into()); }
        slot.pins += 1;
        let id = slot.id;
        self.signal.inner.interested.store(state.slots.len(), Ordering::Release);
        drop(state);
        self.signal.inner.wake.notify();
        Ok(Pin { inner: self.signal.inner.clone(), topic, id })
    }
}
impl Pin {
    pub(crate) fn wait_ready(&self) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut state = self.inner.lock();
        loop {
            if self.inner.closed.load(Ordering::Acquire) { return Err("Profile sample worker stopped.".into()); }
            let slot = state.slots.get(&self.topic).filter(|slot| slot.id == self.id)
                .ok_or("The applied sample owner retired during subscription.")?;
            if let Some(result) = &slot.ready { return result.clone(); }
            let remaining = deadline.checked_duration_since(Instant::now())
                .ok_or("Profile sample baseline preparation exceeded its deadline.")?;
            (state, _) = self.inner.changed.wait_timeout(state, remaining)
                .unwrap_or_else(|error| error.into_inner());
        }
    }
}
impl Drop for Pin {
    fn drop(&mut self) {
        let mut state = self.inner.lock();
        if let Some(slot) = state.slots.get_mut(&self.topic).filter(|slot| slot.id == self.id) {
            slot.pins = slot.pins.saturating_sub(1);
        }
        drop(state);
        self.inner.wake.notify();
    }
}
