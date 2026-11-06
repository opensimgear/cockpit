use super::*;
use osg_streams::{NativeWake, NativeWatch};
use std::sync::Arc;
impl Tracker {
    pub(crate) fn current(&self, id: &str) -> Option<Value> {
        self.entries
            .get(id)
            .filter(|entry| {
                #[cfg(test)]
                if entry.injected {
                    return true;
                }
                entry.process_current()
                    && entry
                        .telemetry
                        .as_ref()
                        .is_some_and(|capture| !capture.expired())
            })
            .and_then(|entry| entry.current.clone())
    }
    /// Share the committed capture so projection can run outside the tracker lock.
    pub(crate) fn telemetry(&self, game: &Value) -> Option<Arc<Capture>> {
        self.entries
            .get(game["id"].as_str()?)
            .filter(|entry| {
                entry.installation == Self::installation(game)
                    && entry.enabled
                    && (entry.reader.is_some() || entry.polling)
                    && entry.process_current()
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
        if entry.polling {
            // A slow BIOS read must not freeze independently acquired ownship
            // telemetry. This handle reads the same producer cache without I/O.
            return Some(entry.detached_physics.as_ref()?.snapshot());
        }
        let Reader::Dcs {
            physics: Some(physics),
            ..
        } = entry.reader.as_mut()?
        else {
            return None;
        };
        Some(physics.snapshot())
    }
    pub(crate) fn dcs_physics_watch(
        &mut self,
        process: &ProcessIdentity,
        wake: Arc<NativeWake>,
    ) -> Result<
        Option<NativeWatch<crate::dcs_physics::Snapshot, crate::dcs_physics::Snapshot>>,
        String,
    > {
        let Some(entry) = self.entries.values_mut().find(|entry| {
            entry.integration == "dcs"
                && entry.enabled
                && entry.process.as_ref() == Some(process)
                && entry.observed.elapsed() <= Duration::from_secs(2)
        }) else {
            return Ok(None);
        };
        let view = if entry.polling {
            entry.detached_physics.as_ref()
        } else {
            match entry.reader.as_ref() {
                Some(Reader::Dcs {
                    physics: Some(physics),
                    ..
                }) => {
                    return physics
                        .view()
                        .watch(wake)
                        .map(Some)
                        .map_err(|error| format!("{error:?}"));
                }
                _ => None,
            }
        };
        view.map(|view| view.watch(wake).map_err(|error| format!("{error:?}")))
            .transpose()
    }
    pub(crate) fn inspect(&mut self, id: &str, client: &str, channels: Vec<String>, observation: Option<std::sync::Weak<crate::observations::TelemetryLease>>) -> Result<(), String> {
        let Some(entry) = self.entries.get_mut(id).filter(|entry| entry.integration.starts_with("x-plane-")) else {
            return Ok(());
        };
        let lease = observation.as_ref().and_then(std::sync::Weak::upgrade);
        if let (Some(lease), Some(wake)) = (&lease, &self.capture_wake) { lease.watch(wake); }
        let mut deferred = false;
        entry.inspection.update(client, channels, Instant::now(), observation, |selected| {
            if let Some(reader) = &mut entry.reader {
                reader.inspect(&entry.integration, selected)?;
            } else {
                deferred = true;
            }
            Ok(())
        })?;
        if deferred { entry.inspection.pending(true); }
        if let Some(wake) = &self.capture_wake { wake.notify(); }
        Ok(())
    }
    pub(crate) fn decorate(&self, game: &mut Value) {
        self.decorate_view(game, false);
    }
    pub(crate) fn decorate_summary(&self, game: &mut Value) {
        self.decorate_view(game, true);
    }
    fn decorate_view(&self, game: &mut Value, summary: bool) {
        let id = game["id"].as_str().unwrap_or("").to_owned();
        let integration = crate::game_discovery::integration_key(game).unwrap_or("");
        let supported = !integration.is_empty();
        let message = if integration.starts_with("assetto-corsa") {
            "Cars are discovered from native telemetry. Load a car once to add it to the vehicle list."
        } else if integration == "dcs" {
            "Aircraft come from the installed DCS-BIOS catalogue. Current aircraft requires fresh DCS-BIOS exports."
        } else {
            "Aircraft come from this installation's Aircraft folder. Current aircraft requires fresh RREF telemetry."
        };
        let mut values = BTreeMap::new();
        if let Some(seen) = self.seen.get(&id) {
            for (id, value) in seen { values.insert(id.as_str(), value); }
        }
        if let Some(entry) = self.entries.get(&id) {
            for value in &entry.vehicles { values.insert(value["id"].as_str().unwrap(), value); }
        }
        game["vehicleSupport"] = json!({"supported":supported,"kind":if integration.starts_with("assetto-corsa"){"car"}else{"aircraft"},"message":message});
        if summary { game["vehicleSupport"]["vehicleCount"] = json!(values.len()); }
        else { game["vehicleSupport"]["vehicles"] = json!(values.values().collect::<Vec<_>>()); }
        game["vehicleSupport"]["retentionError"] = json!(self
            .retention_error
            .clone()
            .or_else(|| self.writer.as_ref().and_then(|writer| writer.error())));
        game["currentVehicle"] = self
            .entries
            .get(&id)
            .filter(|entry| entry.installation_fields.iter().zip(["installPath", "executable", "installed", "integrationKey", "provider"]).all(|(known, field)| known == &game[field]))
            .and_then(|_| self.current(&id))
            .unwrap_or(Value::Null);
        game["vehicleStatus"] = json!({"status":if game["currentVehicle"].is_object(){"ready"}else if self.entries.get(&id).is_some_and(|entry|entry.error.is_some()){"unavailable"}else{"waiting"},"message":self.entries.get(&id).and_then(|entry|entry.error.as_deref()).unwrap_or("Waiting for a running game and fresh vehicle telemetry.")});
    }
}
