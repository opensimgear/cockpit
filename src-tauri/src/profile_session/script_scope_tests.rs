use super::*;
use osg_profiles::{
    Binding, Direction, Endpoint, Operation, Port, PortContract, PortValue, ProcessingNode,
};

fn fixture() -> (Candidate, ExecutionPlan) {
    let mut document = osg_profiles::default_document();
    let contract = PortContract::State {
        schema: osg_mapping::signal::ScalarSchema::Boolean {},
        freshness_ms: 1000,
        fallback: Some(PortValue::Boolean(false)),
    };
    let port = |id: &str, direction| Port {
        id: id.into(),
        name: id.into(),
        direction,
        contract: contract.clone(),
    };
    document.processing_nodes = vec![
        ProcessingNode { id:"constant".into(),name:"Constant".into(),draft:None,operation:Operation::Constant { value:PortValue::Boolean(false) },inputs:vec![],outputs:vec![port("out",Direction::Output)] },
        ProcessingNode { id:"script".into(),name:"Script".into(),draft:None,operation:Operation::Script {
            source:"fn on_start(ctx) { transaction(#{}, #{out:scalar(true)}, []) } fn on_event(ctx) { transaction(ctx.state, #{out:scalar(true)}, []) }".into(),parameters:Default::default() },
            inputs:vec![port("input",Direction::Input)],outputs:vec![port("out",Direction::Output)] },
    ];
    document.bindings.push(Binding {
        id: "wire".into(),
        from: Endpoint {
            node_id: "constant".into(),
            port_id: "out".into(),
        },
        to: Endpoint {
            node_id: "script".into(),
            port_id: "input".into(),
        },
    });
    let snapshot = osg_profiles::compiler_models::CompileSnapshot {
        inventory_session: "scope".into(),
        inventory_revision: "1".into(),
        assignments: vec![],
        resources: vec![],
        game_context: None,
        backend: osg_profiles::compiler_models::BackendCapabilities::default(),
        inventory_diagnostics: vec![],
        read_only: false,
    };
    let plan = osg_profiles::compiler::compile(&document, &snapshot)
        .plan
        .unwrap();
    (
        Candidate {
            document,
            assignments: vec![],
            virtual_slots: Default::default(),
            read_only: false,
            game_id: None,
            launch_preparation: false,
            admission_epoch: None,
            stage: None,
        },
        plan,
    )
}
fn script_blocked(plan: &ExecutionPlan, candidate: &Candidate) -> bool {
    NativeProfileHost::blockers(plan, Some(candidate))
        .iter()
        .any(|blocker| blocker.kind == BlockerKind::Script)
}

#[test]
fn internal_script_execution_cannot_admit_game_launch_or_external_resources() {
    assert!(!osg_mapping_host::service::SCRIPT_LIVE_QUALIFIED);
    let (candidate, plan) = fixture();
    assert!(!script_blocked(&plan, &candidate));
    assert!(NativeProfileHost::blockers(&plan, None).iter().any(|blocker| blocker.kind == BlockerKind::Script));
    let mut game = candidate.clone();
    game.game_id = Some("dcs".into());
    assert!(script_blocked(&plan, &game));
    let mut launch = candidate.clone();
    launch.launch_preparation = true;
    assert!(script_blocked(&plan, &launch));
    let mut external = candidate.clone();
    external.document.resources.push(osg_profiles::Resource {
        id: "external".into(),
        name: "Device".into(),
        kind: osg_profiles::ResourceKind::Device,
        required: false,
        ports: vec![],
    });
    assert!(script_blocked(&plan, &external));
}

#[test]
fn compiled_sinks_and_controllers_keep_the_existing_script_output_gate() {
    let (candidate, mut plan) = fixture();
    plan.program.sinks.push(osg_mapping::program::Sink {
        id: "external".into(),
        endpoint: osg_mapping::program::Endpoint {
            node_id: "device".into(),
            port_id: "lamp".into(),
        },
        from: osg_mapping::program::Endpoint {
            node_id: "script".into(),
            port_id: "out".into(),
        },
        contract: candidate.document.processing_nodes[1].outputs[0]
            .contract
            .clone(),
        kind: osg_mapping::program::SinkKind::Resource {
            adapter: osg_mapping::program::ResourceAdapter::CompanionFeedback,
        },
    });
    assert!(script_blocked(&plan, &candidate));
    plan.program.sinks.clear();
    plan.program
        .controllers
        .push(osg_mapping::program::VirtualController {
            id: "virtual".into(),
            layout: osg_mapping::program::ControllerLayout {
                buttons: vec![],
                axes: vec![],
                hats: vec![],
            },
        });
    assert!(script_blocked(&plan, &candidate));
}
