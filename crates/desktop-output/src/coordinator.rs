//! Parent owns the in-flight ledger before dispatch. Pipe/process failure never
//! proves that a key-down was not submitted and never causes automatic replay.
use crate::transport::ProcessChannel;
use crate::{protocol::*, reducer::Reducer};
use osg_mapping::{
    desktop::{Control, Delivery, Intent},
    engine::{typed::Token, Counter, RunToken},
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
pub const MAX_JOURNAL_BYTES: usize = 131_072;
#[path = "coordinator/guards.rs"]
mod guards;
#[path = "coordinator/delivery.rs"]
mod delivery;
#[path="coordinator/journal.rs"]
mod journal;
#[path = "coordinator/pointer_batch.rs"]
mod pointer_batch;
#[path = "coordinator/scopes.rs"]
mod scopes;
pub use scopes::ScopeUpdate;
#[path = "coordinator/transaction.rs"]
mod transaction;

pub trait Channel: Send {
    fn request(&mut self, request: &Request) -> Result<Ack, String>;
    /// An exact queued request timed out while waiting for its first reply.
    fn has_timed_out_reply(&self) -> bool { false }
    /// Sidecar identity only; recording channels provide no native correlation.
    fn audit_id(&self) -> Option<crate::audit::Id> {
        None
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub phase: String,
    pub enabled: bool,
    pub qualified: bool,
    pub held: Vec<Control>,
    pub possible_held: Vec<Control>,
    pub in_flight: Option<Request>,
    pub uncertain: bool,
    pub cleanup_complete: bool,
    pub error: Option<String>,
    #[serde(default)]
    pub runs: Vec<RunScope>,
    #[serde(default)]
    pub owners: Vec<OwnerSnapshot>,
    #[serde(default)]
    pub shared_controls: usize,
    #[serde(default)] pub journal_owner:Option<RunToken>,
    #[serde(default)] pub journal_scope:Option<Scope>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunScope {
    pub run: RunToken,
    pub scope: Scope,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerSnapshot {
    pub producer: Token,
    pub action_id: String,
    pub controls: Vec<Control>,
}
pub struct Coordinator {
    channel: Box<dyn Channel>,
    reducer: Reducer,
    sequence: u64,
    snapshot: Snapshot,
    clock: fn() -> u64,
    journal: Option<PathBuf>,
    journal_store: std::cell::RefCell<Option<journal::Store>>,
    terminal_stopped: bool,
    stop_committed: bool,
    dispatch_gate: Option<Arc<dyn Fn(&Request) -> bool + Send + Sync>>,
    guard_deliveries: std::collections::BTreeMap<u64, Delivery>,
    scope_epochs: std::collections::BTreeMap<u64, ScopeUpdate>,
    scope_observer: Option<Arc<dyn Fn(ScopeUpdate) + Send + Sync>>,
    admission: Option<ScopeAdmission>,
    projection_revision: u64,
    #[cfg(test)]
    persist_attempts: std::cell::Cell<usize>,
    #[cfg(test)]
    fail_persist_at: std::cell::Cell<Option<usize>>,
}
impl Coordinator {
    fn audit_outcome(&self, request: &Request, ack: Option<&Ack>, error: u64) {
        let Some(channel) = self.channel.audit_id() else { return; };
        let operation = match &request.command {
            Command::Deliver { delivery } | Command::DeliverGuarded { delivery, .. } => delivery.operation_id.0,
            Command::DeliverPointerBatch { deliveries } => deliveries.first().map_or(0, |d| d.operation_id.0),
            _ => 0,
        };
        crate::audit::record(crate::audit::Kind::ParentOutcome, &[
            channel, request.sequence.0, operation,
            ack.map(crate::worker::audit_ack_flags).unwrap_or(0),
            ack.map(|value| value.attempted as u64).unwrap_or(0),
            ack.map(|value| value.confirmed as u64).unwrap_or(0), error,
        ]);
    }
    pub fn spawn(enabled: bool, qualified: bool) -> Result<Self, String> {
        if !enabled {
            return Err("Enable keyboard and mouse output explicitly in local settings.".into());
        }
        if !qualified || !crate::qualification_gate() {
            return Err(crate::QUALIFICATION_REASON.into());
        }
        #[cfg(windows)]
        {
            let channel =
                ProcessChannel::spawn(&std::env::current_exe().map_err(|e| e.to_string())?)?;
            Ok(Self::with_channel(
                Box::new(channel),
                crate::windows::now_us,
            ))
        }
        #[cfg(not(windows))]
        {
            Err("Native keyboard and mouse output is available only on Windows; recording remains available.".into())
        }
    }
    /// Explicit recording/test composition. This constructor has no native adapter.
    pub fn with_channel(channel: Box<dyn Channel>, clock: fn() -> u64) -> Self {
        Self {
            channel,
            reducer: Reducer::default(),
            sequence: 0,
            clock,
            journal: None,
            journal_store: Default::default(),
            terminal_stopped: false,
            stop_committed: false,
            dispatch_gate: None,
            guard_deliveries: Default::default(),
            scope_epochs: Default::default(), scope_observer: None, admission: None,
            projection_revision: 0,
            #[cfg(test)]
            persist_attempts: std::cell::Cell::new(0),
            #[cfg(test)]
            fail_persist_at: std::cell::Cell::new(None),
            snapshot: Snapshot {
                phase: "stopped".into(),
                enabled: true,
                qualified: crate::qualification_gate(),
                held: vec![],
                possible_held: vec![],
                in_flight: None,
                uncertain: false,
                cleanup_complete: true,
                error: None,
                runs: vec![],
                owners: vec![],
                shared_controls: 0,
                journal_owner:None,journal_scope:None,
            },
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.clone()
    }
    /// Recheck delivery authority after durable preparation and before IPC.
    /// A denied delivery still reaches the worker as expired work, preserving
    /// operation fences and releasing its previous held contribution normally.
    pub fn set_dispatch_gate(&mut self, gate: Arc<dyn Fn(&Request) -> bool + Send + Sync>) {
        self.dispatch_gate = Some(gate);
    }
    pub fn bind_scope_before_activate(&mut self,owner:&RunToken,scope:&Scope)->Result<(),String>{
        if self.terminal_stopped||!self.snapshot.runs.is_empty()||!self.snapshot.owners.is_empty()||self.snapshot.in_flight.is_some(){return Err("Desktop application scope can be bound only before activation.".into())}
        if self.snapshot.journal_owner.as_ref().is_some_and(|expected|expected!=owner){return Err("Desktop scope does not belong to the bound ProfileSession journal owner.".into())}
        if self.snapshot.journal_scope.as_ref().is_some_and(|expected|expected!=scope){return Err("The captured desktop application scope is immutable.".into())}
        self.snapshot.journal_owner=Some(owner.clone());self.snapshot.journal_scope=Some(scope.clone());self.persist()
    }
    fn update_owners(&mut self) -> bool {
        let shared_controls = self.reducer.collisions();
        let owners = self
            .reducer
            .owners()
            .into_iter()
            .map(|(producer, action_id, controls)| OwnerSnapshot {
                producer,
                action_id,
                controls,
            })
            .collect::<Vec<_>>();
        let changed = self.snapshot.shared_controls != shared_controls || self.snapshot.owners != owners;
        self.snapshot.shared_controls = shared_controls;
        self.snapshot.owners = owners;
        changed
    }
    fn sync_owners(&mut self) -> Result<(), String> {
        self.update_owners();
        self.persist()
    }
    pub fn activate(&mut self, run: RunToken, scope: Scope) -> Result<(), String> {
        if self.snapshot.journal_owner.as_ref().is_some_and(|owner|owner!=&run){return Err("Desktop activation does not match the bound ProfileSession journal owner.".into())}
        if self.snapshot.journal_scope.as_ref().is_some_and(|expected|expected!=&scope){return Err("Desktop activation scope does not match its captured journal scope.".into())}
        if self.snapshot.journal_owner.is_some()&&self.snapshot.journal_scope.is_none(){self.snapshot.journal_scope=Some(scope.clone());}
        self.send(
            Command::Activate {
                run: run.clone(),
                scope: scope.clone(),
            },
            None,
            vec![],
        )?;
        self.snapshot.runs.push(RunScope { run, scope });
        self.snapshot.phase = "waiting-for-neutral".into();
        self.persist()
    }
    pub fn stop_all(&mut self) -> Result<(), String> {
        if self.terminal_stopped {
            return if self.stop_committed { Ok(()) } else {
                Err("Desktop terminal Stop journal commit failed; preserve recovery evidence.".into())
            };
        }
        let resolving_timeout = self.channel.has_timed_out_reply();
        self.send(Command::Stop, None, vec![])?;
        self.terminal_stopped = true;
        self.reducer.stop();
        self.snapshot.phase = "stopped".into();
        self.snapshot.runs.clear();
        // Exact terminal Stop proves cleanup even when the preceding delivery
        // timed out. Keep recovery evidence if the final journal commit fails.
        let was_uncertain = self.snapshot.uncertain;
        self.snapshot.uncertain &= !resolving_timeout;
        if let Err(error) = self.sync_owners() {
            self.snapshot.uncertain = was_uncertain;
            return Err(error);
        }
        self.stop_committed = true;
        Ok(())
    }
    /// Release exact journal ownership only after the terminal Stop was proved.
    /// An asynchronous channel destructor must not delay scope replacement.
    pub fn retire_stopped_journal(&mut self) -> Result<(), String> {
        if !self.stop_committed || !self.snapshot.cleanup_complete || self.snapshot.in_flight.is_some() {
            return Err("Desktop journal ownership requires a completed terminal Stop.".into());
        }
        self.journal_store.borrow_mut().take();
        self.journal = None;
        Ok(())
    }
    /// Requires explicit user recovery. Only the recorded possible holds release.
    pub fn recover(&mut self, controls: Vec<Control>) -> Result<(), String> {
        self.send(
            Command::Recover {
                controls: controls.clone(),
            },
            None,
            controls,
        )?;
        self.snapshot.uncertain = false;
        self.snapshot.possible_held.clear();
        self.snapshot.in_flight = None;
        self.snapshot.phase = "stopped".into();
        self.reducer.stop();
        self.snapshot.runs.clear();
        self.sync_owners()
    }
}

#[cfg(test)]
#[path = "coordinator_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "coordinator/dispatch_gate_tests.rs"]
mod dispatch_gate_tests;
