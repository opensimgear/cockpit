use super::*;
use crate::resources::*;
use crate::resources_ports::*;
use crate::store::LocalAssignment;
use crate::{ControllerLayout, Operation, Port, ProcessingNode, Resource, VirtualOutput};
use osg_mapping::program::{Rounding, SignalSample, SignalValue};

fn mixed() -> ProfileDocument {
    ProfileDocument::parse(include_bytes!(
        "../../../tests/fixtures/profiles/mixed.json"
    ))
    .unwrap()
}

#[test]
fn visible_authoring_presets_use_native_contracts_and_compile_without_fake_readiness() {
    let document = ProfileDocument::parse(include_bytes!(
        "../../../tests/fixtures/profile-authoring/visible-presets.json"
    ))
    .expect("the editor's complete preset fixture must satisfy native contracts");
    assert_eq!(document.processing_nodes.len(), 29);
    assert_eq!(document.bindings.len(), 66);
    let mut capture = snapshot(&document);
    capture.assignments.clear();
    capture.resources.clear();
    capture.game_context = None;
    capture.backend = BackendCapabilities::default();
    let report = compile(&document, &capture);
    assert_ne!(report.readiness, PlanReadiness::Invalid, "{:?}", report.diagnostics);
    let plan = report.plan.expect("unassigned ports retain the derived graph");
    assert_eq!(plan.program.nodes.len(), 29);
    assert!(plan.branches.iter().all(|branch| branch.readiness != PlanReadiness::Ready));
    assert!(plan.sources.iter().all(|source| source.token.is_none()));
    assert!(plan.sinks.iter().all(|sink| sink.token.is_none()));
}
fn snapshot(document: &ProfileDocument) -> CompileSnapshot {
    let mut result = CompileSnapshot {
        inventory_session: "fixture-inventory".into(),
        inventory_revision: "1".into(),
        assignments: vec![],
        resources: vec![],
        game_context: Some(VerifiedGameContext {
            game_id: "dcs".into(),
            session: "verified-fixture-game".into(),
            provider_setup_revisions: BTreeMap::new(),
        }),
        backend: BackendCapabilities {
            name: "recording-fixture".into(),
            available: true,
            qualified: true,
            visibility_available: true,
            visibility_qualified: true,
            ..Default::default()
        },
        inventory_diagnostics: vec![],
        read_only: false,
    };
    for logical in &document.resources {
        let id = format!("opaque-inventory:{}", logical.id);
        let osg = logical.id.starts_with("can-") || logical.id == "direct-usb";
        let route = Route {
            id: format!("route:{}", logical.id),
            transport: if osg { Transport::Can } else { Transport::Hid },
            provider_generation: "provider-1".into(),
            attributes: BTreeMap::new(),
            baseline_ready: true,
            writable: true,
            priority: 10,
        };
        let ports = logical
            .ports
            .iter()
            .map(|port| ObservedPort {
                descriptor: port.descriptor(),
                contract_hash: port.descriptor().contract_hash().unwrap(),
                route_ids: vec![route.id.clone()],
                current: true,
            })
            .collect::<Vec<_>>();
        let capability_revision = hash(
            &ports
                .iter()
                .map(|port| &port.contract_hash)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let summary = ResourceSummary {
            id: id.clone(),
            name: logical.name.clone(),
            resource_type: match logical.kind {
                ResourceKind::Device if osg => ResourceType::OsgModule,
                ResourceKind::Device => ResourceType::HostController,
                ResourceKind::Companion => ResourceType::Companion,
                ResourceKind::Game => ResourceType::GameIntegration,
            },
            availability: Availability::Online,
            generation: "1".into(),
            capability_revision: capability_revision.clone(),
            port_count: ports.len(),
            cached_port_count: ports.len(),
            connections: vec![Connection {
                scope: if osg { "gateway:fixture" } else { "fixture" }.into(),
                key: logical.id.clone(),
                route: route.clone(),
            }],
            attributes: BTreeMap::new(),
            diagnostics: vec![],
        };
        let tokens = ports
            .iter()
            .map(|port| ResourceToken {
                inventory_session: result.inventory_session.clone(),
                resource_id: id.clone(),
                generation: "1".into(),
                capability_revision: capability_revision.clone(),
                route_id: route.id.clone(),
                provider_session: "provider-session".into(),
                provider_generation: route.provider_generation.clone(),
                port_id: port.descriptor.id.clone(),
                contract_hash: port.contract_hash.clone(),
                access: if port.descriptor.direction == Direction::Output {
                    Access::Read
                } else if matches!(port.descriptor.contract, PortContract::Action { .. }) {
                    Access::Invoke
                } else {
                    Access::Write
                },
            })
            .collect();
        result.assignments.push(LocalAssignment {
            resource_id: logical.id.clone(),
            source_id: id.clone(),
            ports: ports
                .iter()
                .map(|port| PortExpectation {
                    profile_port_id: port.descriptor.id.clone(),
                    observed_port_id: port.descriptor.id.clone(),
                    expected_contract_hash: port.contract_hash.clone(),
                })
                .collect(),
        });
        if logical.kind == ResourceKind::Game {
            result
                .game_context
                .as_mut()
                .unwrap()
                .provider_setup_revisions
                .insert(id, "setup-1".into());
        }
        result.resources.push(CapturedResource {
            summary,
            ports,
            tokens,
            provider_setup_revision: (logical.kind == ResourceKind::Game).then(|| "setup-1".into()),
            game_id: (logical.kind == ResourceKind::Game).then(|| "dcs".into()),
        });
    }
    result
}
fn valid(report: CompilationReport) -> ExecutionPlan {
    assert!(report.plan.is_some(), "{:?}", report.diagnostics);
    report.plan.unwrap()
}
fn has(report: &CompilationReport, code: &str) -> bool {
    report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == code)
}
fn boolean(id: &str, direction: Direction) -> Port {
    Port {
        id: id.into(),
        name: id.into(),
        direction,
        contract: PortContract::State {
            schema: ScalarSchema::Boolean {},
            freshness_ms: 1000,
            fallback: Some(crate::PortValue::Boolean(false)),
        },
    }
}
fn edge(id: &str, from: (&str, &str), to: (&str, &str)) -> Binding {
    Binding {
        id: id.into(),
        from: Endpoint {
            node_id: from.0.into(),
            port_id: from.1.into(),
        },
        to: Endpoint {
            node_id: to.0.into(),
            port_id: to.1.into(),
        },
    }
}
fn button_graph() -> ProfileDocument {
    let mut document = crate::new_document("Button fixture").unwrap();
    for id in ["device-a", "device-b"] {
        document.resources.push(Resource {
            id: id.into(),
            name: id.into(),
            kind: ResourceKind::Device,
            required: true,
            ports: vec![boolean("button", Direction::Output)],
        });
    }
    document.virtual_outputs.push(VirtualOutput {
        id: "controller".into(),
        name: "Controller".into(),
        required: true,
        layout: ControllerLayout {
            buttons: vec!["button".into()],
            axes: vec![],
            hats: vec![],
        },
        ports: vec![boolean("button", Direction::Input)],
    });
    document.bindings.push(edge(
        "button-a",
        ("device-a", "button"),
        ("controller", "button"),
    ));
    document
}

#[test]
fn mixed_graph_compiles_exact_typed_adapters_without_mutating_capture_or_document() {
    let document = mixed();
    let snapshot = snapshot(&document);
    let before = (
        serde_json::to_value(&document).unwrap(),
        serde_json::to_value(&snapshot).unwrap(),
    );
    let report = compile(&document, &snapshot);
    assert_eq!(
        report.readiness,
        PlanReadiness::Ready,
        "{:?}",
        report.diagnostics
    );
    assert!(report.program_hash.as_ref().unwrap().starts_with("sha256:"));
    assert_ne!(
        report.program_hash.as_ref().unwrap(),
        &report.capture_fingerprint
    );
    let plan = valid(report);
    assert_eq!(plan.controllers.len(), 2);
    for adapter in [
        SinkAdapter::VirtualController,
        SinkAdapter::SimulatorCommand,
        SinkAdapter::PhysicalAction,
        SinkAdapter::CompanionFeedback,
    ] {
        assert!(plan.sinks.iter().any(|sink| sink.adapter == adapter));
    }
    assert_eq!(plan.budgets.bindings, document.bindings.len());
    assert_eq!(plan.budgets.virtual_targets, 2);
    assert_eq!(plan.runtime_overrides, document.overrides);
    assert_eq!(plan.companion_assignments, document.companion_assignments);
    assert!(plan
        .trace
        .iter()
        .any(|trace| trace.node_id == "feet-to-metres"
            && trace.incoming_edge_ids == ["altitude-feet"]));
    assert_eq!(
        before,
        (
            serde_json::to_value(&document).unwrap(),
            serde_json::to_value(&snapshot).unwrap()
        )
    );
}

#[test]
fn declaration_reordering_keeps_program_order_hash_and_capture_fingerprint() {
    let document = mixed();
    let captured = snapshot(&document);
    let first = compile(&document, &captured);
    let mut reordered = document.clone();
    reordered.resources.reverse();
    for resource in &mut reordered.resources {
        resource.ports.reverse();
    }
    reordered.bindings.reverse();
    reordered.processing_nodes.reverse();
    for node in &mut reordered.processing_nodes {
        node.inputs.reverse();
        node.outputs.reverse();
    }
    reordered.virtual_outputs.reverse();
    for controller in &mut reordered.virtual_outputs {
        controller.ports.reverse();
    }
    reordered.visibility.reverse();
    reordered.overrides.reverse();
    reordered.companion_assignments.reverse();
    let mut captured_reordered = captured.clone();
    captured_reordered.assignments.reverse();
    captured_reordered.resources.reverse();
    for assignment in &mut captured_reordered.assignments {
        assignment.ports.reverse();
    }
    for resource in &mut captured_reordered.resources {
        resource.ports.reverse();
        resource.tokens.reverse();
        resource.summary.connections.reverse();
    }
    let second = compile(&reordered, &captured_reordered);
    assert_eq!(first.program_hash, second.program_hash);
    assert_eq!(first.document_semantic_hash, second.document_semantic_hash);
    assert_eq!(first.capture_fingerprint, second.capture_fingerprint);
    let a = valid(first);
    let b = valid(second);
    assert_eq!(a.program, b.program);
    assert_eq!(a.topological_order, b.topological_order);
    assert!(
        a.topological_order
            .iter()
            .position(|id| id == "feet-to-metres")
            < a.topological_order
                .iter()
                .position(|id| id == "altitude-threshold")
    );
}

#[test]
fn policy_changes_invalidate_portable_hash_while_capture_generations_do_not() {
    let document = mixed();
    let captured = snapshot(&document);
    let initial = compile(&document, &captured);
    let mut cosmetic = document.clone();
    cosmetic.name = "Renamed".into();
    cosmetic.revision += 1;
    cosmetic.editor.viewport.x += 25.0;
    for resource in &mut cosmetic.resources {
        resource.name = format!("Renamed {}", resource.id);
    }
    for resource in &mut cosmetic.resources {
        for port in &mut resource.ports {
            if let PortContract::Action { parameters, .. } = &mut port.contract {
                for parameter in parameters {
                    parameter.name = "Cosmetic argument".into();
                }
            }
        }
    }
    let cosmetic_report = compile(&cosmetic, &captured);
    assert_eq!(cosmetic_report.readiness, PlanReadiness::Ready);
    assert_eq!(initial.program_hash, cosmetic_report.program_hash);
    assert_eq!(
        initial.document_semantic_hash,
        cosmetic_report.document_semantic_hash
    );
    let mut environment = captured.clone();
    environment.inventory_revision = "2".into();
    assert_eq!(
        initial.program_hash,
        compile(&document, &environment).program_hash
    );
    assert_ne!(
        initial.capture_fingerprint,
        compile(&document, &environment).capture_fingerprint
    );
    for policy in 0..5 {
        let mut changed = document.clone();
        match policy {
            0 => changed.resources[0].required = !changed.resources[0].required,
            1 => changed.eligibility.game_ids = vec!["x-plane".into()],
            2 => changed.visibility[0].mode = crate::VisibilityMode::Visible,
            3 => changed.overrides[0].enabled = !changed.overrides[0].enabled,
            _ => {
                changed.companion_assignments[0].publication_hash =
                    format!("sha256:{}", "b".repeat(64))
            }
        }
        let result = compile(&changed, &captured);
        assert!(result.plan.is_some(), "{:?}", result.diagnostics);
        assert_ne!(initial.program_hash, result.program_hash, "policy {policy}");
    }
}

#[test]
fn missing_ports_and_multiple_writers_are_fatal_even_when_native_readiness_is_absent() {
    let mut document = button_graph();
    let mut captured = snapshot(&document);
    captured.backend.available = false;
    captured.game_context = None;
    document.bindings[0].from.port_id = "missing".into();
    let report = compile(&document, &captured);
    assert!(report.plan.is_none());
    assert!(has(&report, "missingPort"));
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "missingPort"
            && diagnostic.location.edge_ids == ["button-a"]));
    document.bindings[0].from.port_id = "button".into();
    document.bindings.push(edge(
        "button-b",
        ("device-b", "button"),
        ("controller", "button"),
    ));
    let report = compile(&document, &captured);
    assert!(report.plan.is_none());
    assert!(has(&report, "ambiguousWriters"));
}

#[test]
fn explicit_or_replaces_ambiguous_button_writers_and_unbound_inputs_are_errors() {
    let mut document = button_graph();
    document.processing_nodes.push(ProcessingNode {
        id: "or".into(),
        name: "Explicit OR".into(),
        operation: Operation::Any {},
        inputs: vec![
            boolean("a", Direction::Input),
            boolean("b", Direction::Input),
        ],
        outputs: vec![boolean("pressed", Direction::Output)],
    });
    document.bindings = vec![
        edge("a-or", ("device-a", "button"), ("or", "a")),
        edge("b-or", ("device-b", "button"), ("or", "b")),
        edge("or-target", ("or", "pressed"), ("controller", "button")),
    ];
    let captured = snapshot(&document);
    assert_eq!(
        compile(&document, &captured).readiness,
        PlanReadiness::Ready
    );
    document.bindings.remove(1);
    let report = compile(&document, &captured);
    assert!(report.plan.is_none());
    assert!(has(&report, "inputUnbound"));
}

#[test]
fn internal_cycles_fail_but_external_command_telemetry_pairs_are_independent() {
    let mut document = crate::new_document("Cycle fixture").unwrap();
    for id in ["first", "second"] {
        document.processing_nodes.push(ProcessingNode {
            id: id.into(),
            name: id.into(),
            operation: Operation::Identity {},
            inputs: vec![boolean("input", Direction::Input)],
            outputs: vec![boolean("output", Direction::Output)],
        });
    }
    document.bindings = vec![
        edge("one-two", ("first", "output"), ("second", "input")),
        edge("two-one", ("second", "output"), ("first", "input")),
    ];
    let cyclic = compile(&document, &snapshot(&document));
    assert!(cyclic.plan.is_none());
    assert!(has(&cyclic, "internalCycle"));
    document.processing_nodes.clear();
    document.bindings.clear();
    document.resources.push(Resource {
        id: "dcs".into(),
        name: "DCS".into(),
        kind: ResourceKind::Game,
        required: true,
        ports: vec![
            boolean("telemetry", Direction::Output),
            boolean("command", Direction::Input),
        ],
    });
    document.bindings.push(edge(
        "external-pair",
        ("dcs", "telemetry"),
        ("dcs", "command"),
    ));
    let report = compile(&document, &snapshot(&document));
    assert!(report.plan.is_some(), "{:?}", report.diagnostics);
}

#[test]
fn raw_native_ranges_need_an_explicit_range_map_and_changed_contracts_block_preflight() {
    let mut document = mixed();
    let source = document
        .resources
        .iter_mut()
        .find(|resource| resource.id == "third-party-b")
        .unwrap();
    source.ports[0].contract = PortContract::State {
        schema: ScalarSchema::Integer {
            min: 0,
            max: 65535,
            unit: None,
        },
        freshness_ms: 1000,
        fallback: Some(crate::PortValue::Number(0.0)),
    };
    let captured = snapshot(&document);
    assert!(compile(&document, &captured).plan.is_none());
    let raw = document
        .resources
        .iter()
        .find(|resource| resource.id == "third-party-b")
        .unwrap()
        .ports[0]
        .clone();
    let target = document
        .virtual_outputs
        .iter()
        .find(|controller| controller.id == "combined-controller")
        .unwrap()
        .ports
        .iter()
        .find(|port| port.id == "throttle")
        .unwrap()
        .clone();
    document.processing_nodes.push(ProcessingNode {
        id: "native-range".into(),
        name: "Explicit native range".into(),
        operation: Operation::RangeMap {
            input_min: 0.0,
            input_max: 65535.0,
            output_min: 0.0,
            output_max: 32767.0,
            rounding: Rounding::Nearest,
        },
        inputs: vec![Port {
            id: "raw".into(),
            name: "Raw".into(),
            direction: Direction::Input,
            contract: raw.contract.clone(),
        }],
        outputs: vec![Port {
            id: "axis".into(),
            name: "Axis".into(),
            direction: Direction::Output,
            contract: target.contract,
        }],
    });
    document
        .bindings
        .iter_mut()
        .find(|binding| binding.id == "third-party-throttle")
        .unwrap()
        .from = Endpoint {
        node_id: "native-range".into(),
        port_id: "axis".into(),
    };
    document.bindings.push(edge(
        "raw-range",
        ("third-party-b", "axis"),
        ("native-range", "raw"),
    ));
    let ready = compile(&document, &captured);
    assert_eq!(
        ready.readiness,
        PlanReadiness::Ready,
        "{:?}",
        ready.diagnostics
    );
    let mut changed = captured.clone();
    let resource = changed
        .resources
        .iter_mut()
        .find(|resource| resource.summary.id.ends_with("third-party-b"))
        .unwrap();
    resource.ports[0].descriptor.contract = PortContract::State {
        schema: ScalarSchema::Integer {
            min: 0,
            max: 1023,
            unit: None,
        },
        freshness_ms: 1000,
        fallback: Some(crate::PortValue::Number(0.0)),
    };
    resource.ports[0].contract_hash = resource.ports[0].descriptor.contract_hash().unwrap();
    let blocked = compile(&document, &changed);
    assert!(blocked.plan.is_some());
    assert_eq!(blocked.readiness, PlanReadiness::Blocked);
    assert!(has(&blocked, "portContractChanged"));
}

#[test]
fn no_game_context_is_dormant_and_runtime_unavailability_preserves_the_valid_graph() {
    let document = mixed();
    let mut captured = snapshot(&document);
    captured.game_context = None;
    let report = compile(&document, &captured);
    let hash = report.program_hash.clone();
    let plan = valid(report);
    assert!(plan
        .branches
        .iter()
        .any(|branch| branch.readiness == PlanReadiness::Dormant));
    assert!(plan
        .branches
        .iter()
        .any(|branch| branch.readiness == PlanReadiness::Ready));
    captured.backend.available = false;
    captured.read_only = true;
    let blocked = compile(&document, &captured);
    assert_eq!(blocked.readiness, PlanReadiness::Blocked);
    assert_eq!(blocked.program_hash, hash);
    assert!(blocked.plan.is_some());
    let mut optional = button_graph();
    optional.resources[0].required = false;
    optional.virtual_outputs[0].required = false;
    let mut unavailable = snapshot(&optional);
    unavailable.resources[0].summary.availability = Availability::Unavailable;
    assert_eq!(
        compile(&optional, &unavailable).readiness,
        PlanReadiness::Ready
    );
}

#[test]
fn duplicate_physical_aliases_and_virtual_feedback_are_rejected() {
    let document = mixed();
    let mut captured = snapshot(&document);
    let physical = captured.assignments[0].source_id.clone();
    captured
        .assignments
        .iter_mut()
        .find(|assignment| assignment.resource_id == "third-party-b")
        .unwrap()
        .source_id = physical;
    let report = compile(&document, &captured);
    assert!(report.plan.is_none());
    assert!(has(&report, "duplicatePhysicalSource"));
    let mut captured = snapshot(&document);
    captured.resources[0]
        .summary
        .attributes
        .insert("backend".into(), "vjoy".into());
    let report = compile(&document, &captured);
    assert!(report.plan.is_none());
    assert!(has(&report, "virtualOutputFeedback"));
}

#[test]
fn several_current_capture_routes_for_one_physical_resource_are_rejected() {
    let document = mixed();
    let mut captured = snapshot(&document);
    let resource = captured
        .resources
        .iter_mut()
        .find(|resource| resource.summary.id.ends_with("third-party-a"))
        .unwrap();
    let mut second = resource.summary.connections[0].clone();
    second.route.id.push_str("-second");
    second.route.transport = Transport::Usb;
    let id = second.route.id.clone();
    resource.summary.connections.push(second);
    let port = resource
        .ports
        .iter_mut()
        .find(|port| port.descriptor.id == "pressed")
        .unwrap();
    port.route_ids = vec![id.clone()];
    resource
        .tokens
        .iter_mut()
        .find(|token| token.port_id == "pressed")
        .unwrap()
        .route_id = id;
    let report = compile(&document, &captured);
    assert!(report.plan.is_none());
    assert!(has(&report, "duplicatePhysicalInputRoutes"));
}

#[test]
fn aliased_physical_endpoint_writers_are_rejected_before_execution() {
    let mut document = mixed();
    let module = document
        .resources
        .iter_mut()
        .find(|resource| resource.id == "can-a")
        .unwrap();
    let mut alias = module
        .ports
        .iter()
        .find(|port| port.id == "led")
        .unwrap()
        .clone();
    alias.id = "same-led".into();
    module.ports.push(alias);
    document.bindings.push(edge(
        "alias-writer",
        ("warning-action", "action"),
        ("can-a", "same-led"),
    ));
    let report = compile(&document, &snapshot(&document));
    assert!(report.plan.is_none());
    assert!(has(&report, "conflictingEndpointWriters"));
}

#[test]
fn native_osg_state_and_event_button_aliases_cannot_double_route_a_simulator_control() {
    let mut document = mixed();
    document
        .resources
        .iter_mut()
        .find(|resource| resource.id == "third-party-a")
        .unwrap()
        .ports
        .push(boolean("button", Direction::Output));
    let controller = document
        .virtual_outputs
        .iter_mut()
        .find(|controller| controller.id == "combined-controller")
        .unwrap();
    controller.layout.buttons.push("fire".into());
    controller.ports.push(boolean("fire", Direction::Input));
    document.bindings.push(edge(
        "physical-button-virtual",
        ("third-party-a", "button"),
        ("combined-controller", "fire"),
    ));
    let mut captured = snapshot(&document);
    let index = captured
        .resources
        .iter()
        .position(|resource| resource.summary.id.ends_with("third-party-a"))
        .unwrap();
    captured.resources[index].summary.resource_type = ResourceType::OsgModule;
    captured.resources[index].summary.connections[0].scope = "module-usb".into();
    for (logical, observed) in [("button", "button:0"), ("pressed", "button-event:0")] {
        let resource = &mut captured.resources[index];
        resource
            .ports
            .iter_mut()
            .find(|port| port.descriptor.id == logical)
            .unwrap()
            .descriptor
            .id = observed.into();
        resource
            .tokens
            .iter_mut()
            .find(|token| token.port_id == logical)
            .unwrap()
            .port_id = observed.into();
        captured
            .assignments
            .iter_mut()
            .find(|assignment| assignment.resource_id == "third-party-a")
            .unwrap()
            .ports
            .iter_mut()
            .find(|port| port.profile_port_id == logical)
            .unwrap()
            .observed_port_id = observed.into();
    }
    let conflict = compile(&document, &captured);
    assert!(conflict.plan.is_none());
    assert!(has(&conflict, "virtualDirectSimulatorConflict"));
    captured.resources[index].summary.resource_type = ResourceType::HostController;
    let arbitrary_names = compile(&document, &captured);
    assert!(
        arbitrary_names.plan.is_some(),
        "{:?}",
        arbitrary_names.diagnostics
    );
}

#[test]
fn hard_caps_are_actionable_and_do_not_truncate_the_graph() {
    let mut document = button_graph();
    let base = document.virtual_outputs[0].clone();
    for n in 1..9 {
        let mut controller = base.clone();
        controller.id = format!("controller-{n}");
        document.virtual_outputs.push(controller);
    }
    let report = compile(&document, &snapshot(&document));
    assert!(report.plan.is_none());
    assert!(has(&report, "virtualTargetLimit"));
    let mut document = button_graph();
    document.resources.clear();
    document.bindings.clear();
    document.virtual_outputs[0].ports.clear();
    document.virtual_outputs[0].layout.buttons.clear();
    for n in 0..17 {
        let source = format!("device-{n}");
        let port = format!("button-{n}");
        document.resources.push(Resource {
            id: source.clone(),
            name: source.clone(),
            kind: ResourceKind::Device,
            required: true,
            ports: vec![boolean("button", Direction::Output)],
        });
        document.virtual_outputs[0]
            .ports
            .push(boolean(&port, Direction::Input));
        document.virtual_outputs[0]
            .layout
            .buttons
            .push(port.clone());
        document.bindings.push(edge(
            &format!("edge-{n}"),
            (&source, "button"),
            ("controller", &port),
        ));
    }
    let report = compile(&document, &snapshot(&document));
    assert!(report.plan.is_none());
    assert!(has(&report, "sourceResourceLimit"));
    document.resources = vec![document.resources[0].clone(); 65];
    assert!(has(
        &compile(&document, &snapshot(&document)),
        "graphBudgetExceeded"
    ));
}

#[test]
fn malformed_atomic_actions_and_invisible_unit_conversions_have_original_node_diagnostics() {
    let mut document = mixed();
    let captured = snapshot(&document);
    document
        .processing_nodes
        .iter_mut()
        .find(|node| node.id == "feet-to-metres")
        .unwrap()
        .operation = Operation::Scale {
        factor: 0.3048,
        offset: 0.0,
    };
    let implicit = compile(&document, &captured);
    assert!(implicit.plan.is_none());
    assert!(implicit
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "invalidOperation"
            && diagnostic.location.node_id.as_deref() == Some("feet-to-metres")));
    let mut document = mixed();
    document
        .processing_nodes
        .iter_mut()
        .find(|node| node.id == "warning-action")
        .unwrap()
        .operation = Operation::SetOnChange {
        arguments: vec!["missing-argument".into()],
    };
    let malformed = compile(&document, &captured);
    assert!(malformed.plan.is_none());
    assert!(malformed
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "invalidOperation"
            && diagnostic.location.node_id.as_deref() == Some("warning-action")));
}

#[test]
fn malformed_native_contracts_and_stale_tokens_never_grant_ready_routes() {
    let document = mixed();
    let mut captured = snapshot(&document);
    captured.resources[0].ports[0].contract_hash = "forged".into();
    let malformed = compile(&document, &captured);
    assert!(malformed.plan.is_none());
    assert!(has(&malformed, "invalidCapture"));
    let mut captured = snapshot(&document);
    captured.resources[0].tokens[0].provider_generation = "stale".into();
    let stale = compile(&document, &captured);
    assert!(stale.plan.is_some());
    assert_eq!(stale.readiness, PlanReadiness::Blocked);
    assert!(has(&stale, "routeNotReady"));
}

#[test]
fn assignment_and_connection_readiness_failures_keep_a_blocked_traceable_plan() {
    let document = button_graph();
    for variant in 0..9 {
        let mut captured = snapshot(&document);
        let expected_code = match variant {
            0 => {
                captured.assignments.remove(0);
                "resourceUnassigned"
            }
            1 => {
                captured.resources.remove(0);
                "resourceMissing"
            }
            2 => {
                captured.resources[0].summary.resource_type = ResourceType::Companion;
                "resourceKindChanged"
            }
            3 => {
                captured.assignments[0].ports.clear();
                "portUnassigned"
            }
            4 => {
                captured.resources[0].summary.availability = Availability::Offline;
                "resourceUnavailable"
            }
            5 => {
                captured.resources[0].ports[0].current = false;
                "resourceUnavailable"
            }
            6 => {
                captured.resources[0].summary.connections[0]
                    .route
                    .baseline_ready = false;
                "routeNotReady"
            }
            7 => {
                captured.resources[0].tokens[0].inventory_session = "old-inventory".into();
                "routeNotReady"
            }
            _ => {
                let resource = &mut captured.resources[0];
                if let PortContract::State { freshness_ms, .. } =
                    &mut resource.ports[0].descriptor.contract
                {
                    *freshness_ms = 2000;
                }
                resource.ports[0].contract_hash =
                    resource.ports[0].descriptor.contract_hash().unwrap();
                captured.assignments[0].ports[0].expected_contract_hash =
                    resource.ports[0].contract_hash.clone();
                "portContractChanged"
            }
        };
        let report = compile(&document, &captured);
        assert!(report.plan.is_some(), "{variant}: {:?}", report.diagnostics);
        assert_eq!(report.readiness, PlanReadiness::Blocked, "{variant}");
        assert!(
            has(&report, expected_code),
            "{variant}: {:?}",
            report.diagnostics
        );
    }
}

#[test]
fn captured_route_priority_is_explicit_and_ties_do_not_choose_arbitrary_authority() {
    let document = button_graph();
    let mut captured = snapshot(&document);
    let resource = &mut captured.resources[0];
    let mut connection = resource.summary.connections[0].clone();
    connection.route.id = "preferred-usb-route".into();
    connection.route.transport = Transport::Usb;
    connection.route.priority = 20;
    let mut token = resource.tokens[0].clone();
    token.route_id = connection.route.id.clone();
    resource.ports[0]
        .route_ids
        .push(connection.route.id.clone());
    resource.summary.connections.push(connection);
    resource.tokens.push(token);
    let selected = valid(compile(&document, &captured));
    assert_eq!(
        selected.sources[0].route.as_ref().unwrap().id,
        "preferred-usb-route"
    );
    captured.resources[0].summary.connections[1].route.priority = 10;
    let ambiguous = compile(&document, &captured);
    assert!(ambiguous.plan.is_some());
    assert_eq!(ambiguous.readiness, PlanReadiness::Blocked);
    assert!(has(&ambiguous, "routeAmbiguous"));
}

#[test]
fn visibility_and_layout_capabilities_are_readiness_gates_and_capture_bounds_are_fatal() {
    let document = mixed();
    let mut captured = snapshot(&document);
    captured.backend.visibility_qualified = false;
    let hidden = compile(&document, &captured);
    assert!(hidden.plan.is_some());
    assert_eq!(hidden.readiness, PlanReadiness::Blocked);
    assert!(has(&hidden, "visibilityBackendNotReady"));
    captured.backend.visibility_qualified = true;
    captured.backend.max_targets = 1;
    let capacity = compile(&document, &captured);
    assert!(capacity.plan.is_some());
    assert_eq!(capacity.readiness, PlanReadiness::Blocked);
    assert!(has(&capacity, "virtualBackendNotReady"));
    let mut bad = snapshot(&document);
    bad.inventory_revision = "01".into();
    assert!(has(&compile(&document, &bad), "invalidCapture"));
    let mut bad = snapshot(&document);
    bad.assignments.push(bad.assignments[0].clone());
    assert!(has(&compile(&document, &bad), "invalidCapture"));
    let mut bad = snapshot(&document);
    bad.resources.push(bad.resources[0].clone());
    assert!(has(&compile(&document, &bad), "invalidCapture"));
    let mut bad = snapshot(&document);
    let duplicate = bad.assignments[0].ports[0].clone();
    bad.assignments[0].ports.push(duplicate);
    assert!(has(&compile(&document, &bad), "invalidCapture"));
    let mut bad = snapshot(&document);
    bad.assignments[0].resource_id = "another-profile-resource".into();
    assert!(has(&compile(&document, &bad), "unknownLocalAssignment"));
    let mut bad = snapshot(&document);
    bad.assignments[0].ports[0].profile_port_id = "missing".into();
    assert!(has(
        &compile(&document, &bad),
        "unknownLocalPortExpectation"
    ));
    let mut bad = snapshot(&document);
    bad.resources = vec![bad.resources[0].clone(); 65];
    assert!(has(&compile(&document, &bad), "invalidCapture"));
    let blank = crate::new_document("Blank fixture").unwrap();
    let mut empty = snapshot(&blank);
    empty.backend = Default::default();
    empty.game_context = None;
    assert_eq!(compile(&blank, &empty).readiness, PlanReadiness::Ready);
}

#[test]
fn selected_inventory_projection_preserves_total_counts_and_exposes_changed_current_contracts() {
    let mut registry = Registry::new();
    let mut ports = (0..2000)
        .map(|n| boolean(&format!("button-{n}"), Direction::Output).descriptor())
        .collect::<Vec<_>>();
    let observation = |ports: Vec<PortDescriptor>| Observation {
        key: "module".into(),
        name: "Module".into(),
        resource_type: ResourceType::OsgModule,
        evidence: vec![IdentityEvidence {
            kind: EvidenceKind::VerifiedPhysical,
            value: "mcu:fixture".into(),
        }],
        attributes: BTreeMap::new(),
        diagnostics: vec![],
        route: Some(Route {
            id: "usb-route".into(),
            transport: Transport::Usb,
            provider_generation: "boot-1".into(),
            attributes: BTreeMap::new(),
            baseline_ready: true,
            writable: true,
            priority: 20,
        }),
        ports: Some(PortUpdate::complete(ports).unwrap()),
    };
    let batch = |revision, observations| ProviderBatch {
        scope: "module-usb".into(),
        session: "provider".into(),
        revision,
        complete: true,
        observations,
    };
    registry
        .reconcile(batch(1, vec![observation(ports.clone())]))
        .unwrap();
    let id = registry.list(0, 1).unwrap().resources[0].id.clone();
    let expected = PortExpectation {
        profile_port_id: "last".into(),
        observed_port_id: ports[1999].id.clone(),
        expected_contract_hash: ports[1999].contract_hash().unwrap(),
    };
    let selected = registry
        .inspect_selected(&id, &[expected.clone(), expected.clone()])
        .unwrap();
    assert_eq!(selected.total, 2000);
    assert_eq!(selected.resource.cached_port_count, 2000);
    assert_eq!(selected.ports.len(), 1);
    assert_eq!(selected.evidence.len(), 1);
    ports[1999].contract = PortContract::State {
        schema: ScalarSchema::Boolean {},
        freshness_ms: 2000,
        fallback: Some(crate::PortValue::Boolean(false)),
    };
    registry
        .reconcile(batch(2, vec![observation(ports)]))
        .unwrap();
    let changed = registry.inspect_selected(&id, &[expected.clone()]).unwrap();
    assert!(changed.ports[0].current);
    assert_ne!(
        changed.ports[0].contract_hash,
        expected.expected_contract_hash
    );
    registry.reconcile(batch(3, vec![])).unwrap();
    assert!(
        !registry
            .inspect_selected(&id, &[expected.clone()])
            .unwrap()
            .ports[0]
            .current
    );
    assert!(registry
        .inspect_selected(&id, &vec![expected; 1025])
        .is_err());
}

#[test]
fn full_mixed_replay_routes_two_controllers_can_commands_and_typed_telemetry_without_replay() {
    use osg_mapping::engine::{
        typed::{Effect, Frame},
        Counter, TargetState,
    };
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Replay {
        schema_version: u32,
        baselines: Vec<Frame>,
        changes: Vec<Step>,
        controllers: BTreeMap<String, TargetState>,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Step {
        frame: Frame,
        setter: Option<bool>,
        commands: usize,
    }
    fn setter(effects: &[Effect], expected: Option<bool>) {
        for id in ["module-action", "companion-warning"] {
            let values = effects
                .iter()
                .filter(|effect| effect.sink_id == id)
                .filter_map(|effect| {
                    if let SignalSample::Known(SignalValue::ActionPack(pack)) = &effect.sample {
                        Some(&pack.arguments)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(
                values.len(),
                usize::from(expected.is_some()),
                "{id}: {effects:?}"
            );
            if let Some(expected) = expected {
                assert_eq!(values[0].len(), 1);
                assert_eq!(values[0][0].id, "value");
                assert_eq!(values[0][0].value, crate::PortValue::Boolean(expected));
            }
        }
    }
    let replay: Replay = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/profile-replay/mixed-routing.json"
    ))
    .unwrap();
    assert_eq!(replay.schema_version, 1);
    let mut document = mixed();
    let event = document
        .resources
        .iter()
        .find(|resource| resource.id == "third-party-a")
        .unwrap()
        .ports
        .iter()
        .find(|port| port.id == "pressed")
        .unwrap()
        .clone();
    document.resources.push(Resource {
        id: "can-command".into(),
        name: "Separate CAN command module".into(),
        kind: ResourceKind::Device,
        required: true,
        ports: vec![event],
    });
    document
        .bindings
        .iter_mut()
        .find(|binding| binding.id == "dcs-command-trigger")
        .unwrap()
        .from
        .node_id = "can-command".into();
    document.resources.push(Resource {
        id: "third-party-c".into(),
        name: "Hidden physical C".into(),
        kind: ResourceKind::Device,
        required: true,
        ports: vec![],
    });
    document.visibility.push(crate::VisibilityIntent {
        resource_id: "third-party-c".into(),
        mode: crate::VisibilityMode::HiddenWhileActive,
    });
    let report = compile(&document, &snapshot(&document));
    assert_eq!(
        report.readiness,
        PlanReadiness::Ready,
        "{:?}",
        report.diagnostics
    );
    let plan = valid(report);
    assert_eq!(plan.budgets.used_sources, 8);
    assert!(plan
        .visibility
        .iter()
        .any(|intent| intent.resource_id == "third-party-c"
            && intent.inventory_resource_id.is_some()));
    let mut runtime = osg_mapping::runtime::Runtime::default();
    let run = runtime.start_program(plan.program.clone()).unwrap();
    runtime.drain_effects();
    for frame in &replay.baselines {
        runtime.accept_signal_for(&run, frame.clone()).unwrap();
    }
    let baseline = runtime.drain_effects();
    setter(&baseline, Some(true));
    assert!(!baseline
        .iter()
        .any(|effect| effect.sink_id == "dcs-command"
            && matches!(effect.sample, SignalSample::Known(_))));
    let mut latest = BTreeMap::new();
    for record in runtime.recording.page(Counter(0), 32).unwrap().records {
        latest.insert(record.report.target, record.report.state);
    }
    assert_eq!(latest, replay.controllers);
    for step in replay.changes {
        runtime.accept_signal_for(&run, step.frame).unwrap();
        let effects = runtime.drain_effects();
        setter(&effects, step.setter);
        assert_eq!(
            effects
                .iter()
                .filter(|effect| effect.sink_id == "dcs-command"
                    && matches!(effect.sample, SignalSample::Known(_)))
                .count(),
            step.commands
        );
    }
    runtime.advance_for(&run, Counter(1_100_000)).unwrap();
    let stale = runtime.drain_effects();
    assert!(stale
        .iter()
        .any(|effect| effect.sink_id == "module-action" && effect.sample == SignalSample::Stale));
    assert_eq!(
        runtime.typed_engine().unwrap().sample(&program::Endpoint {
            node_id: "dcs".into(),
            port_id: "altitude".into()
        }),
        Some(&SignalSample::Stale)
    );
    runtime.stop();
    assert!(runtime
        .accept_signal_for(&run, replay.baselines[0].clone())
        .is_err());
    let next = runtime.start_program(plan.program).unwrap();
    assert_ne!(run, next);
    runtime.drain_effects();
    let mut event_baseline = replay.baselines.last().unwrap().clone();
    event_baseline.changes[0].sample = SignalSample::scalar(crate::PortValue::Boolean(true));
    runtime.accept_signal_for(&next, event_baseline).unwrap();
    assert!(!runtime
        .drain_effects()
        .iter()
        .any(|effect| effect.sink_id == "dcs-command"
            && matches!(effect.sample, SignalSample::Known(_))));
}
