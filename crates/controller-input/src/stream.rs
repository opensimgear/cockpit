//! Bounded, nonconsuming native report history. UI snapshots never feed mappings.
use crate::registry::{Control, Sample};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Weak,
    },
    time::Instant,
};
pub const REPORT_LIMIT: usize = 1024;
pub const REPORT_WATCH_LIMIT: usize = 64;
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
struct WatchState {
    ids: BTreeSet<String>,
    wake: Arc<dyn Fn() + Send + Sync>,
    read_cursor: AtomicU64,
}
/// A source selection's wake lease. Dropping it revokes the callback without
/// taking a registry lock; the journal retains only a weak reference.
#[must_use = "keep the report watch alive for the selected routing sources"]
pub struct ReportWatch {
    state: Arc<WatchState>,
}
impl ReportWatch {
    /// Acknowledge an atomically copied snapshot or page. A stale acknowledgement
    /// cannot move the consumer backwards or create spurious overflow wakeups.
    pub fn advance(&self, cursor: u64) {
        self.state.read_cursor.fetch_max(cursor, Ordering::Release);
    }
}
mod clock;
pub use clock::Clock;

pub(crate) struct Journal {
    origin: Instant,
    clock_id: uuid::Uuid,
    cursor: u64,
    events: VecDeque<Event>,
    watches: Vec<Weak<WatchState>>,
}
impl Clone for Journal {
    fn clone(&self) -> Self {
        // Metadata transactions cannot notify the live routing owner.
        Self {
            origin: self.origin,
            clock_id: self.clock_id,
            cursor: self.cursor,
            events: self.events.clone(),
            watches: vec![],
        }
    }
}
impl Default for Journal {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
            clock_id: uuid::Uuid::new_v4(),
            cursor: 0,
            events: VecDeque::new(),
            watches: vec![],
        }
    }
}
impl Journal {
    pub(crate) fn watch(
        &mut self,
        ids: &[String],
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<ReportWatch, String> {
        self.watches.retain(|watch| watch.strong_count() > 0);
        if self.watches.len() >= REPORT_WATCH_LIMIT {
            return Err("Controller report watch limit reached.".into());
        }
        let state = Arc::new(WatchState {
            ids: ids.iter().cloned().collect(),
            wake,
            read_cursor: AtomicU64::new(self.cursor),
        });
        self.watches.push(Arc::downgrade(&state));
        Ok(ReportWatch { state })
    }
    fn notify(&mut self, source: &str, evicted: Option<u64>, exhausted: bool) {
        self.watches.retain(|watch| {
            let Some(watch) = watch.upgrade() else {
                return false;
            };
            if exhausted
                || watch.ids.contains(source)
                || evicted.is_some_and(|cursor| cursor > watch.read_cursor.load(Ordering::Acquire))
            {
                // Wake only: the owner drains committed state after the capture
                // mutex exits. Callbacks must not take host/registry locks or do I/O.
                (watch.wake)();
            }
            true
        });
    }
    pub fn now(&self) -> u64 {
        self.clock().now_us()
    }
    pub(crate) fn clock(&self) -> Clock {
        Clock { origin: self.origin, id: self.clock_id }
    }
    pub fn cursor(&self) -> u64 {
        self.cursor
    }
    pub fn publish(&mut self, mut event: Event) -> Option<Event> {
        // Exhaustion faults consumers permanently; it must never reuse a cursor.
        let Some(next) = self.cursor.checked_add(1) else {
            self.events.clear();
            self.notify(&event.source, None, true);
            return None;
        };
        self.cursor = next;
        event.cursor = next;
        event.captured_us = self.now();
        let evicted =
            (self.events.len() == REPORT_LIMIT).then(|| self.events.pop_front().unwrap().cursor);
        self.events.push_back(event.clone());
        self.notify(&event.source, evicted, next == u64::MAX);
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
