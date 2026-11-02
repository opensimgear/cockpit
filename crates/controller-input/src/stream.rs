//! Bounded, nonconsuming native report history. UI snapshots never feed mappings.
use crate::registry::{Control, Sample};
use std::{
    collections::{BTreeMap, VecDeque},
    time::Instant,
    sync::Arc,
};
pub const REPORT_LIMIT: usize = 1024;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    Baseline,
    Change,
    Gap,
    Disconnect,
}
#[derive(Clone, Debug)]
pub struct Event {
    pub cursor: u64,
    pub source: String,
    pub generation: u64,
    pub sequence: u64,
    pub captured_us: u64,
    pub reason: Reason,
    pub values: BTreeMap<String, Sample>,
}
#[derive(Clone, Debug)]
pub struct SourceSnapshot {
    pub id: String,
    pub controls: Vec<Control>,
    pub baseline: Event,
}
#[derive(Debug)]
pub struct Snapshot {
    pub session: String,
    pub cursor: u64,
    pub captured_us: u64,
    pub sources: Vec<SourceSnapshot>,
}
#[derive(Debug)]
pub struct Page {
    pub session: String,
    pub cursor: u64,
    pub captured_us: u64,
    pub gap: bool,
    pub events: Vec<Event>,
}
pub(crate) struct Journal {
    origin: Instant,
    cursor: u64,
    events: VecDeque<Event>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl Clone for Journal {
    fn clone(&self) -> Self {
        // Metadata transactions cannot notify the live routing owner.
        Self {origin:self.origin,cursor:self.cursor,events:self.events.clone(),wake:None}
    }
}
impl Default for Journal {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
            cursor: 0,
            events: VecDeque::new(),
            wake: None,
        }
    }
}
impl Journal {
    pub(crate) fn set_wake(&mut self, wake: Arc<dyn Fn() + Send + Sync>) { self.wake=Some(wake); }
    pub fn now(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_micros())
            .unwrap_or(u64::MAX - 1)
            .saturating_add(1)
    }
    pub fn cursor(&self) -> u64 {
        self.cursor
    }
    pub fn publish(&mut self, mut event: Event) -> Option<Event> {
        // Exhaustion faults consumers permanently; it must never reuse a cursor.
        let Some(next) = self.cursor.checked_add(1) else {
            self.events.clear();
            if let Some(wake)=&self.wake {wake();}
            return None;
        };
        self.cursor = next;
        event.cursor = next;
        event.captured_us = self.now();
        if self.events.len() == REPORT_LIMIT {
            self.events.pop_front();
        }
        self.events.push_back(event.clone());
        if let Some(wake)=&self.wake {wake();}
        Some(event)
    }
    pub fn page(&self, session: &str, expected_session: &str, after: u64) -> Page {
        let gap = expected_session != session
            || after > self.cursor
            || self.cursor == u64::MAX
            || self.events.front().is_some_and(|e| after < e.cursor - 1);
        Page {
            session: session.into(),
            cursor: self.cursor,
            captured_us: self.now(),
            gap,
            events: if gap {
                vec![]
            } else {
                self.events
                    .iter()
                    .filter(|e| e.cursor > after)
                    .cloned()
                    .collect()
            },
        }
    }
}
#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
