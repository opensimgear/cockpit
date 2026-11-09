//! Autonomous lease/scope watchdog. Input and response pipes are bounded; neither
//! a stalled UI nor an idle controller postpones lease expiry or focus cleanup.
use crate::{
    protocol::*,
    reducer::{recovery_releases, Reducer},
};
use osg_mapping::{
    desktop::{Control, Intent},
    engine::{Counter, RunToken},
};
use std::{
    cell::Cell,
    io::BufRead,
    sync::Arc,
    time::Duration,
};
#[cfg(test)]
use std::sync::atomic::Ordering;
#[path = "worker/pointer_batch.rs"]
mod pointer_batch;
#[path = "worker/release_guards.rs"]
mod release_guards;
/// Diagnostic context is scoped to one actual adapter submission. Watchdog and
/// error cleanup must never inherit the incoming request's operation identity.
#[derive(Clone, Copy)]
pub(crate) struct SubmissionContext {
    pub request: u64,
    pub operation: u64,
    pub category: u64,
}
thread_local! {
    static SUBMISSION_CONTEXT: Cell<Option<SubmissionContext>> = const { Cell::new(None) };
}
struct SubmissionGuard { previous: Option<SubmissionContext>, entered: bool }
impl SubmissionGuard {
    fn enter(request: u64, operation: u64, category: u64) -> Self {
        // Unit adapters inspect the actual submission scope without starting a
        // global collector or enabling a native input capability.
        let entered = crate::audit::enabled() || cfg!(test);
        let previous = if entered {
            SUBMISSION_CONTEXT.with(|slot| slot.replace(Some(SubmissionContext { request, operation, category })))
        } else { None };
        Self { previous, entered }
    }
}
impl Drop for SubmissionGuard {
    fn drop(&mut self) {
        if self.entered { SUBMISSION_CONTEXT.with(|slot| slot.set(self.previous)); }
    }
}
#[cfg(any(windows, test))]
pub(crate) fn submission_context() -> Option<SubmissionContext> {
    SUBMISSION_CONTEXT.with(Cell::get)
}
/// ok, uncertain, cleanup-complete, skipped, shared-control collision.
pub(crate) fn audit_ack_flags(ack: &Ack) -> u64 {
    u64::from(ack.ok) | (u64::from(ack.uncertain) << 1)
        | (u64::from(ack.cleanup_complete) << 2) | (u64::from(ack.skipped) << 3)
        | (u64::from(ack.collision) << 4)
}
struct Run {
    token: RunToken,
    scope: Scope,
    lease: u64,
    paused: bool,
    registration: u64,
    epoch: u64,
    loss_epoch: u64,
    source_eligible: bool,
    live: bool,
    scope_bound: bool,
}
pub struct Worker<A: Adapter> {
    adapter: A,
    reducer: Reducer,
    runs: Vec<Run>,
    sequence: u64,
    confirmed: Vec<Control>,
    fault: Option<String>,
    cleanup_complete: bool,
    guards: release_guards::ReleaseGuards,
}
impl<A: Adapter> Worker<A> {
    pub fn new(adapter: A) -> Self {
        Self {
            adapter,
            reducer: Reducer::default(),
            runs: Vec::new(),
            sequence: 0,
            confirmed: Vec::new(),
            fault: None,
            cleanup_complete: true,
            guards: Default::default(),
        }
    }
    pub fn held(&self) -> Vec<Control> { self.reducer.held() }
    pub fn fault(&self) -> Option<&str> { self.fault.as_deref() }
    fn cleanup(&mut self) -> bool {
        let mut possible = self.confirmed.clone();
        possible.extend(self.reducer.held());
        self.reducer.stop();
        self.guards.clear();
        let registrations = self.runs.drain(..).filter(|run| run.scope_bound)
            .map(|run| run.registration).collect::<Vec<_>>();
        let result = recovery_releases(&possible).and_then(|events| {
            if events.is_empty() {
                return Ok(());
            }
            let _context = SubmissionGuard::enter(0, 0, 5);
            let n = self.adapter.submit(&events)?;
            if n != native_count(&events) {
                Err("Cleanup SendInput returned a short count.".into())
            } else {
                Ok(())
            }
        });
        // Unsubscription can wait for observer shutdown. Native releases must
        // already have been attempted, including when submission failed.
        for registration in registrations { self.adapter.retire_scope(registration); }
        self.cleanup_complete = result.is_ok();
        if result.is_ok() {
            self.confirmed.clear()
        }
        self.cleanup_complete
    }
    pub fn stop(&mut self) -> bool { self.cleanup() }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<(usize, usize), String> {
        let attempted = native_count(events);
        if attempted == 0 {
            return Ok((0, 0));
        }
        if attempted > MAX_BATCH {
            return Err("Native input batch exceeds 64 records.".into());
        }
        let _context = SubmissionGuard::enter(0, 0, 4);
        let confirmed = self.adapter.submit(events)?;
        if attempted != confirmed {
            return Err(format!(
                "SendInput accepted {confirmed} of {attempted} records; delivery is uncertain."
            ));
        }
        self.confirmed = self.reducer.held();
        Ok((attempted, confirmed))
    }
    pub fn receive(&mut self, request: Request, now: u64) -> Ack {
        let audit_request = request.sequence.0;
        let (audit_operation, audit_category) = match &request.command {
            Command::Deliver { delivery } | Command::DeliverGuarded { delivery, .. } => (delivery.operation_id.0,
                if matches!(delivery.intent, Intent::Cleanup) { 6 } else { 1 }),
            Command::DeliverPointerBatch { deliveries } => (deliveries.first().map_or(0, |d| d.operation_id.0), 1),
            Command::DeliverKeyPair { deliveries } => (deliveries[0].operation_id.0, 1),
            Command::Recover { .. } => (0, 3),
            _ => (0, 2),
        };
        let mut ack = Ack {
            sequence: request.sequence,
            operation_id: None,
            pointer_batch: None,
            released_guards: vec![],
            ok: false,
            attempted: 0,
            confirmed: 0,
            uncertain: false,
            cleanup_complete: self.cleanup_complete,
            skipped: false,
            scope_rejected: false,
            scopes: vec![],
            collision: false,
            error: None,
            held: vec![],
        };
        let outcome = (|| {
            if request.sequence.0
                != self
                    .sequence
                    .checked_add(1)
                    .ok_or("Desktop sequence exhausted.")?
            {
                return Err("Desktop channel sequence gap or replay.".into());
            }
            self.sequence = request.sequence.0;
            if !matches!(request.command, Command::Stop | Command::Recover { .. }) {
                self.tick(now)?;
                if let Some(error) = &self.fault {
                    return Err(error.clone());
                }
            }
            match &request.command {
                Command::Deliver { delivery } | Command::DeliverGuarded { delivery, .. } => ack.operation_id = Some(delivery.operation_id),
                Command::DeliverPointerBatch { deliveries } => ack.pointer_batch = Some(crate::pointer_batch::describe(deliveries)?),
                Command::DeliverKeyPair { deliveries } => {
                    crate::key_pair::validate(deliveries)?;
                    ack.operation_id = Some(deliveries[0].operation_id);
                }
                _ => {}
            }
            if !self.admit(&request, now, &mut ack)? { return Ok(()); }
            let guard = self.guards.validate(&request.command)?;
            let mut events = Vec::new();
            match request.command {
                Command::Activate { run, scope } => {
                    if run.session.is_empty()
                        || run.session.len() > 128
                        || run.generation.0 == 0
                        || self.runs.len() >= 64
                    {
                        return Err("Invalid or excessive desktop run registration.".into());
                    }
                    if self.runs.iter().any(|r| r.token == run) {
                        return Err("Desktop run is already registered.".into());
                    }
                    if let Scope::Application { identity } = &scope {
                        if identity.pid == 0
                            || identity.created.parse::<u64>().ok().is_none_or(|v| v == 0)
                            || identity.executable.len() > 32768
                        {
                            return Err("Invalid application process identity.".into());
                        }
                    }
                    let evidence = match self.adapter.bind_scope(audit_request, &scope) {
                        Ok(evidence) => evidence,
                        Err(error) => {
                            self.fault = Some(error.clone());
                            self.cleanup();
                            return Err(error);
                        }
                    };
                    let paused = !evidence.eligible;
                    self.runs.push(Run {
                        token: run,
                        scope,
                        lease: now.saturating_add(LEASE_US),
                        paused,
                        registration: audit_request, epoch: 1,
                        loss_epoch: evidence.loss_epoch, source_eligible: evidence.eligible, live: true, scope_bound: true,
                    });
                }
                Command::Renew { run } => {
                    self.runs
                        .iter_mut()
                        .find(|r| r.token == run && r.live)
                        .ok_or("Desktop run lease expired.")?
                        .lease = now.saturating_add(LEASE_US);
                }
                Command::Arm {
                    producer,
                    action_id,
                } => {
                    let run = self
                        .runs
                        .iter_mut()
                        .find(|r| r.token == producer.run)
                        .ok_or("Unknown desktop run.")?;
                    if request.deadline_us.is_some_and(|deadline| now >= deadline.0) {
                        // A scope callback may revoke an arm while its retained
                        // journal evidence is being checked. Preserve channel
                        // order, but never authorize that old neutral baseline.
                        run.paused = true;
                        events = self.reducer.pause(&producer.run);
                        ack.skipped = true;
                    } else {
                        run.paused = false;
                        events = self.reducer.arm(producer, action_id)?;
                    }
                }
                Command::Deliver { delivery } | Command::DeliverGuarded { delivery, .. } => {
                    ack.operation_id = Some(delivery.operation_id);
                    let expired = request.deadline_us.is_some_and(|d| now >= d.0)
                        && !matches!(delivery.intent, Intent::Cleanup);
                    let reduction = self.reducer.delivery(&delivery, expired)?;
                    ack.skipped = reduction.skipped;
                    ack.collision = reduction.collision;
                    events = reduction.events;
                }
                Command::DeliverPointerBatch { deliveries } => {
                    events = self.pointer_batch(deliveries, request.deadline_us, now, &mut ack)?;
                }
                Command::DeliverKeyPair { deliveries } => {
                    if request.deadline_us.is_none_or(|deadline| now < deadline.0) {
                        if let Some((next, presses)) = crate::key_pair::project(&self.reducer, &deliveries)? {
                            self.reducer = next;
                            events = presses;
                        }
                    }
                    ack.skipped = events.is_empty();
                }
                Command::Pause { run } => {
                    events = self.reducer.pause(&run);
                    if let Some(r) = self.runs.iter_mut().find(|r| r.token == run) {
                        r.paused = true
                    }
                }
                Command::Recover { controls } => {
                    if !self.runs.is_empty() || self.reducer.owner_count() != 0 {
                        return Err("Recovery requires stopped desktop output.".into());
                    }
                    events = recovery_releases(&controls)?;
                }
                Command::Stop => {
                    if !self.cleanup() {
                        return Err(
                            "Desktop cleanup is incomplete; explicit recovery is required.".into(),
                        );
                    }
                    return Ok(());
                }
            }
            // Eligibility was checked above for the exact run. Cleanup releases
            // remain eligible regardless of scope or action deadlines.
            ack.attempted = native_count(&events);
            let submitted = if ack.attempted == 0 {
                Ok(0)
            } else if ack.attempted > MAX_BATCH {
                Err("Native input batch exceeds 64 records.".into())
            } else {
                let _context = SubmissionGuard::enter(audit_request, audit_operation, audit_category);
                self.adapter.submit(&events)
            };
            let error = match submitted {
                Ok(count) => {
                    ack.confirmed = count;
                    if count == ack.attempted {
                        self.confirmed = self.reducer.held();
                        None
                    } else {
                        Some(format!(
                            "SendInput accepted {count} of {} records; delivery is uncertain.",
                            ack.attempted
                        ))
                    }
                }
                Err(error) => Some(error),
            };
            if let Some(error) = error {
                ack.uncertain = ack.attempted > 0;
                self.fault = Some(error.clone());
                self.cleanup();
                Err(error)
            } else {
                self.guards.prune(&self.reducer);
                if let Some((delivery, duration)) = guard.filter(|_| !ack.skipped) {
                    self.guards.install(audit_request, delivery, duration, now);
                }
                Ok(())
            }
        })();
        ack.ok = outcome.is_ok();
        ack.error = outcome.err();
        ack.held = self.confirmed.clone();
        ack.cleanup_complete = self.cleanup_complete;
        ack.uncertain |= !ack.ok && self.fault.is_some();
        ack.released_guards = self.guards.take_released();
        ack.scopes = self.scope_receipts();
        self.runs.retain(|run| run.live);
        ack
    }
}
pub fn read_frame(input: &mut impl BufRead) -> Result<Option<Vec<u8>>, String> {
    let mut bytes = Vec::new();
    loop {
        let buffer = input.fill_buf().map_err(|e| e.to_string())?;
        if buffer.is_empty() {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Err("Truncated desktop worker frame.".into())
            };
        }
        let newline = buffer.iter().position(|b| *b == b'\n');
        let n = newline.map_or(buffer.len(), |n| n + 1);
        let payload = newline.unwrap_or(n);
        if bytes.len() + payload > FRAME_LIMIT {
            return Err("Desktop worker frame exceeds 64 KiB.".into());
        }
        bytes.extend_from_slice(&buffer[..payload]);
        input.consume(n);
        if newline.is_some() {
            return Ok(Some(bytes));
        }
    }
}
#[path = "worker/scopes.rs"]
mod scopes;
#[path = "worker/serve.rs"]
mod serve;
#[path = "worker/wait.rs"]
mod wait;
pub use serve::serve;

#[cfg(test)]
#[path = "worker/audit_context_tests.rs"]
mod audit_context_tests;
