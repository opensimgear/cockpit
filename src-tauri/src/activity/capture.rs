use super::event;
use osg_persistence::NewActivityEvent;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entity {
    category: &'static str,
    kind: &'static str,
    id: String,
    name: String,
    value: Value,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Snapshot(pub(super) BTreeMap<String, Entity>, std::collections::BTreeSet<&'static str>);
impl Snapshot {
    pub(crate) fn insert(&mut self, category: &'static str, kind: &'static str, id: &str, name: &str, value: Value) {
        self.0.insert(
            format!("{kind}:{id}"),
            Entity {
                category,
                kind,
                id: id.into(),
                name: name.chars().take(128).collect(),
                value,
            },
        );
    }
    pub(crate) fn games(&mut self, games: &[Value]) {
        for game in games {
            let Some(id) = game["id"].as_str() else { continue };
            let name = game["name"].as_str().unwrap_or(id);
            self.insert(
                "games",
                "game",
                id,
                name,
                select(
                    game,
                    &[
                        "name",
                        "installPath",
                        "executable",
                        "installations",
                        "installed",
                        "selectedInstallationId",
                        "tagNames",
                        "tags",
                        "background",
                        "defaultBackground",
                        "tagIds",
                        "integrationKey",
                        "provider",
                        "integrationCompatible",
                        "metadata",
                    ],
                ),
            );
            let mut setup = select(&game["integrationSetup"], &["state", "installed", "verified", "version"]);
            if let Some(reason) = game["integrationSetup"]["message"].as_str() {
                setup["message"] = json!(super::diagnostics::reason(reason));
            }
            self.insert("integrations", "integration", id, name, setup);
            self.insert(
                "games",
                "game_process",
                id,
                name,
                json!({"online":game["profileContext"]["processSession"].is_string(),"processSession":game["profileContext"]["processSession"]}),
            );
            self.insert(
                "integrations",
                "game_session",
                id,
                name,
                select(&game["integrationContext"], &["status", "contextSession", "providerSetupRevision", "catalogueRevision"]),
            );
            self.insert("games", "game_vehicle", id, name, select(&game["currentVehicle"], &["id", "name"]));
        }
    }
    pub(crate) fn discovery(&mut self, review: &Value) {
        let mut value = select(review, &["scanId", "status", "proposed", "libraries", "artworkReady", "tagged"]);
        value["diagnostics"] = json!(review["diagnostics"].as_array().into_iter().flatten().filter_map(Value::as_str).take(8).map(super::diagnostics::reason).collect::<Vec<_>>());
        self.insert("games", "discovery", "current", "Game discovery", value);
    }
    pub(crate) fn profiles(&mut self, profiles: &[osg_profiles::ProfileDocument]) {
        for profile in profiles {
            let document = serde_json::to_value(profile).unwrap_or(Value::Null);
            self.insert("profiles", "profile", &profile.id, &profile.name, super::changes::profile_snapshot(&document));
        }
    }
    pub(crate) fn profile_session(&mut self, status: Value, names: &BTreeMap<String, String>) {
        let id = status["owner"]["profileId"].as_str().or_else(|| status["appliedSnapshot"]["profileId"].as_str()).unwrap_or("profile-session");
        let name = names.get(id).map(String::as_str).unwrap_or("Profile session");
        let reasons = status["degradation"].as_array().into_iter().flatten().filter_map(Value::as_str).map(super::diagnostics::reason).collect::<Vec<_>>();
        let fault = status["fault"].as_str().map(super::diagnostics::reason);
        self.insert("profiles", "profile_session", "current", name, json!({"profileId":id,"state":status["state"],"degradation":reasons,"fault":fault}));
    }
    pub(crate) fn devices(&mut self, kind: &'static str, rows: &[Value]) {
        for device in rows {
            let Some(id) = device["id"].as_str().or_else(|| device["key"].as_str()) else { continue };
            let name = device["name"].as_str().unwrap_or(id);
            let mut value = select(
                device,
                &[
                    "online",
                    "connected",
                    "present",
                    "name",
                    "firmwareVersion",
                    "version",
                    "boot",
                    "configurationGeneration",
                    "reason",
                    "stateReason",
                    "unavailableReason",
                    "disconnectReason",
                    "lastError",
                    "failureReason",
                    "fault",
                    "error",
                    "diagnostics",
                ],
            );
            for key in ["reason", "stateReason", "unavailableReason", "disconnectReason", "lastError", "failureReason", "fault", "error"] {
                if let Some(reason) = device[key].as_str() {
                    value[key] = json!(super::diagnostics::reason(reason));
                }
            }
            if let Some(diagnostics) = device["diagnostics"].as_array() {
                value["diagnostics"] = json!(diagnostics.iter().filter_map(Value::as_str).map(super::diagnostics::reason).collect::<Vec<_>>());
            }
            self.insert("devices", kind, id, name, value);
        }
    }
    pub(crate) fn unavailable(&mut self, kind: &'static str) {
        self.1.insert(kind);
    }
    pub(super) fn preserve_unavailable(&mut self, before: &Self) {
        for (key, entity) in &before.0 {
            if self.1.contains(entity.kind) {
                self.0.entry(key.clone()).or_insert_with(|| entity.clone());
            }
        }
    }
}
pub(super) fn initial(snapshot: &Snapshot) -> Vec<NewActivityEvent> {
    let mut active = Snapshot::default();
    for (key, entity) in &snapshot.0 {
        if entity.kind == "discovery" && entity.value["status"] != "idle" {
            active.0.insert(key.clone(), entity.clone());
        }
    }
    diff(&Snapshot::default(), &active)
}
fn select(value: &Value, keys: &[&str]) -> Value {
    let mut result = serde_json::Map::new();
    for key in keys {
        if let Some(value) = value.get(*key) {
            // Decorated artwork may contain a full data URL. Retain only a stable
            // content fingerprint rather than copying images between snapshots.
            let value = if matches!(*key, "background" | "defaultBackground") {
                use sha2::Digest;
                json!(format!("{:x}", sha2::Sha256::digest(value.to_string().as_bytes())))
            } else {
                value.clone()
            };
            result.insert((*key).into(), value);
        }
    }
    Value::Object(result)
}
fn online(entity: &Entity) -> bool {
    ["online", "connected", "present"].iter().find_map(|key| entity.value[*key].as_bool()).unwrap_or(false)
}
fn state_name(state: &str) -> String {
    match state {
        "running" | "ready" | "active" => "Active".into(),
        "degraded" | "needs-attention" | "needs_attention" => "Degraded".into(),
        "faulted" | "failed" => "Failed".into(),
        "recovery-needed" | "recovery_needed" => "Needs recovery".into(),
        "awaiting-game" | "awaiting_game" | "waiting-game" => "Waiting for game".into(),
        "starting" | "transitioning" => "Starting".into(),
        "stopped" | "idle" => "Stopped".into(),
        "scanning" => "Scanning".into(),
        "completed" | "complete" => "Completed".into(),
        other => other.replace('-', " "),
    }
}
fn reasons(entity: &Entity) -> Vec<String> {
    let mut output = entity.value["degradation"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>();
    for key in ["fault", "message", "reason", "stateReason", "unavailableReason", "disconnectReason", "lastError", "failureReason", "error"] {
        if let Some(reason) = entity.value[key].as_str().filter(|text| !text.is_empty()) {
            if !output.contains(&reason.to_owned()) {
                output.push(reason.to_owned());
            }
        }
    }
    if entity.kind != "discovery" {
        output.extend(entity.value["diagnostics"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned));
    }
    output
}
fn transition_detail(kind: &str, old: Option<&Entity>, entity: &Entity) -> Option<String> {
    if kind == "profile_session" {
        let before = old.and_then(|old| old.value["state"].as_str()).map(state_name).unwrap_or_else(|| "Not active".into());
        let after = entity.value["state"].as_str().map(state_name).unwrap_or_else(|| "Unknown".into());
        return Some(format!("Session state: {before} → {after}"));
    }
    if kind == "integration" {
        let before = old.and_then(|old| old.value["state"].as_str().or_else(|| old.value["status"].as_str())).map(state_name).unwrap_or_else(|| "Not set up".into());
        let after = entity.value["state"].as_str().or_else(|| entity.value["status"].as_str()).map(state_name).unwrap_or_else(|| "Unknown".into());
        return Some(format!("Integration state: {before} → {after}"));
    }
    if kind == "game_vehicle" {
        let before = old.and_then(|old| old.value["name"].as_str().or_else(|| old.value["id"].as_str())).unwrap_or("None");
        let after = entity.value["name"].as_str().or_else(|| entity.value["id"].as_str()).unwrap_or("None");
        return Some(format!("Current vehicle: “{before}” → “{after}”"));
    }
    None
}
fn changed_lines(kind: &str, old: Option<&Entity>, entity: &Entity) -> Vec<String> {
    if kind=="game_session" {
        let mut lines=vec![];
        let before=old.map(|old|&old.value["status"]).unwrap_or(&Value::Null);
        let after=&entity.value["status"];
        if before!=after {lines.push(format!("Integration availability: {} → {}",before.as_str().map(state_name).unwrap_or_else(||"Unavailable".into()),after.as_str().map(state_name).unwrap_or_else(||"Unknown".into())));}
        if old.is_some_and(|old|old.value["contextSession"]!=entity.value["contextSession"]) {
            lines.push(if entity.value["contextSession"].is_string(){"Game integration session started or refreshed."}else{"Game integration session ended."}.into());
        }
        return lines;
    }
    let mut lines = super::changes::describe(kind, old.map(|entity| &entity.value), &entity.value);
    if kind == "profile" {
        lines.retain(|line| !line.starts_with("Revision:"));
    }
    if let Some(state) = transition_detail(kind, old, entity) {
        lines.retain(|line| !line.starts_with("State") && !line.starts_with("Status"));
        lines.insert(0, state);
    }
    if kind == "game_vehicle" {
        lines.retain(|line| !line.starts_with("Name changed"));
    }
    if kind == "game" {
        lines.retain(|line| !line.starts_with("Tag names changed") && !line.starts_with("Tags changed"));
    }
    for reason in reasons(entity) {
        let safe = super::diagnostics::reason(&reason);
        if !safe.is_empty() {
            lines.push(format!("Reason: {safe}"));
        }
    }
    lines
}
fn transition_for(entity: &Entity, old: Option<&Entity>) -> Option<&'static str> {
    Some(match entity.kind {
        "profile_session" => match entity.value["state"].as_str().unwrap_or("stopped") {
            "running" if old.is_some_and(|old| old.value["state"] == "degraded") => "recovered",
            "running" => "activated",
            "degraded" => "degraded",
            "faulted" | "recovery-needed" => "failed",
            "stopped" => "stopped",
            "awaiting-game" => "awaiting_game",
            _ => "transitioning",
        },
        "integration" => match entity.value["state"].as_str().or_else(|| entity.value["status"].as_str()).unwrap_or("") {
            "ready" | "verified" | "installed" => "ready",
            "failed" | "error" => "failed",
            "needs-attention" | "degraded" | "recovery-needed" => "needs_attention",
            _ => "updated",
        },
        "game_process" => {
            if old.is_none() && !online(entity) {
                return None;
            } else if online(entity) {
                "launched"
            } else {
                "exited"
            }
        }
        "discovery" => match entity.value["status"].as_str().unwrap_or("idle") {
            "idle" => return None,
            "failed" => "failed",
            "ready" if old.is_none_or(|old| old.value["status"] != "ready") => "completed",
            "scanning" if old.is_none_or(|old| old.value["scanId"] != entity.value["scanId"]) => "started",
            _ => "updated",
        },
        "game_vehicle" => {
            if old.is_none() && entity.value["id"].is_null() {
                return None;
            } else if entity.value["id"].is_null() {
                "cleared"
            } else {
                "changed"
            }
        }
        "gateway" | "module" | "controller" | "companion" => {
            if old.is_none() {
                "discovered"
            } else if old.is_some_and(|old| online(old) != online(entity)) {
                if online(entity) {
                    "online"
                } else {
                    "offline"
                }
            } else {
                "updated"
            }
        }
        "game" | "profile" => {
            if old.is_none() {
                "created"
            } else if old.is_some_and(|old| old.name != entity.name) {
                "renamed"
            } else {
                "updated"
            }
        }
        _ => "changed",
    })
}
pub(super) fn diff(before: &Snapshot, after: &Snapshot) -> Vec<NewActivityEvent> {
    let mut events = vec![];
    for (key, entity) in &after.0 {
        let old = before.0.get(key);
        if old.is_some_and(|old| old == entity) {
            continue;
        }
        let Some(transition) = transition_for(entity, old) else {
            continue;
        };
        if matches!(entity.kind,"profile"|"game_session") && old.is_some() && changed_lines(entity.kind, old, entity).is_empty() {
            continue;
        }
        let level = if entity.kind == "integration" && matches!(transition, "needs_attention" | "failed") {
            "warning"
        } else {
            match transition {
                "failed" => "error",
                "degraded" | "offline" | "needs_attention" => "warning",
                _ => "info",
            }
        };
        let id = if entity.kind == "profile_session" {
            entity.value["profileId"].as_str().unwrap_or(&entity.id)
        } else if entity.kind == "discovery" {
            entity.value["scanId"].as_str().unwrap_or(&entity.id)
        } else {
            &entity.id
        };
        let mut message = super::messages::transition(entity.kind, transition, &entity.name);
        let entity_reasons = reasons(entity);
        if !entity_reasons.is_empty() && matches!(entity.kind, "profile_session" | "integration" | "module" | "gateway" | "controller" | "companion") {
            message = format!("{} Reason: {}", message, entity_reasons.iter().map(|text| super::diagnostics::reason(text)).collect::<Vec<_>>().join("; "));
        }
        if entity.kind == "discovery" && transition == "completed" {
            message = "Game discovery finished.".into();
        }
        let event_transition = if entity.kind == "integration" { "changed" } else { transition };
        let mut captured = event(entity.category, &format!("{}_{}", entity.kind, event_transition), level, message, Some(id.into()), Some(entity.name.clone()));
        let mut lines = changed_lines(entity.kind, old, entity);
        if entity.kind == "discovery" {
            lines.push(format!(
                "{} proposed changes; {} artwork ready; {} tags applied.",
                entity.value["proposed"].as_u64().unwrap_or(0),
                entity.value["artworkReady"].as_u64().unwrap_or(0),
                entity.value["tagged"].as_u64().unwrap_or(0)
            ));
            for diagnostic in entity.value["diagnostics"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                lines.push(format!("Diagnostic: {}", super::diagnostics::reason(diagnostic)));
            }
        }
        captured.detail = lines.join("\n");
        events.push(captured);
    }
    for (key, entity) in &before.0 {
        if after.0.contains_key(key) {
            continue;
        }
        // Companion/device disappearance is an availability transition. A game
        // removal already covers its integration/process projections.
        if matches!(entity.kind, "integration" | "game_process" | "game_session" | "game_vehicle" | "discovery") {
            continue;
        }
        let transition = if matches!(entity.kind, "game" | "profile") { "removed" } else { "offline" };
        let mut message = super::messages::transition(entity.kind, transition, &entity.name);
        let previous_reasons = reasons(entity);
        if transition == "offline" && !previous_reasons.is_empty() {
            message = format!("{} Reason: {}", message, previous_reasons.iter().map(|text| super::diagnostics::reason(text)).collect::<Vec<_>>().join("; "));
        }
        let captured = event(
            entity.category,
            &format!("{}_{}", entity.kind, transition),
            if transition == "offline" { "warning" } else { "info" },
            message,
            Some(entity.id.clone()),
            Some(entity.name.clone()),
        );
        events.push(captured);
    }
    events
}

#[cfg(test)]
#[path = "capture_details_tests.rs"]
mod detail_tests;
