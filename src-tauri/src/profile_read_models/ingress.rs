//! Bounded producer handoff. The publication worker owns projection and recovery.
use super::{bounds, AutomaticChange, Event, CAPACITY};
use crossbeam_queue::ArrayQueue;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Arc,
};

const SOURCES: usize = 5;

struct Entry {
    sequence: u64,
    bytes: usize,
    event: Event,
}

fn source(event: &Event) -> usize {
    match event {
        Event::Library(_) => 0,
        Event::Native(_) => 1,
        Event::Desktop(_) => 2,
        Event::Automatic(AutomaticChange::Snapshot(_)) => 3,
        Event::Automatic(AutomaticChange::Emergency { .. }) => 4,
    }
}

pub(super) struct Ingress {
    pending: ArrayQueue<Arc<Entry>>,
    latest: [ArrayQueue<Arc<Entry>>; SOURCES],
    sequence: AtomicU64,
    emergency: AtomicU64,
    emergency_seen: AtomicBool,
    bytes: AtomicUsize,
    overflow: AtomicBool,
    pub(super) failed: AtomicBool,
    pub(super) closed: AtomicBool,
    pub(super) wake: osg_streams::NativeWake,
}

impl Default for Ingress {
    fn default() -> Self {
        Self {
            pending: ArrayQueue::new(CAPACITY),
            latest: std::array::from_fn(|_| ArrayQueue::new(1)),
            sequence: AtomicU64::new(0),
            emergency: AtomicU64::new(0),
            emergency_seen: AtomicBool::new(false),
            bytes: AtomicUsize::new(0),
            overflow: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            wake: Default::default(),
        }
    }
}

impl Ingress {
    pub(super) fn capture(&self, event: Event) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        let bytes = bounds::event(&event);
        let limit = if source(&event) == 3 {
            bounds::MAX_AUTOMATIC_SOURCE_BYTES
        } else {
            bounds::MAX_SOURCE_BYTES
        };
        if bytes > limit {
            self.fail();
            return;
        }
        let Ok(previous) = self
            .sequence
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        else {
            self.fail();
            return;
        };
        let index = source(&event);
        // Emergency notifications may race each other after the authority CAS.
        // Retain the highest authority revision independently of callback order.
        if let Event::Automatic(AutomaticChange::Emergency { revision, enabled }) = &event {
            let Some(packed) = revision
                .checked_mul(2)
                .map(|value| value | u64::from(*enabled))
            else {
                self.fail();
                return;
            };
            self.emergency.fetch_max(packed, Ordering::Release);
            self.emergency_seen.store(true, Ordering::Release);
        }
        let entry = Arc::new(Entry {
            sequence: previous + 1,
            bytes,
            event,
        });
        self.latest[index].force_push(entry.clone());
        let reserved = self
            .bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes)
                    .filter(|total| *total <= bounds::MAX_QUEUE_BYTES)
            })
            .is_ok();
        if reserved {
            if self.pending.push(entry).is_err() {
                self.bytes.fetch_sub(bytes, Ordering::AcqRel);
                self.overflow.store(true, Ordering::Release);
            }
        } else {
            self.overflow.store(true, Ordering::Release);
        }
        self.wake.notify();
    }

    fn fail(&self) {
        self.failed.store(true, Ordering::Release);
        self.close();
    }

    pub(super) fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.wake.notify();
    }

    pub(super) fn drain(&self, last: &mut [u64; SOURCES]) -> (Vec<Event>, bool) {
        let mut entries = Vec::new();
        for _ in 0..CAPACITY {
            let Some(entry) = self.pending.pop() else {
                break;
            };
            self.bytes.fetch_sub(entry.bytes, Ordering::AcqRel);
            entries.push(entry);
        }
        let overflow = self.overflow.swap(false, Ordering::AcqRel);
        if overflow {
            entries.extend(self.latest.iter().filter_map(ArrayQueue::pop));
        }
        entries.sort_unstable_by_key(|entry| entry.sequence);
        let mut events = Vec::new();
        for entry in entries {
            let index = source(&entry.event);
            if index == 4 || entry.sequence > last[index] {
                last[index] = entry.sequence;
                events.push(entry.event.clone());
            }
        }
        if overflow && self.emergency_seen.load(Ordering::Acquire) {
            let packed = self.emergency.load(Ordering::Acquire);
            events.push(Event::Automatic(AutomaticChange::Emergency {
                revision: packed >> 1,
                enabled: packed & 1 != 0,
            }));
        }
        (events, overflow)
    }
}
