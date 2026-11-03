use super::{RegistryError, Topic};
use serde_json::Value;
use std::sync::Arc;

/// Capability retained by the owning service; observation readers cannot mutate it.
#[derive(Clone)]
pub struct Publisher {
    pub(super) topic: Arc<Topic>,
}

impl Publisher {
    /// An idle read model can defer encoding until its owner registers a reader.
    pub fn has_readers(&self) -> bool {
        !self.topic.stream.shared.lock().subscribers.is_empty()
    }
    /// Withdraw a topic when its owner cannot publish a coherent read model.
    pub fn close(&self) {
        self.topic.stream.close();
    }

    pub fn publish(
        &self,
        expected_generation: u64,
        snapshot: Value,
        change: Value,
        captured_us: u64,
        clock_id: &str,
    ) -> Result<(u64, u64), RegistryError> {
        let snapshot = self
            .topic
            .identity
            .encode(&snapshot, captured_us, clock_id, false)?;
        let change = self
            .topic
            .identity
            .encode(&change, captured_us, clock_id, true)?;
        Ok(self.topic.stream.publish_encoded(
            expected_generation,
            snapshot,
            change,
            captured_us,
            clock_id,
        )?)
    }

    /// Publish a complete absolute STATE, replacing each reader's pending state
    /// with the latest snapshot. This deliberately coalesces unread values.
    /// NEVER use for ordered occurrences, relative changes, edges or commands:
    /// those must use `publish` so every admitted event retains its order.
    /// Existing loss/generation boundaries are delivered before the fresh state.
    pub fn publish_latest_state(
        &self,
        expected_generation: u64,
        snapshot: Value,
        captured_us: u64,
        clock_id: &str,
    ) -> Result<u64, RegistryError> {
        let snapshot = self.topic.identity.encode(&snapshot, captured_us, clock_id, false)?;
        Ok(self.topic.stream.update_latest_encoded(expected_generation, snapshot, captured_us, clock_id)?)
    }

    pub fn resynchronize(
        &self,
        expected_generation: u64,
        snapshot: Value,
        captured_us: u64,
        clock_id: &str,
    ) -> Result<u64, RegistryError> {
        self.topic
            .identity
            .validate(&snapshot, captured_us, clock_id, false)?;
        Ok(self
            .topic
            .stream
            .resynchronize(expected_generation, snapshot, captured_us, clock_id)?)
    }

    pub fn replace_generation(
        &self,
        generation: u64,
        snapshot: Value,
        captured_us: u64,
        clock_id: &str,
    ) -> Result<u64, RegistryError> {
        self.topic
            .identity
            .validate(&snapshot, captured_us, clock_id, false)?;
        Ok(self
            .topic
            .stream
            .replace_generation(generation, snapshot, captured_us, clock_id)?)
    }

    pub fn generation(&self) -> u64 {
        self.topic.stream.generation()
    }
    pub fn revision(&self) -> u64 {
        self.topic.stream.revision()
    }
}
