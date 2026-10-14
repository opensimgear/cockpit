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
    fn terminate(&mut self);
}
impl Channel for Worker {
    fn request(&mut self, message: &Message) -> Result<(), String> {
        Worker::request(self, message)
    }
    fn alive(&mut self) -> bool {
        Worker::alive(self)
    }
    fn terminate(&mut self) {
        Worker::terminate(self)
    }
}
pub type Factory = Arc<dyn Fn() -> Result<Box<dyn Channel>, String> + Send + Sync>;

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
        let store = Store::open(path)?;
        let record = store
            .record()
            .cloned()
            .ok_or("No interrupted output is recorded.")?;
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
        self.send(Message::Renew {
            token: self.token.clone(),
            sequence: Counter(next),
        })?;
        self.renewal = next;
        self.renewed = Instant::now();
        Ok(())
    }
    /// Consume at most 128 ordered reports per turn. A journal gap fails closed.
    pub fn poll(&mut self, recording: &RecordingSink) -> Result<(), String> {
        if !self.active() {
            return Err("Virtual output is stopped.".into());
        }
        let result = (|| {
            if !self.worker.as_mut().is_some_and(|worker| worker.alive()) {
                return Err("Virtual output helper exited.".into());
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
