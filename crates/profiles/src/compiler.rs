//! Pure graph compilation over one immutable native capture. This module has no
//! provider callbacks, filesystem access, controller handles or submission API.
pub use crate::compiler_models::*;
use crate::resources::{PortPreparation, ResourceToken, Route};
use crate::resources_ports::PortContract;
use crate::{Binding, Direction, Endpoint, ProfileDocument, ResourceKind};
use osg_mapping::profile::OutputControl;
use osg_mapping::program::{
    self, InputBinding, Node, Program, ResourceAdapter, Sink, SinkKind, SourcePort,
    VirtualController,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[path = "compiler_capture.rs"]
mod capture;
#[path = "compiler_conflicts.rs"]
mod conflicts;
#[path = "compiler_hid_policy.rs"]
pub mod hid_policy;

pub fn compile(document: &ProfileDocument, snapshot: &CompileSnapshot) -> CompilationReport {
    compile_mode(document, snapshot, None)
}
/// Native acquisition preflight only. Fresh delivery tokens must be captured
/// and compiled normally after acquiring the providers and their baselines.
pub fn compile_for_preparation(
    document: &ProfileDocument,
    snapshot: &CompileSnapshot,
    preparations: &[PortPreparation],
) -> CompilationReport {
    compile_mode(document, snapshot, Some(preparations))
}
fn compile_mode(
    document: &ProfileDocument,
    snapshot: &CompileSnapshot,
    preparations: Option<&[PortPreparation]>,
) -> CompilationReport {
    let mut report = CompilationReport {
        schema_version: 1,
        profile_id: document.id.clone(),
        profile_revision: document.revision,
        document_semantic_hash: None,
        program_hash: None,
        capture_fingerprint: capture::fingerprint(snapshot)
            .unwrap_or_else(|_| "invalid-capture".into()),
        readiness: PlanReadiness::Invalid,
        diagnostics: vec![],
        plan: None,
    };
    if let Some(preparations) = preparations {
        let fingerprint = (|| {
            if preparations.len() > 1024 {
                return Err("Preparation exceeds 1024 exact ports.".to_owned());
            }
            let mut pins = preparations
                .iter()
                .map(PortPreparation::fingerprint)
                .collect::<Result<Vec<_>, String>>()?;
            pins.sort();
            hash(&(report.capture_fingerprint.clone(), pins))
        })();
        match fingerprint {
            Ok(value) => report.capture_fingerprint = value,
            Err(message) => {
                error(
                    &mut report.diagnostics,
                    "invalidPreparation",
                    &message,
                    GraphLocation::default(),
                );
                return finish(report);
            }
        }
    }
    let ports = document
        .resources
        .iter()
        .map(|resource| resource.ports.len())
        .sum::<usize>()
        + document
            .processing_nodes
            .iter()
            .map(|node| node.inputs.len() + node.outputs.len())
            .sum::<usize>()
        + document
            .virtual_outputs
            .iter()
            .map(|controller| controller.ports.len())
            .sum::<usize>()
        + document.desktop_outputs.iter().map(|output| output.ports.len()).sum::<usize>();
    if document.resources.len() > crate::validation::MAX_RESOURCES
        || document.processing_nodes.len() > crate::validation::MAX_PROCESSING_NODES
        || document.bindings.len() > crate::validation::MAX_BINDINGS
        || document.virtual_outputs.len() > crate::validation::MAX_VIRTUAL_OUTPUTS
        || document.desktop_outputs.len() > 1
        || ports > crate::validation::MAX_PORTS
    {
        error(&mut report.diagnostics, "graphBudgetExceeded", "The complete graph exceeds its bounded resource, processing, binding, output or port budget; no instructions were truncated.", GraphLocation::default());
        return finish(report);
    }
    graph_diagnostics(document, &mut report.diagnostics);
    if let Err(message) = crate::validate_document(document) {
        error(
            &mut report.diagnostics,
            "invalidGraph",
            &message,
            GraphLocation::default(),
        );
    } else {
        report.document_semantic_hash = crate::semantic_hash(document).ok();
    }
    if let Err(message) = capture::validate_snapshot(snapshot) {
        error(
            &mut report.diagnostics,
            "invalidCapture",
            &message,
            GraphLocation::default(),
        );
    }
    let known: BTreeSet<_> = document.resources.iter().map(|r| r.id.as_str()).collect();
    for assignment in &snapshot.assignments {
        if !known.contains(assignment.resource_id.as_str()) {
            error(
                &mut report.diagnostics,
                "unknownLocalAssignment",
                "A local assignment references a logical resource outside this captured document.",
                resource_location(&assignment.resource_id),
            );
        } else if let Some(resource) = document
            .resources
            .iter()
            .find(|resource| resource.id == assignment.resource_id)
        {
            for expected in &assignment.ports {
                if !resource
                    .ports
                    .iter()
                    .any(|port| port.id == expected.profile_port_id)
                {
                    error(
                        &mut report.diagnostics,
                        "unknownLocalPortExpectation",
                        "A local expectation references a port outside its exact logical resource.",
                        GraphLocation {
                            resource_id: Some(resource.id.clone()),
                            port_id: Some(expected.profile_port_id.clone()),
                            ..Default::default()
                        },
                    );
                }
            }
        }
    }
    if document.virtual_outputs.len() > MAX_VIRTUAL_TARGETS {
        error(&mut report.diagnostics, "virtualTargetLimit", "This execution backend supports at most eight logical virtual controllers; remove or combine explicit layouts.", GraphLocation::default());
    }
    if has_errors(&report) {
        return finish(report);
    }

    let mut bindings = document.bindings.iter().collect::<Vec<_>>();
    bindings.sort_by(|a, b| a.id.cmp(&b.id));
    let mut nodes = document.processing_nodes.iter().collect::<Vec<_>>();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    let mut runtime_nodes = Vec::new();
    for node in &nodes {
        let mut inputs = Vec::new();
        let mut declared = node.inputs.iter().collect::<Vec<_>>();
        declared.sort_by(|a, b| a.id.cmp(&b.id));
        for port in declared {
            let endpoint = Endpoint {
                node_id: node.id.clone(),
                port_id: port.id.clone(),
            };
            if let Some(binding) = bindings.iter().find(|binding| binding.to == endpoint) {
                inputs.push(InputBinding {
                    port_id: port.id.clone(),
                    from: runtime_endpoint(&binding.from),
                    contract: port.contract.clone(),
                    edge_id: binding.id.clone(),
                });
            } else {
                error(&mut report.diagnostics, "inputUnbound", "Connect every declared processing input to an exact output; constants must be explicit nodes.", GraphLocation { node_id: Some(node.id.clone()), port_id: Some(port.id.clone()), ..Default::default() });
            }
        }
        let mut outputs = node
            .outputs
            .iter()
            .map(|port| {
                let mut descriptor = port.descriptor();
                descriptor.name = descriptor.id.clone();
                descriptor
            })
            .collect::<Vec<_>>();
        outputs.sort_by(|a, b| a.id.cmp(&b.id));
        runtime_nodes.push(Node {
            id: node.id.clone(),
            inputs,
            outputs,
            operation: node.operation.clone(),
        });
    }

    let mut source_plans = Vec::new();
    let mut runtime_sources = Vec::new();
    let mut resources = document.resources.iter().collect::<Vec<_>>();
    resources.sort_by(|a, b| a.id.cmp(&b.id));
    for resource in &resources {
        let mut ports = resource
            .ports
            .iter()
            .filter(|port| {
                port.direction == Direction::Output
                    && bindings.iter().any(|binding| {
                        binding.from.node_id == resource.id && binding.from.port_id == port.id
                    })
            })
            .collect::<Vec<_>>();
        ports.sort_by(|a, b| a.id.cmp(&b.id));
        for port in ports {
            let endpoint = Endpoint {
                node_id: resource.id.clone(),
                port_id: port.id.clone(),
            };
            let edges = related_edges(&endpoint, &bindings);
            let resolved = capture::resolve(
                resource,
                port,
                snapshot,
                &mut report.diagnostics,
                edges,
                preparations,
                document.companion_assignments.iter().find(|a| a.resource_id == resource.id).map(|a| a.publication_hash.as_str()),
            );
            runtime_sources.push(SourcePort {
                endpoint: runtime_endpoint(&endpoint),
                contract: port.contract.clone(),
            });
            source_plans.push(SourcePlan {
                endpoint,
                inventory_resource_id: resolved.inventory_id,
                observed_port_id: resolved.observed_id,
                contract: port.contract.clone(),
                token: resolved.token,
                preparation: resolved.preparation,
                route: resolved.route,
                readiness: resolved.readiness,
            });
        }
    }
    let used_sources = source_plans
        .iter()
        .map(|source| &source.endpoint.node_id)
        .collect::<BTreeSet<_>>()
        .len();
    if used_sources > MAX_USED_SOURCES {
        error(&mut report.diagnostics, "sourceResourceLimit", "This runtime supports at most 16 used source resources; unused seeded resource blocks do not count.", GraphLocation::default());
    }

    let mut sink_plans = Vec::new();
    let mut runtime_sinks = Vec::new();
    for binding in &bindings {
        if let Some(resource) = resources
            .iter()
            .find(|resource| resource.id == binding.to.node_id)
        {
            let port = resource
                .ports
                .iter()
                .find(|port| port.id == binding.to.port_id)
                .unwrap();
            let resolved = capture::resolve(
                resource,
                port,
                snapshot,
                &mut report.diagnostics,
                vec![binding.id.clone()],
                preparations,
                document.companion_assignments.iter().find(|a| a.resource_id == resource.id).map(|a| a.publication_hash.as_str()),
            );
            let (adapter, runtime_adapter) = match resource.kind {
                ResourceKind::Game => (
                    SinkAdapter::SimulatorCommand,
                    ResourceAdapter::SimulatorCommand,
                ),
                ResourceKind::Companion => (
                    SinkAdapter::CompanionFeedback,
                    ResourceAdapter::CompanionFeedback,
                ),
                ResourceKind::Device if matches!(port.contract, PortContract::Action { .. }) => {
                    (SinkAdapter::PhysicalAction, ResourceAdapter::DeviceAction)
                }
                ResourceKind::Device => (SinkAdapter::PhysicalState, ResourceAdapter::DeviceAction),
            };
            runtime_sinks.push(Sink {
                id: binding.id.clone(),
                endpoint: runtime_endpoint(&binding.to),
                from: runtime_endpoint(&binding.from),
                contract: port.contract.clone(),
                kind: SinkKind::Resource {
                    adapter: runtime_adapter,
                },
            });
            sink_plans.push(SinkPlan {
                endpoint: binding.to.clone(),
                adapter,
                inventory_resource_id: resolved.inventory_id,
                observed_port_id: resolved.observed_id,
                contract: port.contract.clone(),
                token: resolved.token,
                preparation: resolved.preparation,
                route: resolved.route,
                readiness: resolved.readiness,
            });
        } else if let Some(desktop) = document.desktop_outputs.iter().find(|output| output.id == binding.to.node_id) {
            let port = desktop.ports.iter().find(|port| port.id == binding.to.port_id).unwrap();
            let definition = desktop.actions.iter().find(|action| action.port_id == port.id).unwrap();
            let readiness = if snapshot.backend.desktop_available && snapshot.backend.desktop_qualified && snapshot.backend.desktop_enabled { PlanReadiness::Ready } else { PlanReadiness::Blocked };
            runtime_sinks.push(Sink { id: binding.id.clone(), endpoint: runtime_endpoint(&binding.to), from: runtime_endpoint(&binding.from), contract: port.contract.clone(), kind: SinkKind::Desktop { action: definition.action.clone() } });
            sink_plans.push(SinkPlan { endpoint: binding.to.clone(), adapter: SinkAdapter::DesktopInput, inventory_resource_id: None, observed_port_id: None, contract: port.contract.clone(), token: None, preparation: None, route: None, readiness });
            if readiness == PlanReadiness::Blocked {
                report.diagnostics.push(CompileDiagnostic { code: "desktopBackendNotReady".into(), severity: DiagnosticSeverity::Warning, message: "Desktop input requires separate native availability, local enablement and qualification. Recording preview grants no input authority.".into(), location: GraphLocation { node_id: Some(desktop.id.clone()), port_id: Some(port.id.clone()), edge_ids: vec![binding.id.clone()], ..Default::default() } });
            }
        } else if let Some(controller) = document
            .virtual_outputs
            .iter()
            .find(|controller| controller.id == binding.to.node_id)
        {
            let port = controller
                .ports
                .iter()
                .find(|port| port.id == binding.to.port_id)
                .unwrap();
            let control = virtual_control(&controller.layout, &port.id).unwrap();
            runtime_sinks.push(Sink {
                id: binding.id.clone(),
                endpoint: runtime_endpoint(&binding.to),
                from: runtime_endpoint(&binding.from),
                contract: port.contract.clone(),
                kind: SinkKind::Virtual {
                    controller_id: controller.id.clone(),
                    control,
                },
            });
            sink_plans.push(SinkPlan {
                endpoint: binding.to.clone(),
                adapter: SinkAdapter::VirtualController,
                inventory_resource_id: None,
                observed_port_id: None,
                contract: port.contract.clone(),
                token: None,
                preparation: None,
                route: None,
                readiness: controller_readiness(
                    controller,
                    snapshot,
                    document.virtual_outputs.len(),
                ),
            });
        }
    }
    let mut controllers = document.virtual_outputs.iter().collect::<Vec<_>>();
    controllers.sort_by(|a, b| a.id.cmp(&b.id));
    let mut program = Program {
        sources: runtime_sources,
        nodes: runtime_nodes,
        sinks: runtime_sinks,
        controllers: controllers
            .iter()
            .map(|controller| VirtualController {
                id: controller.id.clone(),
                layout: controller.layout.clone(),
            })
            .collect(),
    };
    for source in &mut program.sources {
        canonical_contract_labels(&mut source.contract);
    }
    for node in &mut program.nodes {
        for input in &mut node.inputs {
            canonical_contract_labels(&mut input.contract);
        }
        for output in &mut node.outputs {
            canonical_contract_labels(&mut output.contract);
        }
    }
    for sink in &mut program.sinks {
        canonical_contract_labels(&mut sink.contract);
    }
    hid_policy::apply(document, snapshot, &source_plans, &mut sink_plans, preparations.is_some(), &mut report.diagnostics);
    conflicts::check(
        document,
        snapshot,
        &source_plans,
        &sink_plans,
        &mut report.diagnostics,
    );
    if has_errors(&report) {
        return finish(report);
    }
    let compiled = match program.compile() {
        Ok(compiled) => compiled,
        Err(message) => {
            error(
                &mut report.diagnostics,
                "invalidExecutionProgram",
                &message,
                GraphLocation::default(),
            );
            return finish(report);
        }
    };
    let topological_order = compiled
        .order
        .iter()
        .map(|index| program.nodes[*index].id.clone())
        .collect();
    let trace = nodes
        .iter()
        .map(|node| {
            let mut inputs = node
                .inputs
                .iter()
                .map(|port| port.id.clone())
                .collect::<Vec<_>>();
            inputs.sort();
            let mut outputs = node
                .outputs
                .iter()
                .map(|port| port.id.clone())
                .collect::<Vec<_>>();
            outputs.sort();
            InstructionTrace {
                node_id: node.id.clone(),
                input_ports: inputs,
                output_ports: outputs,
                incoming_edge_ids: bindings
                    .iter()
                    .filter(|edge| edge.to.node_id == node.id)
                    .map(|edge| edge.id.clone())
                    .collect(),
                outgoing_edge_ids: bindings
                    .iter()
                    .filter(|edge| edge.from.node_id == node.id)
                    .map(|edge| edge.id.clone())
                    .collect(),
            }
        })
        .collect();
    let mut branches = branches(document, &bindings, &source_plans, &sink_plans);
    for branch in &mut branches {
        let desktop = sink_plans.iter().any(|sink| sink.endpoint == branch.sink && sink.adapter == SinkAdapter::DesktopInput);
        let scripts = branch.processing_nodes.iter().filter(|id| document.processing_nodes.iter().any(|node| node.id == **id && matches!(node.operation, program::Operation::Script { .. }))).cloned().collect::<Vec<_>>();
        if desktop && !scripts.is_empty() {
            branch.readiness = PlanReadiness::Blocked;
            if let Some(sink) = sink_plans.iter_mut().find(|sink| sink.endpoint == branch.sink) { sink.readiness = PlanReadiness::Blocked; }
            report.diagnostics.push(CompileDiagnostic { code: "desktopScriptUnqualified".into(), severity: DiagnosticSeverity::Warning,
                message: format!("Desktop output transitively depends on an unqualified live script: {}. Use visible built-in processing for this branch.", scripts.join(", ")),
                location: GraphLocation { node_id: Some(branch.sink.node_id.clone()), port_id: Some(branch.sink.port_id.clone()), edge_ids: branch.edge_ids.clone(), ..Default::default() } });
        }
    }
    let controller_summaries = controllers.iter().map(|controller| {
        let readiness = controller_readiness(controller, snapshot, document.virtual_outputs.len());
        if readiness == PlanReadiness::Blocked {
            report.diagnostics.push(CompileDiagnostic { code: "virtualBackendNotReady".into(), severity: DiagnosticSeverity::Warning,
                message: "The explicit controller layout is retained, but its captured backend is unavailable, unqualified or lacks capacity; compilation grants no output authority.".into(), location: GraphLocation { node_id: Some(controller.id.clone()), ..Default::default() } });
        }
        ControllerSummary { id: controller.id.clone(), name: controller.name.clone(), buttons: controller.layout.buttons.len(), axes: controller.layout.axes.len(), hats: controller.layout.hats.len(),
            connected_ports: bindings.iter().filter(|edge| edge.to.node_id == controller.id).map(|edge| edge.to.port_id.clone()).collect(), readiness }
    }).collect::<Vec<_>>();
    let mut visibility = document
        .visibility
        .iter()
        .map(|intent| {
            let resource = capture::captured(&intent.resource_id, snapshot);
            VisibilitySummary {
                resource_id: intent.resource_id.clone(),
                inventory_resource_id: resource.map(|resource| resource.summary.id.clone()),
                mode: intent.mode,
                readiness: if intent.mode == crate::VisibilityMode::Visible
                    || (snapshot.backend.visibility_available
                        && snapshot.backend.visibility_qualified
                        && resource.is_some_and(|r| {
                            r.summary.availability == crate::resources::Availability::Online
                        })) {
                    PlanReadiness::Ready
                } else {
                    PlanReadiness::Blocked
                },
            }
        })
        .collect::<Vec<_>>();
    visibility.sort_by(|a, b| a.resource_id.cmp(&b.resource_id));
    for intent in visibility
        .iter()
        .filter(|intent| intent.readiness == PlanReadiness::Blocked)
    {
        report.diagnostics.push(CompileDiagnostic { code: "visibilityBackendNotReady".into(), severity: DiagnosticSeverity::Warning,
            message: "Hiding requires a current physical resource and an available, qualified visibility backend; this preview did not change device visibility.".into(),
            location: resource_location(&intent.resource_id) });
    }
    if snapshot.read_only {
        report.diagnostics.push(CompileDiagnostic { code: "inventoryReadOnly".into(), severity: DiagnosticSeverity::Warning,
            message: "Captured inventory persistence is read-only; this preview cannot authorize activation.".into(), location: GraphLocation::default() });
    }
    for message in &snapshot.inventory_diagnostics {
        report.diagnostics.push(CompileDiagnostic {
            code: "inventoryDiagnostic".into(),
            severity: DiagnosticSeverity::Info,
            message: message.clone(),
            location: GraphLocation::default(),
        });
    }
    let disconnected_desktop = sink_plans.is_empty() && !document.desktop_outputs.is_empty();
    if disconnected_desktop {
        report.diagnostics.push(CompileDiagnostic { code: "noExecutableOutput".into(), severity: DiagnosticSeverity::Warning,
            message: "Unconnected keyboard and mouse bindings are inactive.".into(), location: GraphLocation::default() });
    }
    report.readiness = if snapshot.read_only
        || branches
            .iter()
            .any(|branch| branch.required && branch.readiness == PlanReadiness::Blocked)
        || controller_summaries.iter().any(|controller| {
            controller.readiness == PlanReadiness::Blocked
                && document
                    .virtual_outputs
                    .iter()
                    .any(|v| v.id == controller.id && v.required)
        })
        || visibility.iter().any(|intent| {
            intent.readiness == PlanReadiness::Blocked
                && resources
                    .iter()
                    .any(|r| r.id == intent.resource_id && r.required)
        }) {
        PlanReadiness::Blocked
    } else if !branches.is_empty()
        && branches
            .iter()
            .all(|branch| branch.readiness == PlanReadiness::Dormant)
    {
        PlanReadiness::Dormant
    } else {
        PlanReadiness::Ready
    };
    report.program_hash = portable_plan_hash(document, &program).ok();
    let state_slots = program
        .sources
        .iter()
        .map(|source| &source.contract)
        .chain(program.nodes.iter().flat_map(|node| {
            node.inputs
                .iter()
                .map(|port| &port.contract)
                .chain(node.outputs.iter().map(|port| &port.contract))
        }))
        .chain(program.sinks.iter().map(|sink| &sink.contract))
        .filter(|contract| matches!(contract, PortContract::State { .. }))
        .count();
    let budgets = PlanBudgets {
        used_sources,
        virtual_targets: program.controllers.len(),
        source_ports: program.sources.len(),
        processing_nodes: program.nodes.len(),
        sink_ports: program.sinks.len(),
        bindings: bindings.len(),
        state_slots,
        event_queue_capacity: 4096,
        action_queue_capacity: 4096,
        timer_capacity: compiled.timer_reserve,
    };
    let mut runtime_overrides = document.overrides.clone();
    runtime_overrides.sort_by(|a, b| a.resource_id.cmp(&b.resource_id));
    let mut companion_assignments = document.companion_assignments.clone();
    companion_assignments.sort_by(|a, b| a.resource_id.cmp(&b.resource_id));
    report.plan = Some(ExecutionPlan {
        requirements: ExecutionRequirements {
            desktop_input: program.sinks.iter().any(|sink| matches!(sink.kind, SinkKind::Desktop { .. })),
            virtual_devices: !program.controllers.is_empty(),
            visibility: document.visibility.iter().any(|intent| intent.mode == crate::VisibilityMode::HiddenWhileActive),
        },
        program,
        topological_order,
        trace,
        sources: source_plans,
        sinks: sink_plans,
        branches,
        controllers: controller_summaries,
        visibility,
        runtime_overrides,
        companion_assignments,
        budgets,
    });
    finish(report)
}

fn hash(value: &impl Serialize) -> Result<String, String> {
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|error| error.to_string())?)
    ))
}
fn canonical_contract_labels(contract: &mut PortContract) {
    if let PortContract::Action { parameters, .. } | PortContract::Invocation { parameters, .. } =
        contract
    {
        for parameter in parameters {
            parameter.name = parameter.id.clone();
        }
    }
}
fn runtime_endpoint(endpoint: &Endpoint) -> program::Endpoint {
    program::Endpoint {
        node_id: endpoint.node_id.clone(),
        port_id: endpoint.port_id.clone(),
    }
}
fn key(endpoint: &Endpoint) -> (String, String) {
    (endpoint.node_id.clone(), endpoint.port_id.clone())
}
fn resource_location(id: &str) -> GraphLocation {
    GraphLocation {
        resource_id: Some(id.into()),
        ..Default::default()
    }
}
fn error(
    diagnostics: &mut Vec<CompileDiagnostic>,
    code: &str,
    message: &str,
    location: GraphLocation,
) {
    diagnostics.push(CompileDiagnostic {
        code: code.into(),
        severity: DiagnosticSeverity::Error,
        message: message.into(),
        location,
    });
}
fn has_errors(report: &CompilationReport) -> bool {
    report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
}
fn finish(mut report: CompilationReport) -> CompilationReport {
    report.diagnostics.sort_by(|a, b| {
        (
            &a.code,
            &a.location.resource_id,
            &a.location.node_id,
            &a.location.port_id,
            &a.location.edge_ids,
            &a.message,
        )
            .cmp(&(
                &b.code,
                &b.location.resource_id,
                &b.location.node_id,
                &b.location.port_id,
                &b.location.edge_ids,
                &b.message,
            ))
    });
    report
}
fn related_edges(endpoint: &Endpoint, bindings: &[&Binding]) -> Vec<String> {
    bindings
        .iter()
        .filter(|edge| edge.from == *endpoint || edge.to == *endpoint)
        .map(|edge| edge.id.clone())
        .collect()
}
fn virtual_control(layout: &crate::ControllerLayout, id: &str) -> Option<OutputControl> {
    layout
        .buttons
        .iter()
        .position(|port| port == id)
        .map(|index| OutputControl::Button {
            index: index as u16,
        })
        .or_else(|| {
            layout
                .axes
                .iter()
                .position(|axis| axis.port_id == id)
                .map(|index| OutputControl::Axis {
                    index: index as u16,
                })
        })
        .or_else(|| {
            layout
                .hats
                .iter()
                .position(|port| port == id)
                .map(|index| OutputControl::Hat {
                    index: index as u16,
                })
        })
}
fn controller_readiness(
    controller: &crate::VirtualOutput,
    snapshot: &CompileSnapshot,
    target_count: usize,
) -> PlanReadiness {
    let backend = &snapshot.backend;
    if backend.available
        && backend.qualified
        && snapshot.backend.max_targets >= target_count
        && controller.layout.buttons.len() <= backend.max_buttons as usize
        && controller.layout.axes.len() <= backend.max_axes as usize
        && controller.layout.hats.len() <= backend.max_hats as usize
    {
        PlanReadiness::Ready
    } else {
        PlanReadiness::Blocked
    }
}

fn portable_plan_hash(document: &ProfileDocument, program: &Program) -> Result<String, String> {
    let mut policy = document.clone();
    policy.resources.sort_by(|a, b| a.id.cmp(&b.id));
    policy.eligibility.game_ids.sort();
    policy
        .visibility
        .sort_by(|a, b| a.resource_id.cmp(&b.resource_id));
    policy
        .overrides
        .sort_by(|a, b| a.resource_id.cmp(&b.resource_id));
    policy
        .companion_assignments
        .sort_by(|a, b| a.resource_id.cmp(&b.resource_id));
    let required = policy
        .resources
        .iter()
        .map(|r| (&r.id, r.required))
        .chain(policy.virtual_outputs.iter().map(|v| (&v.id, v.required)))
        .chain(policy.desktop_outputs.iter().map(|v| (&v.id, v.required)))
        .collect::<BTreeMap<_, _>>();
    hash(&(
        program,
        required,
        policy.eligibility,
        policy.visibility,
        policy.overrides,
        policy.companion_assignments,
    ))
}

fn graph_diagnostics(document: &ProfileDocument, diagnostics: &mut Vec<CompileDiagnostic>) {
    let ports = document
        .resources
        .iter()
        .map(|node| (&node.id, node.ports.iter().collect::<Vec<_>>()))
        .chain(
            document
                .processing_nodes
                .iter()
                .map(|node| (&node.id, node.inputs.iter().chain(&node.outputs).collect())),
        )
        .chain(
            document
                .virtual_outputs
                .iter()
                .map(|node| (&node.id, node.ports.iter().collect())),
        )
        .chain(document.desktop_outputs.iter().map(|node| (&node.id, node.ports.iter().collect())))
        .collect::<BTreeMap<_, _>>();
    let mut targets = BTreeMap::<(String, String), Vec<String>>::new();
    for binding in &document.bindings {
        for endpoint in [&binding.from, &binding.to] {
            if ports
                .get(&endpoint.node_id)
                .is_none_or(|ports| !ports.iter().any(|port| port.id == endpoint.port_id))
            {
                error(
                    diagnostics,
                    "missingPort",
                    "This binding references a missing exact node or port.",
                    GraphLocation {
                        node_id: Some(endpoint.node_id.clone()),
                        port_id: Some(endpoint.port_id.clone()),
                        edge_ids: vec![binding.id.clone()],
                        ..Default::default()
                    },
                );
            }
        }
        let from = ports
            .get(&binding.from.node_id)
            .and_then(|ports| ports.iter().find(|port| port.id == binding.from.port_id));
        let to = ports
            .get(&binding.to.node_id)
            .and_then(|ports| ports.iter().find(|port| port.id == binding.to.port_id));
        if let (Some(from), Some(to)) = (from, to) {
            if from.direction != Direction::Output
                || to.direction != Direction::Input
                || !from.contract.can_bind_to(&to.contract)
            {
                error(diagnostics, "bindingContract", "This exact binding needs matching output/input direction and type, delivery, unit, range and action arguments; add an explicit compatible converter.", GraphLocation { node_id: Some(binding.to.node_id.clone()), port_id: Some(binding.to.port_id.clone()), edge_ids: vec![binding.id.clone()], ..Default::default() });
            }
        }
        targets
            .entry(key(&binding.to))
            .or_default()
            .push(binding.id.clone());
    }
    for node in &document.processing_nodes {
        if let Err(message) = program::validate_operation(
            &node.operation,
            &node
                .inputs
                .iter()
                .map(crate::Port::descriptor)
                .collect::<Vec<_>>(),
            &node
                .outputs
                .iter()
                .map(crate::Port::descriptor)
                .collect::<Vec<_>>(),
        ) {
            let mut edge_ids = document
                .bindings
                .iter()
                .filter(|edge| edge.from.node_id == node.id || edge.to.node_id == node.id)
                .map(|edge| edge.id.clone())
                .collect::<Vec<_>>();
            edge_ids.sort();
            error(
                diagnostics,
                "invalidOperation",
                &message,
                GraphLocation {
                    node_id: Some(node.id.clone()),
                    edge_ids,
                    ..Default::default()
                },
            );
        }
    }
    for ((node, port), mut edges) in targets {
        if edges.len() > 1 {
            edges.sort();
            error(diagnostics, "ambiguousWriters", "Multiple writers target one input; add an explicit OR or axis/hat priority processor.", GraphLocation { node_id: Some(node), port_id: Some(port), edge_ids: edges, ..Default::default() });
        }
    }
    let ids = document
        .processing_nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut degree = ids
        .iter()
        .map(|id| (*id, 0usize))
        .collect::<BTreeMap<_, _>>();
    let mut edges = BTreeMap::<&str, Vec<&str>>::new();
    for binding in &document.bindings {
        if ids.contains(binding.from.node_id.as_str()) && ids.contains(binding.to.node_id.as_str())
        {
            edges
                .entry(&binding.from.node_id)
                .or_default()
                .push(&binding.to.node_id);
            *degree.get_mut(binding.to.node_id.as_str()).unwrap() += 1;
        }
    }
    let mut pending = degree
        .iter()
        .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
        .collect::<BTreeSet<_>>();
    while let Some(id) = pending.pop_first() {
        if let Some(targets) = edges.get(id) {
            for target in targets {
                let count = degree.get_mut(target).unwrap();
                *count -= 1;
                if *count == 0 {
                    pending.insert(target);
                }
            }
        }
    }
    for (id, _) in degree.iter().filter(|(_, degree)| **degree > 0) {
        let mut edge_ids = document
            .bindings
            .iter()
            .filter(|edge| edge.from.node_id == *id || edge.to.node_id == *id)
            .map(|edge| edge.id.clone())
            .collect::<Vec<_>>();
        edge_ids.sort();
        error(diagnostics, "internalCycle", "This processing dependency is cyclic; external command/telemetry ports remain independent.", GraphLocation { node_id: Some((*id).into()), edge_ids, ..Default::default() });
    }
}

fn branches(
    document: &ProfileDocument,
    bindings: &[&Binding],
    sources: &[SourcePlan],
    sinks: &[SinkPlan],
) -> Vec<BranchPlan> {
    let source_index = sources
        .iter()
        .map(|source| (key(&source.endpoint), source))
        .collect::<BTreeMap<_, _>>();
    let mut incoming = BTreeMap::<(String, String), Vec<&Binding>>::new();
    for edge in bindings {
        incoming.entry(key(&edge.to)).or_default().push(edge);
    }
    let processors = document
        .processing_nodes
        .iter()
        .flat_map(|node| {
            node.outputs
                .iter()
                .map(move |port| ((node.id.clone(), port.id.clone()), node))
        })
        .collect::<BTreeMap<_, _>>();
    let mut result = Vec::new();
    for sink in sinks {
        let mut pending = vec![sink.endpoint.clone()];
        let mut visited = BTreeSet::new();
        let mut source_ports = BTreeMap::new();
        let mut processing = BTreeSet::new();
        let mut edge_ids = BTreeSet::new();
        while let Some(endpoint) = pending.pop() {
            let endpoint_key = key(&endpoint);
            if !visited.insert(endpoint_key.clone()) {
                continue;
            }
            if let Some(source) = source_index.get(&endpoint_key) {
                source_ports.insert(endpoint_key.clone(), *source);
            }
            if let Some(edges) = incoming.get(&endpoint_key) {
                for edge in edges {
                    edge_ids.insert(edge.id.clone());
                    pending.push(edge.from.clone());
                }
            }
            if let Some(node) = processors.get(&endpoint_key) {
                processing.insert(node.id.clone());
                pending.extend(node.inputs.iter().map(|port| Endpoint {
                    node_id: node.id.clone(),
                    port_id: port.id.clone(),
                }));
            }
        }
        let required = document.resources.iter().any(|resource| {
            resource.required
                && (resource.id == sink.endpoint.node_id
                    || source_ports
                        .values()
                        .any(|source| source.endpoint.node_id == resource.id))
        }) || document
            .virtual_outputs
            .iter()
            .any(|controller| controller.id == sink.endpoint.node_id && controller.required)
            || document.desktop_outputs.iter().any(|output| output.id == sink.endpoint.node_id && output.required);
        let states = std::iter::once(sink.readiness)
            .chain(source_ports.values().map(|source| source.readiness))
            .collect::<Vec<_>>();
        let readiness = if states.contains(&PlanReadiness::Dormant) {
            PlanReadiness::Dormant
        } else if states.contains(&PlanReadiness::Blocked) {
            PlanReadiness::Blocked
        } else {
            PlanReadiness::Ready
        };
        result.push(BranchPlan {
            sink: sink.endpoint.clone(),
            source_ports: source_ports
                .into_values()
                .map(|source| source.endpoint.clone())
                .collect(),
            processing_nodes: processing.into_iter().collect(),
            edge_ids: edge_ids.into_iter().collect(),
            required,
            readiness,
        });
    }
    result.sort_by_key(|branch| key(&branch.sink));
    result
}

#[cfg(test)]
#[path = "compiler_tests.rs"]
mod tests;
