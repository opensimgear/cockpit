use super::{
    acquire_owner, next_id,
    state::{Observation, Shared},
};
use crate::ProcessIdentity;
use std::sync::{Arc, Mutex, MutexGuard};

pub type Wake = Arc<dyn Fn(ForegroundSnapshot) + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForegroundSnapshot {
    pub watch_id: u64,
    pub sequence: u64,
    pub identity: Option<ProcessIdentity>,
    pub error: Option<String>,
    pub terminal: bool,
}

struct SubscriberState {
    snapshot: ForegroundSnapshot,
    closed: bool,
    terminal: bool,
}

pub(super) struct ForegroundSubscriber {
    pub(super) id: u64,
    pub(super) wake: Wake,
    state: Mutex<SubscriberState>,
}

impl ForegroundSubscriber {
    pub(super) fn new(id: u64, observation: &Observation, wake: Wake) -> Self {
        Self {
            id,
            wake,
            state: Mutex::new(SubscriberState {
                snapshot: ForegroundSnapshot {
                    watch_id: id,
                    sequence: 1,
                    identity: observation.foreground.clone(),
                    error: observation.error.clone(),
                    terminal: false,
                },
                closed: false,
                terminal: false,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, SubscriberState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub(super) fn snapshot(&self) -> Result<ForegroundSnapshot, String> {
        let state = self.lock();
        if state.closed {
            return Err("Foreground watch is retired.".into());
        }
        Ok(state.snapshot.clone())
    }

    pub(super) fn observe(
        &self,
        observation: &Observation,
    ) -> Result<Option<ForegroundSnapshot>, ()> {
        let mut state = self.lock();
        if state.closed
            || state.terminal
            || (state.snapshot.identity == observation.foreground
                && state.snapshot.error == observation.error)
        {
            return Ok(None);
        }
        let sequence = state.snapshot.sequence.checked_add(1).ok_or(())?;
        state.snapshot = ForegroundSnapshot {
            watch_id: self.id,
            sequence,
            identity: observation.foreground.clone(),
            error: observation.error.clone(),
            terminal: false,
        };
        Ok(Some(state.snapshot.clone()))
    }

    pub(super) fn fail(&self, error: String) -> Option<ForegroundSnapshot> {
        let mut state = self.lock();
        if state.closed || state.terminal {
            return None;
        }
        let sequence = state.snapshot.sequence.saturating_add(1);
        state.snapshot = ForegroundSnapshot {
            watch_id: self.id,
            sequence,
            identity: None,
            error: Some(error),
            terminal: true,
        };
        state.terminal = true;
        Some(state.snapshot.clone())
    }

    pub(super) fn close(&self) {
        self.lock().closed = true;
    }
    pub(super) fn closed(&self) -> bool {
        self.lock().closed
    }
}

/// Receives foreground identity changes from the process-wide scope observer.
/// The wake must be quick: it runs on the shared native event thread.
pub struct ForegroundWatch {
    subscriber: Arc<ForegroundSubscriber>,
    shared: Arc<Shared>,
    _owner: Arc<super::native::Owner>,
}

impl ForegroundWatch {
    pub fn open(wake: Wake) -> Result<Self, String> {
        let owner = acquire_owner()?;
        let id = next_id("Foreground watch identity exhausted.")?;
        let subscriber = owner.shared.subscribe_foreground(id, wake)?;
        Ok(Self {
            subscriber,
            shared: owner.shared.clone(),
            _owner: owner,
        })
    }

    pub fn snapshot(&self) -> Result<ForegroundSnapshot, String> {
        self.subscriber.snapshot()
    }
    pub fn terminal(&self) -> bool {
        self.subscriber.lock().terminal
    }
    pub fn id(&self) -> u64 {
        self.subscriber.id
    }
}

impl Drop for ForegroundWatch {
    fn drop(&mut self) {
        self.shared.unsubscribe_foreground(&self.subscriber);
    }
}

#[cfg(test)]
#[path = "foreground_tests.rs"]
mod tests;
