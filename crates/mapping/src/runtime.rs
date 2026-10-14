use crate::{
    engine::{Assignment, Counter, Engine, Frame, ProducerToken, Report, RunToken},
    profile::{Profile, Value},
};
use serde::Serialize;
use std::collections::VecDeque;
pub const RECORDING_LIMIT: usize = 4096;
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recorded {
    pub cursor: Counter,
    pub report: Report,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingPage {
    pub gap: bool,
    pub next: Counter,
    pub dropped: Counter,
    pub records: Vec<Recorded>,
}
#[derive(Default)]
pub struct RecordingSink {
    cursor: u64,
    dropped: u64,
    records: VecDeque<Recorded>,
}
impl RecordingSink {
    pub fn record(&mut self, reports: Vec<Report>) {
        for report in reports {
            self.cursor = self.cursor.saturating_add(1);
            if self.records.len() == RECORDING_LIMIT {
                self.records.pop_front();
                self.dropped = self.dropped.saturating_add(1);
            }
            self.records.push_back(Recorded {
                cursor: Counter(self.cursor),
                report,
            });
        }
    }
    pub fn page(&self, after: Counter, limit: usize) -> Result<RecordingPage, String> {
        if limit == 0 || limit > 32 || after.0 > self.cursor {
            return Err("Invalid recording cursor or page size.".into());
        }
        let gap = self
            .records
            .front()
            .is_some_and(|record| after.0 < record.cursor.0 - 1);
        let records: Vec<_> = self
            .records
            .iter()
            .filter(|record| record.cursor.0 > after.0)
            .take(limit)
            .cloned()
            .collect();
        let next = records.last().map_or(after, |record| record.cursor);
        Ok(RecordingPage {
            gap,
            next,
            dropped: Counter(self.dropped),
            records,
        })
    }
    pub fn cursor(&self) -> Counter {
        Counter(self.cursor)
    }
}
/// Explicit lifecycle around the pure reducer. Import/save are separate operations.
pub struct Runtime {
    session: String,
    engine: Option<Engine>,
    generation: u64,
    pub recording: RecordingSink,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            session: uuid::Uuid::new_v4().to_string(),
            engine: None,
            generation: 0,
            recording: RecordingSink::default(),
        }
    }
}
impl Runtime {
    pub(crate) fn replay_session() -> Self {
        let mut runtime = Self::default();
        runtime.session = "a78d48b8-3958-4de5-a8b3-d3b52b1e24e7".into();
        runtime
    }
    pub fn token(&self) -> Result<RunToken, String> {
        self.engine
            .as_ref()
            .filter(|e| e.active())
            .map(Engine::run_token)
            .ok_or_else(|| "No active mapping run.".into())
    }
    fn check_run(&self, token: &RunToken) -> Result<(), String> {
        if self.token()? != *token {
            Err("Stale mapping worker session or run generation.".into())
        } else {
            Ok(())
        }
    }
    pub fn accept_for(&mut self, token: &RunToken, frame: Frame) -> Result<(), String> {
        self.check_run(token)?;
        self.accept(frame)
    }
    pub fn advance_for(&mut self, token: &RunToken, now: Counter) -> Result<(), String> {
        self.check_run(token)?;
        self.advance(now)
    }
    pub fn engine(&self) -> Option<&Engine> {
        self.engine.as_ref()
    }
    pub fn start(
        &mut self,
        profile: Profile,
        assignments: Vec<Assignment>,
    ) -> Result<RunToken, String> {
        let generation = self
            .generation
            .checked_add(1)
            .ok_or("Run generation exhausted.")?;
        // Fully validate the candidate before disturbing a currently active run.
        let next = Engine::new(
            profile,
            assignments,
            Counter(generation),
            self.session.clone(),
        )?;
        self.stop();
        self.generation = generation;
        self.engine = Some(next);
        self.flush();
        self.token()
    }
    fn flush(&mut self) {
        if let Some(engine) = &mut self.engine {
            self.recording.record(engine.drain());
        }
    }
    pub(crate) fn accept(&mut self, frame: Frame) -> Result<(), String> {
        let result = self.engine.as_mut().ok_or("No mapping run.")?.accept(frame);
        self.flush();
        result
    }
    pub(crate) fn advance(&mut self, now: Counter) -> Result<(), String> {
        let result = self.engine.as_mut().ok_or("No mapping run.")?.advance(now);
        self.flush();
        result
    }
    pub fn schedule(
        &mut self,
        token: ProducerToken,
        id: String,
        at: Counter,
        value: Value,
    ) -> Result<(), String> {
        self.engine
            .as_mut()
            .ok_or("No mapping run.")?
            .schedule(token, id, at, value)
    }
    pub fn stop(&mut self) {
        if let Some(engine) = &mut self.engine {
            engine.stop();
        }
        self.flush();
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.stop();
    }
}
#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
