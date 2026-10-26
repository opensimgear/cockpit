use super::*;
impl Tracker {
    pub(crate) fn current(&self, id: &str) -> Option<Value> {
        self.entries
            .get(id)
            .filter(|entry| {
                #[cfg(test)]
                if entry.injected {
                    return true;
                }
                entry.observed.elapsed() <= Duration::from_secs(2)
            })
            .and_then(|entry| entry.current.clone())
    }
    pub(crate) fn telemetry(&self, game: &Value) -> Option<Capture> {
        self.entries
            .get(game["id"].as_str()?)
            .filter(|entry| {
                entry.installation == Self::installation(game)
                    && entry.enabled
                    && entry.reader.is_some()
                    && entry.observed.elapsed() <= Duration::from_secs(2)
            })
            .and_then(|entry| entry.telemetry.clone())
    }
    pub(crate) fn dcs_physics(
        &mut self,
        process: &ProcessIdentity,
    ) -> Option<crate::dcs_physics::Snapshot> {
        let entry = self.entries.values_mut().find(|entry| {
            entry.integration == "dcs"
                && entry.enabled
                && entry.process.as_ref() == Some(process)
                && entry.observed.elapsed() <= Duration::from_secs(2)
        })?;
        let Reader::Dcs {
            physics: Some(physics),
            ..
        } = entry.reader.as_mut()?
        else {
            return None;
        };
        Some(physics.snapshot())
    }
    pub(crate) fn inspect(&mut self, id: &str, channels: Vec<String>) -> Result<(), String> {
        if let Some(entry) = self.entries.get_mut(id) {
            if channels != entry.inspection {
                if let Some(reader) = &mut entry.reader {
                    reader.inspect(&entry.integration, &channels)?;
                }
                entry.inspection = channels;
            }
            entry.inspected = Instant::now();
        }
        Ok(())
    }
    pub(crate) fn decorate(&self, game: &mut Value) {
        let id = game["id"].as_str().unwrap_or("").to_owned();
        let integration = crate::game_discovery::integration_key(game).unwrap_or("");
        let supported = !integration.is_empty();
        let mut values = self.seen.get(&id).cloned().unwrap_or_default();
        if let Some(entry) = self.entries.get(&id) {
            for value in &entry.vehicles {
                values.insert(value["id"].as_str().unwrap().into(), value.clone());
            }
        }
        let message = if integration.starts_with("assetto-corsa") {
            "Cars are discovered from native telemetry. Load a car once to add it to the vehicle list."
        } else if integration == "dcs" {
            "Aircraft come from the installed DCS-BIOS catalogue. Current aircraft requires fresh DCS-BIOS exports."
        } else {
            "Aircraft come from this installation's Aircraft folder. Current aircraft requires fresh RREF telemetry."
        };
        game["vehicleSupport"] = json!({"supported":supported,"kind":if integration.starts_with("assetto-corsa"){"car"}else{"aircraft"},"vehicles":values.values().collect::<Vec<_>>(),"message":message});
        game["vehicleSupport"]["retentionError"] = json!(self
            .retention_error
            .clone()
            .or_else(|| self.writer.as_ref().and_then(|writer| writer.error())));
        game["currentVehicle"] = self
            .entries
            .get(&id)
            .filter(|entry| entry.installation == Self::installation(game))
            .and_then(|_| self.current(&id))
            .unwrap_or(Value::Null);
        game["vehicleStatus"] = json!({"status":if game["currentVehicle"].is_object(){"ready"}else if self.entries.get(&id).is_some_and(|entry|entry.error.is_some()){"unavailable"}else{"waiting"},"message":self.entries.get(&id).and_then(|entry|entry.error.as_deref()).unwrap_or("Waiting for a running game and fresh vehicle telemetry.")});
    }
}
