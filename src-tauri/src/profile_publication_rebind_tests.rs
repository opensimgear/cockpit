//! Resource refresh regression tests use genuine Registry captures. The private
//! publication proof is injected here; no delivery token is fabricated.
use super::*;
use osg_profiles::{
    compiler_models::{PlanReadiness, SinkAdapter, SinkPlan, SourcePlan},
    resources::{
        EvidenceKind, IdentityEvidence, Observation, PortExpectation, ProviderBatch, ResourceType,
        Route,
    },
    resources_ports::{ActionSemantics, PortContract, PortDescriptor, PortUpdate, ScalarSchema},
    store::LocalAssignment,
    Direction, Endpoint, PortValue,
};
const GATEWAY: &str = "stm32:0123456789abcdef01234567";
const MODULE: &str = "00000000000000000000000000000001";
fn target() -> Target {
    Target {
        resource: "controller".into(),
        inventory: String::new(),
        gateway: GATEWAY.into(),
        module: MODULE.into(),
        epoch: 65536,
        boot: 10,
        configuration: 2,
        node: 1,
        source: 50,
        profile: json!({"revision":1,"buttons":8,"axes":0,"hats":0,"neutral":[],"axisUsage":[]}),
        slot: 0,
        configured_enabled: true,
        report: 1,
        order: 0,
        enabled: false,
    }
}
fn state() -> Value {
    let t = target();
    json!({"gatewayType":"stm32-can-gateway","gatewaySerial":"0123456789abcdef01234567","protocolVersion":3,"canProtocol":3,"controllerPublicationOverride":{"schema":1,"scope":"gateway-can","nonpersistent":true,"captureAtClaim":true,"restore":"compare-if-unchanged","minLeaseMs":250,"maxLeaseMs":5000,"maxTargets":32,"reconnectRequired":true},"connected":true,"epoch":t.epoch,"publicationRevision":18,"layoutPending":false,"publication":{"phase":"owned","outcome":"ok","leaseId":73,"expiresAtMs":5000,"transition":"1","claimRevision":17,"targetMask":"00000001","enabledMask":"00000000","expectedReconnect":false},"modules":[{"id":MODULE,"online":true,"directUsb":false,"boot":t.boot,"configurationGeneration":t.configuration,"node":t.node,"sourceNode":t.source,"profile":t.profile,"publicationSlot":0,"configuredEnabled":true,"publicationReportId":1,"configuredDescriptorIndex":0,"published":false,"lease":67,"hasInput":true}]})
}
struct ReadOnlyIo(Arc<Mutex<Value>>);
impl Io for ReadOnlyIo {
    fn cached(&self, g: &str) -> Result<Value, String> {
        assert_eq!(g, GATEWAY);
        Ok(self.0.lock().unwrap().clone())
    }
    fn request(
        &self,
        _: &str,
        _: crate::stm32::PublicationRequest,
    ) -> Result<crate::stm32::PublicationReply, String> {
        panic!("The scoped resource guard cannot dispatch publication commands")
    }
}
fn ports() -> Vec<PortDescriptor> {
    vec![
        PortDescriptor {
            id: "button:0".into(),
            name: "Pressed".into(),
            direction: Direction::Output,
            contract: PortContract::State {
                schema: ScalarSchema::Boolean {},
                freshness_ms: 1000,
                fallback: Some(PortValue::Boolean(false)),
            },
        },
        PortDescriptor {
            id: "led.set".into(),
            name: "LED".into(),
            direction: Direction::Input,
            contract: PortContract::Action {
                parameters: vec![],
                semantics: ActionSemantics::Setter,
                endpoint: "led".into(),
                readable: true,
                display: None,
                ownership: None,
            },
        },
    ]
}
fn observation(lease: u32, catalogue: &str) -> Observation {
    let s = state();
    let t = target();
    Observation {
        key: MODULE.into(),
        name: "CAN controller".into(),
        resource_type: ResourceType::OsgModule,
        evidence: vec![IdentityEvidence {
            kind: EvidenceKind::Serial,
            value: MODULE.into(),
        }],
        attributes: BTreeMap::from([
            ("gatewayId".into(), GATEWAY.into()),
            ("moduleId".into(), MODULE.into()),
            (
                "controllerPublicationOverride".into(),
                advertised(&s, &s["modules"][0]).unwrap().to_string(),
            ),
        ]),
        diagnostics: vec![],
        route: Some(Route {
            id: format!("can:{GATEWAY}:{MODULE}"),
            transport: Transport::Can,
            provider_generation: json!([t.epoch, t.boot, lease, t.configuration, lease])
                .to_string(),
            attributes: BTreeMap::from([
                ("gatewayId".into(), GATEWAY.into()),
                ("moduleId".into(), MODULE.into()),
                ("boot".into(), t.boot.to_string()),
                ("node".into(), t.node.to_string()),
                ("actionCatalogueHash".into(), catalogue.into()),
            ]),
            baseline_ready: true,
            writable: true,
            priority: 50,
        }),
        ports: Some(PortUpdate::complete(ports()).unwrap()),
    }
}
struct Fixture {
    runtime: crate::profile_resources::Runtime,
    items: Arc<Mutex<Vec<Observation>>>,
    state: Arc<Mutex<Value>>,
    source: String,
}
impl Fixture {
    fn new() -> Self {
        let items = Arc::new(Mutex::new(vec![observation(66, "catalogue-one")]));
        let observed = items.clone();
        let runtime = crate::profile_resources::Runtime::new(
            None,
            Arc::new(move |revision| {
                vec![(
                    "can-fixture".into(),
                    Ok(ProviderBatch {
                        scope: "can-fixture".into(),
                        session: "native-session".into(),
                        revision,
                        complete: true,
                        observations: observed.lock().unwrap().clone(),
                    }),
                )]
            }),
            false,
        );
        runtime.refresh_native().unwrap();
        let source = runtime
            .request(&json!({"command":"profile_resources_list"}))
            .unwrap()["resources"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        Self {
            runtime,
            items,
            state: Arc::new(Mutex::new(state())),
            source,
        }
    }
    fn plan(&self) -> (ExecutionPlan, CompileSnapshot) {
        let assignment = LocalAssignment {
            resource_id: "controller".into(),
            source_id: self.source.clone(),
            ports: ports()
                .into_iter()
                .map(|p| PortExpectation {
                    profile_port_id: p.id.clone(),
                    observed_port_id: p.id.clone(),
                    expected_contract_hash: p.contract_hash().unwrap(),
                })
                .collect(),
        };
        let (snapshot, pins) = self
            .runtime
            .snapshot_for_preparation(vec![assignment])
            .unwrap();
        let mut plan = osg_profiles::compiler::compile(
            &osg_profiles::default_document(),
            &CompileSnapshot {
                assignments: vec![],
                resources: vec![],
                ..snapshot.clone()
            },
        )
        .plan
        .unwrap();
        for pin in pins {
            let p = pin.resolved().port.clone();
            let endpoint = Endpoint {
                node_id: "controller".into(),
                port_id: p.id.clone(),
            };
            if p.direction == Direction::Output {
                plan.sources.push(SourcePlan {
                    endpoint,
                    inventory_resource_id: Some(self.source.clone()),
                    observed_port_id: Some(p.id),
                    contract: p.contract,
                    token: None,
                    route: Some(pin.resolved().route.clone()),
                    preparation: Some(pin),
                    readiness: PlanReadiness::Ready,
                });
            } else {
                plan.sinks.push(SinkPlan {
                    endpoint,
                    adapter: SinkAdapter::PhysicalAction,
                    inventory_resource_id: Some(self.source.clone()),
                    observed_port_id: Some(p.id),
                    contract: p.contract,
                    token: None,
                    route: Some(pin.resolved().route.clone()),
                    preparation: Some(pin),
                    readiness: PlanReadiness::Ready,
                });
            }
        }
        plan.runtime_overrides = vec![osg_profiles::RuntimeOverride {
            resource_id: "controller".into(),
            enabled: false,
        }];
        (plan, snapshot)
    }
    fn refresh(&self, lease: u32, catalogue: &str) {
        *self.items.lock().unwrap() = vec![observation(lease, catalogue)];
        self.runtime.refresh_native().unwrap();
    }
    fn proof(&self) -> VerifiedPublication {
        let mut target = target();
        target.inventory = self.source.clone();
        let state = self.state.lock().unwrap().clone();
        let active = shared(false);
        active.authority.store(true, Ordering::Release);
        active.status.lock().unwrap().active = true;
        *active.verified.lock().unwrap() = Some((Instant::now(), state.clone()));
        VerifiedPublication {
            targets: vec![target],
            io: Arc::new(ReadOnlyIo(self.state.clone())),
            states: BTreeMap::from([(GATEWAY.into(), state)]),
            live: BTreeMap::from([(GATEWAY.into(), Some(active))]),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.runtime.shutdown();
    }
}
#[test]
fn exact_owned_can_lease_refreshes_real_pins_but_old_delivery_binding_stays_strict() {
    let f = Fixture::new();
    let (before, _) = f.plan();
    let old = f.runtime.bind_delivery_reads(&before).unwrap().sources[0]
        .token
        .clone()
        .unwrap();
    f.refresh(67, "catalogue-one");
    let (after, _) = f.plan();
    let proof = f.proof();
    f.runtime
        .validate_publication_rebind(&before, &after, &proof)
        .unwrap();
    assert!(f.runtime.bind_delivery_reads(&before).is_err());
    assert!(f.runtime.validate_delivery(&old).is_err());
    let current = f.runtime.bind_delivery_reads(&after).unwrap();
    f.runtime
        .validate_delivery(current.sources[0].token.as_ref().unwrap())
        .unwrap();
    let mut unproved = after.clone();
    let route = unproved.sources[0].route.as_mut().unwrap();
    route.provider_generation = json!([65536, 10, 68, 2, 68]).to_string();
    assert!(f
        .runtime
        .validate_publication_rebind(&before, &unproved, &proof)
        .is_err());
}
#[test]
fn same_port_contract_cannot_adopt_a_new_action_catalogue_or_unrelated_route() {
    let f = Fixture::new();
    let (before, _) = f.plan();
    f.refresh(67, "catalogue-two");
    let (after, _) = f.plan();
    assert_eq!(before.sinks[0].contract, after.sinks[0].contract);
    assert!(f
        .runtime
        .validate_publication_rebind(&before, &after, &f.proof())
        .is_err());
    f.refresh(67, "catalogue-one");
    let (mut after, _) = f.plan();
    after.sources[0].endpoint.node_id = "unrelated-controller".into();
    assert!(f
        .runtime
        .validate_publication_rebind(&before, &after, &f.proof())
        .is_err());
}
#[test]
fn observed_direct_usb_publisher_rejects_the_whole_resource_even_if_can_flag_lags() {
    let f = Fixture::new();
    let (plan, mut snapshot) = f.plan();
    assert_eq!(Staged::prepare(&plan, &snapshot).unwrap().targets.len(), 1);
    let mut direct = snapshot.resources[0].summary.connections[0].clone();
    direct.route.id = format!("usb:{MODULE}");
    direct.route.transport = Transport::Usb;
    snapshot.resources[0].summary.connections.push(direct);
    assert!(Staged::prepare(&plan, &snapshot).is_err());
}
#[test]
fn private_refresh_proof_rejects_gateway_swap_visibility_drift_and_later_lease() {
    for (field, value) in [
        ("gatewaySerial", json!("1123456789abcdef01234567")),
        ("lease", json!(68)),
        ("published", json!(true)),
    ] {
        let f = Fixture::new();
        let proof = f.proof();
        if field == "gatewaySerial" {
            f.state.lock().unwrap()[field] = value;
        } else {
            f.state.lock().unwrap()["modules"][0][field] = value;
        }
        assert!(proof.validate().is_err(), "{field}");
    }
}

#[test]
fn incoming_logical_name_can_differ_while_its_sealed_physical_assignment_stays_exact() {
    let f = Fixture::new();
    let (mut before, _) = f.plan();
    for source in &mut before.sources {
        source.endpoint.node_id = "incoming-logical-name".into();
    }
    for sink in &mut before.sinks {
        sink.endpoint.node_id = "incoming-logical-name".into();
    }
    f.refresh(67, "catalogue-one");
    let (mut after, _) = f.plan();
    for source in &mut after.sources {
        source.endpoint.node_id = "incoming-logical-name".into();
    }
    for sink in &mut after.sinks {
        sink.endpoint.node_id = "incoming-logical-name".into();
    }
    f.runtime
        .validate_publication_rebind(&before, &after, &f.proof())
        .unwrap();
    after.sources[0].inventory_resource_id = Some(uuid::Uuid::new_v4().to_string());
    assert!(f
        .runtime
        .validate_publication_rebind(&before, &after, &f.proof())
        .is_err());
}

#[test]
fn stale_or_revoked_private_owner_proof_cannot_refresh_cached_routes() {
    let f = Fixture::new();
    let proof = f.proof();
    let active = proof.live[GATEWAY].as_ref().unwrap();
    active.authority.store(false, Ordering::Release);
    assert!(proof.validate().is_err());
    let proof = f.proof();
    let active = proof.live[GATEWAY].as_ref().unwrap();
    active.verified.lock().unwrap().as_mut().unwrap().0 =
        Instant::now() - Duration::from_millis(501);
    assert!(proof.validate().is_err());
}

#[test]
fn sixty_four_profile_choices_remain_bounded_to_thirty_two_per_gateway() {
    let f = Fixture::new();
    let (mut plan, mut snapshot) = f.plan();
    let original = snapshot.resources[0].clone();
    plan.runtime_overrides.clear();
    snapshot.assignments.clear();
    snapshot.resources.clear();
    for index in 0..33 {
        let gateway = if index < 32 {
            GATEWAY
        } else {
            "stm32:1123456789abcdef01234567"
        };
        let slot = index % 32;
        let module = format!("{:032x}", index + 1);
        let logical = format!("controller-{index}");
        let mut captured = original.clone();
        captured.summary.id = uuid::Uuid::new_v4().to_string();
        captured
            .summary
            .attributes
            .insert("gatewayId".into(), gateway.into());
        captured
            .summary
            .attributes
            .insert("moduleId".into(), module.clone());
        let mut metadata: Value =
            serde_json::from_str(&captured.summary.attributes["controllerPublicationOverride"])
                .unwrap();
        metadata["module"]["id"] = json!(module);
        metadata["module"]["node"] = json!(slot + 1);
        metadata["module"]["publicationSlot"] = json!(slot);
        metadata["module"]["publicationReportId"] = json!(slot + 1);
        metadata["module"]["configuredDescriptorIndex"] = json!(slot);
        captured
            .summary
            .attributes
            .insert("controllerPublicationOverride".into(), metadata.to_string());
        captured.summary.connections[0].route.id = format!("can:{gateway}:{module}");
        snapshot.assignments.push(LocalAssignment {
            resource_id: logical.clone(),
            source_id: captured.summary.id.clone(),
            ports: vec![],
        });
        snapshot.resources.push(captured);
        plan.runtime_overrides.push(osg_profiles::RuntimeOverride {
            resource_id: logical,
            enabled: false,
        });
    }
    assert_eq!(Staged::prepare(&plan, &snapshot).unwrap().targets.len(), 33);
    let captured = snapshot.resources.last_mut().unwrap();
    captured
        .summary
        .attributes
        .insert("gatewayId".into(), GATEWAY.into());
    let module = captured.summary.attributes["moduleId"].clone();
    captured.summary.connections[0].route.id = format!("can:{GATEWAY}:{module}");
    assert!(
        Staged::prepare(&plan, &snapshot).is_err(),
        "One gateway cannot capture duplicate slot zero as its 33rd target"
    );
}
#[test]
fn malformed_sealed_target_receipts_fail_before_any_transport_request() {
    let f = Fixture::new();
    let (plan, snapshot) = f.plan();
    let staged = Staged::prepare(&plan, &snapshot).unwrap();
    let owner = RunOwner {
        session: uuid::Uuid::new_v4().to_string(),
        generation: 1,
        profile_id: "fixture".into(),
        profile_revision: 1,
        game_id: None,
    };
    let actor =
        Overrides::prepare_io(&staged, &owner, Arc::new(ReadOnlyIo(f.state.clone()))).unwrap();
    let original = actor.slots[0].value.clone();
    for mutation in 0..4 {
        let mut value = original.clone();
        match mutation {
            0 => value.targets.push(value.targets[0].clone()),
            1 => value.targets[0].report = 33,
            2 => value.targets[0].profile["revision"] = json!(0),
            _ => value.targets[0].module = "00000000000000000000000000000000".into(),
        };
        assert!(open_receipt(&sealed(&value).unwrap(), &owner).is_err());
    }
}
