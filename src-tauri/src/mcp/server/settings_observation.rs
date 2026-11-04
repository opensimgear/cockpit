use super::*;
use osg_streams::{Publisher, TopicSpec};

const TOPIC: &str = "settings.mcp";

pub(super) struct Observation {
    publisher: Publisher,
    previous: Value,
}

impl Server {
    pub(super) fn observe_settings(&self) -> Result<(), String> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let runtime = &self.manager.observations;
        let publisher = runtime
            .broker()
            .register(
                TopicSpec {
                    topic: TOPIC.into(),
                    source: "mcp-settings".into(),
                    audiences: vec!["ui".into()],
                },
                json!({}),
                runtime.captured_us(),
                TOPIC.into(),
            )
            .map_err(|error| error.to_string())?;
        state.observation = Some(Observation {
            publisher,
            previous: state.status(),
        });
        Ok(())
    }

    pub(super) fn publish_settings(&self, state: &mut State, status: Value) {
        if let Some(observation) = state.observation.as_mut() {
            if observation.previous == status {
                return;
            }
            // Local consumers read the status command after invalidation. The
            // topic contains no credential metadata or issued token.
            if observation
                .publisher
                .publish_latest_state(1, json!({}), self.manager.observations.captured_us(), TOPIC)
                .is_ok()
            {
                observation.previous = status;
            }
        }
    }
}

impl State {
    pub(super) fn status(&self) -> Value {
        let settings = &self.store.settings;
        json!({"settingsRevision":self.store.revision,"enabled":settings.enabled,"port":settings.port,
            "endpoint":format!("http://127.0.0.1:{}/mcp",settings.port),"running":self.listener.is_some(),
            "error":self.error,"credentials":settings.credentials.iter().map(Credential::metadata).collect::<Vec<_>>(),
            "protocolVersions":VERSIONS,"sdk":"rmcp 3.6.0","readOnly":self.store.error.is_some()})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_invalidations_follow_saved_credentials_without_exposing_them() {
        let manager = Arc::new(GatewayManager::mcp_test_manager());
        let server = Server::open(manager.clone(), None);
        let broker = manager.observations.broker();
        assert!(broker.subscribe("mcp-client", "mcp", TOPIC).is_err());
        let mut reader = broker.subscribe("settings-test", "ui", TOPIC).unwrap();
        assert_eq!(reader.try_recv().unwrap().unwrap().revision, "1");

        let issued = server.create_credential("Temporary client".into(), "read".into(), 0).unwrap();
        let frame = reader.try_recv().unwrap().unwrap();
        assert_eq!(frame.revision, "2");
        assert_eq!(frame.payload, Some(json!({})));
        server.status();
        assert!(reader.try_recv().unwrap().is_none());
        server.revoke_credential(issued["credential"]["id"].as_str().unwrap(), 1).unwrap();
        assert_eq!(reader.try_recv().unwrap().unwrap().revision, "3");
        server.shutdown();
    }
}
