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
        self.0.insert(format!("{kind}:{id}"), Entity { category, kind, id: id.into(), name: name.chars().take(128).collect(), value });
    }
    pub(crate) fn games(&mut self, games: &[Value]) {
        for game in games {
            let Some(id) = game["id"].as_str() else { continue };
            let name = game["name"].as_str().unwrap_or(id);
            self.insert("games", "game", id, name, select(game, &["name", "installPath", "executable", "installations", "installed", "selectedInstallationId", "background", "defaultBackground", "tagIds", "integrationKey", "provider", "integrationCompatible", "metadata"]));
            let mut setup=select(&game["integrationSetup"], &["state", "installed", "verified", "version"]);
            if let Some(reason)=game["integrationSetup"]["message"].as_str() {setup["message"]=json!(super::audit::safe_detail(reason));}
            self.insert("integrations", "integration", id, name, setup);
            self.insert("games", "game_process", id, name, json!({"online":game["profileContext"]["processSession"].is_string(),"processSession":game["profileContext"]["processSession"]}));
            self.insert("integrations", "game_session", id, name, select(&game["integrationContext"], &["status", "contextSession","providerSetupRevision","catalogueRevision"]));
            self.insert("games", "game_vehicle", id, name, select(&game["currentVehicle"], &["id","name"]));
        }
    }
    pub(crate) fn discovery(&mut self, review: &Value) {
        let mut value=select(review,&["scanId","status","proposed","libraries","artworkReady","tagged"]);
        value["diagnostics"]=json!(review["diagnostics"].as_array().into_iter().flatten().filter_map(Value::as_str).take(8).map(super::audit::safe_detail).collect::<Vec<_>>());
        self.insert("games","discovery","current","Game discovery",value);
    }
    pub(crate) fn profiles(&mut self, profiles: &[osg_profiles::ProfileDocument]) {
        for profile in profiles {
            self.insert("profiles", "profile", &profile.id, &profile.name, json!({"name":profile.name,"revision":profile.revision}));
        }
    }
    pub(crate) fn profile_session(&mut self, status: Value, names: &BTreeMap<String, String>) {
        let id = status["owner"]["profileId"].as_str().or_else(|| status["appliedSnapshot"]["profileId"].as_str()).unwrap_or("profile-session");
        let name = names.get(id).map(String::as_str).unwrap_or("Profile session");
        self.insert("profiles", "profile_session", "current", name, json!({"profileId":id,"state":status["state"],"owner":status["owner"],"degradation":status["degradation"],"fault":status["fault"]}));
    }
    pub(crate) fn devices(&mut self, kind: &'static str, rows: &[Value]) {
        for device in rows {
            let Some(id) = device["id"].as_str().or_else(|| device["key"].as_str()) else { continue };
            let name = device["name"].as_str().unwrap_or(id);
            self.insert("devices", kind, id, name, select(device, &["online", "connected", "present", "name", "firmwareVersion", "version", "boot", "configurationGeneration"]));
        }
    }
    pub(crate) fn unavailable(&mut self, kind: &'static str) { self.1.insert(kind); }
    pub(super) fn preserve_unavailable(&mut self, before: &Self) { for (key,entity) in &before.0 { if self.1.contains(entity.kind) { self.0.entry(key.clone()).or_insert_with(||entity.clone()); } } }
}
pub(super) fn initial(snapshot: &Snapshot) -> Vec<NewActivityEvent> {
    let mut active=Snapshot::default();
    for (key,entity) in &snapshot.0 {if entity.kind=="discovery"&&entity.value["status"]!="idle" {active.0.insert(key.clone(),entity.clone());}}
    diff(&Snapshot::default(),&active)
}
fn select(value: &Value, keys: &[&str]) -> Value {
    let mut result = serde_json::Map::new();
    for key in keys { if let Some(value) = value.get(*key) {
        // Decorated artwork may contain a full data URL. Retain only a stable
        // content fingerprint rather than copying images between snapshots.
        let value=if matches!(*key,"background"|"defaultBackground") { use sha2::Digest; json!(format!("{:x}",sha2::Sha256::digest(value.to_string().as_bytes()))) } else { value.clone() };
        result.insert((*key).into(), value);
    } }
    Value::Object(result)
}
fn online(entity: &Entity) -> bool {
    ["online", "connected", "present"].iter().find_map(|key|entity.value[*key].as_bool()).unwrap_or(false)
}
pub(super) fn diff(before: &Snapshot, after: &Snapshot) -> Vec<NewActivityEvent> {
    let mut events = vec![];
    for (key, entity) in &after.0 {
        let old = before.0.get(key);
        if old.is_some_and(|old| old == entity) { continue; }
        let transition = match entity.kind {
            "profile_session" => {
                let state = entity.value["state"].as_str().unwrap_or("stopped");
                match state {
                    "running" if old.is_some_and(|old| old.value["state"] == "degraded") => "recovered",
                    "running" => "activated", "degraded" => "degraded", "faulted" | "recovery-needed" => "failed",
                    "stopped" => "stopped", "awaiting-game" => "awaiting_game", _ => "transitioning",
                }
            }
            "game_process" => if old.is_none() && !online(entity) { continue; } else if online(entity) { "launched" } else { "exited" },
            "discovery"=>match entity.value["status"].as_str().unwrap_or("idle") {
                "idle"=>continue,"failed"=>"failed","ready" if old.is_none_or(|old|old.value["status"]!="ready")=>"completed",
                "scanning" if old.is_none_or(|old|old.value["scanId"]!=entity.value["scanId"])=>"started",_=>"updated",
            },
            "game_vehicle"=>if old.is_none()&&entity.value["id"].is_null(){continue}else if entity.value["id"].is_null(){"cleared"}else{"changed"},
            "gateway" | "module" | "controller" | "companion" => {
                if old.is_none() { "discovered" } else if old.is_some_and(|old| online(old) != online(entity)) {
                    if online(entity) { "online" } else { "offline" }
                } else { "updated" }
            }
            "game" | "profile" => if old.is_none() { "created" } else if old.is_some_and(|old| old.name != entity.name) { "renamed" } else { "updated" },
            _ => "changed",
        };
        let level = if entity.kind=="integration"&&matches!(entity.value["state"].as_str(),Some("needs-attention"|"failed")){"warning"}else{match transition { "failed" => "error", "degraded" | "offline" => "warning", _ => "info" }};
        let id = if entity.kind == "profile_session" { entity.value["profileId"].as_str().unwrap_or(&entity.id) } else if entity.kind=="discovery" {entity.value["scanId"].as_str().unwrap_or(&entity.id)}else{ &entity.id };
        let mut captured = event(entity.category, &format!("{}_{}", entity.kind, transition), level, format!("{} {}.", entity.name, transition.replace('_', " ")), Some(id.into()), Some(entity.name.clone()));
        // Details contain changed field names and known state labels only.
        let fields = entity.value.as_object().map(|fields| fields.keys().filter(|key| old.is_none_or(|old| old.value[*key] != entity.value[*key])).cloned().collect::<Vec<_>>()).unwrap_or_default();
        captured.detail = if matches!(entity.kind,"profile_session"|"integration"|"game_session") { format!("State: {}", entity.value["state"].as_str().or_else(||entity.value["status"].as_str()).unwrap_or("unknown")) } else { format!("Changed: {}", fields.join(", ")) };
        if entity.kind=="profile_session" {
            let reasons=entity.value["degradation"].as_array().into_iter().flatten().filter_map(Value::as_str).chain(entity.value["fault"].as_str()).collect::<Vec<_>>();
            if !reasons.is_empty() {captured.detail=super::audit::safe_detail(&format!("{}; {}",captured.detail,reasons.join("; ")));}
        }
        if entity.kind=="integration" {if let Some(reason)=entity.value["message"].as_str(){captured.detail=super::audit::safe_detail(&format!("{}; {reason}",captured.detail));}}
        if entity.kind=="game_vehicle" {captured.detail=entity.value["name"].as_str().or_else(||entity.value["id"].as_str()).map(|name|format!("Current vehicle: {name}")).unwrap_or_else(||"Current vehicle cleared.".into());}
        if entity.kind=="discovery" {captured.detail=super::audit::safe_detail(&format!("{} proposed changes; {} artwork ready; {} tagged. {}",entity.value["proposed"].as_u64().unwrap_or(0),entity.value["artworkReady"].as_u64().unwrap_or(0),entity.value["tagged"].as_u64().unwrap_or(0),entity.value["diagnostics"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join("; ")));}
        events.push(captured);
    }
    for (key, entity) in &before.0 {
        if after.0.contains_key(key) { continue; }
        // Companion/device disappearance is an availability transition. A game
        // removal already covers its integration/process projections.
        if matches!(entity.kind, "integration" | "game_process" | "game_session"|"game_vehicle"|"discovery") { continue; }
        let transition = if matches!(entity.kind, "game" | "profile") { "removed" } else { "offline" };
        events.push(event(entity.category, &format!("{}_{}", entity.kind, transition), if transition=="offline"{"warning"}else{"info"}, format!("{} {transition}.", entity.name), Some(entity.id.clone()), Some(entity.name.clone())));
    }
    events
}
