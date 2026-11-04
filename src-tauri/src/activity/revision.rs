use crate::observations::Runtime;
use osg_streams::{Publisher, TopicSpec};
use serde_json::json;
use std::sync::Mutex;

const TOPIC: &str = "activity.log";

#[derive(Default)]
pub(super) struct Revision(Mutex<Option<(Publisher, Runtime)>>);

impl Revision {
    pub(super) fn observe(&self, runtime: &Runtime) -> Result<(), String> {
        let mut current = self.0.lock().unwrap_or_else(|error| error.into_inner());
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
                json!({}),
                runtime.captured_us(),
                TOPIC.into(),
            )
            .map_err(|error| error.to_string())?;
        *current = Some((publisher, runtime.clone()));
        Ok(())
    }

    pub(super) fn changed(&self) {
        let current = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if let Some((publisher, runtime)) = current.as_ref() {
            // The frame revision is the invalidation. Filtered queries and
            // pinned older pages remain owned by the activity database.
            let _ = publisher.publish_latest_state(1, json!({}), runtime.captured_us(), TOPIC);
        }
    }
}
