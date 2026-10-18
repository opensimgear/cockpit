use crate::{compiler::*, *};

fn document() -> ProfileDocument {
    ProfileDocument::parse(include_bytes!("../../../tests/fixtures/profiles/desktop-only.json")).unwrap()
}
fn snapshot() -> CompileSnapshot {
    CompileSnapshot { inventory_session:"desktop-preview".into(),inventory_revision:"1".into(),assignments:vec![],resources:vec![],game_context:None,backend:BackendCapabilities::default(),inventory_diagnostics:vec![],read_only:false }
}
#[test]
fn desktop_only_roundtrips_without_controllers_or_machine_settings() {
    let source = document();
    let exported = source.export().unwrap();
    assert_eq!(ProfileDocument::parse(&exported).unwrap(), source);
    assert!(source.virtual_outputs.is_empty());
    let hash=source.semantic_hash().unwrap();
    let mut reordered=source.clone();
    reordered.desktop_outputs[0].ports.reverse();
    reordered.desktop_outputs[0].actions.reverse();
    assert_eq!(reordered.semantic_hash().unwrap(),hash);
    let json=serde_json::to_value(source).unwrap();
    assert!(json.get("enabled").is_none());
    assert!(json.get("scope").is_none());
}
#[test]
fn desktop_readiness_and_requirements_are_independent_of_controller_drivers() {
    let document=document();
    let mut capture=snapshot();
    let report=compile(&document,&capture);
    assert_eq!(report.readiness,PlanReadiness::Blocked,"{:?}",report.diagnostics);
    let plan=report.plan.unwrap();
    assert!(plan.requirements.desktop_input);
    assert!(!plan.requirements.virtual_devices);
    assert!(!plan.requirements.visibility);
    assert!(plan.controllers.is_empty());
    assert_eq!(plan.sinks.len(),2);
    assert!(plan.sinks.iter().all(|sink|sink.adapter==SinkAdapter::DesktopInput));
    for available in [false,true] { for enabled in [false,true] { for qualified in [false,true] {
        capture.backend.desktop_available=available;
        capture.backend.desktop_enabled=enabled;
        capture.backend.desktop_qualified=qualified;
        assert_eq!(compile(&document,&capture).readiness,if available&&enabled&&qualified {PlanReadiness::Ready}else{PlanReadiness::Blocked});
    }}}
}
#[test]
fn desktop_exact_actions_reject_wrong_contracts_duplicates_unsupported_keys_and_second_endpoint() {
    let source=document();
    let mut wrong=source.clone();wrong.desktop_outputs[0].ports[0].contract=wrong.desktop_outputs[0].ports[1].contract.clone();assert!(validate_document(&wrong).is_err());
    let mut duplicate=source.clone();duplicate.desktop_outputs[0].actions[1].port_id="hold".into();assert!(validate_document(&duplicate).is_err());
    let mut key=source.clone();key.desktop_outputs[0].actions[0].action=osg_mapping::desktop::Action::KeyHold{key:osg_mapping::desktop::Key(0xffff)};assert!(validate_document(&key).is_err());
    let mut second=source.clone();let mut output=second.desktop_outputs[0].clone();output.id="second-desktop".into();second.desktop_outputs.push(output);assert!(validate_document(&second).is_err());
    assert!(ProfileDocument::parse(&serde_json::to_vec(&second).unwrap()).is_err());
    assert_eq!(compile(&second,&snapshot()).readiness,PlanReadiness::Invalid);
}
#[test]
fn blank_editor_remains_valid_while_desktop_definition_budget_is_shared() {
    assert!(validate_document(&default_document()).is_ok());
    assert_eq!(compile(&default_document(),&snapshot()).readiness,PlanReadiness::Ready);
    let mut disconnected=document();disconnected.bindings.clear();disconnected.processing_nodes.clear();
    let preview=compile(&disconnected,&snapshot());
    assert_eq!(preview.readiness,PlanReadiness::Blocked);
    assert!(preview.plan.is_some(),"unconnected desktop draft retains its editable compiled shape");
    assert!(preview.diagnostics.iter().any(|diagnostic|diagnostic.code=="noExecutableOutput"));
    let mut source=document();
    let base=source.processing_nodes[0].clone();
    for index in 0..253 {let mut node=base.clone();node.id=format!("extra-{index}");source.processing_nodes.push(node);}
    assert!(validate_document(&source).is_err(),"255 processing nodes plus two actions exceed shared budget");
}
#[test]
fn desktop_scripts_are_blocked_transitively_even_when_native_desktop_is_qualified() {
    let mut source=document();
    let mut script=source.processing_nodes[1].clone();
    script.id="script".into();script.operation=Operation::Script {source:"fn on_start(ctx) { transaction(ctx.state, #{}, []) }\nfn on_event(ctx) { transaction(ctx.state, #{out: scalar(true)}, []) }".into(),parameters:Default::default()};
    script.outputs[0].contract=source.processing_nodes[0].outputs[0].contract.clone();
    source.processing_nodes.push(script);
    source.bindings.iter_mut().find(|binding| binding.id=="hold-route").unwrap().from.node_id="script".into();
    source.bindings.push(Binding{id:"script-input".into(),from:Endpoint{node_id:"trigger".into(),port_id:"out".into()},to:Endpoint{node_id:"script".into(),port_id:"in".into()}});
    assert!(validate_document(&source).is_ok());
    let mut capture=snapshot();capture.backend.desktop_available=true;capture.backend.desktop_enabled=true;capture.backend.desktop_qualified=true;
    let report=compile(&source,&capture);
    assert_eq!(report.readiness,PlanReadiness::Blocked,"{:?}",report.diagnostics);
    let diagnostic=report.diagnostics.iter().find(|diagnostic|diagnostic.code=="desktopScriptUnqualified").expect("traceable script qualification diagnostic");
    assert!(diagnostic.location.edge_ids.contains(&"script-input".into()));
    assert!(diagnostic.location.edge_ids.contains(&"hold-route".into()));
}
