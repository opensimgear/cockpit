//! Native-only continuity for publication invalidations on the gateway CDC stream.
use osg_streams::NativeWake;
use serde_json::Value;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, Weak,
};

const MAX_WATCHERS: usize = 8;
const INVALID: &str = "CAN gateway sent an invalid publication stream record.";
pub(crate) const PUBLICATION_EVIDENCE_CHANGED: &str = "The gateway publication evidence changed before dispatch.";

/// A proof can outlive its read, but cannot outlive a change or its CDC owner.
#[derive(Clone, Debug)]
pub(crate) struct PublicationFence {
    counter: Arc<AtomicU64>,
    sequence: u64,
    epoch: u32,
}

impl PublicationFence {
    pub(crate) fn is_current(&self) -> bool {
        self.counter.load(Ordering::Acquire) == self.sequence
    }

    pub(crate) fn same_stream(&self, other: &Self) -> bool {
        self.epoch == other.epoch && Arc::ptr_eq(&self.counter, &other.counter)
    }

    pub(crate) fn epoch(&self) -> u32 {
        self.epoch
    }

    #[cfg(test)]
    pub(crate) fn fixture(epoch: u32, counter: Arc<AtomicU64>) -> Option<Self> {
        let sequence = counter.load(Ordering::Acquire);
        (sequence != 0).then_some(Self { counter, sequence, epoch })
    }
}

#[derive(Default)]
struct State {
    owner: Option<Arc<AtomicU64>>,
    current: Option<PublicationFence>,
    watchers: Vec<Weak<NativeWake>>,
}

impl State {
    fn wakes(&mut self) -> Vec<Arc<NativeWake>> {
        self.watchers.retain(|wake| wake.strong_count() > 0);
        self.watchers.iter().filter_map(Weak::upgrade).collect()
    }
}

#[derive(Clone, Default)]
pub(crate) struct PublicationFeed(Arc<Mutex<State>>);

impl PublicationFeed {
    pub(crate) fn current(&self) -> Option<PublicationFence> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).current.clone()
    }

    pub(crate) fn watch(&self, wake: &Arc<NativeWake>) {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        state.watchers.retain(|watch| watch.strong_count() > 0);
        let wake = Arc::downgrade(wake);
        if state.watchers.len() < MAX_WATCHERS && !state.watchers.iter().any(|old| old.ptr_eq(&wake)) {
            state.watchers.push(wake);
        }
    }

    pub(crate) fn attach(&self) -> Ingress {
        let counter = Arc::new(AtomicU64::new(0));
        let wakes = {
            let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(previous) = state.owner.replace(counter.clone()) {
                previous.store(0, Ordering::Release);
            }
            state.current = None;
            state.wakes()
        };
        for wake in wakes { wake.notify(); }
        Ingress {
            lifetime: Lifetime { feed: self.clone(), counter },
            last: None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Lifetime {
    feed: PublicationFeed,
    counter: Arc<AtomicU64>,
}

impl Lifetime {
    pub(crate) fn close(&self) {
        let wakes = {
            let mut state = self.feed.0.lock().unwrap_or_else(|p| p.into_inner());
            if !state.owner.as_ref().is_some_and(|owner| Arc::ptr_eq(owner, &self.counter)) {
                return;
            }
            self.counter.store(0, Ordering::Release);
            state.owner = None;
            state.current = None;
            state.wakes()
        };
        for wake in wakes { wake.notify(); }
    }
}

pub(crate) struct Ingress {
    lifetime: Lifetime,
    last: Option<(u32, u64)>,
}

impl Ingress {
    pub(crate) fn lifetime(&self) -> Lifetime {
        self.lifetime.clone()
    }

    pub(crate) fn accept(&mut self, value: &Value) -> Result<(), String> {
        let result = self.accept_change(value);
        if result.is_err() { self.lifetime.close(); }
        result
    }

    fn accept_change(&mut self, value: &Value) -> Result<(), String> {
        if value["publicationStream"] != "change" || value.as_object().is_none_or(|row| row.len() != 3) {
            return Err(INVALID.into());
        }
        let epoch = value["epoch"].as_u64().and_then(|epoch| u32::try_from(epoch).ok())
            .filter(|epoch| *epoch != 0).ok_or(INVALID)?;
        let text = value["sequence"].as_str().ok_or(INVALID)?;
        let sequence: u64 = text.parse().map_err(|_| INVALID)?;
        if sequence == 0 || sequence.to_string() != text {
            return Err(INVALID.into());
        }
        let valid = self.last.map_or(sequence == 1, |(boot, previous)| {
            boot == epoch && previous.checked_add(1) == Some(sequence)
        });
        if !valid { return Err("CAN gateway publication stream continuity was lost.".into()); }

        let wakes = {
            let mut state = self.lifetime.feed.0.lock().unwrap_or_else(|p| p.into_inner());
            let counter = &self.lifetime.counter;
            if !state.owner.as_ref().is_some_and(|owner| Arc::ptr_eq(owner, counter)) {
                return Err("CAN gateway publication stream was retired.".into());
            }
            counter.store(sequence, Ordering::Release);
            state.current = Some(PublicationFence { counter: counter.clone(), sequence, epoch });
            state.wakes()
        };
        self.last = Some((epoch, sequence));
        for wake in wakes { wake.notify(); }
        Ok(())
    }
}

impl Drop for Ingress {
    fn drop(&mut self) { self.lifetime.close(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn frame(sequence: &str) -> Value {
        json!({"publicationStream":"change", "epoch":5, "sequence":sequence})
    }

    #[test]
    fn replacement_and_gap_withdraw_old_proofs_and_wake_readers() {
        let feed = PublicationFeed::default();
        let wake = Arc::new(NativeWake::default());
        feed.watch(&wake);
        let mut old = feed.attach();
        let before = wake.observed();
        old.accept(&frame("1")).unwrap();
        assert_ne!(wake.observed(), before);
        let first = feed.current().unwrap();
        old.accept(&frame("2")).unwrap();
        let second = feed.current().unwrap();
        assert!(!first.is_current() && second.is_current() && first.same_stream(&second));
        let mut replacement = feed.attach();
        assert!(!second.is_current() && feed.current().is_none());
        replacement.accept(&frame("1")).unwrap();
        assert!(old.accept(&frame("3")).is_err());
        drop(old);
        let current = feed.current().unwrap();
        assert!(current.is_current() && !current.same_stream(&first));
        let before = wake.observed();
        assert!(replacement.accept(&frame("3")).is_err());
        assert!(!current.is_current() && feed.current().is_none());
        assert_ne!(wake.observed(), before);
        assert!(replacement.accept(&frame("2")).is_err());
    }
}
