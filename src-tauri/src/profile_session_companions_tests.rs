use super::*;
use osg_companion_service::{Publication, PublicationUpload, SourceKind};
use osg_mapping::{
    program::{Program, ResourceAdapter, Sink, SinkKind, SourcePort},
    signal::Direction,
};
use osg_profiles::compiler_models::{BackendCapabilities, CompileSnapshot, SinkPlan, SourcePlan};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};

fn empty() -> ExecutionPlan {
    osg_profiles::compiler::compile(
        &osg_profiles::default_document(),
        &CompileSnapshot {
            inventory_session: "native".into(),
            inventory_revision: "1".into(),
            assignments: vec![],
            resources: vec![],
            game_context: None,
            backend: BackendCapabilities::default(),
            inventory_diagnostics: vec![],
            read_only: false,
        },
    )
    .plan
    .unwrap()
}
fn endpoint(port: &str) -> Endpoint {
    Endpoint {
        node_id: "companion".into(),
        port_id: port.into(),
    }
}
fn wire(hub: &Hub, body: Value) -> Value {
    hub.handle_wire(body, "connection", hub.now_ms())
}
fn pair(hub: &Hub) -> String {
    let offer = hub
        .command(&json!({"command":"companion_pair_offer"}))
        .unwrap();
    let request = wire(
        hub,
        json!({"type":"pair","secret":offer["secret"],"companionId":"screen","name":"Screen"}),
    );
    hub.command(&json!({"command":"companion_approve","requestId":request["requestId"],"companionId":"screen"})).unwrap();
    wire(
        hub,
        json!({"type":"pair-status","requestId":request["requestId"],"secret":offer["secret"]}),
    )["token"]
        .as_str()
        .unwrap()
        .into()
}
struct Fixture {
    hub: Hub,
    provider: Arc<CompanionProvider>,
    clock: Arc<AtomicU64>,
    publication: Publication,
    secret: String,
    plan: ExecutionPlan,
}
impl Fixture {
    fn new() -> Self {
        let clock = Arc::new(AtomicU64::new(100));
        let captured = clock.clone();
        let provider = Arc::new(CompanionProvider::new(Arc::new(move || {
            captured.load(Ordering::Acquire)
        })));
        let hub = Hub::new("adapter-host".into(), provider.clone());
        let secret = pair(&hub);
        let upload: PublicationUpload = serde_json::from_str(include_str!(
            "../../crates/companion-service/tests/fixtures/c1-flight-publication.json"
        ))
        .unwrap();
        let publication = Publication::validate(upload.clone()).unwrap();
        let mut command = serde_json::to_value(upload).unwrap();
        command["command"] = json!("companion_publish");
        hub.command(&command).unwrap();
        hub.command(&json!({"command":"companion_assign","instanceId":"instance","companionId":"screen","publicationHash":publication.publication_hash})).unwrap();
        let ports = crate::profile_resource_ports::companion(
            &native_definition(publication.definition("instance")).unwrap(),
            &publication.feedback,
        )
        .unwrap();
        let input = ports
            .iter()
            .find(|port| {
                port.direction == Direction::Output
                    && matches!(
                        port.contract,
                        PortContract::State {
                            schema: osg_mapping::signal::ScalarSchema::Boolean {},
                            ..
                        }
                    )
            })
            .unwrap()
            .clone();
        let output = ports
            .iter()
            .find(|port| {
                port.direction == Direction::Input
                    && matches!(
                        port.contract,
                        PortContract::State {
                            schema: osg_mapping::signal::ScalarSchema::Boolean {},
                            ..
                        }
                    )
            })
            .unwrap()
            .clone();
        let mut plan = empty();
        plan.companion_assignments
            .push(osg_profiles::CompanionAssignment {
                resource_id: "companion".into(),
                publication_hash: publication.publication_hash.clone(),
            });
        plan.sources.push(SourcePlan {
            endpoint: osg_profiles::Endpoint {
                node_id: "companion".into(),
                port_id: "button".into(),
            },
            inventory_resource_id: Some("inventory".into()),
            observed_port_id: Some(input.id),
            contract: input.contract.clone(),
            token: None,
            preparation: None,
            route: None,
            readiness: PlanReadiness::Ready,
        });
        plan.sinks.push(SinkPlan {
            endpoint: osg_profiles::Endpoint {
                node_id: "companion".into(),
                port_id: "indicator".into(),
            },
            adapter: SinkAdapter::CompanionFeedback,
            inventory_resource_id: Some("inventory".into()),
            observed_port_id: Some(output.id),
            contract: output.contract.clone(),
            token: None,
            preparation: None,
            route: None,
            readiness: PlanReadiness::Ready,
        });
        plan.program = Program {
            sources: vec![SourcePort {
                endpoint: endpoint("button"),
                contract: input.contract,
            }],
            nodes: vec![],
            controllers: vec![],
            sinks: vec![Sink {
                id: "feedback-edge".into(),
                endpoint: endpoint("indicator"),
                from: endpoint("button"),
                contract: output.contract,
                kind: SinkKind::Resource {
                    adapter: ResourceAdapter::CompanionFeedback,
                },
            }],
        };
        Self {
            hub,
            provider,
            clock,
            publication,
            secret,
            plan,
        }
    }
    fn prepared(&self) -> CompanionSources {
        let proof = self
            .hub
            .prepare_profile_assignment("instance", &self.publication.publication_hash)
            .unwrap();
        CompanionSources::prepare(
            &self.plan,
            vec![PreparedCompanion {
                resource_id: "companion".into(),
                proof,
            }],
        )
        .unwrap()
    }
    fn acquire(
        &self,
        adapter: &mut CompanionSources,
        previous: &[AssignmentReceipt],
    ) -> Vec<AssignmentReceipt> {
        let receipts = adapter.capture(&self.hub, previous).unwrap();
        adapter
            .acquire_native(&self.hub, self.provider.clone(), &receipts)
            .unwrap();
        receipts
    }
    fn connect(&self) -> Value {
        self.hub
            .disconnect_connection("connection", self.hub.now_ms());
        assert_eq!(
            wire(&self.hub, json!({"type":"auth","token":self.secret}))["ok"],
            true
        );
        let open = wire(&self.hub, json!({"type":"open","instanceId":"instance"}));
        assert_eq!(open["ok"], true);
        let mut body = self.frame(&open, "baseline", "0");
        body["values"] = serde_json::to_value(
            self.publication
                .controls
                .iter()
                .map(|control| (control.id.clone(), control.fallback()))
                .collect::<BTreeMap<_, _>>(),
        )
        .unwrap();
        assert_eq!(wire(&self.hub, body)["ok"], true);
        open
    }
    fn frame(&self, open: &Value, kind: &str, sequence: &str) -> Value {
        json!({"version":1,"type":kind,"clientId":open["clientId"],"sessionId":open["sessionId"],"generation":open["generation"],"publicationHash":open["publicationHash"],"sequence":sequence})
    }
    fn delivery(&self) -> ExecutionPlan {
        let generation = self.provider.resource_inventory()[0].generation.to_string();
        let mut plan = self.plan.clone();
        let descriptor = |id: &str, contract: &PortContract, access| ResourceToken {
            inventory_session: "native".into(),
            resource_id: "inventory".into(),
            generation: "1".into(),
            capability_revision: "capability".into(),
            route_id: "companion:instance".into(),
            provider_session: "native-provider".into(),
            provider_generation: generation.clone(),
            port_id: id.into(),
            contract_hash: PortDescriptor {
                id: id.into(),
                name: id.into(),
                direction: if access == Access::Read {
                    Direction::Output
                } else {
                    Direction::Input
                },
                contract: contract.clone(),
            }
            .contract_hash()
            .unwrap(),
            access,
        };
        for source in &mut plan.sources {
            source.token = Some(descriptor(
                source.observed_port_id.as_ref().unwrap(),
                &source.contract,
                Access::Read,
            ));
        }
        for sink in &mut plan.sinks {
            sink.token = Some(descriptor(
                sink.observed_port_id.as_ref().unwrap(),
                &sink.contract,
                Access::Write,
            ));
        }
        plan
    }
}

#[test]
fn default_needs_no_companion_initialization_or_dependencies() {
    let adapter = CompanionSources::prepare(&empty(), vec![]).unwrap();
    assert!(adapter.is_empty());
    let runtime =
        crate::companion_host::Runtime::new(None, Arc::new(CompanionProvider::new(Arc::new(|| 1))));
    let mut adapter = adapter;
    adapter.acquire(&runtime, &[]).unwrap();
    assert!(runtime.profile_metadata().is_err());
}
#[test]
fn actual_baseline_and_delivery_tokens_are_required_before_arming_feedback() {
    let fixture = Fixture::new();
    let mut adapter = fixture.prepared();
    let receipts = fixture.acquire(&mut adapter, &[]);
    assert!(!adapter.baseline_ready(&fixture.hub).unwrap());
    let open = fixture.connect();
    assert!(adapter.baseline_ready(&fixture.hub).unwrap());
    assert!(adapter.baseline(&fixture.hub, |_| Ok(()), 100).is_err());
    let plan = fixture.delivery();
    let mut read_only_delivery = plan.clone();
    read_only_delivery.sinks[0].token = None;
    adapter.bind_delivery_plan(&read_only_delivery).unwrap();
    assert!(adapter
        .baseline(&fixture.hub, |_| Ok(()), 100)
        .unwrap()
        .is_some());
    let unarmed = Phase::new(
        plan.program.clone(),
        Counter(1),
        uuid::Uuid::new_v4().to_string(),
    )
    .unwrap();
    assert!(
        adapter.arm(unarmed.run_token()).is_err(),
        "A read-only baseline does not grant feedback writes"
    );
    adapter.bind_delivery_plan(&plan).unwrap();
    let frames = adapter
        .baseline(&fixture.hub, |_| Ok(()), 100)
        .unwrap()
        .unwrap();
    assert_eq!(frames[0].reason, Reason::Baseline);
    let mut phase = Phase::new(
        plan.program.clone(),
        Counter(1),
        uuid::Uuid::new_v4().to_string(),
    )
    .unwrap();
    assert!(!adapter
        .publish_feedback(&fixture.hub, &phase, |_| Ok(()), 100)
        .is_empty());
    for frame in frames {
        phase.accept(frame).unwrap();
    }
    adapter.arm(phase.run_token()).unwrap();
    assert!(adapter.arm(phase.run_token()).is_err());
    assert!(adapter
        .publish_feedback(&fixture.hub, &phase, |_| Ok(()), 100)
        .is_empty());
    let reply = wire(&fixture.hub, fixture.frame(&open, "heartbeat", "1"));
    assert_eq!(reply["feedback"]["values"][0]["value"], false);
    assert_eq!(reply["feedback"]["values"][0]["provenance"], "local");
    assert_eq!(reply["submitted"], false);
    assert!(
        !adapter
            .publish_feedback(&fixture.hub, &phase, |_| Ok(()), 1000)
            .is_empty(),
        "A frozen reducer tick cannot renew stale presentation"
    );
    let mut body = fixture.frame(&fixture.connect(), "heartbeat", "1");
    // Reconnect changes the real source session; old tokens cannot publish.
    body["version"] = json!(1);
    assert_eq!(wire(&fixture.hub, body)["ok"], true);
    assert!(fixture
        .hub
        .validate_profile_assignment(&receipts[0])
        .is_ok());
    assert_eq!(
        adapter
            .poll(
                &fixture.hub,
                |_| Err("Route generation changed".into()),
                200
            )
            .frames[0]
            .reason,
        Reason::Disconnect
    );
}
#[test]
fn native_input_preserves_transition_order_and_old_owner_cleanup_cannot_clear_incoming() {
    let fixture = Fixture::new();
    let mut first = fixture.prepared();
    let old = fixture.acquire(&mut first, &[]);
    let open = fixture.connect();
    first.bind_delivery_plan(&fixture.delivery()).unwrap();
    first
        .baseline(&fixture.hub, |_| Ok(()), 100)
        .unwrap()
        .unwrap();
    fixture.clock.store(200, Ordering::Release);
    let button = fixture
        .publication
        .controls
        .iter()
        .find(|control| matches!(control.kind, SourceKind::Button))
        .unwrap();
    let mut input = fixture.frame(&open, "input", "1");
    input["changes"] =
        json!([{"controlId":button.id,"value":true},{"controlId":button.id,"value":false}]);
    assert_eq!(wire(&fixture.hub, input)["ok"], true);
    let poll = first.poll(&fixture.hub, |_| Ok(()), 200);
    assert!(poll.diagnostics.is_empty());
    assert_eq!(
        poll.frames[0].changes[0].sample,
        SignalSample::scalar(PortValue::Boolean(true))
    );
    assert_eq!(
        poll.frames[1].changes[0].sample,
        SignalSample::scalar(PortValue::Boolean(false))
    );
    let mut second = fixture.prepared();
    let incoming = second.capture(&fixture.hub, &old).unwrap();
    first.stop(&fixture.hub).unwrap();
    second
        .acquire_native(&fixture.hub, fixture.provider.clone(), &incoming)
        .unwrap();
    first.stop(&fixture.hub).unwrap();
    fixture
        .hub
        .validate_profile_assignment(&incoming[0])
        .unwrap();
    assert_eq!(
        fixture.hub.restore_profile_assignment(&old[0]).unwrap(),
        osg_companion_service::profile::AssignmentRestore::OwnershipLost
    );
}
#[test]
fn feedback_preserves_unknown_stale_disconnected_and_never_invents_game_confirmation() {
    let fixture = Fixture::new();
    let adapter = fixture.prepared();
    let output = &adapter.instances[0].outputs[0];
    for (sample, expected) in [
        (SignalSample::Unknown, FeedbackStatus::Unknown),
        (SignalSample::Stale, FeedbackStatus::Stale),
        (SignalSample::Unavailable, FeedbackStatus::Disconnected),
    ] {
        let sample = feedback_sample(output, Some(&sample)).unwrap();
        assert_eq!(sample.status, expected);
        assert!(sample.value.is_none());
        assert_eq!(sample.provenance, FeedbackProvenance::Local);
    }
    let known = feedback_sample(
        output,
        Some(&SignalSample::scalar(PortValue::Boolean(false))),
    )
    .unwrap();
    assert_eq!(known.value, Some(FeedbackValue::Boolean(false)));
    assert_eq!(known.status, FeedbackStatus::Fresh);
    assert!(feedback_sample(
        output,
        Some(&SignalSample::Known(SignalValue::ActionPack(
            osg_mapping::program::ActionPack { arguments: vec![] }
        )))
    )
    .is_err());
}
#[test]
fn malformed_or_unadvertised_companion_contracts_fail_before_any_assignment_effect() {
    let fixture = Fixture::new();
    let before = fixture
        .hub
        .assignment_snapshot(&["instance".into()])
        .unwrap();
    let proof = || {
        fixture
            .hub
            .prepare_profile_assignment("instance", &fixture.publication.publication_hash)
            .unwrap()
    };
    let mut plan = fixture.plan.clone();
    plan.sources[0].observed_port_id = Some("unadvertised".into());
    assert!(CompanionSources::prepare(
        &plan,
        vec![PreparedCompanion {
            resource_id: "companion".into(),
            proof: proof()
        }]
    )
    .is_err());
    let mut plan = fixture.plan.clone();
    plan.companion_assignments.clear();
    assert!(CompanionSources::prepare(
        &plan,
        vec![PreparedCompanion {
            resource_id: "companion".into(),
            proof: proof()
        }]
    )
    .is_err());
    assert!(CompanionSources::prepare(
        &fixture.plan,
        vec![
            PreparedCompanion {
                resource_id: "companion".into(),
                proof: proof()
            },
            PreparedCompanion {
                resource_id: "companion".into(),
                proof: proof()
            }
        ]
    )
    .is_err());
    let mut adapter = fixture.prepared();
    fixture.acquire(&mut adapter, &[]);
    fixture.connect();
    let mut plan = fixture.delivery();
    plan.sinks[0].token.as_mut().unwrap().access = Access::Invoke;
    assert!(adapter.bind_delivery_plan(&plan).is_err());
    assert_eq!(
        before["assignments"][0]["publicationHash"],
        fixture.publication.publication_hash
    );
}

#[test]
fn feedback_only_profile_branch_uses_real_panel_session_without_dummy_graph_inputs() {
    let mut fixture = Fixture::new();
    fixture.plan.sources.clear();
    fixture.plan.program.sources.clear();
    let from = Endpoint {
        node_id: "condition".into(),
        port_id: "state".into(),
    };
    fixture.plan.program.sinks[0].from = from.clone();
    fixture.plan.program.nodes.push(osg_mapping::program::Node {
        id: from.node_id,
        inputs: vec![],
        outputs: vec![PortDescriptor {
            id: from.port_id,
            name: "Visible local condition".into(),
            direction: Direction::Output,
            contract: fixture.plan.sinks[0].contract.clone(),
        }],
        operation: osg_mapping::program::Operation::Constant {
            value: PortValue::Boolean(true),
        },
    });
    let mut adapter = fixture.prepared();
    fixture.acquire(&mut adapter, &[]);
    assert!(!adapter.baseline_ready(&fixture.hub).unwrap());
    let open = fixture.connect();
    adapter.bind_delivery_plan(&fixture.delivery()).unwrap();
    assert!(adapter
        .baseline(&fixture.hub, |_| Ok(()), 100)
        .unwrap()
        .unwrap()
        .is_empty());
    let phase = Phase::new(
        fixture.plan.program.clone(),
        Counter(1),
        uuid::Uuid::new_v4().to_string(),
    )
    .unwrap();
    assert!(phase.program().sources.is_empty());
    adapter.arm(phase.run_token()).unwrap();
    assert!(adapter
        .publish_feedback(&fixture.hub, &phase, |_| Ok(()), 0)
        .is_empty());
    let reply = wire(&fixture.hub, fixture.frame(&open, "heartbeat", "1"));
    assert_eq!(reply["feedback"]["values"][0]["value"], true);
    assert_eq!(reply["feedback"]["values"][0]["provenance"], "local");
    adapter.stop(&fixture.hub).unwrap();
    assert!(adapter
        .acquire_native(&fixture.hub, fixture.provider.clone(), &[])
        .is_err());
    assert!(adapter
        .publish_feedback(&fixture.hub, &phase, |_| Ok(()), 100_000)
        .is_empty());
}

#[test]
fn reconnect_discards_buffered_edges_requires_current_tokens_and_never_changes_provider_identity() {
    let fixture = Fixture::new();
    let mut adapter = fixture.prepared();
    fixture.acquire(&mut adapter, &[]);
    fixture.connect();
    adapter.bind_delivery_plan(&fixture.delivery()).unwrap();
    let initial = adapter
        .baseline(&fixture.hub, |_| Ok(()), 100)
        .unwrap()
        .unwrap();
    fixture.clock.store(200, Ordering::Release);
    let reopened = fixture.connect();
    let loss = adapter.poll(&fixture.hub, |_| Err("Old native token".into()), 200);
    assert_eq!(loss.frames[0].reason, Reason::Disconnect);
    let button = fixture
        .publication
        .controls
        .iter()
        .find(|c| matches!(c.kind, SourceKind::Button))
        .unwrap();
    let mut input = fixture.frame(&reopened, "input", "1");
    input["changes"] =
        json!([{"controlId":button.id,"value":true},{"controlId":button.id,"value":false}]);
    assert_eq!(wire(&fixture.hub, input)["ok"], true);
    let rebound = adapter.rebind(&fixture.hub, &fixture.delivery(), |_| true, |_| Ok(()), 200);
    assert!(rebound.diagnostics.is_empty(), "{:?}", rebound.diagnostics);
    assert_eq!(rebound.frames.len(), 1);
    assert_eq!(rebound.frames[0].reason, Reason::Baseline);
    assert!(rebound.frames[0].sequence.0 > initial[0].sequence.0);
    assert_eq!(
        rebound.frames[0].changes[0].sample,
        SignalSample::scalar(PortValue::Boolean(false))
    );
    assert!(adapter
        .poll(&fixture.hub, |_| Ok(()), 200)
        .frames
        .iter()
        .all(|f| f.reason == Reason::Baseline || f.changes.is_empty()));
    let mut changed = fixture.delivery();
    changed.sources[0].token.as_mut().unwrap().provider_session = "other-provider".into();
    changed.sinks[0].token.as_mut().unwrap().provider_session = "other-provider".into();
    assert!(adapter.bind_delivery_plan(&changed).is_err());
    assert!(!adapter.needs_rebind(|_| Ok(())));
    assert!(
        adapter
            .rebind(&fixture.hub, &fixture.delivery(), |_| true, |_| Ok(()), 201)
            .frames
            .is_empty(),
        "Unchanged live tokens must not reset timers every tick"
    );
}

#[test]
fn one_offline_peer_cannot_block_another_companions_fresh_reconnect_baseline() {
    let fixture = Fixture::new();
    let offer = fixture
        .hub
        .command(&json!({"command":"companion_pair_offer"}))
        .unwrap();
    let pair=fixture.hub.handle_wire(json!({"type":"pair","secret":offer["secret"],"companionId":"second-screen","name":"Second"}),"second-peer",fixture.hub.now_ms());
    fixture.hub.command(&json!({"command":"companion_approve","requestId":pair["requestId"],"companionId":"second-screen"})).unwrap();
    let secret = fixture.hub.handle_wire(
        json!({"type":"pair-status","requestId":pair["requestId"],"secret":offer["secret"]}),
        "second-peer",
        fixture.hub.now_ms(),
    )["token"]
        .clone();
    fixture.hub.command(&json!({"command":"companion_assign","instanceId":"second-instance","companionId":"second-screen","publicationHash":fixture.publication.publication_hash})).unwrap();
    let mut plan = fixture.plan.clone();
    let mut source = plan.sources[0].clone();
    source.endpoint.node_id = "second".into();
    source.inventory_resource_id = Some("second-inventory".into());
    plan.sources.push(source);
    let mut sink = plan.sinks[0].clone();
    sink.endpoint.node_id = "second".into();
    sink.inventory_resource_id = Some("second-inventory".into());
    plan.sinks.push(sink);
    let mut source = plan.program.sources[0].clone();
    source.endpoint.node_id = "second".into();
    plan.program.sources.push(source);
    let mut sink = plan.program.sinks[0].clone();
    sink.id = "second-feedback".into();
    sink.endpoint.node_id = "second".into();
    sink.from.node_id = "second".into();
    plan.program.sinks.push(sink);
    plan.companion_assignments
        .push(osg_profiles::CompanionAssignment {
            resource_id: "second".into(),
            publication_hash: fixture.publication.publication_hash.clone(),
        });
    let bindings = [("companion", "instance"), ("second", "second-instance")]
        .into_iter()
        .map(|(resource, instance)| PreparedCompanion {
            resource_id: resource.into(),
            proof: fixture
                .hub
                .prepare_profile_assignment(instance, &fixture.publication.publication_hash)
                .unwrap(),
        })
        .collect();
    let mut adapter = CompanionSources::prepare(&plan, bindings).unwrap();
    let receipts = adapter.capture(&fixture.hub, &[]).unwrap();
    adapter
        .acquire_native(&fixture.hub, fixture.provider.clone(), &receipts)
        .unwrap();
    fixture.connect();
    assert_eq!(
        fixture.hub.handle_wire(
            json!({"type":"auth","token":secret}),
            "second-peer",
            fixture.hub.now_ms()
        )["ok"],
        true
    );
    let opened = fixture.hub.handle_wire(
        json!({"type":"open","instanceId":"second-instance"}),
        "second-peer",
        fixture.hub.now_ms(),
    );
    let mut baseline = fixture.frame(&opened, "baseline", "0");
    baseline["values"] = serde_json::to_value(
        fixture
            .publication
            .controls
            .iter()
            .map(|c| (c.id.clone(), c.fallback()))
            .collect::<BTreeMap<_, _>>(),
    )
    .unwrap();
    assert_eq!(
        fixture
            .hub
            .handle_wire(baseline, "second-peer", fixture.hub.now_ms())["ok"],
        true
    );
    let delivery = |offline_second: bool| {
        let mut delivery = plan.clone();
        let sources = fixture.provider.resource_inventory();
        for (logical, instance, resource) in [
            ("companion", "instance", "inventory"),
            ("second", "second-instance", "second-inventory"),
        ] {
            let generation = sources
                .iter()
                .find(|s| s.definition.source_id == instance)
                .unwrap()
                .generation
                .to_string();
            let native = fixture.delivery();
            for source in delivery
                .sources
                .iter_mut()
                .filter(|s| s.endpoint.node_id == logical)
            {
                let mut token = native.sources[0].token.clone().unwrap();
                token.resource_id = resource.into();
                token.route_id = format!("companion:{instance}");
                token.provider_generation = generation.clone();
                source.token = Some(token);
                if logical == "second" && offline_second {
                    source.token = None;
                    source.readiness = PlanReadiness::Blocked;
                }
            }
            for sink in delivery
                .sinks
                .iter_mut()
                .filter(|s| s.endpoint.node_id == logical)
            {
                let mut token = native.sinks[0].token.clone().unwrap();
                token.resource_id = resource.into();
                token.route_id = format!("companion:{instance}");
                token.provider_generation = generation.clone();
                sink.token = Some(token);
                if logical == "second" && offline_second {
                    sink.token = None;
                    sink.readiness = PlanReadiness::Blocked;
                }
            }
        }
        delivery
    };
    adapter.bind_delivery_plan(&delivery(false)).unwrap();
    assert_eq!(
        adapter
            .baseline(&fixture.hub, |_| Ok(()), 100)
            .unwrap()
            .unwrap()
            .len(),
        2
    );
    fixture.clock.store(200, Ordering::Release);
    fixture
        .hub
        .disconnect_connection("connection", fixture.hub.now_ms());
    let lost = adapter.poll(
        &fixture.hub,
        |token| {
            if token.resource_id == "inventory" {
                Err("First peer disconnected".into())
            } else {
                Ok(())
            }
        },
        200,
    );
    assert!(lost
        .frames
        .iter()
        .any(|f| f.source == "companion" && f.reason == Reason::Disconnect));
    assert!(!lost
        .frames
        .iter()
        .any(|f| f.source == "second" && f.reason == Reason::Disconnect));
    assert!(adapter.instances[1].ready);
    fixture.connect();
    fixture
        .hub
        .disconnect_connection("second-peer", fixture.hub.now_ms());
    let rebound = adapter.rebind(
        &fixture.hub,
        &delivery(true),
        |logical| logical == "companion",
        |_| Ok(()),
        200,
    );
    assert!(rebound.diagnostics.is_empty(), "{:?}", rebound.diagnostics);
    assert_eq!(rebound.frames.len(), 1);
    assert_eq!(rebound.frames[0].source, "companion");
    assert_eq!(rebound.frames[0].reason, Reason::Baseline);
    assert!(
        adapter.instances[0].ready,
        "The other offline branch did not block reconnect"
    );
}

#[test]
fn a_companion_route_id_cannot_authorize_network_to_usb_transport_transfer() {
    let mut fixture = Fixture::new();
    let route = osg_profiles::resources::Route {
        id: "companion:instance".into(),
        transport: Transport::Network,
        provider_generation: "1".into(),
        attributes: BTreeMap::new(),
        baseline_ready: false,
        writable: false,
        priority: 0,
    };
    fixture.plan.sources[0].route = Some(route.clone());
    fixture.plan.sinks[0].route = Some(route);
    let mut adapter = fixture.prepared();
    fixture.acquire(&mut adapter, &[]);
    fixture.connect();
    let mut delivery = fixture.delivery();
    adapter.bind_delivery_plan(&delivery).unwrap();
    adapter
        .baseline(&fixture.hub, |_| Ok(()), 100)
        .unwrap()
        .unwrap();
    delivery.sources[0].route.as_mut().unwrap().transport = Transport::Usb;
    delivery.sinks[0].route.as_mut().unwrap().transport = Transport::Usb;
    assert!(adapter
        .bind_delivery_plan(&delivery)
        .unwrap_err()
        .contains("transport changed"));
    let rebound = adapter.rebind(&fixture.hub, &delivery, |_| true, |_| Ok(()), 101);
    assert!(rebound.frames.is_empty());
    assert!(rebound.diagnostics[0].contains("transport changed"));
    adapter.bind_delivery_plan(&fixture.delivery()).unwrap();
}
