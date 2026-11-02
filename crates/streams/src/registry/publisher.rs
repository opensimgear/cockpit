use super::{RegistryError, Topic};
use serde_json::Value;
use std::sync::Arc;

/// Capability retained by the owning service; observation readers cannot mutate it.
#[derive(Clone)]
pub struct Publisher {
    pub(super) topic: Arc<Topic>,
}

impl Publisher {
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
        self.topic
            .identity
            .validate(&snapshot, captured_us, clock_id, false)?;
        self.topic
            .identity
            .validate(&change, captured_us, clock_id, true)?;
        Ok(self.topic.stream.publish(
            expected_generation,
            snapshot,
            change,
            captured_us,
            clock_id,
        )?)
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
