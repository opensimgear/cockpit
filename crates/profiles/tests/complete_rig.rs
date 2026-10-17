//! Assembled portable evidence: actual Registry -> compiler -> typed reducer.
//! Nothing here opens a driver, hides a device, launches a game or acknowledges an output.
use osg_mapping::engine::{
    typed::{Effect, Frame},
    Counter, Reason, TargetState,
};
use osg_mapping::program::{Endpoint, SignalSample, SignalValue};
use osg_mapping::runtime::Runtime;
use osg_profiles::compiler::{
    compile, BackendCapabilities, CapturedResource, CompilationReport, CompileSnapshot,
    ExecutionPlan, PlanReadiness, VerifiedGameContext,
};
use osg_profiles::resources::*;
use osg_profiles::resources_ports::{PortContract, PortUpdate};
use osg_profiles::session::AppliedSnapshot;
use osg_profiles::store::LocalAssignment;
use osg_profiles::{
    default_document, semantic_hash, Direction, PortValue, ProfileDocument, ResourceKind,
    VisibilityMode, DEFAULT_PROFILE_ID,
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const DOCUMENT: &[u8] = include_bytes!("../../../tests/fixtures/profiles/complete-rig.json");
const REPLAY: &[u8] = include_bytes!("../../../tests/fixtures/profiles/complete-rig-replay.json");
const SETUP: &str = "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Replay {
    schema_version: u32,
    evidence: String,
    document_sha256: String,
    route_variants: Vec<Variant>,
    baselines: Vec<Frame>,
    baseline: Baseline,
    changes: Vec<Step>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Variant {
    id: String,
    direct_usb: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Baseline {
    controllers: BTreeMap<String, TargetState>,
    samples: Vec<ExpectedSample>,
    requests: Vec<ExpectedRequest>,
    command_count: usize,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExpectedSample {
    node_id: String,
    port_id: String,
    sample: SignalSample,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExpectedRequest {
    edge_id: String,
    sample: SignalSample,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Step {
    frame: Frame,
    requests: Vec<ExpectedRequest>,
    command_count: usize,
}

fn document() -> ProfileDocument {
    ProfileDocument::parse(DOCUMENT).unwrap()
}
fn replay() -> Replay {
    let replay: Replay = serde_json::from_slice(REPLAY).unwrap();
    assert_eq!(replay.schema_version, 1);
    // Git's Windows checkout may use CRLF; pin source content consistently
    // across the three portable CI platforms without weakening graph checks.
    let canonical = std::str::from_utf8(DOCUMENT).unwrap().replace("\r\n", "\n");
    assert_eq!(
        replay.document_sha256,
        format!("sha256:{:x}", Sha256::digest(canonical.as_bytes())),
        "The replay must pin the exact repository graph."
    );
    assert!(replay
        .evidence
        .contains("no driver, game, display or HidHide acknowledgement"));
    replay
}
fn access(port: &osg_profiles::Port) -> Access {
    if port.direction == Direction::Output {
        Access::Read
    } else if matches!(port.contract, PortContract::Action { .. }) {
        Access::Invoke
    } else {
        Access::Write
    }
}

struct Rig {
    registry: Registry,
    batch: ProviderBatch,
    ids: BTreeMap<String, String>,
    assignments: Vec<LocalAssignment>,
}
impl Rig {
    fn new(document: &ProfileDocument, direct_usb: bool) -> Self {
        let mut observations = vec![];
        for resource in &document.resources {
            let osg = resource.id.starts_with("can-") || resource.id == "direct-usb";
            let (resource_type, evidence, transport) = match resource.kind {
                ResourceKind::Device if osg => (
                    ResourceType::OsgModule,
                    EvidenceKind::VerifiedPhysical,
                    Transport::Can,
                ),
                ResourceKind::Device => (
                    ResourceType::HostController,
                    EvidenceKind::LocalPath,
                    Transport::Hid,
                ),
                ResourceKind::Companion => (
                    ResourceType::Companion,
                    EvidenceKind::CompanionInstance,
                    Transport::Network,
                ),
                ResourceKind::Game => (
                    ResourceType::GameIntegration,
                    EvidenceKind::GameScope,
                    Transport::Integration,
                ),
            };
            let attributes = BTreeMap::from([("logicalFixtureId".into(), resource.id.clone())]);
            let route = Route {
                id: format!("{}-route", resource.id),
                transport,
                provider_generation: "fixture-boot-1".into(),
                attributes: if osg {
                    BTreeMap::from([
                        ("gateway".into(), "fixture-can-gateway".into()),
                        ("boot".into(), "fixture-boot-1".into()),
                    ])
                } else {
                    BTreeMap::new()
                },
                baseline_ready: true,
                writable: true,
                priority: 10,
            };
            let observed = Observation {
                key: resource.id.clone(),
                name: resource.name.clone(),
                resource_type,
                evidence: vec![IdentityEvidence {
                    kind: evidence,
                    value: format!("fixture:{}", resource.id),
                }],
                attributes,
                diagnostics: vec![],
                route: Some(route),
                ports: Some(
                    PortUpdate::complete(resource.ports.iter().map(|p| p.descriptor()).collect())
                        .unwrap(),
                ),
            };
            if direct_usb && resource.id == "direct-usb" {
                let mut usb = observed.clone();
                usb.key.push_str("-cdc");
                usb.route = Some(Route {
                    id: "direct-usb-cdc-route".into(),
                    transport: Transport::Usb,
                    attributes: BTreeMap::from([
                        ("port".into(), "fixture-selected-cdc".into()),
                        ("boot".into(), "fixture-boot-1".into()),
                    ]),
                    baseline_ready: true,
                    writable: true,
                    priority: 100,
                    ..observed.route.as_ref().unwrap().clone()
                });
                observations.push(usb);
            }
            observations.push(observed);
        }
        let batch = ProviderBatch {
            scope: "complete-rig-fixture".into(),
            session: "fixture-provider-session-1".into(),
            revision: 1,
            complete: true,
            observations,
        };
        let mut registry = Registry::new();
        registry.reconcile(batch.clone()).unwrap();
        let summaries = registry.list(0, 128).unwrap();
        assert_eq!(
            summaries.total,
            document.resources.len(),
            "USB/CAN is one proven physical resource, not two graph nodes."
        );
        let ids: BTreeMap<_, _> = summaries
            .resources
            .into_iter()
            .map(|r| (r.attributes["logicalFixtureId"].clone(), r.id))
            .collect();
        let assignments = document
            .resources
            .iter()
            .map(|logical| {
                let source_id = ids[&logical.id].clone();
                let observed = registry.inspect(&source_id, 0, 128).unwrap();
                LocalAssignment {
                    resource_id: logical.id.clone(),
                    source_id,
                    ports: logical
                        .ports
                        .iter()
                        .map(|port| {
                            let selected = observed
                                .ports
                                .iter()
                                .find(|p| p.descriptor.id == port.id)
                                .unwrap();
                            PortExpectation {
                                profile_port_id: port.id.clone(),
                                observed_port_id: selected.descriptor.id.clone(),
                                expected_contract_hash: selected.contract_hash.clone(),
                            }
                        })
                        .collect(),
                }
            })
            .collect();
        Self {
            registry,
            batch,
            ids,
            assignments,
        }
    }
    fn capture(&self, document: &ProfileDocument) -> CompileSnapshot {
        let page = self.registry.list(0, 128).unwrap();
        let resources = document
            .resources
            .iter()
            .map(|logical| {
                let assignment = self
                    .assignments
                    .iter()
                    .find(|a| a.resource_id == logical.id)
                    .unwrap();
                let details = self
                    .registry
                    .inspect_selected(&assignment.source_id, &assignment.ports)
                    .unwrap();
                CapturedResource {
                    summary: details.resource,
                    ports: details.ports,
                    tokens: logical
                        .ports
                        .iter()
                        .filter_map(|p| {
                            self.registry
                                .token(&assignment.source_id, &p.id, access(p))
                                .ok()
                        })
                        .collect(),
                    provider_setup_revision: (logical.kind == ResourceKind::Game)
                        .then(|| SETUP.into()),
                    game_id: (logical.kind == ResourceKind::Game).then(|| "dcs".into()),
                }
            })
            .collect();
        CompileSnapshot {
            inventory_session: page.session,
            inventory_revision: page.revision,
            assignments: self.assignments.clone(),
            resources,
            game_context: Some(VerifiedGameContext {
                game_id: "dcs".into(),
                session: "fixture-verified-dcs-context".into(),
                provider_setup_revisions: BTreeMap::from([(self.ids["dcs"].clone(), SETUP.into())]),
            }),
            // Keep real activation qualification unavailable. Running a pure recording
            // reducer below cannot confer driver, visibility, game or companion authority.
            backend: BackendCapabilities::default(),
            inventory_diagnostics: vec![],
            read_only: false,
        }
    }
}
fn execution_plan(report: CompilationReport) -> ExecutionPlan {
    assert_ne!(
        report.readiness,
        PlanReadiness::Invalid,
        "{:?}",
        report.diagnostics
    );
    report
        .plan
        .expect("a valid graph remains inspectable with unavailable native backends")
}
fn latest_controllers(runtime: &Runtime) -> BTreeMap<String, TargetState> {
    let mut latest = BTreeMap::new();
    let mut cursor = Counter(0);
    loop {
        let page = runtime.recording.page(cursor, 32).unwrap();
        if page.records.is_empty() {
            return latest;
        }
        cursor = page.next;
        for record in page.records {
            latest.insert(record.report.target, record.report.state);
        }
    }
}
fn requests(effects: &[Effect], expected: &[ExpectedRequest], commands: usize) {
    let is_command = |effect: &&Effect| {
        effect.sink_id == "dcs-command" && matches!(effect.sample, SignalSample::Known(_))
    };
    assert_eq!(
        effects.iter().filter(is_command).count(),
        commands,
        "one-shot requests: {effects:?}"
    );
    for command in effects.iter().filter(is_command) {
        let SignalSample::Known(SignalValue::ActionPack(pack)) = &command.sample else {
            panic!("a simulator command must be one ordered atomic invocation");
        };
        assert!(
            pack.arguments.is_empty(),
            "this exact DCS command has no arguments"
        );
        assert_eq!(command.endpoint.node_id, "dcs");
        assert_eq!(command.endpoint.port_id, "master-caution");
    }
    let mut actual = effects
        .iter()
        .filter(|e| {
            matches!(
                e.sink_id.as_str(),
                "module-action" | "companion-warning" | "companion-altitude-label"
            )
        })
        .map(|e| (e.sink_id.clone(), e.sample.clone()))
        .collect::<Vec<_>>();
    actual.sort_by(|a, b| a.0.cmp(&b.0));
    let mut expected = expected
        .iter()
        .map(|e| (e.edge_id.clone(), e.sample.clone()))
        .collect::<Vec<_>>();
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        actual, expected,
        "computed/requested effects only; no adapter acknowledgement"
    );
}

#[test]
fn complete_rig_has_explicit_pipelines_and_transport_metadata_never_becomes_graph_edges() {
    let document = document();
    let replay = replay();
    let mut hashes = vec![];
    for variant in replay.route_variants {
        let rig = Rig::new(&document, variant.direct_usb);
        let capture = rig.capture(&document);
        let report = compile(&document, &capture);
        assert_eq!(
            report.readiness,
            PlanReadiness::Blocked,
            "{}: no qualified native backends are asserted",
            variant.id
        );
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "virtualBackendNotReady"));
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "visibilityBackendNotReady"));
        hashes.push(report.program_hash.clone().unwrap());
        let plan = execution_plan(report);
        assert_eq!(plan.budgets.used_sources, 8);
        assert_eq!(plan.controllers.len(), 2);
        assert_eq!(plan.budgets.bindings, 17);
        assert!(plan
            .visibility
            .iter()
            .any(|v| v.resource_id == "third-party-c"
                && v.mode == VisibilityMode::HiddenWhileActive
                && v.inventory_resource_id.is_some()));
        assert!(!plan
            .program
            .sources
            .iter()
            .any(|s| s.endpoint.node_id == "third-party-c"));
        let label = plan
            .branches
            .iter()
            .find(|b| b.sink.node_id == "companion-panel" && b.sink.port_id == "altitude-label")
            .unwrap();
        assert_eq!(
            label
                .source_ports
                .iter()
                .map(|e| (e.node_id.as_str(), e.port_id.as_str()))
                .collect::<Vec<_>>(),
            [("dcs", "altitude")]
        );
        assert_eq!(
            label
                .processing_nodes
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "feet-to-metres",
                "altitude-threshold",
                "altitude-visible",
                "altitude-format"
            ])
        );
        assert_eq!(
            label
                .edge_ids
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "altitude-feet",
                "altitude-metres",
                "altitude-display-value",
                "altitude-display-condition",
                "altitude-format-input",
                "companion-altitude-label"
            ])
        );
        let command = plan
            .branches
            .iter()
            .find(|b| b.sink.node_id == "dcs")
            .unwrap();
        assert_eq!(command.source_ports[0].node_id, "can-command");
        assert_eq!(command.processing_nodes, ["command-trigger"]);
        let module = capture
            .resources
            .iter()
            .find(|r| r.summary.id == rig.ids["direct-usb"])
            .unwrap();
        assert_eq!(
            module.summary.connections.len(),
            if variant.direct_usb { 2 } else { 1 }
        );
        assert!(module
            .summary
            .connections
            .iter()
            .any(|c| c.route.transport == Transport::Can
                && c.route.attributes.get("gateway").is_some()));
        assert!(module.summary.connections.iter().all(|c| c.route.baseline_ready && c.route.writable), "USB priority is tested with both observed routes usable, rather than only a USB fallback.");
        let direct = plan
            .sources
            .iter()
            .find(|s| s.endpoint.node_id == "direct-usb")
            .unwrap();
        assert_eq!(
            direct.route.as_ref().unwrap().transport,
            if variant.direct_usb {
                Transport::Usb
            } else {
                Transport::Can
            }
        );
        assert!(plan
            .program
            .nodes
            .iter()
            .all(|n| !n.id.contains("route") && !n.id.contains("gateway")));
        let portable = serde_json::to_string(&document).unwrap();
        for local_id in rig.ids.values() {
            assert!(!portable.contains(local_id));
        }
        assert!(!portable.contains("fixture-selected-cdc"));
        assert!(!portable.contains("fixture-can-gateway"));
    }
    assert_eq!(
        hashes[0], hashes[1],
        "transport/route changes cannot change the portable program"
    );
}

#[test]
fn complete_rig_replay_computes_controllers_conditional_text_and_requests_without_event_replay() {
    let document = document();
    let replay = replay();
    for variant in &replay.route_variants {
        let rig = Rig::new(&document, variant.direct_usb);
        let program = execution_plan(compile(&document, &rig.capture(&document))).program;
        let mut runtime = Runtime::default();
        let run = runtime.start_program(program.clone()).unwrap();
        runtime.drain_effects();
        for frame in &replay.baselines {
            runtime.accept_signal_for(&run, frame.clone()).unwrap();
        }
        requests(
            &runtime.drain_effects(),
            &replay.baseline.requests,
            replay.baseline.command_count,
        );
        assert_eq!(
            latest_controllers(&runtime),
            replay.baseline.controllers,
            "{}",
            variant.id
        );
        for expected in &replay.baseline.samples {
            assert_eq!(
                runtime.typed_engine().unwrap().sample(&Endpoint {
                    node_id: expected.node_id.clone(),
                    port_id: expected.port_id.clone()
                }),
                Some(&expected.sample)
            );
        }
        for step in &replay.changes {
            runtime.accept_signal_for(&run, step.frame.clone()).unwrap();
            requests(&runtime.drain_effects(), &step.requests, step.command_count);
        }
        // A queued one-shot cannot outlive loss of its exact source. A new
        // held baseline also cannot turn it into a new command.
        let mut command = replay.changes[0].frame.clone();
        command.sequence = Counter(3);
        command.captured_us = Counter(700);
        // NativeProfileHost admits source pumps through this batch boundary,
        // which revokes every loss before any queued work can be consumed.
        let loss = runtime
            .typed_engine_mut()
            .unwrap()
            .accept_batch([
                command.clone(),
                Frame {
                    source: "can-command".into(),
                    generation: Counter(1),
                    sequence: Counter(4),
                    captured_us: Counter(701),
                    reason: Reason::Disconnect,
                    changes: vec![],
                },
            ])
            .unwrap();
        assert!(!loss.is_empty());
        assert!(runtime
            .drain_effects()
            .iter()
            .all(|e| e.sink_id != "dcs-command" || !matches!(e.sample, SignalSample::Known(_))));
        command.sequence = Counter(5);
        command.captured_us = Counter(702);
        assert!(
            runtime.accept_signal_for(&run, command.clone()).is_err(),
            "a lost source must rebaseline"
        );
        command.reason = Reason::Baseline;
        command.generation = Counter(2);
        command.sequence = Counter(1);
        command.captured_us = Counter(703);
        runtime.accept_signal_for(&run, command.clone()).unwrap();
        assert!(runtime
            .drain_effects()
            .iter()
            .all(|e| e.sink_id != "dcs-command" || !matches!(e.sample, SignalSample::Known(_))));
        command.reason = Reason::Change;
        command.sequence = Counter(2);
        command.captured_us = Counter(704);
        runtime.accept_signal_for(&run, command).unwrap();
        requests(&runtime.drain_effects(), &[], 1);
        runtime.advance_for(&run, Counter(1_100_000)).unwrap();
        let stale = runtime.drain_effects();
        assert!(stale
            .iter()
            .any(|e| e.sink_id == "companion-altitude-label" && e.sample == SignalSample::Stale));
        assert!(stale
            .iter()
            .all(|e| e.sink_id != "dcs-command" || !matches!(e.sample, SignalSample::Known(_))));
        assert_eq!(
            runtime.typed_engine().unwrap().sample(&Endpoint {
                node_id: "dcs".into(),
                port_id: "altitude".into()
            }),
            Some(&SignalSample::Stale)
        );
        assert!(latest_controllers(&runtime)
            .values()
            .all(|v| v.axes.iter().all(|a| *a == 0) && v.buttons.iter().all(|b| !b)));
        runtime.stop();
        assert!(runtime
            .accept_signal_for(&run, replay.baselines[0].clone())
            .is_err());
        let next = runtime.start_program(program).unwrap();
        assert_ne!(run, next);
        runtime.drain_effects();
        let mut held = replay.baselines.last().unwrap().clone();
        held.changes[0].sample = SignalSample::scalar(PortValue::Boolean(true));
        runtime.accept_signal_for(&next, held).unwrap();
        assert!(runtime
            .drain_effects()
            .iter()
            .all(|e| e.sink_id != "dcs-command" || !matches!(e.sample, SignalSample::Known(_))));
    }
}

#[test]
fn complete_rig_retains_offline_blocks_and_changed_contracts_but_cannot_reuse_delivery_tokens() {
    let document = document();
    let mut rig = Rig::new(&document, true);
    let before = compile(&document, &rig.capture(&document));
    let token = rig
        .registry
        .token(&rig.ids["can-a"], "axis", Access::Read)
        .unwrap();
    rig.batch.revision = 2;
    rig.batch.observations.retain(|o| o.key != "can-a");
    for observation in rig
        .batch
        .observations
        .iter_mut()
        .filter(|o| o.key.starts_with("direct-usb"))
    {
        let ports = observation.ports.as_mut().unwrap();
        if let PortContract::State { freshness_ms, .. } = &mut ports.ports[0].contract {
            *freshness_ms = 750;
        }
        *ports = PortUpdate::complete(ports.ports.clone()).unwrap();
    }
    rig.registry.reconcile(rig.batch.clone()).unwrap();
    assert!(rig
        .registry
        .validate_token(&token, "axis", Access::Read)
        .is_err());
    let captured = rig.capture(&document);
    let report = compile(&document, &captured);
    assert_eq!(report.program_hash, before.program_hash);
    assert_ne!(report.capture_fingerprint, before.capture_fingerprint);
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.code == "resourceUnavailable"
            && d.location.resource_id.as_deref() == Some("can-a")));
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.code == "portContractChanged"
            && d.location.resource_id.as_deref() == Some("direct-usb")));
    let plan = execution_plan(report);
    for logical in ["can-a", "direct-usb"] {
        let source = plan
            .sources
            .iter()
            .find(|s| s.endpoint.node_id == logical)
            .unwrap();
        assert_eq!(source.readiness, PlanReadiness::Blocked);
        assert!(source.token.is_none());
        assert!(document.resources.iter().any(|r| r.id == logical));
        assert!(document
            .editor
            .positions
            .iter()
            .any(|p| p.node_id == logical));
    }
    assert_eq!(
        captured
            .resources
            .iter()
            .find(|r| r.summary.id == rig.ids["can-a"])
            .unwrap()
            .summary
            .availability,
        Availability::Offline
    );
    let mut no_context = captured.clone();
    no_context.game_context = None;
    let dormant = execution_plan(compile(&document, &no_context));
    assert!(dormant
        .sources
        .iter()
        .filter(|s| s.endpoint.node_id == "dcs")
        .all(|s| s.readiness == PlanReadiness::Dormant));
    assert!(dormant
        .branches
        .iter()
        .filter(|b| b.source_ports.iter().any(|s| s.node_id == "dcs"))
        .all(|b| b.readiness == PlanReadiness::Dormant));
}

#[test]
fn complete_rig_default_copy_and_captured_applied_document_are_independent() {
    let document = document();
    let rig = Rig::new(&document, true);
    let mut service = osg_profiles::service::Service::new(None);
    let saved = service.request(&json!({"command":"profile_editor_save","document":document,"expectedRevision":null,"assignments":rig.assignments})).unwrap();
    let original: ProfileDocument = serde_json::from_value(saved["document"].clone()).unwrap();
    let applied = AppliedSnapshot::capture(&original).unwrap();
    let copied = service.request(&json!({"command":"profile_duplicate","id":original.id,"name":"Independent complete rig"})).unwrap();
    let mut copy: ProfileDocument = serde_json::from_value(copied["document"].clone()).unwrap();
    assert_ne!(copy.id, original.id);
    assert_eq!(
        semantic_hash(&copy).unwrap(),
        semantic_hash(&original).unwrap()
    );
    assert_eq!(copy.bindings, original.bindings);
    let copy_assignments = service
        .request(&json!({"command":"profile_assignments_get","id":copy.id}))
        .unwrap();
    assert_eq!(
        copy_assignments["assignments"],
        json!([]),
        "copies require explicit local assignment; no old native identity is inherited"
    );
    copy.visibility.clear();
    copy.processing_nodes
        .iter_mut()
        .find(|n| n.id == "altitude-format")
        .unwrap()
        .operation = osg_profiles::Operation::Format {
        decimals: 0,
        prefix: "ALT ".into(),
        suffix: " m".into(),
        rounding: osg_mapping::program::Rounding::Nearest,
        overflow: osg_mapping::program::TextOverflow::Reject,
    };
    service
        .request(&json!({"command":"profile_save","document":copy,"expectedRevision":1}))
        .unwrap();
    let stored_original = service
        .request(&json!({"command":"profile_get","id":original.id}))
        .unwrap();
    assert_eq!(
        stored_original["document"],
        serde_json::to_value(&original).unwrap()
    );
    assert_eq!(applied.document(), &original);
    let exported = service
        .request(&json!({"command":"profile_export","id":original.id}))
        .unwrap();
    assert_eq!(
        exported["document"],
        serde_json::to_value(&original).unwrap()
    );
    let portable = serde_json::to_string(&exported).unwrap();
    for native_id in rig.ids.values() {
        assert!(!portable.contains(native_id));
    }
    let default = service
        .request(&json!({"command":"profile_get","id":DEFAULT_PROFILE_ID}))
        .unwrap();
    assert_eq!(
        default["document"],
        serde_json::to_value(default_document()).unwrap()
    );
    let status = service
        .request(&json!({"command":"profile_status"}))
        .unwrap();
    assert_eq!(status["session"]["selectedProfileId"], DEFAULT_PROFILE_ID);
    assert_eq!(status["session"]["state"], "stopped");
    assert!(status["session"]["appliedSnapshot"].is_null());
}
