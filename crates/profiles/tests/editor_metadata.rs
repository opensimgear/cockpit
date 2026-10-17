//! Layout metadata stays bounded, portable and independent of execution.
use osg_profiles::{model::*, ProfileDocument};
use serde_json::{json, Value};

fn document() -> ProfileDocument {
    ProfileDocument::parse(include_bytes!("../../../tests/fixtures/profiles/complete-rig.json")).unwrap()
}
fn decorated() -> ProfileDocument {
    let mut document = document();
    document.editor.groups = vec![EditorGroup {
        id: "physical-inputs".into(), name: "Physical inputs".into(),
        members: document.resources.iter().take(2).map(|node| node.id.clone()).collect(),
    }];
    document.editor.collapsed_port_groups = vec![CollapsedPortGroup {
        node_id: document.resources[0].id.clone(), group: EditorPortGroup::Outputs,
    }];
    document
}
fn rejected(value: &Value) {
    assert!(ProfileDocument::parse(&serde_json::to_vec(value).unwrap()).is_err(), "{value}");
}
#[test]
fn metadata_roundtrips_without_changing_empty_exports_or_execution_identity() {
    let original = document();
    let mut value = serde_json::to_value(&original).unwrap();
    assert!(value["editor"].get("groups").is_none());
    assert!(value["editor"].get("collapsedPortGroups").is_none());
    value["editor"]["groups"] = json!([]);
    value["editor"]["collapsedPortGroups"] = json!([]);
    let empty = ProfileDocument::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(empty.export().unwrap(), original.export().unwrap());
    let changed = decorated();
    assert_eq!(changed.semantic_hash().unwrap(), original.semantic_hash().unwrap());
    assert_eq!(ProfileDocument::parse(&changed.export().unwrap()).unwrap(), changed);
}
#[test]
fn stored_exported_imported_and_duplicated_groups_remain_independent_of_runtime() {
    let parent = std::env::temp_dir();
    #[cfg(unix)]
    let parent = parent.canonicalize().unwrap();
    let directory = tempfile::tempdir_in(parent).unwrap();
    let mut service = osg_profiles::service::Service::new(Some(directory.path().into()));
    let graph = decorated();
    service.request(&json!({"command":"profile_editor_save","document":graph,"assignments":[],"expectedRevision":null})).unwrap();
    let state = service.request(&json!({"command":"profile_status"})).unwrap();
    drop(service);
    let mut service = osg_profiles::service::Service::new(Some(directory.path().into()));
    let saved = service.request(&json!({"command":"profile_export","id":graph.id})).unwrap()["document"].clone();
    assert_eq!(saved["editor"]["groups"], serde_json::to_value(&graph.editor.groups).unwrap());
    assert_eq!(saved["editor"]["collapsedPortGroups"], serde_json::to_value(&graph.editor.collapsed_port_groups).unwrap());
    let imported = service.request(&json!({"command":"profile_import","document":saved,"name":"Imported grouping"})).unwrap();
    let copied = service.request(&json!({"command":"profile_duplicate","id":graph.id,"name":"Copied grouping"})).unwrap();
    for result in [imported,copied] {
        assert_ne!(result["document"]["id"], graph.id);
        assert_eq!(result["document"]["editor"], saved["editor"]);
        assert_eq!(result["session"], state["session"]);
    }
}
#[test]
fn group_names_ids_membership_and_collection_limits_are_strict() {
    let valid = serde_json::to_value(decorated()).unwrap();
    let mut invalid = vec![];
    for (field, value) in [("id",json!("../group")),("name",json!(" bad ")),("name",json!("x".repeat(81))),("members",json!([])),("members",json!(["missing"])),("authority",json!(true))] {
        let mut changed=valid.clone();changed["editor"]["groups"][0][field]=value;invalid.push(changed);
    }
    for count in [2,33] {
        let mut changed=valid.clone();changed["editor"]["groups"]=json!(vec![valid["editor"]["groups"][0].clone();count]);invalid.push(changed);
    }
    let mut repeated=valid.clone();
    repeated["editor"]["groups"][0]["members"]=json!(vec![document().resources[0].id.clone();337]);invalid.push(repeated.clone());
    assert!(ProfileDocument::parse_draft(&serde_json::to_vec(&repeated).unwrap()).is_err());
    let mut overlap=valid.clone();let mut second=overlap["editor"]["groups"][0].clone();second["id"]=json!("other-group");
    overlap["editor"]["groups"].as_array_mut().unwrap().push(second);invalid.push(overlap);
    for value in invalid {rejected(&value);}
}
#[test]
fn collapsed_categories_require_known_unique_nodes_and_exact_supported_values() {
    let valid=serde_json::to_value(decorated()).unwrap();
    for (field,value) in [("nodeId",json!("missing")),("group",json!("all")),("enabled",json!(true))] {
        let mut changed=valid.clone();changed["editor"]["collapsedPortGroups"][0][field]=value;rejected(&changed);
    }
    for count in [2,1009] {
        let mut changed=valid.clone();changed["editor"]["collapsedPortGroups"]=json!(vec![valid["editor"]["collapsedPortGroups"][0].clone();count]);rejected(&changed);
        if count>1008 {assert!(ProfileDocument::parse_draft(&serde_json::to_vec(&changed).unwrap()).is_err());}
    }
    let mut all=decorated();
    all.editor.collapsed_port_groups=[EditorPortGroup::Outputs,EditorPortGroup::Inputs,EditorPortGroup::Actions].into_iter().map(|group|CollapsedPortGroup{node_id:all.resources[0].id.clone(),group}).collect();
    osg_profiles::validate_document(&all).unwrap();
}
#[test]
fn compiled_program_hash_is_unchanged_by_grouping_and_collapsed_ports() {
    use osg_profiles::compiler_models::{BackendCapabilities,CompileSnapshot};
    let mut plain=osg_profiles::default_document();
    plain.processing_nodes.push(ProcessingNode{id:"constant".into(),name:"Constant".into(),inputs:vec![],outputs:vec![Port{id:"value".into(),name:"Value".into(),direction:Direction::Output,contract:PortContract::State{schema:osg_mapping::signal::ScalarSchema::Boolean{},freshness_ms:1000,fallback:Some(PortValue::Boolean(false))}}],operation:Operation::Constant{value:PortValue::Boolean(true)}});
    let capture=CompileSnapshot{inventory_session:"metadata-test".into(),inventory_revision:"1".into(),assignments:vec![],resources:vec![],game_context:None,backend:BackendCapabilities::default(),inventory_diagnostics:vec![],read_only:false};
    let first=osg_profiles::compiler::compile(&plain,&capture);
    assert!(first.plan.is_some(),"{first:?}");
    let mut changed=plain.clone();changed.editor.groups.push(EditorGroup{id:"logic".into(),name:"Logic".into(),members:vec!["constant".into()]});changed.editor.collapsed_port_groups.push(CollapsedPortGroup{node_id:"constant".into(),group:EditorPortGroup::Outputs});
    let second=osg_profiles::compiler::compile(&changed,&capture);
    assert!(second.plan.is_some(),"{second:?}");
    assert_eq!(first.document_semantic_hash,second.document_semantic_hash);
    assert_eq!(first.program_hash,second.program_hash);
    assert!(first.program_hash.is_some());
}
