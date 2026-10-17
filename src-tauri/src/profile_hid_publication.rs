//! Current native publication facts. No display name, authored flag or HID
//! absence can establish that a physical publisher is disabled.
use crate::module_usb_fleet::SlotSnapshot;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
fn fresh(value: Option<u64>, now: u64) -> bool {
    value
        .and_then(|at| now.checked_sub(at))
        .is_some_and(|age| age <= 3000)
}
pub(crate) fn can_status(gateway: &str, state: &Value, module: &Value) -> &'static str {
    let uid = state["gatewaySerial"].as_str().unwrap_or("");
    if uid.len() != 24
        || !uid.bytes().all(|b| b.is_ascii_hexdigit())
        || gateway != format!("stm32:{uid}")
        || state["connected"] != true
        || state["canProtocol"] != 3
        || state["layoutPending"] != false
        || !fresh(state["nativeSnapshotAtMs"].as_u64(), now_ms())
        || module["online"] != true
        || module["boot"].as_u64().is_none_or(|n| n == 0)
        || module["configurationGeneration"]
            .as_u64()
            .is_none_or(|n| n == 0)
        || module["directUsb"] != false
    {
        return "unknown";
    }
    match (
        module["published"].as_bool(),
        state["publication"]["phase"].as_str(),
    ) {
        (Some(true), Some("idle" | "owned")) => "published",
        (Some(false), Some("idle")) if state["publication"]["outcome"] == "no_change" => "disabled",
        (Some(false), Some("owned"))
            if state["publication"]["outcome"] == "ok"
                && state["publication"]["expectedReconnect"] == false =>
        {
            "lease-disabled"
        }
        _ => "unknown",
    }
}
pub(crate) fn usb_status(slot: &SlotSnapshot) -> &'static str {
    let snapshot = &slot.snapshot;
    let Some(route) = &snapshot.route else {
        return "unknown";
    };
    let Some(identity) = &snapshot.identity else {
        return "unknown";
    };
    let Some(config) = &snapshot.config else {
        return "unknown";
    };
    if !slot.present
        || !slot.admissible
        || !snapshot.connected
        || snapshot.error.is_some()
        || identity.module_id != route.module_id
        || route.boot == 0
        || config["view"] != "active"
        || config["valid"] != true
        || config["pendingApply"] != false
        || config["generation"] != route.configuration_generation
        || !fresh(snapshot.observed_at_ms, now_ms())
    {
        return "unknown";
    }
    match config["layout"]["reportBytes"].as_u64() {
        Some(0) => "disabled",
        Some(_) => "published",
        None => "unknown",
    }
}

#[cfg(test)]
#[path = "profile_hid_publication_tests.rs"]
pub(crate) mod tests;
