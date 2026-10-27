use super::*;
use serde_json::json;

fn details(kind: &str, before: Value, after: Value) -> String {
    describe(kind, Some(&before), &after).join("\n")
}

#[test]
fn scalar_changes_show_both_values_and_missing_values() {
    for (kind, old, new, expected) in [
        ("profile", json!({"name":"Race"}), json!({"name":"Race night"}), "Name: “Race” → “Race night”"),
        ("game", json!({}), json!({"executable":"C:/race.exe"}), "Executable: not set → “C:/race.exe”"),
        ("device", json!({"connected":true}), json!({"connected":false}), "Connected: yes → no"),
        ("device", json!({"freshnessMs":50}), json!({"freshnessMs":100}), "Freshness (ms): 50 → 100"),
        ("integration", json!({"setup":{"port":4000}}), json!({"setup":{"port":5000}}), "Setup / Port: 4000 → 5000"),
    ] { assert_eq!(details(kind,old,new), expected); }
}

#[test]
fn profile_graph_changes_explain_routing_settings_and_requirements() {
    let before = json!({"resources":[{"id":"wheel","name":"Wheel","required":false,"ports":[{"id":"axis","contract":{"freshnessMs":50}}]}],
        "processingNodes":[{"id":"trim","name":"Trim","operation":{"kind":"scale","factor":1.0}}],
        "bindings":[{"id":"link","from":{"nodeId":"wheel","portId":"axis-1"},"to":{"nodeId":"virtual","portId":"steering"}}]});
    let mut after = before.clone();
    after["resources"][0]["required"] = json!(true);
    after["resources"][0]["ports"][0]["contract"]["freshnessMs"] = json!(100);
    after["processingNodes"][0]["operation"]["factor"] = json!(2.0);
    after["bindings"][0]["to"]["portId"] = json!("throttle");
    let result = details("profile",before,after);
    assert!(result.contains("Resources “Wheel” / Required: no → yes"),"{result}");
    assert!(result.contains("Ports “axis” / Contract / Freshness (ms): 50 → 100"),"{result}");
    assert!(result.contains("Processing nodes “Trim” / Operation / Factor: 1.0 → 2.0"),"{result}");
    assert!(result.contains("Connection: “wheel / axis-1 → virtual / steering” → “wheel / axis-1 → virtual / throttle”"));
}

#[test]
fn graph_additions_and_removals_have_names_and_routes() {
    let graph = json!({"resources":[{"id":"wheel","name":"Wheel","required":true}],
        "bindings":[{"id":"link","from":{"nodeId":"wheel","portId":"axis-1"},"to":{"nodeId":"virtual","portId":"steering"}}]});
    let added = details("profile",json!({}),graph.clone());
    assert!(added.contains("Resources “Wheel” added."));
    assert!(added.contains("Required: not set → yes"));
    assert!(added.contains("Connection “wheel / axis-1 → virtual / steering” added."));
    let removed = details("profile",graph,json!({}));
    assert!(removed.contains("Resources “Wheel” removed."));
    assert!(removed.contains("Connection “wheel / axis-1 → virtual / steering” removed."));
}

#[test]
fn tags_resolve_names_and_installation_edits_are_precise() {
    let result = details("game",json!({"tagIds":["old"],"selectedInstallationId":"copy-a","installed":false}),
        json!({"tagIds":["race"],"tagNames":{"race":"Racing","old":"Old games"},"selectedInstallationId":"copy-b","installed":true}));
    assert!(result.contains("Tags added: Racing"));
    assert!(result.contains("Tags removed: Old games"));
    assert!(result.contains("Selected installation id: “copy-a” → “copy-b”"));
    assert!(result.contains("Installed: no → yes"));
    assert!(!result.contains("tagIds"));
}

#[test]
fn reasons_are_not_truncated() {
    let reason = "The controller cannot be opened. ".repeat(100);
    let result = details("device",json!({}),json!({"stateReason":reason}));
    assert!(result.contains(&reason));
}

#[test]
fn arbitrary_json_and_credentials_never_reach_details_or_safe_snapshots() {
    let sentinel = "DO_NOT_LEAK_CREDENTIAL";
    let value = json!({"password":sentinel,"providerSetup":{"host":"localhost","port":5000,"apiKey":sentinel,"body":{"name":sentinel}},
        "unknownField":{"name":sentinel},"processingNodes":[{"id":"script","operation":{"kind":"script","source":sentinel,"parameters":{"token":sentinel}}}]});
    let result = details("profile",json!({}),value.clone());
    assert!(!result.contains(sentinel),"{result}");
    assert!(result.contains("Unknown field changed."));
    let projected = profile_snapshot(&value);
    assert!(!projected.to_string().contains(sentinel));
    assert!(!projected.to_string().contains("password"));
    assert!(details("profile",json!({}),projected).contains("Script content updated."));
    let integration = details("integration",json!({}),value["providerSetup"].clone());
    assert!(integration.contains("Host: not set → “localhost”"));
    assert!(!integration.contains(sentinel));
}

#[test]
fn unchanged_and_opaque_changes_are_safe_and_output_is_bounded() {
    assert!(describe("profile",Some(&json!({"name":"Race"})),&json!({"name":"Race"})).is_empty());
    let resources = (0..100).map(|i|json!({"id":format!("node-{i}"),"name":format!("Node {i}"),"required":true})).collect::<Vec<_>>();
    let result = describe("profile",None,&json!({"resources":resources}));
    assert_eq!(result.len(),LIMIT);
    assert!(result.last().unwrap().ends_with("further changes."));
    let mut after = json!({"processingNodes":[{"id":"script","operation":{"kind":"script","source":"new script"}}]});
    let before = profile_snapshot(&after);
    after["processingNodes"][0]["operation"]["source"] = json!("edited script");
    let result = details("profile",before,profile_snapshot(&after));
    assert!(result.contains("Script content updated."));
    assert!(!result.contains("edited script"));
}

#[test]
fn game_projection_ignores_runtime_decoration_and_never_retains_artwork_bytes() {
    use base64::Engine;
    use sha2::{Digest,Sha256};
    let image=b"artwork bytes";
    let reference=format!("artwork:{:x}.jpg",Sha256::digest(image));
    let raw=json!({"id":"race","name":"Race","background":{"image":reference,"positionX":50,"positionY":50},"metadata":{"opaque":"SENSITIVE_METADATA"}});
    let mut decorated=raw.clone();
    decorated["background"]["image"]=json!(format!("data:image/jpeg;base64,{}",base64::engine::general_purpose::STANDARD.encode(image)));
    decorated["integrationSetup"]=json!({"state":"ready"});
    decorated["profileContext"]=json!({"processSession":"runtime"});
    decorated["artworkSearch"]=json!({"status":"loading"});
    let before=game_snapshot(&raw);
    let after=game_snapshot(&decorated);
    assert_eq!(before,after);
    assert!(describe("game",Some(&before),&after).is_empty());
    assert!(!before.to_string().contains("SENSITIVE_METADATA"));
    assert!(!before.to_string().contains("artwork:"));
    assert!(!before.to_string().contains("data:image"));
}

#[test]
fn revision_is_bookkeeping_and_local_assignment_changes_show_actual_sources_and_ports() {
    assert!(describe("profile",Some(&json!({"revision":1})),&json!({"revision":2})).is_empty());
    let before=json!({"assignments":[{"resourceId":"wheel","sourceId":"controller-a","ports":[{"profilePortId":"steering","observedPortId":"axis-1"}]}]});
    let after=json!({"assignments":[{"resourceId":"wheel","sourceId":"controller-b","ports":[{"profilePortId":"steering","observedPortId":"axis-2"}]}]});
    let result=describe("profile",Some(&profile_snapshot(&before)),&profile_snapshot(&after)).join("\n");
    assert!(result.contains("Assignments “wheel” / Source: “controller-a” → “controller-b”"),"{result}");
    assert!(result.contains("Ports “steering” / Observed port: “axis-1” → “axis-2”"),"{result}");
}
