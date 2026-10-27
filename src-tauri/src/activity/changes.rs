//! Human-readable changes from explicitly permitted snapshot fields.
//! Unknown fields expose only their label, never their contents.
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const LIMIT: usize = 64;

/// Retains only approved display values. Script bodies and opaque settings are
/// represented by digests, so snapshots can detect edits without storing them.
pub(crate) fn profile_snapshot(document: &Value) -> Value {
    projection("profile", document)
}

/// Raw catalogue entries and UI decoration share only these stable fields.
pub(crate) fn game_snapshot(game: &Value) -> Value {
    let mut result = serde_json::Map::new();
    for key in ["id","name","executable","installPath","installed","selectedInstallationId","tagIds","integrationKey","provider","integrationCompatible"] {
        result.insert(key.into(),game[key].clone());
    }
    for key in ["background","defaultBackground"] { result.insert(key.into(),artwork_fingerprint(&game[key])); }
    result.insert("metadata".into(),fingerprint(&game["metadata"]));
    let installations = game["installations"].as_array().into_iter().flatten().map(|installation| {
        let mut fields=serde_json::Map::new();
        for key in ["id","root","installPath","executable","available","source"] { fields.insert(key.into(),installation[key].clone()); }
        Value::Object(fields)
    }).collect();
    result.insert("installations".into(),Value::Array(installations));
    if let Some(names)=game["tagNames"].as_object() {
        result.insert("tagNames".into(),Value::Object(names.iter().filter(|(_,name)|name.is_string()).map(|(id,name)|(id.clone(),name.clone())).collect()));
    }
    Value::Object(result)
}

fn artwork_fingerprint(background: &Value) -> Value {
    use base64::Engine;
    use sha2::{Digest,Sha256};
    let Some(image)=background["image"].as_str() else { return fingerprint(background); };
    let hash=if let Some(reference)=image.strip_prefix("artwork:").and_then(|value|value.strip_suffix(".jpg")) {
        reference.to_owned()
    } else if let Some(encoded)=image.strip_prefix("data:image/jpeg;base64,") {
        base64::engine::general_purpose::STANDARD.decode(encoded).map(|bytes|format!("{:x}",Sha256::digest(bytes))).unwrap_or_else(|_|image.to_owned())
    } else { image.to_owned() };
    // Keep crop edits meaningful while equating a managed asset and its data URL.
    let normalized=serde_json::json!({"image":hash,"positionX":background["positionX"],"positionY":background["positionY"],"crop":background["crop"]});
    fingerprint(&normalized)
}

fn fingerprint(value: &Value) -> Value {
    use sha2::{Digest, Sha256};
    Value::String(format!("{:x}", Sha256::digest(value.to_string().as_bytes())))
}

fn projection(context: &str, value: &Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(fields.iter().filter_map(|(key, value)| {
            if secret(key) { return None; }
            let value = if marker_only(key) || (!permitted(context, key) && key != "id") {
                fingerprint(value)
            } else { projection(key, value) };
            Some((key.clone(), value))
        }).collect()),
        Value::Array(values) => Value::Array(values.iter().map(|value| projection(context, value)).collect()),
        _ => value.clone(),
    }
}

pub(super) fn describe(kind: &str, before: Option<&Value>, after: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    object(kind, "", before.unwrap_or(&Value::Null), after, &mut lines);
    if lines.len() > LIMIT {
        let omitted = lines.len() - (LIMIT - 1);
        lines.truncate(LIMIT - 1);
        lines.push(format!("{omitted} further changes."));
    }
    lines
}

fn secret(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    ["password", "secret", "token", "credential", "authorization", "privatekey", "api_key", "apikey", "requestbody"]
        .iter().any(|part| key.contains(part))
}

fn label(key: &str) -> String {
    match key {
        "tagIds" => return "Tags".into(),
        "gameIds" => return "Games".into(),
        "updateHz" => return "Update rate (Hz)".into(),
        "deadZone" | "deadzonePercent" => return "Deadzone".into(),
        "centerPercent" => return "Center (%)".into(),
        "freshnessMs" => return "Freshness (ms)".into(),
        "sourceId" => return "Source".into(),
        "profilePortId" => return "Logical port".into(),
        "observedPortId" => return "Observed port".into(),
        "closeToTray" => return "Close to tray".into(),
        "minimizeToTray" => return "Minimize to tray".into(),
        "delivery" => return "Notification delivery".into(),
        "position" => return "Notification position".into(),
        _ => (),
    }
    let mut result = String::new();
    for c in key.chars() {
        if c == '_' || c == '-' { result.push(' '); }
        else {
            if c.is_uppercase() && !result.is_empty() { result.push(' '); }
            if result.is_empty() { result.extend(c.to_uppercase()); }
            else { result.extend(c.to_lowercase()); }
        }
    }
    result
}

fn path(prefix: &str, key: &str) -> String {
    if prefix.is_empty() { label(key) } else { format!("{prefix} / {}", label(key)) }
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some("not set".into()),
        Value::String(text) => Some(format!("“{text}”")),
        Value::Bool(value) => Some(if *value { "yes" } else { "no" }.into()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn changed(title: &str, before: &Value, after: &Value, values: bool, out: &mut Vec<String>) {
    if before == after { return; }
    match (values.then(|| scalar(before)).flatten(), values.then(|| scalar(after)).flatten()) {
        (Some(old), Some(new)) => out.push(format!("{title}: {old} → {new}")),
        _ => out.push(format!("{title} changed.")),
    }
}

fn permitted(context: &str, key: &str) -> bool {
    let fields = match context {
        "profile" => "name revision eligibility resources processingNodes bindings virtualOutputs virtualDevices desktopOutputs visibility overrides companionAssignments vehicleTabs editor assignments localAssignments presentation panels",
        "game" => "name executable installPath installed selectedInstallationId tagIds integrationKey provider integrationCompatible installations background defaultBackground metadata",
        "device" | "gateway" | "module" | "controller" | "companion" => "name online connected present firmwareVersion version boot configurationGeneration state status freshness freshnessMs reason stateReason unavailableReason disconnectReason lastError host port updateHz",
        "integration" => "state status installed verified version message reason host port updateHz connection setup providerSetup",
        "game_session" => "status state contextSession providerSetupRevision catalogueRevision reason message",
        "discovery" => "status proposed libraries artworkReady tagged diagnostics",
        "configuration" => "name enabled selectedProfileId profileId autoStart host port updateHz connection setup providerSetup assignments mode scope userFolder aircraftModule deadzone deadZone deadzonePercent centerPercent invert scale factor offset min max priority rule readOnly closeToTray minimizeToTray delivery position",
        "connection" | "setup" | "providerSetup" => "host port updateHz",
        "resources" | "processingNodes" | "virtualOutputs" | "virtualDevices" | "desktopOutputs" => "id name kind required ports inputs outputs operation layout scope actions draft",
        "bindings" => "id from to",
        "from" | "to" => "nodeId portId",
        "operation" | "rule" => "kind factor offset min max inputMin inputMax outputMin outputMax rounding from to clamp centerPercent deadzonePercent points comparison threshold press release direction diagonals order edge decimals prefix suffix overflow value whenTrue whenFalse unknown stale unavailable arguments rule inputs source parameters deadZone invert scale curve positive range pressed released gesture holdUs doubleUs pulseUs durationUs repeatUs steps",
        "ports" | "inputs" | "outputs" => "id name direction contract profilePortId observedPortId expectedContractHash",
        "contract" => "delivery schema freshnessMs fallback parameters semantics endpoint readable display ownership",
        "schema" => "type min max unit maxBytes encoding values",
        "layout" => "buttons axes hats",
        "axes" => "portId range fallback",
        "scope" => "kind applicationReference",
        "eligibility" => "gameIds",
        "visibility" => "resourceId mode",
        "overrides" => "resourceId enabled",
        "assignments" | "localAssignments" | "companionAssignments" => "resourceId sourceId deviceId companionId panelId publicationHash enabled name ports",
        "panels" | "presentation" | "vehicleTabs" => "id name panelId resourceId vehicleId tabs panels widgets elements required enabled width height",
        "installations" => "id root installPath executable available source",
        "points" | "curve" => "input output",
        "steps" => "afterUs pressed",
        "actions" => "portId action",
        "action" => "kind key keys button modifiers direction amount x y durationMs",
        "draft" => "preset configured",
        _ => "",
    };
    fields.split_whitespace().any(|field| field == key)
}

fn marker_only(key: &str) -> bool {
    matches!(key, "source" | "parameters" | "background" | "defaultBackground" | "metadata" | "editor" | "publicationHash")
}

fn object(context: &str, prefix: &str, before: &Value, after: &Value, out: &mut Vec<String>) {
    let keys: BTreeSet<&str> = before.as_object().into_iter().flat_map(|map| map.keys().map(String::as_str))
        .chain(after.as_object().into_iter().flat_map(|map| map.keys().map(String::as_str))).collect();
    for key in keys {
        if secret(key) || matches!(key,"id" | "schemaVersion" | "revision" | "tagNames" | "tags" | "scanId") { continue; }
        let old = &before[key];
        let new = &after[key];
        if old == new { continue; }
        let title = path(prefix, key);
        if !permitted(context, key) || marker_only(key) {
            if key == "source" && context == "operation" { out.push(format!("{prefix} / Script content updated.")); }
            else { changed(&title, old, new, false, out); }
        } else if matches!(key, "tagIds" | "gameIds") {
            set_changes(&title, old, new, after, out);
        } else if key == "bindings" {
            bindings(old, new, out);
        } else if old.is_array() || new.is_array() {
            array(key, &title, old, new, out);
        } else if old.is_object() || new.is_object() {
            object(key, &title, old, new, out);
        } else {
            changed(&title, old, new, true, out);
        }
    }
}

fn strings(value: &Value) -> BTreeSet<&str> {
    value.as_array().into_iter().flatten().filter_map(Value::as_str).collect()
}

fn set_changes(title: &str, before: &Value, after: &Value, owner: &Value, out: &mut Vec<String>) {
    let old = strings(before);
    let new = strings(after);
    for (verb, values) in [("added", new.difference(&old).copied().collect::<Vec<_>>()), ("removed", old.difference(&new).copied().collect())] {
        if values.is_empty() { continue; }
        let names = values.iter().map(|id| {
            owner["tagNames"][*id].as_str().map(str::to_owned)
                .or_else(|| owner["tags"].as_array().into_iter().flatten().find(|tag| tag["id"].as_str() == Some(*id)).and_then(|tag| tag["name"].as_str()).map(str::to_owned))
                .unwrap_or_else(|| label(id))
        }).collect::<Vec<_>>();
        out.push(format!("{title} {verb}: {}", names.join(", ")));
    }
}

fn identity(value: &Value) -> Option<&str> {
    ["id", "resourceId", "portId", "profilePortId", "panelId", "vehicleId"].iter().find_map(|key| value[*key].as_str())
}

fn items(value: &Value) -> BTreeMap<String, &Value> {
    value.as_array().into_iter().flatten().enumerate().map(|(index, value)|
        (identity(value).map(str::to_owned).unwrap_or_else(|| format!("#{}", index + 1)), value)).collect()
}

fn array(context: &str, title: &str, before: &Value, after: &Value, out: &mut Vec<String>) {
    let old = items(before);
    let new = items(after);
    let keys: BTreeSet<_> = old.keys().chain(new.keys()).collect();
    for id in keys {
        let a = old.get(id).copied().unwrap_or(&Value::Null);
        let b = new.get(id).copied().unwrap_or(&Value::Null);
        if a == b { continue; }
        let item = if b.is_null() { a } else { b };
        let name = item["name"].as_str().or_else(|| identity(item)).unwrap_or(id);
        let item_title = format!("{title} “{name}”");
        if a.is_null() || b.is_null() {
            let verb = if a.is_null() { "added" } else { "removed" };
            if item.is_object() {
                out.push(format!("{item_title} {verb}."));
                // New operation/required settings explain what was created.
                if a.is_null() { object(context, &item_title, a, b, out); }
            } else if let Some(value) = scalar(item) {
                out.push(format!("{title}: {value} {verb}."));
            }
        } else if item.is_object() {
            object(context, &item_title, a, b, out);
        } else { changed(&item_title, a, b, true, out); }
    }
}

fn route(value: &Value) -> String {
    let endpoint = |key| {
        let node = value[key]["nodeId"].as_str().unwrap_or("not set");
        let port = value[key]["portId"].as_str().unwrap_or("not set");
        format!("{node} / {port}")
    };
    format!("{} → {}", endpoint("from"), endpoint("to"))
}

fn bindings(before: &Value, after: &Value, out: &mut Vec<String>) {
    let old = items(before);
    let new = items(after);
    for (id, value) in &old {
        if !new.contains_key(id) { out.push(format!("Connection “{}” removed.", route(value))); }
    }
    for (id, value) in &new {
        match old.get(id) {
            None => out.push(format!("Connection “{}” added.", route(value))),
            Some(previous) if *previous != *value => out.push(format!("Connection: “{}” → “{}”", route(previous), route(value))),
            _ => (),
        }
    }
}

#[cfg(test)]
#[path = "changes_tests.rs"]
mod tests;
