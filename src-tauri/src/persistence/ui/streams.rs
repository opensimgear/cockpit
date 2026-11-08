use super::*;

// Remembered inventory exceeds the change frame budget, so publish its revision.
pub(crate) const OBSERVED_KEYS: [&str; 8] = [
    "theme", "devices-mode", "canvas-navigation", "canvas-styling", "tray", "progress", "remembered", "game-process-events",
];

pub(super) fn payload(key: &str, entry: Result<&Entry, &String>) -> Value {
    match entry {
        Ok(entry) if key == "remembered" => json!({"ready":true,"revision":entry.revision.to_string()}),
        Ok(entry) => json!({"ready":true,"revision":entry.revision.to_string(),"value":entry.value}),
        Err(error) => json!({"ready":false,"error":error}),
    }
}

impl Preferences {
    pub(crate) fn bind_observations(&self, runtime: &crate::observations::Runtime) {
        // Explicit service bootstrap, never subscription-triggered collection.
        for key in OBSERVED_KEYS { let _ = self.read(key); }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        for key in OBSERVED_KEYS {
            let fallback = Entry {revision:0,value:defaults(key)};
            let initial = payload(key, match state.errors.get(key) {
                Some(error) => Err(error),
                None => Ok(state.entries.get(key).unwrap_or(&fallback)),
            });
            let publisher = runtime.broker().register(osg_streams::TopicSpec {
                topic:format!("preferences.{key}"),source:"ui-preferences".into(),
                audiences:vec!["ui".into(),"browser".into(),"mcp".into()],
            }, initial, runtime.captured_us(), "cockpit-state".into());
            if let Ok(publisher) = publisher { state.streams.insert(key.into(),publisher); }
        }
        state.observation_clock=Some(runtime.clone());
    }
}
