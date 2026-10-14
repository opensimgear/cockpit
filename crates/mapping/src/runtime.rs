use crate::{
    engine::{Assignment, Counter, Engine, Frame, ProducerToken, Report},
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
        if limit == 0 || limit > 64 || after.0 > self.cursor {
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
#[derive(Default)]
pub struct Runtime {
    engine: Option<Engine>,
    generation: u64,
    pub recording: RecordingSink,
}
impl Runtime {
    pub fn engine(&self) -> Option<&Engine> {
        self.engine.as_ref()
    }
    pub fn start(
        &mut self,
        profile: Profile,
        assignments: Vec<Assignment>,
    ) -> Result<Counter, String> {
        let generation = self
            .generation
            .checked_add(1)
            .ok_or("Run generation exhausted.")?;
        // Fully validate the candidate before disturbing a currently active run.
        let next = Engine::new(profile, assignments, Counter(generation))?;
        self.stop();
        self.generation = generation;
        self.engine = Some(next);
        self.flush();
        Ok(Counter(generation))
    }
    fn flush(&mut self) {
        if let Some(engine) = &mut self.engine {
            self.recording.record(engine.drain());
        }
    }
    pub fn accept(&mut self, frame: Frame) -> Result<(), String> {
        let result = self.engine.as_mut().ok_or("No mapping run.")?.accept(frame);
        self.flush();
        result
    }
    pub fn advance(&mut self, now: Counter) -> Result<(), String> {
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
