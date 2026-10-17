//! Isolated script callbacks for the shared reducer. All workers are prepared
//! before takeover; active polling never waits for compilation or a reply.
use osg_mapping::{
    engine::typed::{Phase, ScriptInvocation, ScriptTicket, ScriptTransaction},
    program::{Operation, Program},
};
use osg_mapping_host::TypedWorker;
use std::{collections::BTreeMap, path::Path};
trait Worker: Send {
    fn pending(&self) -> bool;
    fn poll(&mut self) -> Option<(ScriptTicket, Result<ScriptTransaction, String>)>;
    fn alive(&mut self) -> bool;
    fn submit(&mut self, request: ScriptInvocation) -> Result<(), String>;
    fn terminate(&mut self);
}
impl Worker for TypedWorker {
    fn pending(&self) -> bool {
        TypedWorker::pending(self).is_some()
    }
    fn poll(&mut self) -> Option<(ScriptTicket, Result<ScriptTransaction, String>)> {
        TypedWorker::poll(self)
    }
    fn alive(&mut self) -> bool {
        TypedWorker::alive(self)
    }
    fn submit(&mut self, request: ScriptInvocation) -> Result<(), String> {
        TypedWorker::submit(self, request)
    }
    fn terminate(&mut self) {
        TypedWorker::terminate(self)
    }
}
pub(crate) struct Scripts {
    workers: BTreeMap<String, Box<dyn Worker>>,
    pending: BTreeMap<String, ScriptInvocation>,
    faults: BTreeMap<String, String>,
}
impl Scripts {
    pub(crate) fn prepare(program: &Program, executable: &Path) -> Result<Self, String> {
        let mut workers: BTreeMap<String, Box<dyn Worker>> = BTreeMap::new();
        for node in &program.nodes {
            if matches!(node.operation, Operation::Script { .. }) {
                workers.insert(
                    node.id.clone(),
                    Box::new(TypedWorker::spawn(executable, node)?),
                );
            }
        }
        Ok(Self {
            workers,
            pending: BTreeMap::new(),
            faults: BTreeMap::new(),
        })
    }
    pub(crate) fn poll(&mut self, reducer: &mut Phase) -> Vec<String> {
        for (id, worker) in &mut self.workers {
            if let Some((ticket, result)) = worker.poll() {
                if reducer.script_ticket_current(&ticket) {
                    match result {
                        Ok(transaction) => {
                            if let Err(error) = reducer.script_reply(id, &ticket, transaction) {
                                self.faults.insert(id.clone(), error);
                            }
                        }
                        Err(error) => {
                            reducer.script_fault(id, &error);
                            self.faults.insert(id.clone(), error);
                        }
                    }
                }
            }
            if !worker.alive() {
                self.faults.entry(id.clone()).or_insert_with(|| {
                    "Script helper exited; apply again after repairing the script.".into()
                });
                // A helper cannot deliver future transactions after it dies.
                // The reducer withdraws this node's contribution, not the run.
                reducer.script_fault(id, "Script helper exited.");
            }
        }
        for request in reducer.script_requests() {
            self.pending.insert(request.node_id.clone(), request);
        }
        self.pending
            .retain(|_, request| reducer.script_ticket_current(&request.ticket));
        let ready = self
            .pending
            .iter()
            .filter(|(id, _)| {
                self.workers
                    .get(id.as_str())
                    .is_some_and(|worker| !worker.pending())
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in ready {
            let request = self.pending.remove(&id).unwrap();
            if let Some(worker) = self.workers.get_mut(&id) {
                if let Err(error) = worker.submit(request) {
                    reducer.script_fault(&id, &error);
                    self.faults.insert(id, error);
                }
            }
        }
        self.faults
            .iter()
            .map(|(id, error)| format!("Script {id}: {error}"))
            .collect()
    }
    pub(crate) fn stop(&mut self) {
        self.pending.clear();
        for worker in self.workers.values_mut() {
            worker.terminate();
        }
    }
}

#[cfg(test)]
mod tests {
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
    }
    struct Fake(Arc<Mutex<State>>);
    impl Worker for Fake {
        fn pending(&self) -> bool {
            self.0.lock().unwrap().pending.is_some()
        }
        fn poll(&mut self) -> Option<(ScriptTicket, Result<ScriptTransaction, String>)> {
            let mut state = self.0.lock().unwrap();
            let result = state.replies.pop_front()?;
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
            self.0.lock().unwrap().dead = true;
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
                    source: "fn on_start(ctx) { transaction(#{output: scalar(true)}, [], #{}) }"
                        .into(),
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
            faults: BTreeMap::new(),
        };
        let mut phase =
            Phase::new(program(), Counter(1), uuid::Uuid::new_v4().to_string()).unwrap();
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
        assert!(Scripts::prepare(&empty, Path::new("missing-helper")).is_ok());
        assert!(Scripts::prepare(&program(), Path::new("missing-helper")).is_err());
    }
}
