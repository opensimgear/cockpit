//! Current native publication facts. No display name, authored flag or HID
//! absence can establish that a physical publisher is disabled.
use crate::module_usb_fleet::SlotSnapshot;
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const FRESH_MS: u64 = 3000;

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
        .is_some_and(|age| age <= FRESH_MS)
}
pub(crate) fn freshness_delay(value: Option<u64>, now: u64) -> Option<Duration> {
    let remaining = value?.checked_add(FRESH_MS + 1)?.checked_sub(now)?;
    (remaining > 0).then(|| Duration::from_millis(remaining))
}

/// A sequenced native CAN stream can prove that its cached snapshot is still
/// current without relying on the snapshot's wall-clock timestamp. Legacy
/// point-in-time snapshots keep the timestamp-based freshness rule.
pub(crate) fn can_freshness_delay(
    inventory_current: Option<bool>,
    snapshot_at: Option<u64>,
    now: u64,
) -> Option<Duration> {
    if inventory_current.is_some() {
        None
    } else {
        freshness_delay(snapshot_at, now)
    }
}

fn can_snapshot_fresh(state: &Value, now: u64) -> bool {
    match state.get("nativeInventoryCurrent") {
        Some(Value::Bool(current)) => *current,
        Some(_) => false,
        None => fresh(state["nativeSnapshotAtMs"].as_u64(), now),
    }
}

pub(crate) fn freshness_restored(before: Option<u64>, after: Option<u64>, now: u64) -> bool {
    before != after && !fresh(before, now) && fresh(after, now)
}
pub(crate) fn can_status(gateway: &str, state: &Value, module: &Value) -> &'static str {
    let uid = state["gatewaySerial"].as_str().unwrap_or("");
    if uid.len() != 24
        || !uid.bytes().all(|b| b.is_ascii_hexdigit())
        || gateway != format!("stm32:{uid}")
        || state["connected"] != true
        || state["canProtocol"] != 3
        || state["layoutPending"] != false
        || !can_snapshot_fresh(state, now_ms())
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
