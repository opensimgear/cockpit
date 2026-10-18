use super::*;
use osg_mapping::{
    engine::{Counter, RunToken},
    program::{ActionPack, Endpoint, ResourceAdapter, Sink, SinkKind},
    signal::{ActionParameter, PortValue, ScalarSchema},
};
use osg_profiles::compiler_models::{BackendCapabilities, BranchPlan, CompileSnapshot};
use serde_json::json;
static SERIAL: Mutex<()> = Mutex::new(());
#[derive(Default)]
struct Block {
    entered: Mutex<bool>,
    released: Mutex<bool>,
    wake: Condvar,
}
impl Block {
    fn wait(&self) {
        *self.entered.lock().unwrap() = true;
        self.wake.notify_all();
        let mut done = self.released.lock().unwrap();
        while !*done {
            done = self.wake.wait(done).unwrap();
        }
    }
    fn entered(&self) {
        eventually(|| *self.entered.lock().unwrap());
    }
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.wake.notify_all();
    }
}
#[derive(Default)]
pub(crate) struct Probe {
    calls: Mutex<Vec<(String, Vec<NamedArgument>)>>,
    block: Mutex<Option<(String, Arc<Block>)>>,
    fail: Mutex<Option<String>>,
}
impl Probe {
    fn call(&self, command: &str, args: &[NamedArgument]) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push((command.into(), args.to_vec()));
        let block = self
            .block
            .lock()
            .unwrap()
            .as_ref()
            .filter(|(c, _)| c == command)
            .map(|(_, b)| b.clone());
        if let Some(b) = block {
            b.wait();
        }
        if self.fail.lock().unwrap().as_deref() == Some(command) {
            return Err(format!("failed {command}"));
        }
        Ok(())
    }
    pub(crate) fn fail_command(&self, command: &str) {
        *self.fail.lock().unwrap() = Some(command.into());
    }
    pub(crate) fn count(&self, c: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(s, _)| s == c)
            .count()
    }
    fn hold(&self, c: &str) -> Arc<Block> {
        let block = Arc::new(Block::default());
        *self.block.lock().unwrap() = Some((c.into(), block.clone()));
        block
    }
}
struct Fake {
    key: String,
    probe: Arc<Probe>,
}
impl Driver for Fake {
    fn key(&self) -> String {
        self.key.clone()
    }
    fn receipt(&self) -> Receipt {
        Receipt {
            kind: "physical-action-lease".into(),
            resource_id: self.key.clone(),
            state: json!({"sealedHex":"opaque cleanup"}),
        }
    }
    fn claim(&mut self) -> Result<(), String> {
        self.probe.call("claim", &[])
    }
    fn bind(&mut self, _: &SinkPlan) -> Result<(), String> {
        self.probe.call("bind", &[])
    }
    fn invoke(&mut self, _: &RunOwner, a: &[NamedArgument]) -> Result<(), String> {
        self.probe.call("invoke", a)
    }
    fn renew(&mut self, _: &RunOwner) -> Result<(), String> {
        self.probe.call("renew", &[])
    }
    fn release(&mut self) -> Result<Released, String> {
        self.probe.call("release", &[])?;
        Ok(Released::Restored)
    }
}
fn owner() -> RunOwner {
    RunOwner {
        session: uuid::Uuid::new_v4().to_string(),
        generation: 2,
        profile_id: uuid::Uuid::new_v4().to_string(),
        profile_revision: 3,
        game_id: None,
    }
}
fn plan(count: usize, one_shot: bool) -> ExecutionPlan {
    let mut p = osg_profiles::compiler::compile(
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
    .unwrap();
    for n in 0..count {
        let contract = PortContract::Action {
            parameters: vec![ActionParameter {
                id: "value".into(),
                name: "Value".into(),
                schema: ScalarSchema::Boolean {},
            }],
            semantics: if one_shot {
                ActionSemantics::OneShot
            } else {
                ActionSemantics::Setter
            },
            endpoint: format!("endpoint-{n}"),
            readable: !one_shot,
            display: None,
            ownership: None,
        };
        let endpoint = osg_profiles::Endpoint {
            node_id: format!("panel-{n}"),
            port_id: "write".into(),
        };
        p.sinks.push(SinkPlan {
            endpoint: endpoint.clone(),
            adapter: SinkAdapter::PhysicalAction,
            inventory_resource_id: None,
            observed_port_id: Some(format!("action:set-{n}")),
            contract: contract.clone(),
            token: None,
            preparation: None,
            route: None,
            readiness: PlanReadiness::Ready,
        });
        p.branches.push(BranchPlan {
            sink: endpoint.clone(),
            source_ports: vec![],
            processing_nodes: vec![],
            edge_ids: vec![format!("edge-{n}")],
            required: true,
            readiness: PlanReadiness::Ready,
        });
        p.program.sinks.push(Sink {
            id: format!("edge-{n}"),
            endpoint: Endpoint {
                node_id: endpoint.node_id,
                port_id: endpoint.port_id,
            },
            from: Endpoint {
                node_id: "upstream".into(),
                port_id: "output".into(),
            },
            contract,
            kind: SinkKind::Resource {
                adapter: ResourceAdapter::DeviceAction,
            },
        });
    }
    p
}
fn fixture(
    count: usize,
    one_shot: bool,
) -> (ExecutionPlan, RunOwner, PanelActions, Vec<Arc<Probe>>) {
    let p = plan(count, one_shot);
    let o = owner();
    let probes = (0..count)
        .map(|_| Arc::new(Probe::default()))
        .collect::<Vec<_>>();
    let mut n = 0;
    let actions = PanelActions::prepare_with(&p, &o, |_| {
        let i = n;
        n += 1;
        Ok(Box::new(Fake {
            key: format!("physical-{i}"),
            probe: probes[i].clone(),
        }))
    })
    .unwrap();
    (p, o, actions, probes)
}
fn eventually(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "native physical worker did not finish its bounded operation"
        );
        thread::sleep(Duration::from_millis(2));
    }
}
fn live(actions: &mut PanelActions, plan: &ExecutionPlan) {
    actions.acquire().unwrap();
    eventually(|| actions.poll().acquired);
    eventually(|| actions.bind_delivery(plan).unwrap());
    actions.start().unwrap();
}
fn stopped(actions: &mut PanelActions) {
    actions.stop().unwrap();
    eventually(|| actions.poll().stopped);
    assert!(actions.poll().cleanup_complete);
}
fn effect(owner: &RunOwner, n: usize, sequence: u64, value: bool) -> Effect {
    Effect {
        producer: None,
        observations: vec![],
        run: RunToken {
            session: owner.session.clone(),
            generation: Counter(owner.generation),
        },
        sequence: Counter(sequence),
        at_us: Counter(sequence),
        sink_id: format!("edge-{n}"),
        endpoint: Endpoint {
            node_id: format!("panel-{n}"),
            port_id: "write".into(),
        },
        sample: SignalSample::Known(SignalValue::ActionPack(ActionPack {
            arguments: vec![NamedArgument {
                id: "value".into(),
                value: PortValue::Boolean(value),
            }],
        })),
        reason: ReportReason::Change,
    }
}
#[test]
fn capture_is_read_only_and_live_dispatch_uses_bound_owner_with_fifo_and_no_coalescing() {
    let _serial = SERIAL.lock().unwrap();
    let (p, o, mut a, probes) = fixture(1, false);
    assert_eq!(a.capture_receipts().len(), 1);
    assert!(probes[0].calls.lock().unwrap().is_empty());
    assert!(a.start().is_err());
    assert!(a.bind_delivery(&p).is_err());
    live(&mut a, &p);
    assert_eq!(probes[0].count("claim"), 1);
    assert_eq!(probes[0].count("bind"), 1);
    assert!(a.acquire().is_err());
    assert!(a.start().is_err());
    a.dispatch(&[
        effect(&o, 0, 1, true),
        effect(&o, 0, 2, false),
        effect(&o, 0, 3, true),
    ])
    .unwrap();
    eventually(|| probes[0].count("invoke") == 3);
    let values = probes[0]
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(c, _)| c == "invoke")
        .map(|(_, v)| v[0].value.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        vec![
            PortValue::Boolean(true),
            PortValue::Boolean(false),
            PortValue::Boolean(true)
        ]
    );
    stopped(&mut a);
    assert_eq!(probes[0].count("release"), 1);
}
#[test]
fn stop_during_claim_is_nonblocking_and_late_claim_cannot_start_or_invoke() {
    let _serial = SERIAL.lock().unwrap();
    let (p, o, mut a, probes) = fixture(1, false);
    let block = probes[0].hold("claim");
    a.acquire().unwrap();
    block.entered();
    let status = a.stop().unwrap();
    assert!(!status.stopped);
    assert!(a.dispatch(&[effect(&o, 0, 1, true)]).is_err());
    assert!(a.bind_delivery(&p).is_err());
    assert!(a.start().is_err());
    block.release();
    eventually(|| a.poll().stopped);
    assert!(a.poll().cleanup_complete);
    assert_eq!(probes[0].count("invoke"), 0);
    assert_eq!(probes[0].count("release"), 1);
}
#[test]
fn batch_loss_revokes_every_affected_queue_before_earlier_tuple_but_independent_endpoint_runs() {
    let _serial = SERIAL.lock().unwrap();
    let (p, o, mut a, probes) = fixture(2, false);
    live(&mut a, &p);
    let mut loss = effect(&o, 0, 3, false);
    loss.reason = ReportReason::SourceLost;
    loss.sample = SignalSample::Stale;
    a.dispatch(&[effect(&o, 0, 1, true), effect(&o, 1, 2, true), loss])
        .unwrap();
    eventually(|| probes[0].count("release") == 1 && probes[1].count("invoke") == 1);
    assert_eq!(probes[0].count("invoke"), 0);
    assert!(a.poll().required_failed);
    a.dispatch(&[effect(&o, 0, 4, true)]).unwrap();
    assert_eq!(probes[0].count("invoke"), 0);
    stopped(&mut a);
}
#[test]
fn in_flight_output_finishes_but_source_loss_discards_every_later_queued_tuple_without_reclaim() {
    let _serial = SERIAL.lock().unwrap();
    let (p, o, mut a, probes) = fixture(1, true);
    live(&mut a, &p);
    let block = probes[0].hold("invoke");
    a.dispatch(&[effect(&o, 0, 1, true)]).unwrap();
    block.entered();
    for seq in 2..8 {
        a.dispatch(&[effect(&o, 0, seq, true)]).unwrap();
    }
    let mut loss = effect(&o, 0, 8, false);
    loss.sample = SignalSample::Unknown;
    a.dispatch(&[loss]).unwrap();
    block.release();
    eventually(|| a.poll().stopped);
    assert_eq!(probes[0].count("invoke"), 1);
    assert_eq!(probes[0].count("claim"), 1);
    assert_eq!(probes[0].count("release"), 1);
    assert!(a.poll().cleanup_complete);
}
#[test]
fn bounded_backpressure_faults_endpoint_and_discards_queued_one_shots_instead_of_replaying() {
    let _serial = SERIAL.lock().unwrap();
    let (p, o, mut a, probes) = fixture(1, true);
    live(&mut a, &p);
    let block = probes[0].hold("invoke");
    a.dispatch(&[effect(&o, 0, 1, true)]).unwrap();
    block.entered();
    let queued = (2..(QUEUE as u64 + 3))
        .map(|seq| effect(&o, 0, seq, true))
        .collect::<Vec<_>>();
    assert!(a.dispatch(&queued).is_err());
    assert!(a.poll().required_failed);
    block.release();
    eventually(|| a.poll().stopped);
    assert_eq!(probes[0].count("invoke"), 1);
    assert!(a.poll().cleanup_complete);
}
#[test]
fn old_run_duplicate_sequence_wrong_sink_and_baseline_one_shot_never_enter_worker() {
    let _serial = SERIAL.lock().unwrap();
    for kind in 0..4 {
        let (p, o, mut a, probes) = fixture(1, true);
        live(&mut a, &p);
        let mut e = effect(&o, 0, 1, true);
        match kind {
            0 => e.run.generation = Counter(1),
            1 => e.sequence = Counter(0),
            2 => e.sink_id = "other".into(),
            _ => e.reason = ReportReason::Baseline,
        };
        assert!(a.dispatch(&[e]).is_err());
        stopped(&mut a);
        assert_eq!(probes[0].count("invoke"), 0);
    }
}
#[test]
fn initial_unknown_baseline_is_not_an_action_and_cleanup_error_keeps_receipt_unresolved() {
    let _serial = SERIAL.lock().unwrap();
    let (p, o, mut a, probes) = fixture(1, false);
    live(&mut a, &p);
    let mut e = effect(&o, 0, 1, false);
    e.reason = ReportReason::Baseline;
    e.sample = SignalSample::Unknown;
    a.dispatch(&[e]).unwrap();
    assert_eq!(probes[0].count("invoke"), 0);
    assert!(!a.poll().required_failed);
    *probes[0].fail.lock().unwrap() = Some("release".into());
    a.stop().unwrap();
    eventually(|| a.poll().stopped);
    assert!(!a.poll().cleanup_complete);
    assert!(a
        .poll()
        .diagnostics
        .iter()
        .any(|d| d.contains("cleanup remains unresolved")));
    assert_eq!(a.capture_receipts().len(), 1);
}
#[test]
fn renewal_happens_without_ui_pumping_and_failure_revokes_without_retry() {
    let _serial = SERIAL.lock().unwrap();
    let (p, o, mut a, probes) = fixture(1, false);
    live(&mut a, &p);
    assert_eq!(probes[0].count("renew"), 1);
    *probes[0].fail.lock().unwrap() = Some("renew".into());
    eventually(|| a.poll().stopped);
    assert_eq!(probes[0].count("renew"), 2);
    assert!(a.poll().required_failed);
    assert!(
        a.dispatch(&[effect(&o, 0, 1, true)]).is_ok(),
        "revoked queue ignores later tuples without reopening ownership"
    );
    assert_eq!(probes[0].count("invoke"), 0);
    assert_eq!(probes[0].count("claim"), 1);
    assert!(a.poll().cleanup_complete);
}
#[test]
fn optional_unsupported_is_reported_required_unsupported_and_aliases_block_before_mutation() {
    let _serial = SERIAL.lock().unwrap();
    let mut p = plan(1, false);
    p.branches[0].required = false;
    let o = owner();
    let a =
        PanelActions::prepare_with(&p, &o, |_| Err("missing advertised capture".into())).unwrap();
    assert!(a.is_empty());
    assert_eq!(a.optional_unavailable().len(), 1);
    p.branches[0].required = true;
    assert!(
        PanelActions::prepare_with(&p, &o, |_| Err("missing advertised capture".into())).is_err()
    );
    let probe = Arc::new(Probe::default());
    assert!(
        PanelActions::prepare_with(&plan(2, false), &o, |_| Ok(Box::new(Fake {
            key: "same endpoint".into(),
            probe: probe.clone()
        })))
        .is_err()
    );
    assert!(probe.calls.lock().unwrap().is_empty());
    assert!(
        PanelActions::prepare_with(&plan(MAX_ENDPOINTS + 1, false), &o, |s| Ok(Box::new(
            Fake {
                key: s.endpoint.node_id.clone(),
                probe: probe.clone()
            }
        )))
        .is_err()
    );
}
#[test]
fn native_worker_budget_is_shared_until_outgoing_owner_has_released_and_cleanup_only_recovery_cannot_claim(
) {
    let _serial = SERIAL.lock().unwrap();
    let (p, _, mut a, probes) = fixture(MAX_ENDPOINTS, false);
    let holds = probes.iter().map(|p| p.hold("claim")).collect::<Vec<_>>();
    a.acquire().unwrap();
    for b in &holds {
        b.entered();
    }
    let (_, _, mut incoming, other) = fixture(1, false);
    assert!(incoming.acquire().is_err());
    assert_eq!(other[0].count("claim"), 0);
    a.stop().unwrap();
    for b in holds {
        b.release();
    }
    eventually(|| a.poll().stopped);
    eventually(|| WORKERS.load(Ordering::Acquire) == 0);
    incoming.recovery = true;
    incoming.stop().unwrap();
    eventually(|| incoming.poll().stopped);
    assert_eq!(other[0].count("claim"), 0);
    assert_eq!(other[0].count("invoke"), 0);
    assert_eq!(other[0].count("release"), 1);
    assert!(incoming.start().is_err());
    assert!(incoming.bind_delivery(&p).is_err());
}
#[test]
fn prepared_stop_never_spawns_or_claims_and_bad_delivery_plan_does_not_create_authority() {
    let _serial = SERIAL.lock().unwrap();
    let (_, _, mut a, probes) = fixture(1, false);
    let status = a.stop().unwrap();
    assert!(status.stopped && status.cleanup_complete);
    assert!(a.acquire().is_err());
    assert!(probes[0].calls.lock().unwrap().is_empty());
    let (mut p, _, mut a, probes) = fixture(1, false);
    a.acquire().unwrap();
    eventually(|| a.poll().acquired);
    p.sinks[0].endpoint.port_id = "other".into();
    assert!(a.bind_delivery(&p).is_err());
    assert!(a.start().is_err());
    stopped(&mut a);
    assert_eq!(probes[0].count("invoke"), 0);
}

#[test]
fn unbounded_or_wrongly_named_tuples_are_rejected_before_queue_copy() {
    let _serial = SERIAL.lock().unwrap();
    for kind in 0..3 {
        let (p, o, mut a, probes) = fixture(1, false);
        live(&mut a, &p);
        let mut e = effect(&o, 0, 1, true);
        if let SignalSample::Known(SignalValue::ActionPack(pack)) = &mut e.sample {
            match kind {
                0 => pack.arguments[0].id = "wrong".into(),
                1 => pack.arguments[0].value = PortValue::Text("x".repeat(1_000_000)),
                _ => pack.arguments.extend((0..100).map(|_| NamedArgument {
                    id: "value".into(),
                    value: PortValue::Boolean(true),
                })),
            }
        }
        assert!(a.dispatch(&[e]).is_err());
        stopped(&mut a);
        assert_eq!(probes[0].count("invoke"), 0);
    }
}

#[test]
fn dormant_branches_are_sealed_without_claim_and_late_ready_admits_only_original_endpoint_before_start(
) {
    let _serial = SERIAL.lock().unwrap();
    let mut p = plan(1, false);
    p.branches[0].readiness = PlanReadiness::Dormant;
    let o = owner();
    let probe = Arc::new(Probe::default());
    let mut a = PanelActions::prepare_with(&p, &o, |_| {
        Ok(Box::new(Fake {
            key: "anticipated".into(),
            probe: probe.clone(),
        }))
    })
    .unwrap();
    let original = serde_json::to_value(a.capture_receipts()).unwrap();
    a.acquire().unwrap();
    assert!(a.poll().acquired && a.poll().bound);
    assert_eq!(probe.count("claim"), 0);
    a.acquire_ready(&p).unwrap();
    assert_eq!(probe.count("claim"), 0);
    let mut changed = p.clone();
    changed.program.sinks[0].from.node_id = "new-program".into();
    assert!(a.acquire_ready(&changed).is_err());
    assert_eq!(probe.count("claim"), 0);
    p.branches[0].readiness = PlanReadiness::Ready;
    a.acquire_ready(&p).unwrap();
    eventually(|| a.poll().acquired);
    eventually(|| a.bind_delivery(&p).unwrap());
    a.acquire_ready(&p).unwrap();
    assert_eq!(probe.count("claim"), 1);
    assert_eq!(
        serde_json::to_value(a.capture_receipts()).unwrap(),
        original
    );
    a.start().unwrap();
    assert!(a.acquire_ready(&p).is_err());
    a.dispatch(&[effect(&o, 0, 1, true)]).unwrap();
    eventually(|| probe.count("invoke") == 1);
    stopped(&mut a);
}
#[test]
fn dormant_required_unsupported_blocks_before_spawn_and_inert_stop_never_claims() {
    let _serial = SERIAL.lock().unwrap();
    let mut p = plan(1, false);
    p.branches[0].readiness = PlanReadiness::Dormant;
    let o = owner();
    assert!(
        PanelActions::prepare_with(&p, &o, |_| Err("unsupported anticipated endpoint".into()))
            .is_err()
    );
    let probe = Arc::new(Probe::default());
    let mut a = PanelActions::prepare_with(&p, &o, |_| {
        Ok(Box::new(Fake {
            key: "anticipated".into(),
            probe: probe.clone(),
        }))
    })
    .unwrap();
    a.acquire().unwrap();
    a.start().unwrap();
    let mut absent = effect(&o, 0, 1, false);
    absent.reason = ReportReason::SourceLost;
    absent.sample = SignalSample::Unavailable;
    a.dispatch(&[absent]).unwrap();
    assert!(
        !a.poll().required_failed,
        "an absent game cannot fault a deliberately dormant endpoint"
    );
    assert!(a.dispatch(&[effect(&o, 0, 2, true)]).is_err());
    stopped(&mut a);
    assert_eq!(probe.count("claim"), 0);
    assert_eq!(probe.count("release"), 0);
}

fn fresh_token(generation: &str) -> osg_profiles::resources::ResourceToken {
    osg_profiles::resources::ResourceToken {
        inventory_session: "native".into(),
        resource_id: "resource".into(),
        generation: generation.into(),
        capability_revision: "catalogue".into(),
        route_id: "exact-route".into(),
        provider_session: "provider".into(),
        provider_generation: "exact-boot-config".into(),
        port_id: "action:set".into(),
        contract_hash: "exact-hash".into(),
        access: osg_profiles::resources::Access::Invoke,
    }
}
#[test]
fn pre_start_real_token_refresh_rebinds_and_renews_same_lease_once_without_reclaim() {
    let _serial = SERIAL.lock().unwrap();
    let (mut p, _, mut a, probes) = fixture(1, false);
    a.acquire().unwrap();
    eventually(|| a.poll().acquired);
    eventually(|| a.bind_delivery(&p).unwrap());
    assert!(a.owns_endpoint(&p.sinks[0].endpoint));
    // A completed bind may already be visible on the first poll. Hold the
    // refresh itself so the pending assertion has a deterministic cause.
    let refresh = probes[0].hold("bind");
    p.sinks[0].token = Some(fresh_token("2"));
    let ready_while_blocked = a.bind_delivery(&p).unwrap();
    refresh.entered();
    let start_while_blocked = a.start();
    refresh.release();
    assert!(!ready_while_blocked);
    assert!(start_while_blocked.is_err());
    eventually(|| a.bind_delivery(&p).unwrap());
    assert_eq!(probes[0].count("claim"), 1);
    assert_eq!(probes[0].count("bind"), 2);
    assert_eq!(probes[0].count("renew"), 2);
    a.bind_delivery(&p).unwrap();
    assert_eq!(probes[0].count("bind"), 2);
    a.start().unwrap();
    p.sinks[0].token = Some(fresh_token("3"));
    assert!(a.bind_delivery(&p).is_err());
    stopped(&mut a);
}
#[test]
fn failed_token_rebind_revokes_and_releases_without_reclaim_or_output() {
    let _serial = SERIAL.lock().unwrap();
    let (mut p, _, mut a, probes) = fixture(1, false);
    a.acquire().unwrap();
    eventually(|| a.poll().acquired);
    eventually(|| a.bind_delivery(&p).unwrap());
    *probes[0].fail.lock().unwrap() = Some("bind".into());
    p.sinks[0].token = Some(fresh_token("changed"));
    a.bind_delivery(&p).unwrap();
    eventually(|| a.poll().stopped);
    assert!(a.poll().required_failed && a.poll().cleanup_complete);
    assert_eq!(probes[0].count("claim"), 1);
    assert_eq!(probes[0].count("invoke"), 0);
    assert!(a.bind_delivery(&p).is_err());
}

/// An isolated test driver, never a provider or production sealing fallback.
pub(crate) fn prepared_probe(
    plan: &ExecutionPlan,
    owner: &RunOwner,
) -> (PanelActions, Vec<Arc<Probe>>) {
    let probes = (0..plan.sinks.len())
        .map(|_| Arc::new(Probe::default()))
        .collect::<Vec<_>>();
    let mut n = 0;
    let actions = PanelActions::prepare_with(plan, owner, |sink| {
        let index = n;
        n += 1;
        Ok(Box::new(Fake {
            key: format!("{}:{}", sink.endpoint.node_id, sink.endpoint.port_id),
            probe: probes[index].clone(),
        }))
    })
    .unwrap();
    (actions, probes)
}

#[test]
fn withdrawn_optional_dormant_slot_never_claims_during_late_admission() {
    let _serial = SERIAL.lock().unwrap();
    let mut p = plan(1, false);
    p.branches[0].required = false;
    p.branches[0].readiness = PlanReadiness::Dormant;
    let o = owner();
    let (mut a, probes) = prepared_probe(&p, &o);
    let receipts = a.capture_receipts();
    a.acquire().unwrap();
    a.revoke_optional(
        &p.sinks[0].endpoint,
        "Native optional proof was lost".into(),
    )
    .unwrap();
    p.branches[0].readiness = PlanReadiness::Ready;
    a.acquire_ready(&p).unwrap();
    assert!(a.bind_delivery(&p).unwrap());
    a.start().unwrap();
    assert!(a.dispatch(&[effect(&o, 0, 1, true)]).is_err());
    stopped(&mut a);
    assert_eq!(probes[0].count("claim"), 0);
    assert_eq!(probes[0].count("release"), 0);
    assert_eq!(probes[0].count("invoke"), 0);
    assert_eq!(
        serde_json::to_value(a.capture_receipts()).unwrap(),
        serde_json::to_value(receipts).unwrap()
    );
    let (p, _, mut required, _) = fixture(1, false);
    assert!(required
        .revoke_optional(&p.sinks[0].endpoint, "lost".into())
        .is_err());
    stopped(&mut required);
}
