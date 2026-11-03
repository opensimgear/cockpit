use super::*;
use osg_mapping::engine::{Counter, Reason, typed::{Change, Frame}};
use osg_profiles::{Binding, ProcessingNode, Resource, ResourceKind, compiler::compile,
    compiler_models::{BackendCapabilities, CompileSnapshot}};

fn endpoint(node: &str, port: &str) -> Endpoint { Endpoint {node_id:node.into(),port_id:port.into()} }
fn port(id: &str, direction: Direction) -> Port {
    Port {id:id.into(),name:id.into(),direction,contract:PortContract::State {
        schema:osg_profiles::resources_ports::ScalarSchema::Boolean {},freshness_ms:1000,fallback:None}}
}
fn fixture() -> (ProfileDocument, Phase, OwnedRun) {
    let mut document = osg_profiles::default_document();
    document.resources.push(Resource {id:"panel".into(),name:"Panel".into(),kind:ResourceKind::Companion,required:false,
        ports:vec![port("pressed",Direction::Output),port("indicator",Direction::Input),port("unbound",Direction::Input)]});
    document.processing_nodes.push(ProcessingNode {draft:None,id:"invert".into(),name:"Invert".into(),
        operation:osg_mapping::program::Operation::Not {},inputs:vec![port("input",Direction::Input)],
        outputs:vec![port("output",Direction::Output)]});
    document.bindings = vec![Binding {id:"in".into(),from:endpoint("panel","pressed"),to:endpoint("invert","input")},
        Binding {id:"out".into(),from:endpoint("invert","output"),to:endpoint("panel","indicator")}];
    let report = compile(&document,&CompileSnapshot {inventory_session:"fixture".into(),inventory_revision:"1".into(),
        assignments:vec![],resources:vec![],game_context:None,backend:BackendCapabilities::default(),inventory_diagnostics:vec![],read_only:false});
    let session = "b3738156-6794-42dc-978b-2376dce0c6c6".to_string();
    let phase = Phase::new(report.plan.unwrap().program,Counter(1),session.clone()).unwrap();
    let run = OwnedRun {owner:RunOwner {session,generation:1,profile_id:document.id.clone(),profile_revision:document.revision,game_id:None},
        snapshot:osg_profiles::session::AppliedSnapshot::capture(&document).unwrap().summary().clone(),
        program_hash:report.program_hash.unwrap(),capture_fingerprint:report.capture_fingerprint,receipts:vec![],attempted:vec![]};
    (document,phase,run)
}
fn plan(document: &ProfileDocument, run: &OwnedRun, node: &str) -> Arc<NodePlan> {
    NodePlan::prepare(document,document,run,&topic(&run.owner,node)).unwrap()
}
fn feed(phase: &mut Phase, sequence: u64, at: u64, sample: SignalSample) {
    crate::profile_admission_test_clock::frame(phase,Frame {source:"panel".into(),generation:Counter(1),sequence:Counter(sequence),
        captured_us:Counter(at),reason:if sequence == 1 {Reason::Baseline}else{Reason::Change},changes:vec![Change {port_id:"pressed".into(),sample}]}).unwrap();
}

#[test]
fn exact_node_plan_maps_inputs_and_preserves_false_unknown_and_quality_without_advancing() {
    let (document,mut phase,run) = fixture();
    let plan = plan(&document,&run,"panel");
    let first = plan.capture(&phase,&run).unwrap();
    assert!(first.samples.iter().all(|value| *value == SignalSample::Unknown));
    feed(&mut phase,1,100,SignalSample::scalar(PortValue::Boolean(true)));
    let before = (phase.at_us(),phase.sample_revision(),phase.sample_revision_us());
    let captured = plan.capture(&phase,&run).unwrap();
    let payload = plan.encode(&captured).unwrap();
    assert_eq!(payload.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(),
        ["appliedSnapshot","nodeId","owner","ports","programHash"]);
    assert_eq!(payload["ports"][0]["endpoint"]["portId"],"indicator");
    assert_eq!(payload["ports"][0]["sample"],json!({"status":"known","value":{"kind":"scalar","value":false}}));
    assert_eq!(payload["ports"][2]["sample"],json!({"status":"unknown"}));
    assert_eq!(payload["ports"][0].as_object().unwrap().len(),4);
    assert_eq!((phase.at_us(),phase.sample_revision(),phase.sample_revision_us()),before);
    phase.advance(Counter(1_000_100)).unwrap();
    let expired = plan.capture(&phase,&run).unwrap();
    assert_eq!(expired.samples[0],SignalSample::Stale);
    assert_eq!(expired.captured_us,1_000_100);
    feed(&mut phase,2,1_000_110,SignalSample::Unavailable);
    assert_eq!(plan.capture(&phase,&run).unwrap().samples[1],SignalSample::Unavailable);
    phase.stop();
    assert!(plan.capture(&phase,&run).is_err());
}

#[test]
fn node_preparation_and_capture_reject_changed_owners_revisions_hashes_and_unknown_nodes() {
    let (document,phase,run) = fixture();
    let plan = plan(&document,&run,"invert");
    assert!(NodePlan::prepare(&document,&document,&run,&topic(&run.owner,"missing")).is_err());
    for changed in 0..4 {
        let mut wrong = run.clone();
        match changed {0=>wrong.owner.generation+=1,1=>wrong.owner.game_id=Some("dcs".into()),
            2=>wrong.program_hash.push('0'),_=>wrong.snapshot.semantic_hash.push('0')}
        assert!(plan.capture(&phase,&wrong).is_err());
    }
    let mut source = document.clone();source.resources[0].required=true;
    assert!(NodePlan::prepare(&document,&source,&run,&plan.topic).is_err());
    let mut variant = document.clone();variant.revision+=1;
    assert!(NodePlan::prepare(&variant,&document,&run,&plan.topic).is_err());
    let mut too_many = document.clone();
    too_many.resources[0].ports = (0..129).map(|id|port(&format!("port-{id}"),Direction::Output)).collect();
    assert!(NodePlan::prepare(&too_many,&document,&run,&topic(&run.owner,"panel")).is_err());
    assert!(plan.encode(&CapturedSamples {revision:0,captured_us:0,samples:vec![]}).is_err());
}

#[test]
fn raw_and_encoded_budgets_reject_oversized_scalar_and_action_allocations() {
    let (document,phase,run) = fixture();
    let plan = plan(&document,&run,"panel");
    let mut captured = plan.capture(&phase,&run).unwrap();
    captured.samples[0] = SignalSample::scalar(PortValue::Text("\0".repeat(MAX_PAYLOAD_BYTES / 3)));
    assert!(plan.encode(&captured).is_err(),"escaped JSON counts toward wire budget");
    let large = SignalSample::scalar(PortValue::Text("x".repeat(MAX_PAYLOAD_BYTES)));
    assert!(sample_bytes(&large).unwrap()+size_of::<SignalSample>() > MAX_PAYLOAD_BYTES);
    let pack = SignalSample::Known(SignalValue::ActionPack(osg_mapping::program::ActionPack {
        arguments:vec![osg_mapping::program::NamedArgument {id:"id".into(),value:PortValue::Text("x".repeat(MAX_PAYLOAD_BYTES))}]}));
    assert!(sample_bytes(&pack).unwrap() > MAX_PAYLOAD_BYTES);
}

#[test]
fn numeric_zero_is_an_observed_value() {
    let (mut document,_,mut run) = fixture();
    document.processing_nodes.clear();document.bindings.clear();
    let contract = PortContract::State {schema:osg_profiles::resources_ports::ScalarSchema::Number {
        min:-1.0,max:1.0,unit:None},freshness_ms:1000,fallback:None};
    document.resources[0].ports = vec![Port {contract:contract.clone(),..port("pressed",Direction::Output)}];
    run.snapshot = osg_profiles::session::AppliedSnapshot::capture(&document).unwrap().summary().clone();
    let program = osg_mapping::program::Program {sources:vec![osg_mapping::program::SourcePort {
        endpoint:RuntimeEndpoint {node_id:"panel".into(),port_id:"pressed".into()},contract}],
        nodes:vec![],sinks:vec![],controllers:vec![]};
    let mut phase = Phase::new(program,Counter(1),run.owner.session.clone()).unwrap();
    let plan = plan(&document,&run,"panel");
    feed(&mut phase,1,100,SignalSample::scalar(PortValue::Number(0.0)));
    let capture = plan.capture(&phase,&run).unwrap();
    assert_eq!(capture.samples,[SignalSample::scalar(PortValue::Number(0.0))]);
    assert_eq!(plan.encode(&capture).unwrap()["ports"][0]["sample"]["value"]["value"],0.0);
    assert!(plan.capture(&phase,&OwnedRun {program_hash:"changed".into(),..run.clone()}).is_err());
}

#[test]
fn templates_bind_cached_metadata_and_share_declarations_across_owners() {
    let (mut document,phase,run) = fixture();
    let templates = NodeTemplates::prepare(&document,&document).unwrap();
    let first = templates.bind(&run,&topic(&run.owner,"panel")).unwrap();
    assert_eq!(first.port_count(),3);
    let mut next = run.clone();next.owner.generation+=1;
    let second = templates.bind(&next,&topic(&next.owner,"panel")).unwrap();
    assert!(Arc::ptr_eq(&first.ports,&second.ports),"binding shares immutable contract descriptors");
    assert_ne!(first.topic,second.topic);
    document.resources[0].ports.clear();
    let retained = templates.bind(&run,&first.topic).unwrap();
    assert_eq!(retained.port_count(),3);
    assert_eq!(retained.capture(&phase,&run).unwrap().samples.len(),3);
    next.snapshot.semantic_hash.push('0');
    assert!(templates.bind(&next,&second.topic).is_err());
    assert!(templates.bind(&run,&second.topic).is_err());
}

#[test]
fn templates_reject_global_port_and_duplicate_node_bounds() {
    let (mut document,_,_) = fixture();
    let source = document.clone();
    let resource = document.resources[0].clone();
    document.resources = (0..9).map(|node| Resource {id:format!("panel-{node}"),
        ports:(0..128).map(|port_id|port(&format!("port-{port_id}"),Direction::Output)).collect(),..resource.clone()}).collect();
    assert!(NodeTemplates::prepare(&document,&source).is_err());
    document.resources = vec![resource.clone(),resource];
    assert!(NodeTemplates::prepare(&document,&source).is_err());
    document.resources = vec![document.resources[0].clone();65];
    assert!(NodeTemplates::prepare(&document,&source).is_err());
}

#[test]
fn deterministic_topic_vector_matches_the_frontend_json_array_contract() {
    let owner = RunOwner {session:"b3738156-6794-42dc-978b-2376dce0c6c6".into(),generation:7,
        profile_id:"11111111-1111-4111-8111-111111111111".into(),profile_revision:3,game_id:Some("dcs".into())};
    assert_eq!(topic(&owner,"virtual-1"),"profiles.samples.f9d15e05c22364b7a5f70a60116ab984faaea8bce05609245c29a7326045822b");
}
