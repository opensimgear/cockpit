//! Trusted native game-coordinator assignment snapshots and conflict-aware restoration.
//! These helpers never open a session, arm a source, or submit controller input.
use crate::game_mapping::companions::{AssignmentState, Capture, Snapshot};
use osg_companion_service::Hub;
use serde_json::{json, Value};
use std::collections::BTreeSet;
fn value<T: serde::Serialize>(data: &T) -> Result<Value, String> {
    serde_json::to_value(data).map_err(|e| e.to_string())
}
fn ids(capture: &Capture) -> Vec<String> {
    capture
        .assignments
        .iter()
        .map(|a| a.instance_id.clone())
        .collect()
}
/// Local read-only capture. Unknown or missing selected assignments are errors.
pub(super) fn capture(hub: &Hub, payload: &Value) -> Result<Value, String> {
    let selected = if let Some(selected) = payload.get("instanceIds") {
        let values = selected
            .as_array()
            .filter(|v| !v.is_empty() && v.len() <= 8)
            .ok_or("Select one to eight assigned companion instances.")?;
        let mut seen = BTreeSet::new();
        let mut ids = Vec::new();
        for v in values {
            let id = v
                .as_str()
                .filter(|id| osg_mapping::profile::identifier(id))
                .ok_or("Invalid companion instance identity.")?;
            if !seen.insert(id) {
                return Err("Select each companion instance once.".into());
            }
            ids.push(id.to_string());
        }
        ids
    } else {
        let status = hub.command(&json!({"command":"companion_status"}))?;
        let instances = status["instances"]
            .as_array()
            .ok_or("Refresh companion assignments before capturing.")?;
        if instances.is_empty() || instances.len() > 8 {
            return Err(
                "Select one to eight assigned companion instance IDs before capturing.".into(),
            );
        }
        instances
            .iter()
            .map(|a| {
                a["instanceId"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or("Invalid retained companion identity.".into())
            })
            .collect::<Result<Vec<_>, String>>()?
    };
    let captured = hub.assignment_snapshot(&selected)?;
    value(&Capture::parse(&captured).map_err(|_| {
        "Every captured companion needs a current paired installation and published revision."
    })?)
}
pub(super) fn snapshot(hub: &Hub, captured: &Value) -> Result<Value, String> {
    let desired = Capture::parse(captured)?;
    let desired_value = value(&desired)?;
    hub.preflight_assignments(&desired_value)?;
    let baseline = AssignmentState::parse(&hub.assignment_snapshot(&ids(&desired))?)?;
    let snapshot = Snapshot {
        owner: uuid::Uuid::new_v4().to_string(),
        desired,
        baseline,
        taken: false,
        restored: false,
    };
    let snapshot = value(&snapshot)?;
    Snapshot::parse(&snapshot)?;
    Ok(snapshot)
}
pub(super) fn take(hub: &Hub, journal: &Value) -> Result<Value, String> {
    let snapshot = Snapshot::parse(journal)?;
    if !snapshot.taken || snapshot.restored {
        return Err("Checkpoint companion assignment ownership before applying it.".into());
    }
    let desired = value(&snapshot.desired)?;
    let baseline = value(&snapshot.baseline)?;
    hub.preflight_assignments(&desired)?;
    let current = hub.assignment_snapshot(&ids(&snapshot.desired))?;
    if same(&current, &desired)? {
        return Ok(current);
    }
    if !same(&current, &baseline)? {
        return Err(
            "Companion assignments changed after game preflight; preserve the newer setup.".into(),
        );
    }
    hub.compare_assignments(&baseline, &desired)
}
pub(super) fn restore(hub: &Hub, journal: &Value, _recovery: bool) -> Result<Value, String> {
    let snapshot = Snapshot::parse(journal)?;
    let baseline = value(&snapshot.baseline)?;
    let current = hub.assignment_snapshot(&ids(&snapshot.desired))?;
    if same(&current, &baseline)? {
        return Ok(current);
    }
    if !snapshot.taken || snapshot.restored {
        return Err("This game journal does not own the current companion assignments.".into());
    }
    let desired = value(&snapshot.desired)?;
    if !same(&current, &desired)? {
        return Err(
            "Companion assignments changed outside the game session. Recovery preserved them."
                .into(),
        );
    }
    hub.compare_assignments(&desired, &baseline)
}
pub(super) fn abandon(_hub: &Hub, journal: &Value) -> Result<Value, String> {
    Snapshot::parse(journal)?;
    Ok(json!({"abandoned":true}))
}
fn same(first: &Value, second: &Value) -> Result<bool, String> {
    let mut a = AssignmentState::parse(first)?.assignments;
    let mut b = AssignmentState::parse(second)?.assignments;
    a.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
    b.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
    Ok(a == b)
}
#[cfg(test)]
#[path = "companion_game_owner_tests.rs"]
mod tests;
