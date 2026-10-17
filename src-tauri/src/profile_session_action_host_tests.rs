use super::*;
use crate::game_discovery::{Platform, ProcessIdentity};
use crate::profile_session_actions::tests::{prepared_probe, Probe};
use osg_mapping::{
    engine::{typed::Effect, ReportReason, RunToken},
    program::{ActionPack, NamedArgument, SignalSample, SignalValue},
    signal::*,
};
use osg_profiles::{compiler_models::BackendCapabilities, resources::*, store::LocalAssignment, *};
use std::{collections::VecDeque, sync::Mutex};

struct NoGame;
impl Platform for NoGame {
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<serde_json::Value> {
        vec![]
    }
    fn validate(&self, _: &str, _: &str) -> Result<(), String> {
        Err("No process".into())
    }
    fn launch(&self, _: &serde_json::Value) -> Result<ProcessIdentity, String> {
        Err("No process".into())
    }
    fn find(&self, _: &serde_json::Value) -> Result<Option<ProcessIdentity>, String> {
        Ok(None)
    }
    fn alive(&self, _: &ProcessIdentity) -> Result<bool, String> {
        Ok(false)
    }
}
fn state_contract() -> PortContract {
    PortContract::State {
        schema: ScalarSchema::Boolean {},
        freshness_ms: 1000,
        fallback: None,
    }
}
fn parameters() -> Vec<ActionParameter> {
    vec![ActionParameter {
        id: "value".into(),
        name: "Value".into(),
        schema: ScalarSchema::Boolean {},
    }]
}
fn action_contract() -> PortContract {
    PortContract::Action {
        parameters: parameters(),
        semantics: ActionSemantics::Setter,
        endpoint: "endpoint-0".into(),
        readable: true,
        display: None,
        ownership: None,
    }
}
fn port(id: &str, direction: Direction, contract: PortContract) -> Port {
    Port {
        id: id.into(),
        name: id.into(),
        direction,
        contract,
    }
}
fn endpoint(node: &str, port: &str) -> osg_profiles::Endpoint {
    osg_profiles::Endpoint {
        node_id: node.into(),
        port_id: port.into(),
    }
}
fn document(required: bool) -> ProfileDocument {
    let mut doc = default_document();
    doc.resources.push(Resource {
        id: "panel-0".into(),
        name: "Panel".into(),
        kind: ResourceKind::Device,
        required,
        ports: vec![port("write", Direction::Input, action_contract())],
    });
    doc.processing_nodes = vec![
        ProcessingNode {
            id: "constant".into(),
            name: "Constant".into(),
            operation: Operation::Constant {
                value: PortValue::Boolean(true),
            },
            inputs: vec![],
            outputs: vec![port("value", Direction::Output, state_contract())],
        },
        ProcessingNode {
            id: "pack".into(),
            name: "Explicit Setter".into(),
            operation: Operation::SetOnChange {
                arguments: vec!["value".into()],
            },
            inputs: vec![port("value", Direction::Input, state_contract())],
            outputs: vec![port(
                "invoke",
                Direction::Output,
                PortContract::Invocation {
                    parameters: parameters(),
                    semantics: ActionSemantics::Setter,
                },
            )],
        },
    ];
    doc.bindings = vec![
        Binding {
            id: "value".into(),
            from: endpoint("constant", "value"),
            to: endpoint("pack", "value"),
        },
        Binding {
            id: "write".into(),
            from: endpoint("pack", "invoke"),
            to: endpoint("panel-0", "write"),
        },
    ];
    validate_document(&doc).unwrap();
    doc
}
fn observation(generation: u64) -> Observation {
    Observation {
        key: "module-0".into(),
        name: "Native module".into(),
        resource_type: ResourceType::OsgModule,
        evidence: vec![],
        attributes: BTreeMap::new(),
        diagnostics: vec![],
        route: Some(Route {
            id: "can:module-0".into(),
            transport: Transport::Can,
            provider_generation: generation.to_string(),
            attributes: BTreeMap::new(),
            baseline_ready: false,
            writable: true,
            priority: 0,
        }),
        ports: Some(
            PortUpdate::complete(vec![PortDescriptor {
                id: "action:set-0".into(),
                name: "Set".into(),
                direction: Direction::Input,
                contract: action_contract(),
            }])
            .unwrap(),
        ),
    }
}
struct Fixture {
    _directory: tempfile::TempDir,
    host: NativeProfileHost,
    prepared: Prepared,
    run: OwnedRun,
    probe: Arc<Probe>,
    generation: Arc<AtomicU64>,
}
impl Fixture {
    fn new() -> Self {
        Self::with_required(true)
    }
    fn with_required(required: bool) -> Self {
        let directory = crate::test_directory();
        let generation = Arc::new(AtomicU64::new(1));
        let observed = generation.clone();
        let resources = Arc::new(crate::profile_resources::Runtime::new(
            None,
            Arc::new(move |revision| {
                vec![(
                    "module".into(),
                    Ok(ProviderBatch {
                        scope: "module".into(),
                        session: "native-provider".into(),
                        revision,
                        complete: true,
                        observations: vec![observation(observed.load(Ordering::Acquire))],
                    }),
                )]
            }),
            false,
        ));
        resources.refresh_native().unwrap();
        let id = resources
            .request(&json!({"command":"profile_resources_list"}))
            .unwrap()["resources"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let descriptor = observation(1).ports.unwrap().ports.remove(0);
        let assignments = vec![LocalAssignment {
            resource_id: "panel-0".into(),
            source_id: id,
            ports: vec![PortExpectation {
                profile_port_id: "write".into(),
                observed_port_id: descriptor.id.clone(),
                expected_contract_hash: descriptor.contract_hash().unwrap(),
            }],
        }];
        let controllers = Arc::new(osg_controller_input::ControllerService::new(None, false));
        let games = Arc::new(crate::games::GamesRuntime::new(
            Arc::new(
                |_| json!({"ok":true,"state":{"integration":{"running":false},"dcsBios":{"running":false}}}),
            ),
            {
                let fixture_metadata_path = directory.path().join("games.json");
                crate::games::Paths {
                    installations: fixture_metadata_path.with_extension("installations-v1.json"),
                    provider_setup: fixture_metadata_path.with_extension("integration-setup.json"),
                    recovery: fixture_metadata_path.with_extension("recovery.json"),
                }
            },
            Arc::new(NoGame)));
        let companions = Arc::new(crate::companion_host::Runtime::new(
            None,
            Arc::new(osg_mapping_host::companions::CompanionProvider::new(
                Arc::new(|| 0),
            )),
        ));
        let mut host = NativeProfileHost::new(
            resources.clone(),
            controllers.clone(),
            games.clone(),
            companions.clone(),
            Arc::new(crate::gateways::ProfilePanelIo::empty()),
            false,
            Arc::new(AtomicU64::new(1)),
        );
        // Keep production provider/sealing admission intact. The isolated actor
        // substitutes only its serial driver; native compilation/token checks
        // and the actual Host lifecycle below remain production paths.
        let mut candidate = Candidate {
            document: default_document(),
            assignments: vec![],
            read_only: false,
            game_id: None,
            admission_epoch: Some(1),
            stage: None,
        };
        candidate.stage = Some(
            NativeProfileHost::stage(
                &resources,
                &games,
                &companions,
                controllers,
                false,
                &candidate,
            )
            .unwrap(),
        );
        let owner = RunOwner {
            session: uuid::Uuid::new_v4().to_string(),
            generation: 1,
            profile_id: candidate.document.id.clone(),
            profile_revision: 1,
            game_id: None,
        };
        host.prepare(&candidate, &owner).unwrap();
        let document = document(required);
        let mut snapshot = resources.snapshot_for_compile(assignments.clone()).unwrap();
        snapshot.backend = BackendCapabilities::default();
        let report = osg_profiles::compiler::compile(&document, &snapshot);
        assert_eq!(
            report.readiness,
            PlanReadiness::Ready,
            "{:?}",
            report.diagnostics
        );
        let plan = report.plan.clone().unwrap();
        let prepared = Prepared {
            plan: plan.clone(),
            admission: Admission::default(),
            program_hash: report.program_hash.clone().unwrap(),
            capture_fingerprint: report.capture_fingerprint.clone(),
        };
        let (actions, probes) = prepared_probe(&plan, &owner);
        let state = host.run(&owner).unwrap();
        state.document = document.clone();
        state.assignments = assignments;
        state.backend = snapshot.backend;
        state.report = report;
        state.actions = actions;
        let receipts = host.capture(&prepared, &owner, None).unwrap();
        let run = OwnedRun {
            owner,
            snapshot: osg_profiles::session::AppliedSnapshot::capture(&document)
                .unwrap()
                .summary()
                .clone(),
            program_hash: prepared.program_hash.clone(),
            capture_fingerprint: prepared.capture_fingerprint.clone(),
            receipts,
            attempted: vec![],
        };
        Self {
            _directory: directory,
            host,
            prepared,
            run,
            probe: probes[0].clone(),
            generation,
        }
    }
    fn acquire(&mut self) {
        self.run.attempted.push(ApplyPhase::Acquire);
        self.host
            .enter(ApplyPhase::Acquire, &self.prepared, &self.run)
            .unwrap();
    }
    fn arm(&mut self) {
        self.acquire();
        for phase in [ApplyPhase::Baseline, ApplyPhase::Visibility] {
            self.run.attempted.push(phase);
            self.host.enter(phase, &self.prepared, &self.run).unwrap();
        }
        self.host.set_authority(Some(&self.run.owner)).unwrap();
        self.run.attempted.push(ApplyPhase::Start);
        self.host
            .enter(ApplyPhase::Start, &self.prepared, &self.run)
            .unwrap();
    }
    fn effect(&self, sequence: u64) -> Effect {
        Effect {
            run: RunToken {
                session: self.run.owner.session.clone(),
                generation: Counter(self.run.owner.generation),
            },
            sequence: Counter(sequence),
            at_us: Counter(sequence),
            sink_id: "write".into(),
            endpoint: osg_mapping::program::Endpoint {
                node_id: "panel-0".into(),
                port_id: "write".into(),
            },
            sample: SignalSample::Known(SignalValue::ActionPack(ActionPack {
                arguments: vec![NamedArgument {
                    id: "value".into(),
                    value: PortValue::Boolean(true),
                }],
            })),
            reason: ReportReason::Change,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.host.set_authority(None);
        let _ = self.host.neutralize(&self.run);
        let _ = self.host.restore(&self.run, false);
        self.host.resources.shutdown();
        self.host.games.shutdown();
    }
}
fn eventually(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "Native action phase did not finish"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn actual_host_dispatches_the_compiled_constant_setter_once_at_start() {
    let mut f = Fixture::new();
    assert_eq!(f.probe.count("invoke"), 0);
    f.arm();
    eventually(|| f.probe.count("invoke") == 1);
    for _ in 0..3 {
        f.host.health(&f.run).unwrap();
    }
    assert_eq!(f.probe.count("invoke"), 1);
    assert_eq!(f.probe.count("claim"), 1);
    f.host.neutralize(&f.run).unwrap();
    assert_eq!(f.probe.count("release"), 1);
    f.host.restore(&f.run, false).unwrap();
    assert_eq!(f.probe.count("invoke"), 1);
}

#[test]
fn actual_host_acquires_neutral_lease_then_starts_and_restores_without_reclaim() {
    let mut f = Fixture::new();
    assert!(has_receipts(&f.run.receipts));
    assert_eq!(f.probe.count("claim"), 0);
    f.acquire();
    assert_eq!(f.probe.count("claim"), 1);
    assert!(held(f.host.run(&f.run.owner).unwrap()).is_ok());
    assert_eq!(f.probe.count("invoke"), 0);
    let effect = f.effect(1);
    assert!(!dispatch(f.host.run(&f.run.owner).unwrap(), &[effect]).is_empty());
    assert_eq!(f.probe.count("invoke"), 0);
    for phase in [ApplyPhase::Baseline, ApplyPhase::Visibility] {
        f.run.attempted.push(phase);
        f.host.enter(phase, &f.prepared, &f.run).unwrap();
    }
    f.host.set_authority(Some(&f.run.owner)).unwrap();
    f.run.attempted.push(ApplyPhase::Start);
    f.host
        .enter(ApplyPhase::Start, &f.prepared, &f.run)
        .unwrap();
    let effect = f.effect(100);
    dispatch(f.host.run(&f.run.owner).unwrap(), &[effect]);
    eventually(|| f.probe.count("invoke") >= 1);
    f.host.health(&f.run).unwrap();
    f.host.neutralize(&f.run).unwrap();
    assert_eq!(f.probe.count("release"), 1);
    assert_eq!(f.probe.count("claim"), 1);
    f.host.restore(&f.run, false).unwrap();
    assert!(!f.host.prepared.contains_key(&key(&f.run.owner)));
}
#[test]
fn stale_native_route_and_changed_program_never_bind_or_start() {
    let mut f = Fixture::new();
    f.acquire();
    let before = f.probe.count("bind");
    let original = f.prepared.plan.clone();
    f.generation.store(2, Ordering::Release);
    f.host.resources.refresh_native().unwrap();
    assert!(f.host.bind_actions(&f.run.owner, &original).is_err());
    assert_eq!(f.probe.count("bind"), before);
    assert_eq!(f.probe.count("invoke"), 0);
    f.host.run(&f.run.owner).unwrap().report.program_hash = Some("changed".into());
    assert!(f.host.refresh_action_binding(&f.run.owner).is_err());
}
#[test]
fn exact_receipt_and_cleanup_failure_keep_native_recovery_owned() {
    let mut f = Fixture::new();
    f.acquire();
    let mut replaced = f.run.clone();
    replaced.receipts[0].state = json!({"sealedHex":"another owner"});
    assert!(f.host.restore_actions(&replaced, false).is_err());
    assert_eq!(f.probe.count("release"), 0);
    f.probe.fail_command("release");
    assert!(f.host.neutralize(&f.run).is_err());
    assert!(f.host.prepared.contains_key(&key(&f.run.owner)));
    assert!(
        !f.host
            .run(&f.run.owner)
            .unwrap()
            .actions
            .poll()
            .cleanup_complete
    );
    assert!(f.host.restore(&f.run, false).is_err());
    assert!(f.host.prepared.contains_key(&key(&f.run.owner)));
    assert_eq!(f.probe.count("release"), 1);
    f.host.discard_preparation(&f.run.owner);
    assert!(f.host.recovery_actions.contains_key(&key(&f.run.owner)));
    assert_eq!(f.probe.count("claim"), 1);
}
#[test]
fn cancelled_epoch_blocks_claim_and_discard_revokes_pending_actor() {
    let mut f = Fixture::new();
    f.host.admission_epoch.store(2, Ordering::Release);
    assert!(f.host.acquire_actions(&f.run.owner).is_err());
    f.host.discard_preparation(&f.run.owner);
    assert_eq!(f.probe.count("claim"), 0);
    assert_eq!(f.probe.count("release"), 0);
    assert_eq!(f.probe.count("invoke"), 0);
}
#[test]
fn retiring_an_unacquired_owner_never_claims_or_releases() {
    let mut f = Fixture::new();
    f.host.neutralize(&f.run).unwrap();
    f.host.restore(&f.run, false).unwrap();
    assert_eq!(f.probe.count("claim"), 0);
    assert_eq!(f.probe.count("release"), 0);
    assert!(
        f.host.restore_actions(&f.run, false).is_err(),
        "Unknown opaque test recovery must still use production sealed receipt validation"
    );
    let mut no_receipts = f.run.clone();
    no_receipts.receipts.clear();
    assert!(f.host.restore_actions(&no_receipts, false).is_ok());
}
#[test]
fn required_legacy_action_capability_blocks_before_any_native_claim() {
    let mut f = Fixture::new();
    let state = f.host.run(&f.run.owner).unwrap();
    let candidate = Candidate {
        document: state.document.clone(),
        assignments: state.assignments.clone(),
        read_only: false,
        game_id: None,
        admission_epoch: Some(1),
        stage: None,
    };
    let stage = NativeProfileHost::stage(
        &f.host.resources,
        &f.host.games,
        &f.host.companions,
        f.host.controllers.clone(),
        false,
        &candidate,
    )
    .unwrap();
    let c = Candidate {
        stage: Some(stage),
        ..candidate
    };
    let mut owner = f.run.owner.clone();
    owner.session = uuid::Uuid::new_v4().to_string();
    assert!(f.host.prepare(&c, &owner).is_err());
    assert_eq!(f.probe.count("claim"), 0);
    assert!(!f.host.prepared.contains_key(&key(&owner)));
}
#[derive(Default)]
struct Scripted {
    states: Mutex<VecDeque<ActionPoll>>,
    binds: usize,
    stops: usize,
}
impl Actor for Scripted {
    fn poll(&self) -> ActionPoll {
        let mut p = self.states.lock().unwrap();
        if p.len() > 1 {
            p.pop_front().unwrap()
        } else {
            {
                let current = p.front().unwrap();
                ActionPoll {
                    acquired: current.acquired,
                    bound: current.bound,
                    stopped: current.stopped,
                    cleanup_complete: current.cleanup_complete,
                    required_failed: current.required_failed,
                    diagnostics: current.diagnostics.clone(),
                }
            }
        }
    }
    fn bind(&mut self, _: &ExecutionPlan) -> Result<bool, String> {
        self.binds += 1;
        Ok(self.poll().bound)
    }
    fn stop(&mut self) -> Result<ActionPoll, String> {
        self.stops += 1;
        Ok(self.poll())
    }
}
fn status(acquired: bool, bound: bool, stopped: bool, cleanup_complete: bool) -> ActionPoll {
    ActionPoll {
        acquired,
        bound,
        stopped,
        cleanup_complete,
        required_failed: false,
        diagnostics: vec![],
    }
}
#[test]
fn bounded_phase_wait_honors_pending_cleanup_cancellation_and_failure() {
    let mut actor = Scripted {
        states: Mutex::new(VecDeque::from([
            status(false, false, false, false),
            status(true, true, false, false),
        ])),
        ..Default::default()
    };
    let mut keeps = 0;
    wait(
        &mut actor,
        Goal::Acquired,
        None,
        Duration::from_secs(1),
        || Ok(()),
        || {
            keeps += 1;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(keeps, 1);
    assert!(wait(
        &mut actor,
        Goal::Acquired,
        None,
        Duration::ZERO,
        || Err("cancelled".into()),
        || Ok(())
    )
    .unwrap_err()
    .contains("cancelled"));
    actor.states = Mutex::new(VecDeque::from([status(false, false, false, false)]));
    assert!(wait(
        &mut actor,
        Goal::Acquired,
        None,
        Duration::ZERO,
        || Ok(()),
        || Ok(())
    )
    .is_err());
    let mut failed = status(false, false, false, false);
    failed.required_failed = true;
    failed.diagnostics.push("lease lost".into());
    actor.states = Mutex::new(VecDeque::from([failed]));
    assert_eq!(
        wait(
            &mut actor,
            Goal::Acquired,
            None,
            Duration::ZERO,
            || Ok(()),
            || Ok(())
        )
        .unwrap_err(),
        "lease lost"
    );
    actor.states = Mutex::new(VecDeque::from([
        status(false, false, false, false),
        status(false, false, true, true),
    ]));
    wait(
        &mut actor,
        Goal::Restored,
        None,
        Duration::from_secs(1),
        || Ok(()),
        || Ok(()),
    )
    .unwrap();
    assert_eq!(actor.stops, 2);
    actor.states = Mutex::new(VecDeque::from([status(false, false, true, false)]));
    assert!(wait(
        &mut actor,
        Goal::Restored,
        None,
        Duration::ZERO,
        || Ok(()),
        || Ok(())
    )
    .unwrap_err()
    .contains("retain"));
}

#[test]
fn optional_stale_route_revokes_once_and_independent_program_still_starts() {
    let mut f = Fixture::with_required(false);
    f.acquire();
    let original = f.prepared.plan.clone();
    let receipts = f.run.receipts.clone();
    f.generation.store(2, Ordering::Release);
    f.host.resources.refresh_native().unwrap();
    f.host.bind_actions(&f.run.owner, &original).unwrap();
    eventually(|| f.probe.count("release") == 1);
    assert!(held(f.host.run(&f.run.owner).unwrap()).is_ok());
    for phase in [ApplyPhase::Baseline, ApplyPhase::Visibility] {
        f.run.attempted.push(phase);
        f.host.enter(phase, &f.prepared, &f.run).unwrap();
    }
    f.host.set_authority(Some(&f.run.owner)).unwrap();
    f.run.attempted.push(ApplyPhase::Start);
    f.host
        .enter(ApplyPhase::Start, &f.prepared, &f.run)
        .unwrap();
    let health = f.host.health(&f.run).unwrap();
    assert!(health.stop.is_none());
    assert!(!health.degraded.is_empty());
    assert_eq!(f.probe.count("claim"), 1);
    assert_eq!(f.probe.count("bind"), 1);
    assert_eq!(f.probe.count("invoke"), 0);
    assert_eq!(
        serde_json::to_value(&f.run.receipts).unwrap(),
        serde_json::to_value(receipts).unwrap()
    );
    f.host.neutralize(&f.run).unwrap();
    f.host.restore(&f.run, false).unwrap();
    assert_eq!(f.probe.count("release"), 1);
}
#[test]
fn active_endpoint_failure_degrades_its_branch_without_global_stop() {
    let mut f = Fixture::new();
    f.arm();
    f.probe.fail_command("invoke");
    let effect = f.effect(100);
    dispatch(f.host.run(&f.run.owner).unwrap(), &[effect]);
    eventually(|| {
        f.host
            .run(&f.run.owner)
            .unwrap()
            .actions
            .poll()
            .required_failed
    });
    let health = f.host.health(&f.run).unwrap();
    assert!(health.stop.is_none());
    assert!(!health.degraded.is_empty());
    assert!(held(f.host.run(&f.run.owner).unwrap()).is_err());
    f.host.neutralize(&f.run).unwrap();
    assert_eq!(f.probe.count("claim"), 1);
    assert_eq!(f.probe.count("release"), 1);
}
#[test]
fn empty_action_adapter_never_captures_unrelated_inventory() {
    let mut f = Fixture::new();
    let snapshot = f.host.resources.snapshot_for_compile(vec![]).unwrap();
    let plan = osg_profiles::compiler::compile(&default_document(), &snapshot)
        .plan
        .unwrap();
    f.host.run(&f.run.owner).unwrap().actions = prepared_probe(&plan, &f.run.owner).0;
    f.host.resources.shutdown();
    assert!(f.host.refresh_action_binding(&f.run.owner).is_ok());
}
