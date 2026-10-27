use super::*;

impl Runtime {
    /// A new tombstone is an explicit Forget. Poll timestamps and rediscovery
    /// never add tombstones and therefore never become user activity here.
    pub(crate) fn remembered_changes(&self, before: &Value, after: &Value, request: &Value) {
        for id in after["forgotten"].as_object().into_iter().flat_map(|forgotten| forgotten.keys()) {
            if before["forgotten"].get(id).is_some() { continue; }
            if after["forgotten"][id]["activityRecorded"] == true { continue; }
            let name=before["components"].as_array().into_iter().flatten().find(|component|component["id"].as_str()==Some(id))
                .and_then(|component|component["name"].as_str()).unwrap_or(id);
            let mut captured=event("devices","device_forgotten","info",format!("{name} forgotten."),Some(id.clone()),Some(name.into()));
            captured.source=if request["clientId"].as_str().is_some_and(|id|id.starts_with("cockpit-mcp-")){"mcp"}else{"cockpit"}.into();
            let _=self.sender.send(Message::Event(captured));
        }
    }
}
