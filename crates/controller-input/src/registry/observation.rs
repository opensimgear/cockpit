//! Typed capture-to-preview handoff. The registry never calls the wire broker.
use super::*;
use osg_streams::Broker;
use std::sync::Arc;
mod queue;
mod worker;
use queue::{Actor, Shared, Slot, Update};

pub(super) const MAX_BINDINGS: usize = 64;
#[derive(Default)]
pub(super) struct Observations {
    actor: Option<Actor>,
    epoch: u64,
    closed: bool,
}
// Metadata transactions are speculative and must never publish through a clone.
impl Clone for Observations {
    fn clone(&self) -> Self {
        Self::default()
    }
}
/// A pinned typed observation boundary. Waiting and subscribing are deliberately
/// separate from preparation so neither can hold the native registry mutex.
pub struct Observation {
    shared: Arc<Shared>,
    slot: Arc<Slot>,
    epoch: u64,
}
impl Observation {
    pub fn with_subscription<T>(
        self,
        action: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        self.shared.wait(&self.slot, self.epoch)?;
        action()
    }
}
impl Drop for Observation {
    fn drop(&mut self) {
        self.slot.unpin();
        self.shared.wake.notify_one();
    }
}
/// Taken under the registry mutex, stopped and joined after releasing it.
pub struct ObservationShutdown(Option<Actor>);
impl ObservationShutdown {
    pub fn shutdown(mut self) {
        if let Some(actor) = self.0.take() {
            actor.shutdown();
        }
    }
}
impl Registry {
    /// Capture a typed boundary and pin its topic without waiting for encoding,
    /// broker locks, publication or subscription. At most 64 topics are admitted;
    /// the worker also bounds pending preparations to 64 additional sources.
    pub fn prepare_observation(
        &mut self,
        broker: &Broker,
        id: &str,
    ) -> Result<Observation, String> {
        if self.observations.closed {
            return Err("Controller observation worker stopped.".into());
        }
        let source = self.sources.get(id).ok_or("Unknown controller.")?;
        let epoch = self.observations.next_epoch()?;
        if self.observations.actor.is_none() {
            self.observations.actor = Some(Actor::start()?);
        }
        let shared = self.observations.actor.as_ref().unwrap().shared.clone();
        let slot = shared.prepare(broker, id, &self.session, source, epoch)?;
        Ok(Observation {
            shared,
            slot,
            epoch,
        })
    }
    pub fn take_observation_shutdown(&mut self) -> ObservationShutdown {
        self.observations.closed = true;
        ObservationShutdown(self.observations.actor.take())
    }
    /// Reports update capture evidence before sending typed immutable state.
    /// Queue overflow retains a bounded latest baseline and marks explicit loss.
    pub(super) fn publish_observation(
        &mut self,
        id: &str,
        reason: &str,
        captured: Option<&Event>,
        _descriptors: bool,
    ) {
        if let Some(source) = self.sources.get_mut(id) {
            if let Some(event) = captured {
                source.captured_us = event.captured_us;
                source.cursor = event.cursor;
            } else if reason == "baseline" || reason == "change" {
                source.captured_us = self.journal.now();
                source.cursor = self.journal.cursor();
            }
        }
        let Ok(epoch) = self.observations.next_epoch() else {
            if let Some(actor) = &self.observations.actor {
                actor.shared.stop();
            }
            return;
        };
        if let Some(actor) = &self.observations.actor {
            actor.shared.capture(id, |slot| {
                Update::new(
                    id,
                    &self.session,
                    self.sources.get(id),
                    reason,
                    epoch,
                    false,
                    slot,
                )
            });
        }
    }
    pub(super) fn observation_metadata(&mut self) {
        let ids = self
            .observations
            .actor
            .as_ref()
            .map(|actor| actor.shared.ids())
            .unwrap_or_default();
        for id in ids {
            self.publish_observation(&id, "metadata", None, true);
        }
    }
}
impl Observations {
    fn next_epoch(&mut self) -> Result<u64, String> {
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or("Controller observation epoch exhausted.")?;
        Ok(self.epoch)
    }
}
#[cfg(test)]
mod concurrency_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
#[path = "observation_tests.rs"]
mod tests;
