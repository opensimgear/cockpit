//! Read-only, bounded pages of passive game telemetry, independent of profiles.
use super::*;
use crate::game_telemetry_inspection as inspect;

impl GamesRuntime {
    pub(crate) fn dcs_physics_snapshot(
        &self,
        process: &crate::game_discovery::ProcessIdentity,
    ) -> Option<crate::dcs_physics::Snapshot> {
        self.inner
            .vehicles
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .dcs_physics(process)
    }
    pub(crate) fn inspect_telemetry(&self, request: &Value) -> Result<Value, String> {
        let allowed = [
            "command",
            "clientId",
            "gameId",
            "offset",
            "limit",
            "search",
            "group",
            "background",
        ];
        if request
            .as_object()
            .is_none_or(|object| object.keys().any(|key| !allowed.contains(&key.as_str())))
        {
            return Err("Unsupported telemetry inspection field.".into());
        }
        let index = |field: &str, default: u64, max: u64| -> Result<usize, String> {
            match request.get(field) {
                None => Ok(default as usize),
                Some(value) => value
                    .as_u64()
                    .filter(|value| *value <= max)
                    .map(|value| value as usize)
                    .ok_or_else(|| "Choose a bounded telemetry page.".into()),
            }
        };
        let offset = index("offset", 0, 65_536)?;
        let limit = index("limit", 64, 128)?;
        if limit == 0 {
            return Err("Choose at least one telemetry channel.".into());
        }
        let text = |field: &str, max: usize| -> Result<String, String> {
            match request.get(field) {
                None => Ok(String::new()),
                Some(value) => value
                    .as_str()
                    .filter(|value| value.len() <= max && !value.chars().any(char::is_control))
                    .map(|value| value.trim().to_owned())
                    .ok_or_else(|| "Invalid telemetry filter.".into()),
            }
        };
        let search = text("search", 200)?.to_lowercase();
        let group = text("group", 256)?;
        let games = self.resource_catalogue();
        let captures = {
            let tracker = self
                .inner
                .vehicles
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            games
                .iter()
                .map(|game| {
                    (
                        game["id"].as_str().unwrap_or("").to_owned(),
                        tracker.telemetry(game),
                    )
                })
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        let selected = if let Some(id) = request.get("gameId") {
            game_id(id)?.to_owned()
        } else {
            games
                .iter()
                .find(|game| {
                    captures
                        .get(game["id"].as_str().unwrap_or(""))
                        .and_then(Option::as_ref)
                        .is_some_and(|capture| {
                            !capture.expired()
                                && matches!(capture.status.as_str(), "live" | "paused")
                        })
                })
                .or_else(|| games.iter().find(|game| game["installed"] == true))
                .or_else(|| games.first())
                .and_then(|game| game["id"].as_str())
                .unwrap_or("")
                .to_owned()
        };
        let vehicle = games
            .iter()
            .find(|game| game["id"] == selected)
            .and_then(|game| game["currentVehicle"]["id"].as_str());
        let catalogue = games
            .iter()
            .find(|game| game["id"] == selected)
            .and_then(crate::game_discovery::integration_key)
            .ok_or_else(|| "Attach a compatible integration to inspect telemetry.".to_owned())
            .and_then(|key| {
                self.inner
                    .setup
                    .resource_integration_catalogue_for_vehicle(key, vehicle)
            });
        let catalogue_error = catalogue.as_ref().err().cloned();
        let all = catalogue
            .as_ref()
            .map(inspect::channels)
            .unwrap_or_default();
        let mut result = vec![];
        for game in games {
            let id = game["id"].as_str().unwrap_or("");
            let capture = captures.get(id).and_then(Option::as_ref);
            let fallback_status = if game["installed"] != true {
                "not-installed"
            } else if game["integrationSetup"]["state"] != "ready" {
                "setup-required"
            } else {
                "waiting"
            };
            let status = capture
                .map(|capture| {
                    if capture.expired() {
                        "stale"
                    } else {
                        match capture.status.as_str() {
                            "ready" => "live",
                            "off" => "waiting",
                            status => status,
                        }
                    }
                })
                .unwrap_or(fallback_status);
            let selected_game = id == selected;
            let (channels, total, groups, requested) = if selected_game {
                inspect::page(&all, capture, offset, limit, &search, &group)
            } else {
                (vec![], 0, vec![], vec![])
            };
            if selected_game {
                self.inner
                    .vehicles
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .inspect(id, requested)?;
            }
            let message = if selected_game {
                catalogue_error.as_deref().or_else(|| {
                    catalogue
                        .as_ref()
                        .ok()
                        .and_then(|catalogue| catalogue["catalogueWarning"].as_str())
                })
            } else {
                None
            }
            .or_else(|| capture.and_then(|capture| capture.message.as_deref()))
            .or_else(|| game["vehicleStatus"]["message"].as_str());
            result.push(json!({"id":id,"name":game["name"].as_str().unwrap_or(id),"status":status,"source":capture.map(|capture|capture.source.as_str()).unwrap_or(if crate::game_discovery::integration_key(&game)==Some("dcs") {"dcs-bios"} else if game["provider"]=="x-plane" {"rref"} else if crate::game_discovery::integration_key(&game).is_some() {"shared-memory"} else {"none"}),"message":message,"packet":capture.map(|capture|capture.packet.clone()).unwrap_or(Value::Null),"ageMs":capture.and_then(|capture|capture.age_ms.map(|age|age.saturating_add(capture.at.elapsed().as_millis() as u64))),"context":capture.map(|capture|capture.context.clone()).unwrap_or(Value::Null),"groups":groups,"total":total,"offset":if selected_game {offset} else {0},"limit":limit,"channels":channels}));
        }
        Ok(json!({"schemaVersion":1,"games":result}))
    }
}
