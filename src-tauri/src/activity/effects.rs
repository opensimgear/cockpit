//! Safe terminal state projections for an admitted native mutation.
//!
//! These projections let the activity writer match observer facts to the
//! mutation that caused them. They deliberately contain only stable snapshot
//! subjects; request bodies and transient session fields never enter the map.
use super::capture::{self, Snapshot};
use super::context::Context;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

const REMOVED: &str = "removed";

pub(super) fn expected(
    request: &Value,
    response: &Value,
    context: Option<&Context>,
) -> BTreeMap<String, String> {
    if response["ok"] != true {
        let command = request["command"].as_str().unwrap_or("");
        if !matches!(command, "profile_apply" | "profile_stop" | "profile_recover" | "profile_game_launch") {
            return BTreeMap::new();
        }
        let Some(status) = profile_status(&response["state"]["profiles"])
            .filter(|status| matches!(status["state"].as_str(), Some("faulted" | "recovery-needed")))
        else {
            return BTreeMap::new();
        };
        let mut snapshot = Snapshot::default();
        snapshot.profile_session(status.clone(), &BTreeMap::new());
        let mut expected = BTreeMap::new();
        take_state(&snapshot, &mut expected, "profile_session:current");
        return expected;
    }

    let command = request["command"].as_str().unwrap_or("");
    let mut snapshot = Snapshot::default();
    let mut expected = BTreeMap::new();

    if is_profile_document(command) {
        let profiles = &response["state"]["profiles"];
        let id = profiles["document"]["id"]
            .as_str()
            .or_else(|| request["id"].as_str())
            .or_else(|| request["profileId"].as_str())
            .or_else(|| request["document"]["id"].as_str());
        if let Some(id) = id {
            let subject = format!("profile:{id}");
            if command == "profile_delete" {
                expected.insert(subject, REMOVED.into());
            } else if let Some(value) = profiles.get("activityChangeAfter")
                .or_else(|| response["state"].get("activityChangeAfter"))
                .or_else(|| profiles["activityChange"].get("after"))
            {
                snapshot.profile_value(id, value);
                take_state(&snapshot, &mut expected, &subject);
            } else if let Some(document) = profiles.get("document").filter(|value| value.is_object()) {
                snapshot.profile_value(id, &super::changes::profile_snapshot(document));
                take_state(&snapshot, &mut expected, &subject);
            }
        }
        return expected;
    }

    if matches!(command, "profile_apply" | "profile_stop" | "profile_recover" | "profile_game_launch") {
        if let Some(status) = profile_status(&response["state"]["profiles"]) {
            snapshot.profile_session(status.clone(), &BTreeMap::new());
            take_state(&snapshot, &mut expected, "profile_session:current");
        }
        if command == "profile_game_launch" {
            // A game launch owns only the target game's process fact. It must
            // not claim the game's metadata, integration, vehicle, or session.
            if let Some(game_id) = request["gameId"].as_str() {
                if let Some(game) = game(response, game_id) {
                    let mut process = Snapshot::default();
                    process.games(std::slice::from_ref(game));
                    take_state(&process, &mut expected, &format!("game_process:{game_id}"));
                }
            }
        }
        return expected;
    }

    if is_game_metadata(command) {
        if command == "game_remove" {
            if let Some(id) = request["gameId"].as_str().or_else(|| request["id"].as_str()) {
                expected.insert(format!("game:{id}"), REMOVED.into());
            }
        } else if command == "game_discovery_apply" {
            let catalogue = response["state"]["games"]["catalogue"].as_array().map(Vec::as_slice).unwrap_or(&[]);
            let targets = game_discovery_targets(request, response, catalogue);
            if !targets.is_empty() {
                snapshot.games(catalogue);
                for (id, removed) in targets {
                    let subject = format!("game:{id}");
                    if removed {
                        expected.insert(subject, REMOVED.into());
                    } else {
                        take_state(&snapshot, &mut expected, &subject);
                    }
                }
            }
            if let Some(discovery) = discovery_state(response) {
                snapshot.discovery(discovery);
                take_state(&snapshot, &mut expected, "discovery:current");
            }
        } else if let Some(catalogue) = response["state"]["games"]["catalogue"].as_array() {
            let targets = game_targets(command, request, response, catalogue);
            if !targets.is_empty() {
                snapshot.games(catalogue);
                for id in targets {
                    take_state(&snapshot, &mut expected, &format!("game:{id}"));
                }
            }
        }
        return expected;
    }

    if command == "game_discovery_start" {
        let discovery = discovery_state(response);
        if let Some(discovery) = discovery {
            snapshot.discovery(discovery);
            take_state(&snapshot, &mut expected, "discovery:current");
        }
        return expected;
    }

    if is_integration_mutation(command) {
        expected_integration(request, response, &mut snapshot, &mut expected);
        return expected;
    }

    if command.starts_with("controller_") {
        expected_controllers(request, response, &mut snapshot, &mut expected);
        return expected;
    }
    if command.starts_with("companion_") {
        expected_companions(request, response, &mut snapshot, &mut expected);
        return expected;
    }
    if command.starts_with("module_usb_") {
        expected_usb_modules(request, response, &mut snapshot, &mut expected);
        return expected;
    }
    if is_gateway_command(command) {
        expected_gateway(request, response, &mut snapshot, &mut expected);
        return expected;
    }

    // The metadata context is useful for a few mutations outside the entity
    // snapshot families (settings, rules, and preference changes). Do not turn
    // those into broad entity claims.
    let _ = context;
    expected
}

fn is_profile_document(command: &str) -> bool {
    matches!(command,
        "profile_create" | "profile_duplicate" | "profile_import" | "profile_save" |
        "profile_editor_save" | "profile_draft_save" | "profile_assignments_save" |
        "profile_rename" | "profile_delete")
}

fn is_game_metadata(command: &str) -> bool {
    matches!(command,
        "game_add" | "game_save" | "game_remove" | "game_installation_select" |
        "game_rescan" | "game_tags_save" | "game_background_save" | "game_discovery_apply" |
        "game_tag_definitions_save")
}

fn game_targets(
    command: &str,
    request: &Value,
    response: &Value,
    catalogue: &[Value],
) -> BTreeSet<String> {
    if command == "game_discovery_apply" {
        return game_discovery_targets(request, response, catalogue)
            .into_keys()
            .collect();
    }
    if command == "game_tag_definitions_save" {
        return catalogue.iter().filter_map(|game| game["id"].as_str().map(str::to_owned)).collect();
    }
    let id = request["gameId"].as_str()
        .or_else(|| request["id"].as_str())
        .or_else(|| response["state"]["games"]["addedGameId"].as_str());
    id.map(|id| BTreeSet::from([id.to_owned()])).unwrap_or_default()
}

fn game_discovery_targets(
    request: &Value,
    response: &Value,
    catalogue: &[Value],
) -> BTreeMap<String, bool> {
    let entries = response["state"]["games"]["discovery"]["entries"].as_array();
    let actions = request["actions"].as_array();
    let (Some(entries), Some(actions)) = (entries, actions) else { return BTreeMap::new() };
    let mut targets = BTreeMap::new();
    for entry in entries {
        let Some(entry_id) = entry["id"].as_str() else { continue };
        let action = actions.iter().find(|action| action["id"].as_str() == Some(entry_id));
        let selected = action.and_then(|action| action["selected"].as_bool()).unwrap_or(entry["selected"] == true);
        if !selected { continue; }
        let Some(game_id) = entry["gameId"].as_str() else { continue };
        // Remove entries cannot be redirected by a gameId override in apply.
        let target = if entry["action"] == "remove" {
            game_id
        } else {
            action.and_then(|action| action["gameId"].as_str()).unwrap_or(game_id)
        };
        if catalogue.iter().any(|game| game["id"].as_str() == Some(target)) {
            targets.insert(target.to_owned(), false);
        } else if entry["action"] == "remove" {
            // A removal is proven only by the selected remove entry and its
            // acknowledged absence from the returned catalogue.
            targets.insert(target.to_owned(), true);
        }
    }
    targets
}

fn is_integration_mutation(command: &str) -> bool {
    matches!(command,
        "game_integration_settings_save" | "game_integration_setup" |
        "game_integration_remove" | "game_integration_recover" | "game_integration_cancel" |
        "integration_install" | "integration_remove" | "integration_start" |
        "integration_stop" | "integration_verify" |
        "dcs_install" | "dcs_remove" | "dcs_start" | "dcs_stop")
}

fn expected_integration(
    request: &Value,
    response: &Value,
    snapshot: &mut Snapshot,
    output: &mut BTreeMap<String, String>,
) {
    let state = &response["state"];
    let Some(id) = request["gameId"].as_str()
        .or_else(|| request["id"].as_str())
        .or_else(|| state["gameId"].as_str())
    else { return };

    if let Some(catalogue) = state["games"]["catalogue"].as_array() {
        if catalogue.iter().any(|game| game["id"].as_str() == Some(id)) {
            snapshot.games(catalogue);
            take_state(snapshot, output, &format!("integration:{id}"));
            return;
        }
    }

    let setup = state["integrationSetup"].as_object().map(|_| &state["integrationSetup"])
        .or_else(|| state["games"]["integrationSetup"].as_object().map(|_| &state["games"]["integrationSetup"]));
    if let Some(setup) = setup {
        let game_name = state["gameName"].as_str().unwrap_or(id);
        snapshot.games(&[json!({"id":id,"name":game_name,"integrationSetup":setup})]);
        take_state(snapshot, output, &format!("integration:{id}"));
    }
}

fn game<'a>(response: &'a Value, id: &str) -> Option<&'a Value> {
    response["state"]["games"]["catalogue"].as_array()?.iter().find(|row| row["id"] == id)
}

fn discovery_state(response: &Value) -> Option<&Value> {
    response["state"]["games"].get("discovery")
        .or_else(|| response["state"].get("discovery"))
        .filter(|value| value.is_object())
}

fn profile_status(profiles: &Value) -> Option<&Value> {
    profiles.get("profileSession")
        .or_else(|| profiles.get("session"))
        .filter(|status| status.is_object() && status["state"].is_string())
}

fn take_state(snapshot: &Snapshot, output: &mut BTreeMap<String, String>, subject: &str) {
    let states = capture::states(snapshot);
    if let Some(value) = states.get(subject) {
        output.insert(subject.to_owned(), value.clone());
    }
}

fn expected_controllers(
    request: &Value,
    response: &Value,
    snapshot: &mut Snapshot,
    output: &mut BTreeMap<String, String>,
) {
    let command = request["command"].as_str().unwrap_or("");
    let mut ids = BTreeSet::new();
    if let Some(id) = request["sourceId"].as_str() { ids.insert(id.to_owned()); }
    if let Some(rows) = request["sourceIds"].as_array() {
        ids.extend(rows.iter().filter_map(Value::as_str).map(str::to_owned));
    }
    if command == "controller_visibility_restore" {
        if let Some(job) = response["state"]["controllers"]["visibility"]["job"]["sourceIds"].as_array() {
            ids.extend(job.iter().filter_map(Value::as_str).map(str::to_owned));
        }
    }
    if ids.is_empty() {
        return;
    }
    let rows = controller_rows(&response["state"]["controllers"]);
    for row in rows {
        let Some(id) = row["sourceId"].as_str().or_else(|| row["id"].as_str()) else { continue };
        if !ids.contains(id) { continue; }
        let mut normalized = row.clone();
        normalized["id"] = json!(id);
        snapshot.devices("controller", std::slice::from_ref(&normalized));
        take_state(snapshot, output, &format!("controller:{id}"));
    }
    if command == "controller_forget" {
        if let Some(id) = request["sourceId"].as_str() {
            output.insert(format!("controller:{id}"), REMOVED.into());
        }
    }
}

fn controller_rows(state: &Value) -> Vec<Value> {
    let mut rows = vec![];
    for key in ["sources", "controllers"] {
        if let Some(values) = state[key].as_array() { rows.extend(values.iter().cloned()); }
    }
    for key in ["source", "controller"] {
        if state[key].is_object() { rows.push(state[key].clone()); }
    }
    if state["sourceId"].is_string() || state["id"].is_string() { rows.push(state.clone()); }
    rows
}

fn expected_companions(
    request: &Value,
    response: &Value,
    snapshot: &mut Snapshot,
    output: &mut BTreeMap<String, String>,
) {
    let command = request["command"].as_str().unwrap_or("");
    let payload = request.get("companion").unwrap_or(request);
    let mut ids = BTreeSet::new();
    for key in ["companionId", "id"] {
        if let Some(id) = payload[key].as_str() { ids.insert(id.to_owned()); }
    }
    if let Some(instance_id) = payload["instanceId"].as_str() {
        let host_instances = &response["state"]["companions"]["host"]["instances"];
        if let Some(companion_id) = host_instances.as_array().into_iter().flatten()
            .find(|instance| instance["instanceId"] == instance_id)
            .and_then(|instance| instance["companionId"].as_str())
        {
            ids.insert(companion_id.to_owned());
        }
    }
    let mut rows = vec![];
    let companions = &response["state"]["companions"];
    if command.starts_with("companion_usb_") {
        if let Some(id) = companions["usb"]["device"]["companionId"].as_str() {ids.insert(id.to_owned());}
    }
    for path in ["host.clients", "clients", "companions"] {
        let value = path.split('.').fold(companions, |current, key| &current[key]);
        if let Some(values) = value.as_array() { rows.extend(values.iter().cloned()); }
    }
    for row in rows {
        let Some(id) = row["companionId"].as_str().or_else(|| row["id"].as_str()) else { continue };
        if !ids.contains(id) { continue; }
        let name = row["name"].as_str().unwrap_or(id);
        let instance_id = companion_instance_id(companions, id);
        snapshot.insert("companions", "companion", id, name, json!({
            "connected": row["connected"],
            "instanceId": instance_id,
            "name": name
        }));
        take_state(snapshot, output, &format!("companion:{id}"));
    }
    if command == "companion_usb_provision" {
        if let Some(id) = companions["usb"]["device"]["companionId"].as_str() {
            let name = payload["name"].as_str().unwrap_or(id);
            let instance_id = companion_instance_id(companions, id);
            let connected = companions["host"]["clients"].as_array().into_iter().flatten()
                .find(|row| row["companionId"] == id)
                .map(|row| row["connected"].clone()).unwrap_or(json!(true));
            snapshot.insert("companions", "companion", id, name, json!({"connected":connected,"instanceId":instance_id,"name":name}));
            take_state(snapshot, output, &format!("companion:{id}"));
        }
    }
    if matches!(command, "companion_revoke" | "companion_forget") {
        if let Some(id) = payload["companionId"].as_str() {
            output.insert(format!("companion:{id}"), REMOVED.into());
        }
    }
}

fn companion_instance_id(companions: &Value, companion_id: &str) -> String {
    let mut instances = companions["host"]["instances"].as_array().into_iter().flatten()
        .filter(|instance| instance["companionId"] == companion_id)
        .filter_map(|instance| instance["instanceId"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    instances.sort();
    instances.into_iter().next()
        .unwrap_or_else(|| osg_companion_service::profile::profile_instance_id(companion_id))
}

fn expected_usb_modules(
    request: &Value,
    response: &Value,
    snapshot: &mut Snapshot,
    output: &mut BTreeMap<String, String>,
) {
    let key = request["key"].as_str();
    let module_usb = &response["state"]["moduleUsb"];
    let module = module_usb.get("module").filter(|value| value.is_object())
        .or_else(|| module_usb.get("key").filter(|_| module_usb.is_object()))
        .map(|_| module_usb.get("module").unwrap_or(module_usb))
        .or_else(|| module_usb.get("snapshot").filter(|value| value.is_object()));
    let Some(module) = module else { return };
    if let (Some(requested),Some(returned))=(key,module["key"].as_str()) {if requested!=returned{return;}}
    let Some(id) = key.or_else(|| module["key"].as_str()) else { return };
    let snap = module.get("snapshot").unwrap_or(module);
    let Some(present) = module["present"].as_bool().or_else(|| snap["present"].as_bool()) else { return };
    snapshot.insert("devices", "module", id, id, json!({
        "connected": snap["connected"],
        "present": present,
        "identity": snap["identity"],
        "route": snap["route"]
    }));
    take_state(snapshot, output, &format!("module:{id}"));
}

fn is_gateway_command(command: &str) -> bool {
    matches!(command,
        "connect" | "disconnect" | "reconnect" | "start" | "stop" | "apply" | "save" |
        "update-profile" | "claim" | "release" | "forget" | "register" | "unregister" |
        "enable" | "disable" | "configure" | "config_apply" | "config_apply_save" |
        "config_save" | "config_discard" | "config_reset" | "firmware_flash" | "flash")
}

fn expected_gateway(
    request: &Value,
    response: &Value,
    snapshot: &mut Snapshot,
    output: &mut BTreeMap<String, String>,
) {
    let state = &response["state"];
    let gateway_id = gateway_id(request, response);
    if let Some(module_id) = request["moduleId"].as_str() {
        if let Some(gateway_id) = gateway_id.as_deref() {
            let id = format!("{gateway_id}:{module_id}");
            if let Some(module) = state["modules"].as_array().into_iter().flatten().find(|row| {
                row["id"].as_str().or_else(|| row["moduleId"].as_str()) == Some(module_id)
            }) {
                let mut row = module.clone();
                row["id"] = json!(id);
                snapshot.devices("module", std::slice::from_ref(&row));
                take_state(snapshot, output, &format!("module:{id}"));
            } else if matches!(request["command"].as_str(),Some("forget"|"unregister"))&&state["modules"].is_array() {
                output.insert(format!("module:{id}"),REMOVED.into());
            }
        }
        return;
    }
    let Some(id) = gateway_id.as_deref() else { return };
    let present = state["present"].as_bool().or_else(|| {
        state["ports"].as_array().map(|ports| ports.iter().any(|port| port["gatewayId"] == id))
    });
    let Some(present) = present else { return };
    snapshot.insert("devices", "gateway", id, id, json!({
        "present": present,
        "connected": state["connected"],
        "running": state["running"]
    }));
    take_state(snapshot, output, &format!("gateway:{id}"));
}

fn gateway_id(request: &Value, response: &Value) -> Option<String> {
    let state = &response["state"];
    request["gatewayId"].as_str()
        .or_else(|| state["gatewayId"].as_str())
        .or_else(|| state["gateway"]["gatewayId"].as_str())
        .or_else(|| state["gateway"]["id"].as_str())
        .or_else(|| state["device"]["gatewayId"].as_str())
        .or_else(|| state["devices"]["gatewayId"].as_str())
        .or_else(|| response["gatewayId"].as_str())
        .map(str::to_owned)
        .or_else(|| {
            let backend = state["backend"].as_str()?;
            let serial = state["gatewaySerial"].as_str()?;
            Some(format!("{backend}:{serial}"))
        })
}

#[cfg(test)]
#[path = "effects_tests.rs"]
mod tests;
