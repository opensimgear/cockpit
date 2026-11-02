//! Isolated script callbacks for the shared reducer. All workers are prepared
//! before takeover; active polling never waits for compilation or a reply.
use osg_mapping::{
    engine::typed::{Phase, ScriptInvocation, ScriptTicket, ScriptTransaction},
    program::{Operation, Program},
};
use osg_mapping_host::TypedWorker;
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Instant};
pub(crate) type Wake = Arc<dyn Fn() + Send + Sync>;
trait Worker: Send {
    fn pending(&self) -> bool;
    fn poll(&mut self) -> Option<(ScriptTicket, Result<ScriptTransaction, String>)>;
    fn alive(&mut self) -> bool;
    fn submit(&mut self, request: ScriptInvocation) -> Result<(), String>;
    fn terminate(&mut self);
    fn response_deadline(&self) -> Option<Instant>;
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
    fn response_deadline(&self) -> Option<Instant> {
        TypedWorker::response_deadline(self)
    }
}
pub(crate) struct Scripts {
    workers: BTreeMap<String, Box<dyn Worker>>,
    pending: BTreeMap<String, ScriptInvocation>,
    completed: BTreeMap<String, (ScriptTicket, Result<ScriptTransaction, String>)>,
    faults: BTreeMap<String, String>,
}
impl Scripts {
    pub(crate) fn prepare(
        program: &Program,
        executable: &Path,
        wake: Option<Wake>,
    ) -> Result<Self, String> {
        let mut workers: BTreeMap<String, Box<dyn Worker>> = BTreeMap::new();
        for node in &program.nodes {
            if matches!(node.operation, Operation::Script { .. }) {
                workers.insert(
                    node.id.clone(),
                    Box::new(match &wake {
                        Some(wake) => TypedWorker::spawn_with_wake(executable, node, wake.clone())?,
                        None => TypedWorker::spawn(executable, node)?,
                    }),
                );
            }
        }
        Ok(Self {
            workers,
            pending: BTreeMap::new(),
            completed: BTreeMap::new(),
            faults: BTreeMap::new(),
        })
    }
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.workers
            .values()
            .filter_map(|worker| worker.response_deadline())
            .min()
    }
    /// Install only revocations before source admission can advance timers.
    /// Successful replies remain queued until source losses have been applied.
    pub(crate) fn revoke_failed(&mut self, reducer: &mut Phase) -> Vec<String> {
        let now = Instant::now();
        for (id, worker) in &mut self.workers {
            let failure = if worker.response_deadline().is_some_and(|at| at <= now) {
                worker.terminate();
                Some("Script helper response deadline expired.".to_owned())
            } else {
                if !self.completed.contains_key(id) {
                    if let Some(completion) = worker.poll() {
                        self.completed.insert(id.clone(), completion);
                    }
                }
                if !worker.alive() {
                    Some(self.completed.get(id).and_then(|(_, result)| result.as_ref().err()).cloned()
                        .unwrap_or_else(|| "Script helper exited or its transport failed; apply again after repairing the script.".into()))
                } else {
                    None
                }
            };
            if let Some(error) = failure {
                self.completed.remove(id);
                reducer.script_fault(id, &error);
                self.faults.entry(id.clone()).or_insert(error);
            }
        }
        self.faults
            .iter()
            .map(|(id, error)| format!("Script {id}: {error}"))
            .collect()
    }
    pub(crate) fn poll(&mut self, reducer: &mut Phase) -> Vec<String> {
        for (id, worker) in &mut self.workers {
            if let Some((ticket, result)) = self.completed.remove(id).or_else(|| worker.poll()) {
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
        self.completed.clear();
        for worker in self.workers.values_mut() {
            worker.terminate();
        }
    }
}

#[cfg(test)]
#[path = "profile_session/scripts_tests.rs"]
mod tests;
