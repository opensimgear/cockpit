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
};
pub const MAX_JOURNAL_BYTES: usize = 131_072;
#[path="coordinator/journal.rs"]
mod journal;
#[path = "coordinator/pointer_batch.rs"]
mod pointer_batch;

pub trait Channel: Send {
    fn request(&mut self, request: &Request) -> Result<Ack, String>;
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
    #[cfg(test)]
    persist_attempts: std::cell::Cell<usize>,
    #[cfg(test)]
    fail_persist_at: std::cell::Cell<Option<usize>>,
}
impl Coordinator {
    fn audit_outcome(&self, request: &Request, ack: Option<&Ack>, error: u64) {
        let Some(channel) = self.channel.audit_id() else { return; };
        let operation = match &request.command {
            Command::Deliver { delivery } => delivery.operation_id.0,
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
    fn send(
        &mut self,
        command: Command,
        deadline_us: Option<Counter>,
        possible: Vec<Control>,
    ) -> Result<Ack, String> {
        if self.terminal_stopped {
            return Err("Desktop worker is stopped; acquire a fresh coordinator before activation or recovery.".into());
        }
        if self.snapshot.uncertain && !matches!(command, Command::Stop | Command::Recover { .. }) {
            return Err(
                "Desktop delivery is uncertain; explicit stopped-session recovery is required."
                    .into(),
            );
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Desktop channel sequence exhausted.")?;
        // Relative motion cannot acquire or release a held control and is never
        // replayed. Its operation counter is volatile, so successful pointer
        // updates need no recovery-evidence transaction on the lease path.
        let pointer = matches!(&command, Command::Deliver { delivery } if matches!(delivery.intent, Intent::Pointer { .. }))
            || matches!(&command, Command::DeliverPointerBatch { .. });
        let changes_ledger = !pointer && !matches!(command, Command::Renew { .. });
        let request = Request {
            sequence: Counter(self.sequence),
            deadline_us,
            command,
        };
        if let Some(channel) = self.channel.audit_id() {
            let (operation, command, run) = match &request.command {
                Command::Activate { run, .. } => (0, 1, Some(run)),
                Command::Arm { producer, .. } => (0, 2, Some(&producer.run)),
                Command::Deliver { delivery } => (delivery.operation_id.0, 3, Some(&delivery.producer.run)),
                Command::DeliverPointerBatch { deliveries } => (deliveries[0].operation_id.0, 8, Some(&deliveries[0].producer.run)),
                Command::Renew { run } => (0, 4, Some(run)),
                Command::Pause { run } => (0, 5, Some(run)),
                Command::Recover { .. } => (0, 6, None),
                Command::Stop => (0, 7, None),
            };
            let run_alias = run.and_then(|run| crate::audit::alias(crate::audit::AliasKind::Run, run.session.as_bytes())).unwrap_or(0);
            crate::audit::record(crate::audit::Kind::RequestIssued,
                &[channel, request.sequence.0, operation, command, run_alias, run.map_or(0, |run| run.generation.0)]);
        }
        if pointer {
            if let Err(error) = self.verify_current_evidence() {
                self.audit_outcome(&request, None, 1);
                return Err(error);
            }
        }
        if changes_ledger {
            if !self.snapshot.uncertain {
                self.snapshot.possible_held = self.snapshot.held.clone();
            }
            self.snapshot.possible_held.extend(possible);
            self.snapshot.in_flight = Some(request.clone());
            if let Err(error) = self.persist() {
                self.audit_outcome(&request, None, 1);
                return Err(error);
            }
        }
        let result = self.channel.request(&request).and_then(|ack| {
            if ack.sequence != request.sequence {
                return Err("Stale desktop worker acknowledgement.".into());
            }
            if let Command::Deliver { delivery } = &request.command {
                if ack.operation_id != Some(delivery.operation_id) {
                    return Err("Desktop acknowledgement has the wrong operation identity.".into());
                }
            }
            pointer_batch::validate_ack(&request, &ack)?;
            if ack.attempted > MAX_BATCH
                || ack.confirmed > ack.attempted
                || (ack.ok && ack.attempted != ack.confirmed)
            {
                return Err(
                    "Desktop worker acknowledged an incomplete or invalid native batch.".into(),
                );
            }
            crate::reducer::recovery_releases(&ack.held)?;
            if pointer && ack.held != self.snapshot.held {
                return Err("Desktop pointer acknowledgement changed the held-control ledger.".into());
            }
            if matches!(request.command, Command::Stop | Command::Recover { .. })
                && (!ack.cleanup_complete || !ack.held.is_empty())
            {
                return Err("Desktop worker cleanup acknowledgement is incomplete.".into());
            }
            Ok(ack)
        });
        match result {
            Ok(ack) => {
                self.snapshot.held = ack.held.clone();
                self.snapshot.cleanup_complete = ack.cleanup_complete;
                self.snapshot.error = ack.error.clone();
                if ack.uncertain {
                    self.snapshot.uncertain = true;
                    self.snapshot.phase = "faulted".into();
                    let _ = self.persist();
                    self.audit_outcome(&request, Some(&ack), 2);
                    return Err(ack.error.unwrap_or("Uncertain native submission.".into()));
                }
                self.snapshot.in_flight = None;
                if ack.ok {
                    self.snapshot.possible_held = ack.held.clone();
                    if changes_ledger {
                        if let Err(error) = self.persist() {
                            self.snapshot.uncertain = true;
                            self.snapshot.cleanup_complete = false;
                            self.audit_outcome(&request, Some(&ack), 3);
                            return Err(error);
                        }
                    }
                    self.audit_outcome(&request, Some(&ack), 0);
                    Ok(ack)
                } else {
                    self.snapshot.phase = "paused".into();
                    let _ = self.persist();
                    self.audit_outcome(&request, Some(&ack), 4);
                    Err(ack
                        .error
                        .unwrap_or("Desktop worker rejected output.".into()))
                }
            }
            Err(error) => {
                self.snapshot.uncertain = true;
                self.snapshot.cleanup_complete = false;
                self.snapshot.phase = "faulted".into();
                self.snapshot.error = Some(error.clone());
                let _ = self.persist();
                self.audit_outcome(&request, None, 5);
                Err(error)
            }
        }
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
    pub fn arm(&mut self, producer: Token, action_id: String) -> Result<(), String> {
        let mut next = self.reducer.clone();
        next.arm(producer.clone(), action_id.clone())?;
        self.send(
            Command::Arm {
                producer,
                action_id,
            },
            None,
            next.held(),
        )?;
        self.reducer = next;
        self.snapshot.phase = "active".into();
        self.sync_owners()
    }
    /// `source_now_us` is the mapping engine's current explicit clock. Convert its
    /// remaining deadline to the worker's monotonic clock, never compare origins.
    pub fn submit(&mut self, delivery: Delivery, source_now_us: u64) -> Result<Ack, String> {
        let expired = delivery.deadline_us.is_some_and(|d| source_now_us >= d.0)
            && !matches!(delivery.intent, Intent::Cleanup);
        let deadline_us = delivery
            .deadline_us
            .map(|d| Counter((self.clock)().saturating_add(d.0.saturating_sub(source_now_us))));
        let mut next = self.reducer.clone();
        next.delivery(&delivery, expired)?;
        let ack = self.send(
            Command::Deliver {
                delivery: delivery.clone(),
            },
            deadline_us,
            next.held(),
        )?;
        // Worker-clock expiry may occur during durable dispatch/IPC. Mirror the
        // cancellation rather than retaining a hold that was never submitted.
        if ack.skipped && matches!(delivery.intent, Intent::Held { .. }) {
            next = self.reducer.clone();
            next.delivery(&delivery, true)?;
        }
        self.reducer = next;
        // Hold-changing send() calls durably committed the validated ACK;
        // pointer motion preserved that ledger. Owner operation counters are
        // intentionally absent from the journal format.
        if self.update_owners() {
            self.persist()?;
        }
        Ok(ack)
    }
    pub fn renew(&mut self, run: RunToken) -> Result<(), String> {
        self.send(Command::Renew { run }, None, self.reducer.held())?;
        Ok(())
    }
    pub fn pause(&mut self, run: RunToken) -> Result<(), String> {
        let mut next = self.reducer.clone();
        next.pause(&run);
        self.send(Command::Pause { run }, None, next.held())?;
        self.reducer = next;
        self.snapshot.phase = "paused".into();
        self.sync_owners()
    }
    pub fn stop_all(&mut self) -> Result<(), String> {
        if self.terminal_stopped {
            return Ok(());
        }
        self.send(Command::Stop, None, vec![])?;
        self.terminal_stopped = true;
        self.reducer.stop();
        self.snapshot.phase = "stopped".into();
        self.snapshot.runs.clear();
        self.sync_owners()
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
