//! Immutable, optional companion assignment intent for game schema 3.
//! This module validates data only; it cannot pair, arm, or send game input.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Assignment {
    pub instance_id: String,
    pub companion_id: String,
    pub publication_hash: String,
    pub capability_fingerprint: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Capture {
    pub schema_version: u8,
    pub assignments: Vec<Assignment>,
}
fn digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value.as_bytes()[7..]
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
}
impl Capture {
    pub fn parse(value: &Value) -> Result<Self, String> {
        if value.to_string().len() > 4096 {
            return Err("Captured companion assignments exceed 4 KiB.".into());
        }
        let capture: Self = serde_json::from_value(value.clone())
            .map_err(|_| "Invalid captured companion assignments.")?;
        if capture.schema_version != 1
            || capture.assignments.is_empty()
            || capture.assignments.len() > 8
        {
            return Err("Capture at most eight companion assignments with schema 1.".into());
        }
        let mut ids = BTreeSet::new();
        for a in &capture.assignments {
            if !osg_mapping::profile::identifier(&a.instance_id)
                || !osg_mapping::profile::identifier(&a.companion_id)
                || !digest(&a.publication_hash)
                || !digest(&a.capability_fingerprint)
                || !ids.insert(&a.instance_id)
            {
                return Err(
                    "Companion assignment identities, immutable hashes or duplicates are invalid."
                        .into(),
                );
            }
        }
        Ok(capture)
    }
}
/// Exact assignment state, including explicit absence, for compare-and-swap recovery.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StateAssignment {
    pub instance_id: String,
    pub companion_id: Option<String>,
    pub publication_hash: Option<String>,
    pub capability_fingerprint: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentState {
    pub schema_version: u8,
    pub assignments: Vec<StateAssignment>,
}
impl AssignmentState {
    pub fn parse(value: &Value) -> Result<Self, String> {
        if value.to_string().len() > 4096 {
            return Err("Companion baseline exceeds 4 KiB.".into());
        }
        let state: Self =
            serde_json::from_value(value.clone()).map_err(|_| "Invalid companion baseline.")?;
        if state.schema_version != 1 || state.assignments.len() > 8 {
            return Err("Invalid companion baseline version/budget.".into());
        }
        let mut ids = BTreeSet::new();
        for (row, a) in value["assignments"]
            .as_array()
            .unwrap()
            .iter()
            .zip(&state.assignments)
        {
            if !osg_mapping::profile::identifier(&a.instance_id)
                || !ids.insert(&a.instance_id)
                || ["companionId", "publicationHash", "capabilityFingerprint"]
                    .iter()
                    .any(|key| row.get(key).is_none())
            {
                return Err(
                    "Companion baseline identities and explicit state are required.".into(),
                );
            }
            match (
                &a.companion_id,
                &a.publication_hash,
                &a.capability_fingerprint,
            ) {
                (None, None, None) => {}
                (Some(id), Some(hash), Some(fingerprint))
                    if osg_mapping::profile::identifier(id)
                        && digest(hash)
                        && digest(fingerprint) => {}
                _ => return Err(
                    "Companion baseline must contain a complete assignment or explicit absence."
                        .into(),
                ),
            }
        }
        Ok(state)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub owner: String,
    pub desired: Capture,
    pub baseline: AssignmentState,
    pub taken: bool,
    pub restored: bool,
}
impl Snapshot {
    pub fn parse(value: &Value) -> Result<Self, String> {
        if value.to_string().len() > 8192 {
            return Err("Companion recovery snapshot exceeds 8 KiB.".into());
        }
        let s: Self = serde_json::from_value(value.clone())
            .map_err(|_| "Invalid companion recovery snapshot.")?;
        if uuid::Uuid::parse_str(&s.owner)
            .map(|u| u.to_string())
            .ok()
            .as_deref()
            != Some(s.owner.as_str())
        {
            return Err("Invalid companion recovery owner.".into());
        }
        Capture::parse(&value["desired"])?;
        AssignmentState::parse(&value["baseline"])?;
        if s.baseline.assignments.len() != s.desired.assignments.len()
            || s.baseline.assignments.iter().any(|a| {
                !s.desired
                    .assignments
                    .iter()
                    .any(|d| d.instance_id == a.instance_id)
            })
        {
            return Err("Companion recovery may restore only captured instance identities.".into());
        }
        Ok(s)
    }
}
pub fn profile(value: &Value) -> Result<(), String> {
    if value["schemaVersion"] != 3 {
        if value.get("companions").is_some() {
            return Err(
                "Upgrade this game profile to schema 3 before capturing companion assignments."
                    .into(),
            );
        }
    } else if !value["companions"].is_null() {
        Capture::parse(&value["companions"])?;
    }
    Ok(())
}
pub fn journal(value: &Value) -> Result<(), String> {
    if value["companions"].is_null() {
        return Ok(());
    }
    if value["schemaVersion"] != 3 {
        return Err("Companion recovery requires journal schema 3.".into());
    }
    Snapshot::parse(&value["companions"])?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn capture() -> Value {
        json!({"schemaVersion":1,"assignments":[{"instanceId":"panel-a","companionId":"installation-a","publicationHash":format!("sha256:{}","a".repeat(64)),"capabilityFingerprint":format!("sha256:{}","b".repeat(64))}]})
    }
    #[test]
    fn captures_are_bounded_strict_and_detached_from_later_edits() {
        let original = capture();
        let c = Capture::parse(&original).unwrap();
        let mut edited = original.clone();
        edited["assignments"][0]["publicationHash"] = json!(format!("sha256:{}", "c".repeat(64)));
        assert_ne!(c, Capture::parse(&edited).unwrap());
        for (pointer, v) in [
            ("/schemaVersion", json!(2)),
            ("/assignments/0/instanceId", json!("bad/id")),
            ("/assignments/0/companionId", json!("")),
            ("/assignments/0/publicationHash", json!("sha256:x")),
            (
                "/assignments/0/capabilityFingerprint",
                json!(format!("sha256:{}", "A".repeat(64))),
            ),
            ("/assignments/0/instanceId", json!("x".repeat(65))),
        ] {
            let mut bad = original.clone();
            *bad.pointer_mut(pointer).unwrap() = v;
            assert!(Capture::parse(&bad).is_err(), "{pointer}");
        }
        for v in [
            json!({"schemaVersion":1,"assignments":[],"token":"secret"}),
            json!({"schemaVersion":1,"assignments":[original["assignments"][0],original["assignments"][0]]}),
            json!({"schemaVersion":1,"assignments":vec![original["assignments"][0].clone();9]}),
            json!({"schemaVersion":1,"assignments":[{"extra":"x".repeat(4097)}]}),
            json!(null),
        ] {
            assert!(Capture::parse(&v).is_err());
        }
        assert!(Capture::parse(&json!({"schemaVersion":1,"assignments":[]})).is_err());
    }
    #[test]
    fn old_profiles_and_journals_remain_unchanged_and_migration_is_explicit() {
        for version in [1, 2] {
            let old = json!({"schemaVersion":version});
            assert!(profile(&old).is_ok());
            assert!(journal(&old).is_ok());
            assert!(profile(&json!({"schemaVersion":version,"companions":null})).is_err());
        }
        assert!(profile(&json!({"schemaVersion":3,"companions":capture()})).is_ok());
        assert!(profile(&json!({"schemaVersion":3,"companions":null})).is_ok());
        assert!(profile(&json!({"schemaVersion":3,"companions":{}})).is_err());
    }
    #[test]
    fn recovery_ownership_phases_and_restore_scope_are_validated() {
        let valid = json!({"owner":"667891ca-e20a-459e-9d3c-c0e1c30a2bb0","desired":capture(),"baseline":{"schemaVersion":1,"assignments":[{"instanceId":"panel-a","companionId":null,"publicationHash":null,"capabilityFingerprint":null}]},"taken":true,"restored":false});
        assert!(Snapshot::parse(&valid).is_ok());
        assert!(journal(&json!({"schemaVersion":3,"companions":valid})).is_ok());
        assert!(journal(&json!({"schemaVersion":2,"companions":valid})).is_err());
        for (pointer, v) in [
            ("/owner", json!("invalid")),
            ("/taken", json!(null)),
            ("/restored", json!(0)),
            ("/desired", json!(null)),
            ("/baseline", json!({})),
        ] {
            let mut bad = valid.clone();
            *bad.pointer_mut(pointer).unwrap() = v;
            assert!(Snapshot::parse(&bad).is_err());
        }
        let mut bad = valid.clone();
        bad["baseline"] = capture();
        bad["baseline"]["assignments"][0]["instanceId"] = json!("someone-else");
        assert!(Snapshot::parse(&bad).is_err());
        bad = valid.clone();
        bad["extra"] = json!(true);
        assert!(Snapshot::parse(&bad).is_err());
        bad = valid.clone();
        bad["owner"] = json!("x".repeat(8193));
        assert!(Snapshot::parse(&bad).is_err());
        assert!(AssignmentState::parse(&capture()).is_ok());
        for v in [
            json!({"schemaVersion":2,"assignments":[]}),
            json!({"schemaVersion":1,"assignments":[{"instanceId":"panel-a","companionId":null,"publicationHash":null}]}),
            json!({"schemaVersion":1,"assignments":[{"instanceId":"panel-a","companionId":"installation","publicationHash":null,"capabilityFingerprint":null}]}),
            json!({"schemaVersion":1,"assignments":[{"instanceId":"bad/id","companionId":null,"publicationHash":null,"capabilityFingerprint":null}]}),
            json!({"schemaVersion":1,"assignments":vec![capture()["assignments"][0].clone();2]}),
            json!({"schemaVersion":1,"assignments":[{"extra":"x".repeat(4097)}]}),
        ] {
            assert!(AssignmentState::parse(&v).is_err());
        }
    }
}
