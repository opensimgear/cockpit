use super::*;
use osg_streams::{Publisher, TopicSpec};

const TOPIC: &str = "storage.backups";

pub(super) struct Observation {
    publisher: Publisher,
    runtime: crate::observations::Runtime,
}

impl Runtime {
    pub(crate) fn bind_observation(&self, runtime: &crate::observations::Runtime) -> Result<(), String> {
        if self.database.is_err() {
            return Ok(());
        }
        let publisher = runtime.broker().register(
            TopicSpec {
                topic: TOPIC.into(),
                source: "backup-storage".into(),
                audiences: vec!["ui".into(), "mcp".into()],
            },
            self.observation_payload("idle"),
            runtime.captured_us(),
            TOPIC.into(),
        ).map_err(|error| error.to_string())?;
        *self.observation.lock().unwrap_or_else(|error| error.into_inner()) =
            Some(Observation { publisher, runtime: runtime.clone() });
        Ok(())
    }

    fn observation_payload(&self, phase: &str) -> Value {
        // Filesystem and database diagnostics may contain private paths.
        let mut payload = json!({"phase":phase,"status":self.status().ok()});
        if phase == "creating" {
            payload["createId"] = json!(self.create_id());
        }
        payload
    }

    pub(super) fn publish_observation(&self, phase: &str) {
        let observation = self.observation.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(observation) = observation.as_ref() {
            let _ = observation.publisher.publish_latest_state(
                1, self.observation_payload(phase), observation.runtime.captured_us(), TOPIC,
            );
        }
    }

    pub(super) fn observe_operation<T>(
        &self,
        phase: &str,
        operation: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        self.publish_observation(phase);
        let result = operation();
        self.publish_observation(if result.is_ok() { "complete" } else { "failed" });
        result
    }
}
