//! Restore-only handling of retained game companion ownership.
//! These helpers never open a session, arm a source, or submit controller input.
use crate::retained_game_companions::{AssignmentState, Capture, Snapshot};
use osg_companion_service::Hub;
use serde_json::{json, Value};
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
