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
        bounds: None,
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
    for (field, value) in [("id",json!("../group")),("name",json!(" bad ")),("name",json!("x".repeat(81))),("members",json!(["missing"])),("authority",json!(true))] {
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
    plain.processing_nodes.push(ProcessingNode{ draft: None,id:"constant".into(),name:"Constant".into(),inputs:vec![],outputs:vec![Port{id:"value".into(),name:"Value".into(),direction:Direction::Output,contract:PortContract::State{schema:osg_mapping::signal::ScalarSchema::Boolean{},freshness_ms:1000,fallback:Some(PortValue::Boolean(false))}}],operation:Operation::Constant{value:PortValue::Boolean(true)}});
    let capture=CompileSnapshot{inventory_session:"metadata-test".into(),inventory_revision:"1".into(),assignments:vec![],resources:vec![],game_context:None,backend:BackendCapabilities::default(),inventory_diagnostics:vec![],read_only:false};
    let first=osg_profiles::compiler::compile(&plain,&capture);
    assert!(first.plan.is_some(),"{first:?}");
    let mut changed=plain.clone();changed.editor.groups.push(EditorGroup{id:"logic".into(),name:"Logic".into(),members:vec!["constant".into()],bounds:Some(EditorGroupBounds{x:0.0,y:0.0,width:440.0,height:360.0})});changed.editor.collapsed_port_groups.push(CollapsedPortGroup{node_id:"constant".into(),group:EditorPortGroup::Outputs});
    let second=osg_profiles::compiler::compile(&changed,&capture);
    assert!(second.plan.is_some(),"{second:?}");
    assert_eq!(first.document_semantic_hash,second.document_semantic_hash);
    assert_eq!(first.program_hash,second.program_hash);
    assert!(first.program_hash.is_some());
}

#[test]
fn empty_group_frames_roundtrip_and_reject_invalid_or_overlapping_geometry() {
    let mut graph=document();
    graph.editor.groups=vec![
        EditorGroup{id:"empty".into(),name:"Empty group".into(),members:vec![],bounds:Some(EditorGroupBounds{x:-20.0,y:0.0,width:440.0,height:360.0})},
        EditorGroup{id:"second".into(),name:"Second group".into(),members:vec![],bounds:Some(EditorGroupBounds{x:420.0,y:0.0,width:328.0,height:300.0})},
    ];
    assert_eq!(graph.semantic_hash().unwrap(),document().semantic_hash().unwrap());
    assert_eq!(ProfileDocument::parse(&graph.export().unwrap()).unwrap(),graph);
    let valid=serde_json::to_value(&graph).unwrap();
    for (field,value) in [("x",json!(1_000_001)),("y",json!(-1_000_001)),("width",json!(327)),("height",json!(299)),("width",json!(2_001_001)),("height",json!(2_001_001)),("extra",json!(false))] {
        let mut changed=valid.clone();changed["editor"]["groups"][0]["bounds"][field]=value;rejected(&changed);
    }
    for value in [Value::Null,json!({}),json!([]),json!("frame")] {
        let mut changed=valid.clone();changed["editor"]["groups"][0]["bounds"]=value;rejected(&changed);
    }
    let mut changed=valid.clone();changed["editor"]["groups"][1]["bounds"]["x"]=json!(419);rejected(&changed);
    // Legacy groups have no stored frame and remain readable until an editor action materializes one.
    let mut legacy=document();legacy.editor.groups.push(EditorGroup{id:"empty".into(),name:"Empty legacy group".into(),members:vec![],bounds:None});
    osg_profiles::validate_document(&legacy).unwrap();
}

#[test]
fn arrow_style_and_authored_paths_roundtrip_without_changing_execution_identity() {
    let original=document();
    let mut changed=original.clone();
    changed.editor.arrow_style=Some(EditorArrowStyle::Orthogonal);
    changed.editor.wire_routes=vec![EditorWireRoute{binding_id:changed.bindings[0].id.clone(),points:vec![
        EditorWirePoint{x:100.0,y:80.0},EditorWirePoint{x:250.0,y:80.0},EditorWirePoint{x:250.0,y:380.0},EditorWirePoint{x:500.0,y:380.0},
    ],automatic:false,handles:vec![EditorWirePoint{x:250.0,y:200.0}]}];
    assert_eq!(changed.semantic_hash().unwrap(),original.semantic_hash().unwrap());
    assert_eq!(ProfileDocument::parse(&changed.export().unwrap()).unwrap(),changed);
    let mut value=serde_json::to_value(&changed).unwrap();
    let valid=value.clone();
    for style in [Value::Null,json!("right-angle"),json!(true)] {
        value=valid.clone();value["editor"]["arrowStyle"]=style;rejected(&value);
    }
    for route in [
        json!({"bindingId":"missing","points":[{"x":0,"y":0},{"x":100,"y":0}]}),
        json!({"bindingId":changed.bindings[0].id,"points":[]}),
        json!({"bindingId":changed.bindings[0].id,"points":[{"x":0,"y":0},{"x":1,"y":1}]}),
        json!({"bindingId":changed.bindings[0].id,"points":[{"x":0,"y":0},{"x":0,"y":0}]}),
        json!({"bindingId":changed.bindings[0].id,"points":[{"x":0,"y":0},{"x":1_000_001,"y":0}]}),
        json!({"bindingId":changed.bindings[0].id,"points":[{"x":0,"y":0,"z":0},{"x":100,"y":0}]}),
    ] {value=valid.clone();value["editor"]["wireRoutes"]=json!([route]);rejected(&value);}
    value=valid.clone();value["editor"]["wireRoutes"]=json!([valid["editor"]["wireRoutes"][0],valid["editor"]["wireRoutes"][0]]);rejected(&value);
    value=valid.clone();value["editor"]["wireRoutes"][0]["points"]=json!((0..129).map(|x|json!({"x":x,"y":0})).collect::<Vec<_>>());
    rejected(&value);assert!(ProfileDocument::parse_draft(&serde_json::to_vec(&value).unwrap()).is_err());
    for handles in [Value::Null,json!([{"x":200,"y":200}]),json!([{"x":250,"y":200},{"x":250,"y":200}]),json!([{"x":250,"y":200,"z":0}])] {
        value=valid.clone();value["editor"]["wireRoutes"][0]["handles"]=handles;rejected(&value);
    }
    value=valid.clone();value["editor"]["wireRoutes"][0]["handles"]=json!((0..33).map(|index|json!({"x":250,"y":100+index})).collect::<Vec<_>>());
    rejected(&value);assert!(ProfileDocument::parse_draft(&serde_json::to_vec(&value).unwrap()).is_err());
    value=valid;value["editor"]["wireRoutes"]=json!(vec![value["editor"]["wireRoutes"][0].clone();513]);
    rejected(&value);assert!(ProfileDocument::parse_draft(&serde_json::to_vec(&value).unwrap()).is_err());
}

#[test]
fn automatic_corridors_preserve_generated_status_without_changing_execution_identity() {
    let original=document();
    let mut value=serde_json::to_value(&original).unwrap();
    value["editor"]["wireRoutes"]=json!([{"bindingId":original.bindings[0].id,"points":[{"x":100,"y":80},{"x":250,"y":80},{"x":250,"y":380},{"x":500,"y":380}]}]);
    let authored=ProfileDocument::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(!authored.editor.wire_routes[0].automatic);
    assert!(serde_json::to_value(&authored).unwrap()["editor"]["wireRoutes"][0].get("automatic").is_none());
    value["editor"]["wireRoutes"][0]["automatic"]=json!(false);
    assert_eq!(ProfileDocument::parse(&serde_json::to_vec(&value).unwrap()).unwrap(),authored);
    value["editor"]["wireRoutes"][0]["automatic"]=json!(true);
    let generated=ProfileDocument::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(generated.editor.wire_routes[0].automatic);
    assert_eq!(ProfileDocument::parse(&generated.export().unwrap()).unwrap(),generated);
    assert_eq!(generated.semantic_hash().unwrap(),original.semantic_hash().unwrap());
    for automatic in [Value::Null,json!("true"),json!(1),json!({}),json!([])] {
        let mut changed=value.clone();changed["editor"]["wireRoutes"][0]["automatic"]=automatic;rejected(&changed);
        assert!(ProfileDocument::parse_draft(&serde_json::to_vec(&changed).unwrap()).is_err());
    }
    value["editor"]["wireRoutes"][0]["handles"]=json!([]);
    assert!(ProfileDocument::parse(&serde_json::to_vec(&value).unwrap()).is_ok());
    value["editor"]["wireRoutes"][0]["handles"]=json!([{"x":250,"y":200}]);rejected(&value);
    value["editor"]["wireRoutes"][0]["automatic"]=json!(false);
    assert!(ProfileDocument::parse(&serde_json::to_vec(&value).unwrap()).is_ok());
}

#[test]
fn saved_wire_layout_preserves_exact_metadata_across_service_restart() {
    let parent = std::env::temp_dir();
    #[cfg(unix)]
    let parent = parent.canonicalize().unwrap();
    let directory = tempfile::tempdir_in(parent).unwrap();
    let mut service=osg_profiles::service::Service::new(Some(directory.path().into()));
    let mut graph=decorated();graph.editor.arrow_style=Some(EditorArrowStyle::Smooth);
    graph.editor.wire_routes=vec![EditorWireRoute{binding_id:graph.bindings[0].id.clone(),points:vec![EditorWirePoint{x:0.0,y:0.0},EditorWirePoint{x:100.0,y:0.0}],automatic:false,handles:vec![EditorWirePoint{x:50.0,y:0.0}]},EditorWireRoute{binding_id:graph.bindings[1].id.clone(),points:vec![EditorWirePoint{x:0.0,y:100.0},EditorWirePoint{x:100.0,y:100.0}],automatic:true,handles:vec![]}];
    let saved=service.request(&json!({"command":"profile_editor_save","document":graph,"assignments":[],"expectedRevision":null})).unwrap();
    drop(service);
    let mut service=osg_profiles::service::Service::new(Some(directory.path().into()));
    let exported=service.request(&json!({"command":"profile_export","id":graph.id})).unwrap();
    assert_eq!(exported["document"]["editor"],saved["document"]["editor"]);
}
