//! Game schema migration and immutable optional mapping intent.
use osg_mapping_host::service::game::Capture;
use serde_json::Value;
#[path = "game_companions.rs"]
pub mod companions;
pub fn validate(profile: &Value) -> Result<(), String> {
    if profile["schemaVersion"] == 1 {
        if profile.get("mapping").is_some() {
            return Err(
                "Migrate this game profile to schema 2 before capturing mapping intent.".into(),
            );
        }
        return Ok(());
    }
    if !matches!(profile["schemaVersion"].as_u64(), Some(2 | 3)) {
        return Err("Unsupported game profile schema.".into());
    }
    if !profile["mapping"].is_null() {
        Capture::parse(&profile["mapping"])?;
    }
    Ok(())
}
pub fn journal(value: &Value) -> Result<(), String> {
    if value["mapping"].is_null() {
        return Ok(());
    }
    if !matches!(value["schemaVersion"].as_u64(), Some(2 | 3)) {
        return Err("Mapping recovery requires journal schema 2.".into());
    }
    let m = &value["mapping"];
    let owner = m["owner"]
        .as_str()
        .ok_or("Missing mapping recovery owner.")?;
    if uuid::Uuid::parse_str(owner)
        .ok()
        .map(|u| u.to_string())
        .as_deref()
        != Some(owner)
    {
        return Err("Invalid mapping recovery owner.".into());
    }
    Capture::parse(&m["desired"])?;
    if !m["baseline"].is_null() {
        Capture::parse_recording(&m["baseline"])?;
    }
    for key in ["taken", "armed", "stopped", "resumed"] {
        if !m[key].is_boolean() {
            return Err("Invalid mapping recovery phase.".into());
        }
    }
    if !m["baselineToken"].is_null() {
        serde_json::from_value::<osg_mapping::engine::RunToken>(m["baselineToken"].clone())
            .map_err(|e| e.to_string())?;
    }
    serde_json::from_value::<osg_mapping_host::settings::Preferences>(m["settings"].clone())
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn overlap(profile: &Value) -> Result<(), String> {
    if profile["mapping"].is_null() || profile["integration"].is_null() {
        return Ok(());
    }
    let captured = Capture::parse(&profile["mapping"])?;
    let config = &profile["integration"];
    for mapping in captured
        .assignments
        .iter()
        .filter(|a| a.source.origin == osg_mapping::profile::Origin::OsgPanelEvents)
    {
        for assignment in config["assignments"].as_array().into_iter().flatten() {
            let id = format!(
                "panel-events:{}/{}",
                assignment["gatewayId"].as_str().unwrap_or(""),
                assignment["moduleId"].as_str().unwrap_or("")
            );
            if mapping.source.id != id {
                continue;
            }
            for input in config["document"]["inputs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|i| i["role"] == assignment["role"])
            {
                let index = input["control"]
                    .as_u64()
                    .ok_or("Invalid direct-command control.")?;
                let kind = input["kind"].as_str().unwrap_or("");
                let controls = if kind == "axis" {
                    vec![format!("axis:{index}")]
                } else if kind == "encoder" {
                    // Encoder offsets depend on the device; flag shared button routes conservatively.
                    mapping
                        .controls
                        .iter()
                        .filter(|c| c.source_control.starts_with("button:"))
                        .map(|c| c.source_control.clone())
                        .collect()
                } else {
                    vec![format!("button:{index}")]
                };
                if mapping
                    .controls
                    .iter()
                    .any(|c| controls.contains(&c.source_control))
                {
                    return Err(format!("Known input overlap: panel {} feeds both mapping and direct simulator command {}. Remove one route before activation.",assignment["moduleId"],input["command"]));
                }
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn migration_is_explicit_and_legacy_documents_stay_readable() {
        let mut p = json!({"schemaVersion":1});
        assert!(validate(&p).is_ok());
        p["mapping"] = Value::Null;
        assert!(validate(&p).is_err());
        p["schemaVersion"] = json!(2);
        assert!(validate(&p).is_ok());
        p["mapping"] = json!({});
        assert!(validate(&p).is_err());
        assert!(journal(&json!({"schemaVersion":1})).is_ok());
    }
    fn captured() -> Value {
        let profile = osg_mapping::profile::Profile::parse(include_bytes!(
            "../../crates/mapping/fixtures/direct-profile.json"
        ))
        .unwrap();
        let assignments=profile.roles.iter().map(|r|json!({"role":r.id,"source":{"origin":r.origin,"id":r.id},"controls":r.controls.iter().map(|c|json!({"control":c.id,"sourceControl":c.id})).collect::<Vec<_>>()})).collect::<Vec<_>>();
        json!({"profile":profile,"assignments":assignments,"targets":[],"visibility":null})
    }
    #[test]
    fn recovery_validates_snapshots_ownership_and_phases_without_starting_anything() {
        let c = captured();
        let valid = json!({"schemaVersion":2,"mapping":{"owner":"667891ca-e20a-459e-9d3c-c0e1c30a2bb0","desired":c,"baseline":c,"baselineToken":{"session":"667891ca-e20a-459e-9d3c-c0e1c30a2bb0","generation":"1"},"settings":osg_mapping_host::settings::Preferences::default(),"taken":true,"armed":true,"stopped":false,"resumed":false}});
        assert!(journal(&valid).is_ok());
        let mut preview_baseline = valid.clone();
        preview_baseline["mapping"]["baseline"]["profile"]["requirements"]["virtualDevices"] =
            json!(true);
        preview_baseline["mapping"]["baseline"]["profile"]["requirements"]["deviceHiding"] =
            json!(true);
        assert!(
            journal(&preview_baseline).is_ok(),
            "A prior preview stays a preview even when its profile requests optional features"
        );
        assert!(validate(&json!({"schemaVersion":2,"mapping":c})).is_ok());
        for (pointer, value) in [
            ("/schemaVersion", json!(1)),
            ("/mapping/owner", json!("bad")),
            ("/mapping/owner", Value::Null),
            ("/mapping/desired", Value::Null),
            ("/mapping/baseline", json!({})),
            ("/mapping/baselineToken", json!({})),
            ("/mapping/settings", Value::Null),
            ("/mapping/taken", Value::Null),
        ] {
            let mut invalid = valid.clone();
            *invalid.pointer_mut(pointer).unwrap() = value;
            assert!(journal(&invalid).is_err(), "{pointer}");
        }
        assert!(validate(&json!({"schemaVersion":9})).is_err());
    }
    #[test]
    fn only_known_shared_panel_controls_are_diagnosed_as_overlap() {
        let mut c = captured();
        c["profile"]["roles"][0]["origin"] = json!("osg-panel-events");
        c["assignments"][0]["source"] = json!({"origin":"osg-panel-events","id":"panel-events:stm32:00112233445566778899aabb/00112233445566778899aabbccddeeff"});
        c["assignments"][0]["controls"][0]["sourceControl"] = json!("button:0");
        c["assignments"][0]["controls"][1]["sourceControl"] = json!("axis:0");
        let mut p = json!({"schemaVersion":2,"mapping":c,"integration":{"assignments":[{"role":"direct","gatewayId":"stm32:00112233445566778899aabb","moduleId":"00112233445566778899aabbccddeeff"}],"document":{"inputs":[{"role":"direct","kind":"button","control":0,"command":"GEAR"}]}}});
        for kind in ["button", "axis", "encoder"] {
            p["integration"]["document"]["inputs"][0]["kind"] = json!(kind);
            assert!(overlap(&p).unwrap_err().contains("overlap"));
        }
        p["integration"]["document"]["inputs"][0]["kind"] = json!("button");
        p["integration"]["document"]["inputs"][0]["control"] = json!(1);
        assert!(overlap(&p).is_ok());
        p["integration"]["assignments"][0]["gatewayId"] = json!("other");
        assert!(overlap(&p).is_ok());
        p["mapping"] = Value::Null;
        assert!(overlap(&p).is_ok());
    }
}
