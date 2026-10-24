//! Current operation receipts; old JSON envelopes are never imported.
use super::super::*;
use std::path::Component;

pub(super) fn validate(value: &Value) -> Result<(), String> {
    let fields = [
        "id",
        "target",
        "createdScripts",
        "installModule",
        "files",
        "exportBefore",
        "exportAfter",
        "version",
        "archiveSha256",
        "aircraftBefore",
        "aircraftAfter",
        "createdAircraftDir",
        "physicsBefore",
        "physicsAfter",
        "createdPhysicsDir",
    ];
    let object = value.as_object().ok_or("Invalid DCS setup receipt.")?;
    if object.len() != fields.len() || fields.iter().any(|key| !object.contains_key(*key)) {
        return Err("Invalid DCS setup receipt fields. Start from a fresh database.".into());
    }
    let id = value["id"].as_str().ok_or("Missing DCS setup identity.")?;
    if !uuid::Uuid::parse_str(id).is_ok_and(|parsed| parsed.to_string() == id) {
        return Err("Invalid DCS setup identity.".into());
    }
    let target = value["target"]
        .as_str()
        .ok_or("Missing DCS setup target.")?;
    if target.len() > 4096
        || target.chars().any(char::is_control)
        || !Path::new(target).is_absolute()
        || Path::new(target)
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("Invalid DCS setup target.".into());
    }
    for key in [
        "createdScripts",
        "installModule",
        "createdAircraftDir",
        "createdPhysicsDir",
    ] {
        if !value[key].is_boolean() {
            return Err("Invalid DCS setup ownership flag.".into());
        }
    }
    if value["version"] != VERSION || value["archiveSha256"] != ARCHIVE_SHA256 {
        return Err("DCS setup receipt does not match this build's approved release.".into());
    }
    for key in [
        "exportBefore",
        "exportAfter",
        "aircraftBefore",
        "aircraftAfter",
        "physicsBefore",
        "physicsAfter",
    ] {
        if !value[key].is_null() && !value[key].as_str().is_some_and(digest) {
            return Err("Invalid DCS setup recovery digest.".into());
        }
    }
    if value["exportAfter"].is_null() || value["physicsAfter"].is_null() {
        return Err("Missing DCS setup output fingerprint.".into());
    }
    let files = value["files"]
        .as_object()
        .filter(|files| files.len() <= 4096)
        .ok_or("Invalid DCS setup file manifest.")?;
    for (name, expected) in files {
        safe_relative(name)?;
        if !expected.as_str().is_some_and(digest) {
            return Err("Invalid DCS setup file digest.".into());
        }
    }
    if serde_json::to_vec(value)
        .map_err(|_| "Cannot encode DCS setup receipt.")?
        .len()
        > 2 * 1024 * 1024
    {
        return Err("DCS setup receipt exceeds its byte budget.".into());
    }
    Ok(())
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
