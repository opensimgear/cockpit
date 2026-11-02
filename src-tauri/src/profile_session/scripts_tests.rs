use super::*;
use osg_mapping::{
    engine::{
        typed::{Change, Frame},
        Counter, Reason,
    },
    program::*,
    signal::*,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
#[derive(Default)]
struct State {
    pending: Option<ScriptTicket>,
    replies: VecDeque<Result<ScriptTransaction, String>>,
    submitted: Vec<ScriptTicket>,
    dead: bool,
    fail_submit: bool,
    fail_closed: bool,
    deadline: Option<Instant>,
}
struct Fake(Arc<Mutex<State>>);
impl Worker for Fake {
    fn pending(&self) -> bool {
        self.0.lock().unwrap().pending.is_some()
    }
    fn response_deadline(&self) -> Option<Instant> {
        let state = self.0.lock().unwrap();
        state.pending.as_ref().and(state.deadline)
    }
    fn poll(&mut self) -> Option<(ScriptTicket, Result<ScriptTransaction, String>)> {
        let mut state = self.0.lock().unwrap();
        let result = state.replies.pop_front()?;
        if result.is_err() && state.fail_closed {
            state.dead = true;
        }
        Some((state.pending.take()?, result))
    }
    fn alive(&mut self) -> bool {
        !self.0.lock().unwrap().dead
    }
    fn submit(&mut self, request: ScriptInvocation) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        if state.fail_submit {
            return Err("submit failed".into());
        }
        assert!(state.pending.is_none());
        state.submitted.push(request.ticket.clone());
        state.pending = Some(request.ticket);
        Ok(())
    }
    fn terminate(&mut self) {
        let mut state = self.0.lock().unwrap();
        state.dead = true;
        state.pending = None;
    }
}
fn endpoint(node: &str, port: &str) -> Endpoint {
    Endpoint {
        node_id: node.into(),
        port_id: port.into(),
    }
}
fn program() -> Program {
    let contract = PortContract::State {
        schema: ScalarSchema::Boolean {},
        freshness_ms: 100,
        fallback: None,
    };
    Program {
        sources: vec![SourcePort {
            endpoint: endpoint("physical", "value"),
            contract: contract.clone(),
        }],
        nodes: vec![Node {
            id: "script".into(),
            inputs: vec![InputBinding {
                port_id: "input".into(),
                from: endpoint("physical", "value"),
                contract: contract.clone(),
                edge_id: "wire".into(),
            }],
            outputs: vec![PortDescriptor {
                id: "output".into(),
                name: "Output".into(),
                direction: Direction::Output,
                contract,
            }],
            operation: Operation::Script {
                source: "fn on_start(ctx) { transaction(#{}, #{output: scalar(true)}, []) } fn on_event(ctx) { transaction(ctx.state, #{output: scalar(true)}, []) }".into(),
                parameters: BTreeMap::new(),
            },
        }],
        sinks: vec![],
        controllers: vec![],
    }
}
fn fixture() -> (Scripts, Phase, Arc<Mutex<State>>) {
    let shared = Arc::new(Mutex::new(State::default()));
    let scripts = Scripts {
        workers: BTreeMap::from([(
            "script".into(),
            Box::new(Fake(shared.clone())) as Box<dyn Worker>,
        )]),
        pending: BTreeMap::new(),
        completed: BTreeMap::new(),
        faults: BTreeMap::new(),
    };
    let mut phase = Phase::new(program(), Counter(1), uuid::Uuid::new_v4().to_string()).unwrap();
    feed(&mut phase, 1, Reason::Baseline, true);
    (scripts, phase, shared)
}
fn feed(phase: &mut Phase, sequence: u64, reason: Reason, value: bool) {
    phase
        .accept(Frame {
            source: "physical".into(),
            generation: Counter(1),
            sequence: Counter(sequence),
            captured_us: Counter(sequence),
            reason,
            changes: if matches!(reason, Reason::Gap | Reason::Disconnect) {
                vec![]
            } else {
                vec![Change {
                    port_id: "value".into(),
                    sample: SignalSample::scalar(PortValue::Boolean(value)),
                }]
            },
        })
        .unwrap();
}
fn reply(value: bool) -> ScriptTransaction {
    ScriptTransaction {
        outputs: BTreeMap::from([(
            "output".into(),
            SignalValue::Scalar(PortValue::Boolean(value)),
        )]),
        ..Default::default()
    }
}
#[test]
fn complete_callback_commits_and_helper_death_withdraws_only_its_contribution() {
    let (mut scripts, mut phase, shared) = fixture();
    assert!(scripts.poll(&mut phase).is_empty());
    assert_eq!(shared.lock().unwrap().submitted.len(), 1);
    shared.lock().unwrap().replies.push_back(Ok(reply(true)));
    assert!(scripts.poll(&mut phase).is_empty());
    assert_eq!(
        phase.sample(&endpoint("script", "output")),
        Some(&SignalSample::scalar(PortValue::Boolean(true)))
    );
    shared.lock().unwrap().dead = true;
    assert!(!scripts.poll(&mut phase).is_empty());
    assert_eq!(
        phase.sample(&endpoint("script", "output")),
        Some(&SignalSample::Unknown)
    );
    assert!(phase.active());
}
#[test]
fn stale_error_cannot_fault_replacement_callback_or_consume_a_busy_worker() {
    let (mut scripts, mut phase, shared) = fixture();
    scripts.poll(&mut phase);
    feed(&mut phase, 2, Reason::Gap, false);
    feed(&mut phase, 3, Reason::Baseline, false);
    assert!(scripts.poll(&mut phase).is_empty());
    assert_eq!(shared.lock().unwrap().submitted.len(), 1);
    assert!(scripts.pending.len() <= scripts.workers.len());
    shared
        .lock()
        .unwrap()
        .replies
        .push_back(Err("old callback".into()));
    assert!(scripts.poll(&mut phase).is_empty());
    assert_eq!(shared.lock().unwrap().submitted.len(), 2);
    shared.lock().unwrap().replies.push_back(Ok(reply(false)));
    assert!(scripts.poll(&mut phase).is_empty());
    assert_eq!(
        phase.sample(&endpoint("script", "output")),
        Some(&SignalSample::scalar(PortValue::Boolean(false)))
    );
    scripts.stop();
    assert!(shared.lock().unwrap().dead);
    assert!(scripts.pending.is_empty());
}
#[test]
fn submission_or_invalid_transaction_fails_the_node_and_empty_program_needs_no_helper() {
    let (mut scripts, mut phase, shared) = fixture();
    shared.lock().unwrap().fail_submit = true;
    assert!(!scripts.poll(&mut phase).is_empty());
    assert!(phase.active());
    let (mut scripts, mut phase, shared) = fixture();
    scripts.poll(&mut phase);
    shared
        .lock()
        .unwrap()
        .replies
        .push_back(Ok(ScriptTransaction {
            outputs: BTreeMap::from([(
                "wrong".into(),
                SignalValue::Scalar(PortValue::Boolean(true)),
            )]),
            ..Default::default()
        }));
    assert!(!scripts.poll(&mut phase).is_empty());
    let empty = Program {
        sources: vec![],
        nodes: vec![],
        sinks: vec![],
        controllers: vec![],
    };
    assert!(Scripts::prepare(&empty, Path::new("missing-helper"), None).is_ok());
    assert!(Scripts::prepare(&program(), Path::new("missing-helper"), None).is_err());
}

#[test]
fn pending_script_deadline_is_not_extended_by_service_and_disappears_on_completion() {
    let (mut scripts, mut phase, shared) = fixture();
    let deadline = Instant::now() + std::time::Duration::from_millis(100);
    shared.lock().unwrap().deadline = Some(deadline);
    assert_eq!(scripts.next_deadline(), None);
    scripts.poll(&mut phase);
    assert_eq!(scripts.next_deadline(), Some(deadline));
    scripts.poll(&mut phase);
    assert_eq!(scripts.next_deadline(), Some(deadline));
    shared
        .lock()
        .unwrap()
        .replies
        .push_back(Ok(ScriptTransaction::default()));
    scripts.poll(&mut phase);
    assert_eq!(scripts.next_deadline(), None);
}

#[test]
fn earliest_pending_worker_deadline_is_removed_by_stop() {
    let (mut scripts, mut phase, shared) = fixture();
    scripts.poll(&mut phase);
    let later = Instant::now() + std::time::Duration::from_millis(100);
    shared.lock().unwrap().deadline = Some(later);
    let second = Arc::new(Mutex::new(State {
        pending: shared.lock().unwrap().pending.clone(),
        deadline: Some(later - std::time::Duration::from_millis(90)),
        ..Default::default()
    }));
    scripts
        .workers
        .insert("second".into(), Box::new(Fake(second)));
    assert_eq!(
        scripts.next_deadline(),
        Some(later - std::time::Duration::from_millis(90))
    );
    scripts.stop();
    assert_eq!(scripts.next_deadline(), None);
}

#[test]
fn committed_idle_failure_revokes_due_script_timer_and_preserves_independent_work() {
    let (mut scripts, _, shared) = fixture();
    let mut definition = program();
    let contract = definition.nodes[0].outputs[0].contract.clone();
    definition.nodes.push(Node {
        id: "independent".into(),
        inputs: vec![InputBinding {
            port_id: "input".into(),
            from: endpoint("physical", "value"),
            contract: contract.clone(),
            edge_id: "independent-edge".into(),
        }],
        outputs: vec![PortDescriptor {
            id: "output".into(),
            name: "Output".into(),
            direction: Direction::Output,
            contract: contract.clone(),
        }],
        operation: Operation::Identity {},
    });
    for node in ["script", "independent"] {
        definition.sinks.push(Sink {
            id: node.into(),
            endpoint: endpoint("destination", node),
            from: endpoint(node, "output"),
            contract: contract.clone(),
            kind: SinkKind::Resource {
                adapter: ResourceAdapter::CompanionFeedback,
            },
        });
    }
    let mut phase = Phase::new(definition, Counter(1), uuid::Uuid::new_v4().to_string()).unwrap();
    feed(&mut phase, 1, Reason::Baseline, false);
    scripts.poll(&mut phase);
    shared.lock().unwrap().replies.push_back(Ok(reply(false)));
    scripts.poll(&mut phase);
    for (node, at) in [("script", 10), ("independent", 20)] {
        phase
            .schedule(
                phase.token(node).unwrap(),
                "due".into(),
                Counter(at),
                BTreeMap::from([(
                    "output".into(),
                    SignalValue::Scalar(PortValue::Boolean(true)),
                )]),
            )
            .unwrap();
    }
    phase.drain();
    phase.drain_reports();
    shared.lock().unwrap().dead = true;
    assert!(!scripts.revoke_failed(&mut phase).is_empty());
    phase.advance(Counter(20)).unwrap();
    assert_eq!(
        phase.sample(&endpoint("script", "output")),
        Some(&SignalSample::Unknown)
    );
    let effects = phase.drain();
    assert!(!effects.iter().any(|effect| effect.sink_id == "script"
        && effect.sample == SignalSample::scalar(PortValue::Boolean(true))));
    assert!(effects.iter().any(|effect| effect.sink_id == "independent"
        && effect.sample == SignalSample::scalar(PortValue::Boolean(true))));
    assert!(phase.active());
}

#[test]
fn response_timeout_is_a_revocation_before_mapping_time_advances() {
    let (mut scripts, mut phase, shared) = fixture();
    scripts.poll(&mut phase);
    shared.lock().unwrap().deadline = Some(Instant::now() - std::time::Duration::from_millis(1));
    let before = phase.at_us();
    assert!(!scripts.revoke_failed(&mut phase).is_empty());
    assert_eq!(phase.at_us(), before);
    assert!(shared.lock().unwrap().dead);
    assert_eq!(scripts.next_deadline(), None);
    assert_eq!(
        phase.sample(&endpoint("script", "output")),
        Some(&SignalSample::Unknown)
    );
}

#[test]
fn queued_callback_error_cannot_run_its_due_timer_before_revocation() {
    let (mut scripts, mut phase, shared) = fixture();
    scripts.poll(&mut phase);
    phase
        .schedule(
            phase.token("script").unwrap(),
            "due".into(),
            Counter(10),
            BTreeMap::from([(
                "output".into(),
                SignalValue::Scalar(PortValue::Boolean(true)),
            )]),
        )
        .unwrap();
    {
        let mut worker = shared.lock().unwrap();
        worker.fail_closed = true;
        worker
            .replies
            .push_back(Err("callback evaluation failed".into()));
    }
    assert!(!scripts.revoke_failed(&mut phase).is_empty());
    phase.advance(Counter(10)).unwrap();
    assert_eq!(
        phase.sample(&endpoint("script", "output")),
        Some(&SignalSample::Unknown)
    );
    assert!(phase.active());
}

#[test]
fn staged_success_and_nonterminal_error_are_fenced_by_the_following_source_loss() {
    for result in [Ok(reply(true)), Err("obsolete callback".into())] {
        let (mut scripts, mut phase, shared) = fixture();
        scripts.poll(&mut phase);
        shared.lock().unwrap().replies.push_back(result);
        assert!(scripts.revoke_failed(&mut phase).is_empty());
        assert_eq!(scripts.completed.len(), 1);
        assert_eq!(
            phase.sample(&endpoint("script", "output")),
            Some(&SignalSample::Unknown)
        );
        feed(&mut phase, 2, Reason::Gap, false);
        feed(&mut phase, 3, Reason::Baseline, false);
        assert!(scripts.poll(&mut phase).is_empty());
        assert!(scripts.completed.is_empty());
        assert_eq!(shared.lock().unwrap().submitted.len(), 2);
        assert_eq!(
            phase.sample(&endpoint("script", "output")),
            Some(&SignalSample::Unknown)
        );
    }
}

#[test]
fn stopping_clears_a_staged_success_before_it_can_emit() {
    let (mut scripts, mut phase, shared) = fixture();
    scripts.poll(&mut phase);
    shared.lock().unwrap().replies.push_back(Ok(reply(true)));
    scripts.revoke_failed(&mut phase);
    assert_eq!(scripts.completed.len(), 1);
    scripts.stop();
    assert!(scripts.completed.is_empty());
    assert_eq!(
        phase.sample(&endpoint("script", "output")),
        Some(&SignalSample::Unknown)
    );
}
