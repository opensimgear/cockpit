use osg_profiles::{compiler::compile, compiler_models::*, ProfileDocument};
use serde_json::json;

fn document() -> ProfileDocument {
    ProfileDocument::parse(include_bytes!("../../../tests/fixtures/profile-authoring/canvas-drafts.json")).unwrap()
}
fn snapshot() -> CompileSnapshot {
    CompileSnapshot { inventory_session: "canvas-drafts".into(), inventory_revision: "1".into(), assignments: vec![], resources: vec![], game_context: None, backend: BackendCapabilities::default(), inventory_diagnostics: vec![], read_only: false }
}

#[test]
fn all_canvas_presets_roundtrip_and_pending_contracts_never_compile_to_an_executable_plan() {
    let graph=document();
    assert_eq!(graph.processing_nodes.len(),30);
    assert_eq!(graph.processing_nodes.iter().filter(|node|node.draft.is_some()).count(),29);
    let multiply=graph.processing_nodes.iter().find(|node|node.draft.as_ref().is_some_and(|draft|draft.preset=="scale")).unwrap();
    assert_eq!(serde_json::to_value(&multiply.operation).unwrap(),json!({"kind":"scale","factor":10.0,"offset":0.0}));
    assert_eq!(multiply.draft.as_ref().unwrap().configured,vec!["factor"]);
    assert_eq!(ProfileDocument::parse(&graph.export().unwrap()).unwrap(),graph);
    let report=compile(&graph,&snapshot());
    assert_eq!(report.readiness,PlanReadiness::Invalid);
    assert!(report.plan.is_none());assert!(report.program_hash.is_none());
    assert_eq!(report.diagnostics.iter().filter(|row|row.code=="unresolvedTransformation").count(),29);
    for node in graph.processing_nodes.iter().filter(|node|node.draft.is_some()) {
        assert!(report.diagnostics.iter().any(|row|row.code=="unresolvedTransformation"&&row.location.node_id.as_deref()==Some(&node.id)));
    }
    let mut resolved=graph.clone();for node in &mut resolved.processing_nodes {node.draft=None;}
    assert_ne!(graph.semantic_hash().unwrap(),resolved.semantic_hash().unwrap(),"Pending contract state must participate in semantic identity.");
}

#[test]
fn draft_presets_and_configured_parameters_are_strict_on_import_and_preview() {
    let graph=document();let valid=serde_json::to_value(&graph).unwrap();
    for draft in [json!(null),json!({"preset":"unknown"}),json!({"preset":"scale","authority":true}),json!({"preset":"identity","configured":["missing"]}),json!({"preset":"identity","configured":["kind"]}),json!({"preset":"identity","configured":["kind","kind"]})] {
        let mut value=valid.clone();value["processingNodes"][0]["draft"]=draft;
        assert!(ProfileDocument::parse(&serde_json::to_vec(&value).unwrap()).is_err(),"{value}");
    }
    for draft in [json!(null),json!({"preset":"scale","authority":true})] {
        let mut value=valid.clone();value["processingNodes"][0]["draft"]=draft;
        assert!(ProfileDocument::parse_draft(&serde_json::to_vec(&value).unwrap()).is_err());
    }
}

#[test]
fn atomic_editor_save_restart_export_and_duplicate_preserve_pending_parameters_without_activating() {
    let directory=tempfile::tempdir().unwrap();let graph=document();
    let mut service=osg_profiles::service::Service::new(Some(directory.path().into()));
    let before=service.request(&json!({"command":"profile_status"})).unwrap();
    service.request(&json!({"command":"profile_editor_save","document":graph,"assignments":[],"expectedRevision":null})).unwrap();
    assert_eq!(service.request(&json!({"command":"profile_status"})).unwrap()["session"],before["session"]);
    drop(service);
    let mut service=osg_profiles::service::Service::new(Some(directory.path().into()));
    let saved=service.request(&json!({"command":"profile_export","id":graph.id})).unwrap()["document"].clone();
    assert_eq!(saved["processingNodes"],serde_json::to_value(&graph.processing_nodes).unwrap());
    let copy=service.request(&json!({"command":"profile_duplicate","id":graph.id,"name":"Copied canvas drafts"})).unwrap();
    assert_eq!(copy["document"]["processingNodes"],saved["processingNodes"]);
    assert_eq!(copy["session"],before["session"]);
}
