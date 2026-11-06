use super::Settings;
use crate::observations::Runtime;
use osg_streams::{Publisher, TopicSpec};
use serde_json::{json, Value};
use std::sync::Mutex;

const TOPIC: &str = "settings.desktop";

pub(crate) struct Observation {
    publisher: Publisher,
    previous: Mutex<Value>,
}

fn snapshot(settings: &Settings, native: bool) -> Value {
    json!({"enabled":settings.value.enabled,"scope":settings.value.scope,
        "qualified":native && super::qualified(),"reason":super::reason(),
        "error":settings.error})
}

impl Observation {
    pub(crate) fn register(runtime: &Runtime, native: bool) -> Result<Self, String> {
        let settings = if native { Settings::native() } else { Settings::open(None) };
        let initial = snapshot(&settings, native);
        let publisher = runtime.broker().register(TopicSpec {
            topic: TOPIC.into(), source: "desktop-settings".into(), audiences: vec!["ui".into(), "mcp".into()],
        }, initial.clone(), runtime.captured_us(), TOPIC.into()).map_err(|error| error.to_string())?;
        Ok(Self { publisher, previous: Mutex::new(initial) })
    }

    pub(crate) fn committed(&self, runtime: &Runtime, settings: &Settings) {
        let next = snapshot(settings, true);
        let mut previous = self.previous.lock().unwrap_or_else(|error| error.into_inner());
        if *previous != next && self.publisher.publish_latest_state(1, next.clone(), runtime.captured_us(), TOPIC).is_ok() {
            *previous = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_enablement_is_shared_and_identical_updates_are_quiet() {
        let runtime = Runtime::new();
        let observation = Observation::register(&runtime, false).unwrap();
        let broker = runtime.broker();
        let mut ui = broker.subscribe("desktop-settings-ui", "ui", TOPIC).unwrap();
        let mut mcp = broker.subscribe("desktop-settings-mcp", "mcp", TOPIC).unwrap();
        let _ = ui.try_recv().unwrap().unwrap();
        let _ = mcp.try_recv().unwrap().unwrap();
        let directory = crate::test_directory();
        let mut settings = Settings::open(Some(directory.path().join("desktop.json")));
        settings.configure(true, super::super::Scope::DesktopWide).unwrap();
        observation.committed(&runtime, &settings);
        let change = ui.try_recv().unwrap().unwrap();
        assert_eq!(mcp.try_recv().unwrap().unwrap(), change);
        let payload = change.payload.unwrap();
        assert_eq!(payload["enabled"], true);
        assert_eq!(payload["scope"]["kind"], "desktop-wide");
        assert!(payload.get("application").is_none());
        observation.committed(&runtime, &settings);
        assert!(ui.try_recv().unwrap().is_none());
        runtime.close();
    }
}
