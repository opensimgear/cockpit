use super::{Runtime, Services};
use crate::observations;
use osg_streams::{Publisher, TopicSpec};
use serde_json::{json, Value};

const TOPIC: &str = "companion.listener";

pub(super) struct Observation {
    publisher: Publisher,
    runtime: observations::Runtime,
    previous: Value,
}

impl Runtime {
    pub(crate) fn observe_listener(&self, runtime: &observations::Runtime) -> Result<(), String> {
        let mut observation = self.listener_observation.lock().unwrap_or_else(|p| p.into_inner());
        let publisher = runtime.broker().register(
            TopicSpec {
                topic: TOPIC.into(),
                source: "companion-listener".into(),
                audiences: vec!["ui".into(), "browser".into(), "mcp".into()],
            },
            json!({}), runtime.captured_us(), TOPIC.into(),
        ).map_err(|error| error.to_string())?;
        *observation = Some(Observation { publisher, runtime: runtime.clone(), previous: Value::Null });
        Ok(())
    }

    pub(super) fn notify_listener(&self, services: Result<&Services, &str>) {
        let mut observation = self.listener_observation.lock().unwrap_or_else(|p| p.into_inner());
        let Some(observation) = observation.as_mut() else { return; };
        let stamp = match services {
            Ok(services) => {
                let (revision, _) = services.saved_listener.snapshot();
                let live = services.server.status();
                json!([revision, live.enabled, live.endpoint, live.discovery, live.last_error, live.ca_spki_sha256])
            }
            Err(error) => json!([error]),
        };
        if observation.previous != stamp && observation.publisher.publish_latest_state(
            observation.publisher.generation(), json!({}), observation.runtime.captured_us(), TOPIC,
        ).is_ok() {
            observation.previous = stamp;
        }
    }
}
