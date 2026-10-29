//! One owned feeder process per live run, independent from UI polling.
use crate::output_journal::{Phase, Store};
use crate::{
    feeder::{Message, RENEW_US},
    supervisor::Worker,
    vjoy::Binding,
};
use osg_mapping::{
    engine::{Counter, RunToken},
    runtime::RecordingSink,
};
use std::{path::PathBuf, sync::Arc, time::Instant};

pub trait Channel: Send {
    fn request(&mut self, message: &Message) -> Result<(), String>;
    fn alive(&mut self) -> bool;
    fn exit_error(&mut self) -> String {
        "Virtual output helper exited.".into()
    }
    /// Identity of this retained child only; unknown channels grant no handoff proof.
    fn process_id(&self) -> Option<u32> {
        None
    }
    fn terminate(&mut self);
}
impl Channel for Worker {
    fn request(&mut self, message: &Message) -> Result<(), String> {
        Worker::request(self, message)
    }
    fn alive(&mut self) -> bool {
        Worker::alive(self)
    }
    fn exit_error(&mut self) -> String {
        Worker::exit_error(self)
    }
    fn process_id(&self) -> Option<u32> {
        Some(Worker::process_id(self))
    }
    fn terminate(&mut self) {
        Worker::terminate(self)
    }
}
pub type Factory = Arc<dyn Fn() -> Result<Box<dyn Channel>, String> + Send + Sync>;

/// Volatile evidence from an acknowledged, still-live owned output worker.
/// This is never serialized or reconstructed from a journal/PID after restart.
#[derive(Clone)]
pub struct RunningOutput {
    token: RunToken,
    bindings: Vec<Binding>,
    binding_identity: Vec<u8>,
    process_id: u32,
}
impl RunningOutput {
    pub fn token(&self) -> &RunToken {
        &self.token
    }
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }
    pub fn process_id(&self) -> u32 {
        self.process_id
    }
    /// A stored clone does not prove liveness; compare it with a newly captured proof.
    pub fn same_as(&self, current: &Self) -> bool {
        self.token == current.token
            && self.process_id == current.process_id
            && self.binding_identity == current.binding_identity
    }
}

pub struct Session {
    factory: Factory,
    worker: Option<Box<dyn Channel>>,
    token: RunToken,
    bindings: Vec<Binding>,
    cursor: Counter,
    renewal: u64,
    renewed: Instant,
    recovery_required: bool,
    journal: Option<Store>,
}
impl Session {
    pub fn start(
        factory: Factory,
        token: RunToken,
        bindings: Vec<Binding>,
        cursor: Counter,
    ) -> Result<Self, String> {
        Self::start_with_journal(factory, token, bindings, cursor, None)
    }
    pub fn start_with_journal(
        factory: Factory,
        token: RunToken,
        bindings: Vec<Binding>,
        cursor: Counter,
        path: Option<PathBuf>,
    ) -> Result<Self, String> {
        let mut journal = path.map(Store::open).transpose()?;
        if let Some(store) = &mut journal {
            store.begin(token.clone(), bindings.clone())?;
        }
        let mut worker = match factory() {
            Ok(worker) => worker,
            Err(error) => {
                if let Some(store) = &mut journal {
                    store.transition(Phase::Clean)?;
                }
                return Err(error);
            }
        };
        if let Err(error) = worker.request(&Message::Start {
            token: token.clone(),
            bindings: bindings.clone(),
        }) {
            worker.terminate();
            // A missing Start acknowledgement does not prove which slots were acquired.
            // Do not guess ownership or reset every planned slot.
            return Err(format!(
                "Feeder activation failed: {error}. Inspect target ownership before retrying."
            ));
        }
        if let Some(store) = &mut journal {
            if let Err(error) = store.transition(Phase::Confirmed) {
                // Start was acknowledged, but ownership could not be saved. Stop
                // immediately; never return a running session without its record.
                if worker
                    .request(&Message::Stop {
                        token: token.clone(),
                    })
                    .is_ok()
                {
                    let _ = store.transition(Phase::Clean);
                }
                worker.terminate();
                return Err(format!("Unable to persist output ownership: {error}"));
            }
        }
        Ok(Self {
            journal,
            factory,
            worker: Some(worker),
            token,
            bindings,
            cursor,
            renewal: 0,
            renewed: Instant::now(),
            recovery_required: false,
        })
    }
    /// Explicit restart recovery requires a previously acknowledged run.
    /// Intent/release gaps require inspection and explicit no-output acknowledgement.
    pub fn recover_stored(factory: Factory, path: PathBuf) -> Result<(), String> {
        Self::recover_stored_checked(factory, path, None)
    }
    /// Restore only the complete journal which a native cleanup caller inspected.
    /// Comparison happens after taking its storage lock and before opening a feeder.
    pub fn recover_stored_exact(
        factory: Factory,
        path: PathBuf,
        expected: &crate::output_journal::Record,
    ) -> Result<(), String> {
        Self::recover_stored_checked(factory, path, Some(expected))
    }
    fn recover_stored_checked(
        factory: Factory,
        path: PathBuf,
        expected: Option<&crate::output_journal::Record>,
    ) -> Result<(), String> {
        let store = Store::open(path)?;
        let record = store
            .record()
            .cloned()
            .ok_or("No interrupted output is recorded.")?;
        if let Some(expected) = expected {
            if serde_json::to_vec(&record).map_err(|e| e.to_string())?
                != serde_json::to_vec(expected).map_err(|e| e.to_string())?
            {
                return Err("The complete retained output journal changed; inspect its current owner before cleanup.".into());
            }
        }
        if record.phase != Phase::Confirmed {
            return Err("Output ownership is ambiguous or already clean; inspect it before acknowledging recovery.".into());
        }
        let mut session = Self {
            factory,
            worker: None,
            token: record.token,
            bindings: record.bindings,
            cursor: Counter(0),
            renewal: 0,
            renewed: Instant::now(),
            recovery_required: true,
            journal: Some(store),
        };
        session.recover()
    }
    fn persist_phase(&mut self, phase: Phase) -> Result<(), String> {
        if let Some(store) = &mut self.journal {
            store.transition(phase)?;
        }
        if phase == Phase::Clean {
            self.journal = None;
        }
        Ok(())
    }
    pub fn active(&self) -> bool {
        self.worker.is_some()
    }
    pub fn recovery_required(&self) -> bool {
        self.recovery_required
    }
    pub fn cursor(&self) -> Counter {
        self.cursor
    }
    fn send(&mut self, message: Message) -> Result<(), String> {
        self.worker
            .as_mut()
            .ok_or("Virtual output is stopped.")?
            .request(&message)
    }
    fn renew(&mut self) -> Result<(), String> {
        if self.renewed.elapsed().as_micros() < u128::from(RENEW_US) {
            return Ok(());
        }
        let next = self
            .renewal
            .checked_add(1)
            .ok_or("Lease renewal counter exhausted.")?;
        let sent = Instant::now();
        self.send(Message::Renew {
            token: self.token.clone(),
            sequence: Counter(next),
        })?;
        self.renewal = next;
        // The child lease starts on receipt, not when its acknowledgement is
        // consumed. A delayed acknowledgement must not postpone the next renewal.
        self.renewed = sent;
        Ok(())
    }
    /// Keep prepared, neutral output leased while an owned visibility change
    /// verifies fresh capture. No mapping report is submitted before arming.
    pub fn keep_alive(&mut self) -> Result<(), String> {
        let result = match self.worker.as_mut() {
            Some(worker) => {
                if worker.alive() {
                    self.renew()
                } else {
                    Err(worker.exit_error())
                }
            }
            None => Err("Virtual output helper exited.".into()),
        };
        result.map_err(|error| self.fail(error))
    }
    pub fn token(&self) -> &RunToken {
        &self.token
    }
    /// Inspect the same retained worker and its locked, confirmed journal without
    /// spawning, renewing, submitting reports or changing output ownership.
    pub fn handoff_snapshot(&mut self) -> Result<Option<RunningOutput>, String> {
        if self.recovery_required {
            return Err("Virtual output requires recovery; no live handoff is authorized.".into());
        }
        let Some(worker) = self.worker.as_mut() else {
            return Ok(None);
        };
        let Some(journal) = self.journal.as_ref() else {
            return Ok(None);
        };
        journal.verify_current()?;
        let record = journal
            .record()
            .ok_or("No confirmed output journal exists.")?;
        let binding_identity = serde_json::to_vec(&self.bindings).map_err(|e| e.to_string())?;
        if record.phase != Phase::Confirmed
            || record.token != self.token
            || serde_json::to_vec(&record.bindings).map_err(|e| e.to_string())? != binding_identity
        {
            return Err("The confirmed output journal does not match this live run.".into());
        }
        if !worker.alive() {
            return Err("The owned output worker is no longer alive.".into());
        }
        let Some(process_id) = worker.process_id().filter(|pid| *pid != 0) else {
            return Ok(None);
        };
        Ok(Some(RunningOutput {
            token: self.token.clone(),
            bindings: self.bindings.clone(),
            binding_identity,
            process_id,
        }))
    }
    /// Consume at most 128 ordered reports per turn. A journal gap fails closed.
    pub fn poll(&mut self, recording: &RecordingSink) -> Result<(), String> {
        if !self.active() {
            return Err("Virtual output is stopped.".into());
        }
        let result = (|| {
            match self.worker.as_mut() {
                Some(worker) => {
                    if !worker.alive() {
                        return Err(worker.exit_error());
                    }
                }
                None => return Err("Virtual output helper exited.".into()),
            }
            self.renew()?;
            let deadline = Instant::now() + std::time::Duration::from_millis(8);
            for _ in 0..4 {
                let page = recording.page(self.cursor, 32)?;
                if page.gap {
                    return Err(
                        "Virtual output history overflowed; restart from fresh baselines.".into(),
                    );
                }
                let count = page.records.len();
                for record in page.records {
                    if record.report.session != self.token.session
                        || record.report.run_generation != self.token.generation
                    {
                        return Err("A report belongs to another mapping run.".into());
                    }
                    self.renew()?;
                    self.send(Message::Report {
                        token: self.token.clone(),
                        report: record.report,
                    })?;
                    self.cursor = record.cursor;
                    if Instant::now() >= deadline {
                        return Ok(());
                    }
                }
                if count < 32 {
                    break;
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            return Err(self.fail(error));
        }
        Ok(())
    }
    fn terminate(&mut self) {
        if let Some(mut worker) = self.worker.take() {
            worker.terminate();
        }
    }
    fn fail(&mut self, error: String) -> String {
        self.terminate();
        // Start was acknowledged, so these exact bindings were owned by this run.
        self.recovery_required = true;
        match self.recover() {
            Ok(()) => format!("{error} Fallback recovery completed; mapping remains stopped."),
            Err(recovery) => format!("{error} Recovery requires attention: {recovery}"),
        }
    }
    /// Recovery never resumes reports. The fresh worker rechecks shape and FREE status.
    pub fn recover(&mut self) -> Result<(), String> {
        if self.active() {
            return Err("Stop virtual output before recovery.".into());
        }
        if !self.recovery_required {
            return Ok(());
        }
        let mut worker = (self.factory)()?;
        let token = RunToken {
            session: uuid::Uuid::new_v4().to_string(),
            generation: Counter(1),
        };
        if let Err(error) = self.persist_phase(Phase::Releasing) {
            worker.terminate();
            return Err(error);
        }
        let result = worker.request(&Message::Recover {
            token,
            bindings: self.bindings.clone(),
        });
        worker.terminate();
        if result.is_ok() {
            self.persist_phase(Phase::Clean)?;
            self.recovery_required = false;
        }
        result
    }
    pub fn stop(&mut self) -> Result<(), String> {
        if !self.active() {
            return Ok(());
        }
        if let Err(error) = self.persist_phase(Phase::Releasing) {
            self.terminate();
            self.recovery_required = true;
            return Err(error);
        }
        let result = self.send(Message::Stop {
            token: self.token.clone(),
        });
        if let Err(error) = result {
            return Err(self.fail(error));
        }
        self.terminate();
        self.recovery_required = true;
        self.persist_phase(Phase::Clean)?;
        self.recovery_required = false;
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "session_fault_tests.rs"]
mod fault_tests;
