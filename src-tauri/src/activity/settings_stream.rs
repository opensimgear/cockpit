use crate::observations::Runtime;
use osg_persistence::{ActivitySettings, Database};
use osg_streams::{Publisher, TopicSpec};
use serde_json::json;
use std::sync::Mutex;

const TOPIC: &str = "settings.activity";

#[derive(Default)]
pub(super) struct SettingsStream(Mutex<Option<(Publisher, Runtime)>>);

impl SettingsStream {
    pub(super) fn observe(&self, runtime: &Runtime, database: &Database) -> Result<(), String> {
        let mut current = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let settings = database.activity_settings()?;
        let publisher = runtime
            .broker()
            .register(
                TopicSpec {
                    topic: TOPIC.into(),
                    source: "activity".into(),
                    audiences: osg_streams::KNOWN_AUDIENCES
                        .iter()
                        .map(|value| (*value).into())
                        .collect(),
                },
                json!(settings),
                runtime.captured_us(),
                TOPIC.into(),
            )
            .map_err(|error| error.to_string())?;
        *current = Some((publisher, runtime.clone()));
        Ok(())
    }

    pub(super) fn save(&self, database: &Database, days: u32) -> Result<ActivitySettings, String> {
        // Keep commit and publication in order across UI and MCP callers.
        let current = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let settings = database.activity_settings_save(days)?;
        if let Some((publisher, runtime)) = current.as_ref() {
            let _ = publisher.publish_latest_state(1, json!(settings), runtime.captured_us(), TOPIC);
        }
        Ok(settings)
    }
}
