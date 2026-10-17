use super::*;
use crate::compiler::{compile, BackendCapabilities, CompileSnapshot, PlanReadiness};
use serde_json::{json, Value};

fn blank() -> Value {
    serde_json::to_value(crate::default_document()).unwrap()
}
fn parse(value: &Value) -> Result<ProfileDocument, String> {
    ProfileDocument::parse_draft(&serde_json::to_vec(value).unwrap())
}
fn snapshot() -> CompileSnapshot {
    CompileSnapshot {
        inventory_session: "native-capture".into(),
        inventory_revision: "0".into(),
        assignments: vec![],
        resources: vec![],
        game_context: None,
        backend: BackendCapabilities::default(),
        inventory_diagnostics: vec![],
        read_only: false,
    }
}

#[test]
fn invalid_graph_drafts_reach_compiler_diagnostics_but_never_strict_storage_parsing() {
    let mut document = blank();
    document["bindings"] = json!([{"id":"missing-edge","from":{"nodeId":"missing-resource","portId":"output"},"to":{"nodeId":"missing-target","portId":"input"}}]);
    let bytes = serde_json::to_vec(&document).unwrap();
    assert!(ProfileDocument::parse(&bytes).is_err());
    let draft = ProfileDocument::parse_draft(&bytes).unwrap();
    let report = compile(&draft, &snapshot());
    assert_eq!(report.readiness, PlanReadiness::Invalid);
    assert!(report.plan.is_none());
    assert!(report.program_hash.is_none());
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "missingPort"
            && diagnostic.location.edge_ids == ["missing-edge"]));
    let mut operation = blank();
    let state = |id: &str, direction: &str| json!({"id":id,"name":id,"direction":direction,"contract":{"delivery":"state","schema":{"type":"boolean"},"freshnessMs":1000,"fallback":false}});
    operation["processingNodes"] = json!([{"id":"invalid-transform","name":"Invalid transform","operation":{"kind":"unit-convert","from":"ft","to":"m","rounding":"none"},"inputs":[state("input","input")],"outputs":[state("output","output")]}]);
    let draft = parse(&operation).unwrap();
    let report = compile(&draft, &snapshot());
    assert!(report.plan.is_none());
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "invalidOperation"
            && diagnostic.location.node_id.as_deref() == Some("invalid-transform")));
}

#[test]
fn draft_parser_keeps_strict_schema_identity_revision_and_serde_shape() {
    for (key, value) in [
        ("schemaVersion", json!(1)),
        ("id", json!("not-an-id")),
        ("id", json!("AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA")),
        ("revision", json!(0)),
        ("revision", json!(crate::MAX_REVISION + 1)),
        ("name", json!(" untrimmed")),
        ("name", json!("")),
        ("resources", json!(false)),
    ] {
        let mut draft = blank();
        draft[key] = value;
        assert!(parse(&draft).is_err(), "{key}");
    }
    let mut draft = blank();
    draft["machineAssignments"] = json!([]);
    assert!(parse(&draft).is_err());
    let mut draft = blank();
    draft["editor"]["viewport"]["unknown"] = json!(false);
    assert!(parse(&draft).is_err());
    let mut draft = blank();
    draft.as_object_mut().unwrap().remove("visibility");
    assert!(parse(&draft).is_err());
    assert!(ProfileDocument::parse_draft(br#"{"schemaVersion":2,"name":NaN}"#).is_err());
    assert!(ProfileDocument::parse_draft(b"{").is_err());
    let mut draft = blank();
    draft["revision"] = json!(crate::MAX_REVISION);
    assert!(parse(&draft).is_ok());
    let mut exact = serde_json::to_vec(&blank()).unwrap();
    exact.resize(crate::MAX_PROFILE_BYTES, b' ');
    assert!(ProfileDocument::parse_draft(&exact).is_ok());
    exact.push(b' ');
    assert!(ProfileDocument::parse_draft(&exact).is_err());
}

#[test]
fn draft_collection_limits_apply_before_native_capture_without_truncation() {
    for (field, row, count) in [
        (
            "resources",
            json!({"id":"resource","name":"Resource","kind":"device","required":false,"ports":[]}),
            65,
        ),
        (
            "processingNodes",
            json!({"id":"node","name":"Node","operation":{"kind":"identity"},"inputs":[],"outputs":[]}),
            257,
        ),
        (
            "virtualOutputs",
            json!({"id":"controller","name":"Controller","required":false,"ports":[],"layout":{"buttons":[],"axes":[],"hats":[]}}),
            17,
        ),
        (
            "bindings",
            json!({"id":"edge","from":{"nodeId":"source","portId":"port"},"to":{"nodeId":"target","portId":"port"}}),
            513,
        ),
        (
            "visibility",
            json!({"resourceId":"resource","mode":"visible"}),
            65,
        ),
        (
            "overrides",
            json!({"resourceId":"resource","enabled":true}),
            65,
        ),
        (
            "companionAssignments",
            json!({"resourceId":"companion","publicationHash":format!("sha256:{}","a".repeat(64))}),
            65,
        ),
    ] {
        let mut draft = blank();
        draft[field] = Value::Array(vec![row; count]);
        assert!(parse(&draft).is_err(), "{field}");
    }
    let mut draft = blank();
    draft["eligibility"]["gameIds"] = json!(vec!["dcs"; 33]);
    assert!(parse(&draft).is_err());
    let mut draft = blank();
    draft["editor"]["positions"] = json!(vec![json!({"nodeId":"node","x":0,"y":0}); 337]);
    assert!(parse(&draft).is_err());
    let port = json!({"id":"port","name":"Port","direction":"output","contract":{"delivery":"state","schema":{"type":"boolean"},"freshnessMs":1000,"fallback":false}});
    let resource = |ports: Vec<Value>| json!({"id":"resource","name":"Resource","kind":"device","required":false,"ports":ports});
    let mut draft = blank();
    draft["resources"] = json!([resource(vec![port.clone(); 129])]);
    assert!(parse(&draft).is_err());
    let mut draft = blank();
    draft["resources"] = json!(vec![resource(vec![port; 128]); 9]);
    assert!(parse(&draft).is_err());
}
