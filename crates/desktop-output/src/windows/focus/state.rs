use super::{foreground::ForegroundSubscriber, ScopeSnapshot, Wake};
use crate::{ProcessIdentity, Scope};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

pub(super) const WATCH_LIMIT: usize = 64;
#[derive(Clone)]
pub(super) struct Observation {
    pub foreground: Option<ProcessIdentity>,
    pub error: Option<String>,
}
impl Observation {
    pub(super) fn unavailable() -> Self {
        Self {
            foreground: None,
            error: None,
        }
    }
    pub(super) fn failed(error: String) -> Self {
        Self {
            foreground: None,
            error: Some(error),
        }
    }
    fn eligible(&self, scope: &Scope) -> bool {
        self.error.is_none()
            && self.foreground.as_ref().is_some_and(|current| match scope {
                Scope::DesktopWide => true,
                Scope::Application { identity } => super::super::same_identity(identity, current),
            })
    }
}
struct SubState {
    snapshot: ScopeSnapshot,
    terminal: Option<String>,
    closed: bool,
}
pub(super) struct Subscriber {
    pub(super) id: u64,
    scope: Scope,
    wake: Wake,
    state: Mutex<SubState>,
}
impl Subscriber {
    fn lock(&self) -> MutexGuard<'_, SubState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
    pub(super) fn snapshot(&self) -> Result<ScopeSnapshot, String> {
        let state = self.lock();
        if let Some(error) = &state.terminal {
            return Err(error.clone());
        }
        if state.closed {
            return Err("Desktop scope watch is retired.".into());
        }
        Ok(state.snapshot.clone())
    }
}
struct State {
    subscribers: BTreeMap<u64, Weak<Subscriber>>,
    foreground_subscribers: BTreeMap<u64, Weak<ForegroundSubscriber>>,
    last: Option<Observation>,
    terminal: Option<String>,
    closing: bool,
}
pub(super) struct Shared {
    state: Mutex<State>,
    pub(super) lifetime: super::lifetime::Lifetime,
}
impl Shared {
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(State {
                subscribers: BTreeMap::new(),
                foreground_subscribers: BTreeMap::new(),
                last: None,
                terminal: None,
                closing: false,
            }),
            lifetime: super::lifetime::Lifetime::new(),
        }
    }
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
    pub(super) fn subscribe(
        &self,
        id: u64,
        scope: Scope,
        wake: Wake,
    ) -> Result<Arc<Subscriber>, String> {
        let mut state = self.lock();
        if let Some(error) = &state.terminal {
            return Err(error.clone());
        }
        if state.closing {
            return Err("Desktop scope observer is stopping.".into());
        }
        state
            .subscribers
            .retain(|_, subscriber| subscriber.strong_count() > 0);
        state
            .foreground_subscribers
            .retain(|_, subscriber| subscriber.strong_count() > 0);
        if state.subscribers.len() + state.foreground_subscribers.len() >= WATCH_LIMIT {
            return Err("Desktop scope watch limit exceeded.".into());
        }
        let last = state
            .last
            .as_ref()
            .ok_or("Desktop scope observer is not initialized.")?;
        let eligible = last.eligible(&scope);
        let subscriber = Arc::new(Subscriber {
            id,
            scope,
            wake,
            state: Mutex::new(SubState {
                snapshot: ScopeSnapshot {
                    watch_id: id,
                    sequence: 1,
                    loss_epoch: u64::from(!eligible),
                    eligible,
                    error: last.error.clone(),
                },
                terminal: None,
                closed: false,
            }),
        });
        state.subscribers.insert(id, Arc::downgrade(&subscriber));
        Ok(subscriber)
    }
    pub(super) fn subscribe_foreground(
        &self,
        id: u64,
        wake: super::foreground::Wake,
    ) -> Result<Arc<ForegroundSubscriber>, String> {
        let mut state = self.lock();
        if let Some(error) = &state.terminal {
            return Err(error.clone());
        }
        if state.closing {
            return Err("Desktop scope observer is stopping.".into());
        }
        state
            .subscribers
            .retain(|_, subscriber| subscriber.strong_count() > 0);
        state
            .foreground_subscribers
            .retain(|_, subscriber| subscriber.strong_count() > 0);
        if state.subscribers.len() + state.foreground_subscribers.len() >= WATCH_LIMIT {
            return Err("Desktop scope watch limit exceeded.".into());
        }
        let last = state
            .last
            .as_ref()
            .ok_or("Desktop scope observer is not initialized.")?;
        let subscriber = Arc::new(ForegroundSubscriber::new(id, last, wake));
        state
            .foreground_subscribers
            .insert(id, Arc::downgrade(&subscriber));
        Ok(subscriber)
    }
    pub(super) fn unsubscribe(&self, subscriber: &Subscriber) {
        let mut state = self.lock();
        subscriber.lock().closed = true;
        state.subscribers.remove(&subscriber.id);
    }
    pub(super) fn unsubscribe_foreground(&self, subscriber: &ForegroundSubscriber) {
        let mut state = self.lock();
        subscriber.close();
        state.foreground_subscribers.remove(&subscriber.id);
    }
    pub(super) fn close(&self) {
        self.lock().closing = true;
    }
    pub(super) fn closing(&self) -> bool {
        self.lock().closing
    }
    pub(super) fn terminal(&self) -> Option<String> {
        self.lock().terminal.clone()
    }
    pub(super) fn observe(&self, observation: Observation, forced_loss: bool) {
        let mut callbacks = Vec::new();
        let mut foreground_callbacks = Vec::new();
        let mut exhausted = false;
        {
            let mut state = self.lock();
            if state.closing || state.terminal.is_some() {
                return;
            }
            state.last = Some(observation.clone());
            state.foreground_subscribers.retain(|_, weak| {
                let Some(subscriber) = weak.upgrade() else {
                    return false;
                };
                match subscriber.observe(&observation) {
                    Ok(Some(snapshot)) => foreground_callbacks.push((subscriber, snapshot)),
                    Ok(None) => {}
                    Err(()) => {
                        if let Some(snapshot) =
                            subscriber.fail("Foreground watch sequence exhausted.".into())
                        {
                            foreground_callbacks.push((subscriber, snapshot));
                        }
                    }
                }
                true
            });
            state.subscribers.retain(|_, weak| {
                let Some(subscriber) = weak.upgrade() else {
                    return false;
                };
                let mut sub = subscriber.lock();
                if sub.closed {
                    return false;
                }
                let eligible = observation.eligible(&subscriber.scope);
                // Each ineligible event is evidence of a boundary, including
                // events drained while already paused. Never collapse these
                // with a later eligible observation or forced desktop return.
                let loss = forced_loss || !eligible;
                if !loss
                    && sub.snapshot.eligible == eligible
                    && sub.snapshot.error == observation.error
                {
                    return true;
                }
                let sequence = sub.snapshot.sequence.checked_add(1);
                let epoch = if loss {
                    sub.snapshot.loss_epoch.checked_add(1)
                } else {
                    Some(sub.snapshot.loss_epoch)
                };
                if let (Some(sequence), Some(loss_epoch)) = (sequence, epoch) {
                    sub.snapshot = ScopeSnapshot {
                        watch_id: subscriber.id,
                        sequence,
                        loss_epoch,
                        eligible,
                        error: observation.error.clone(),
                    };
                    callbacks.push((subscriber.clone(), sub.snapshot.clone()));
                } else {
                    exhausted = true;
                }
                true
            });
        }
        if exhausted {
            self.fail("Desktop scope sequence exhausted.".into());
            return;
        }
        self.dispatch(callbacks);
        self.dispatch_foreground(foreground_callbacks);
    }
    fn dispatch(&self, callbacks: Vec<(Arc<Subscriber>, ScopeSnapshot)>) {
        for (subscriber, snapshot) in callbacks {
            if subscriber.lock().closed {
                continue;
            }
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (subscriber.wake)(snapshot)
            }))
            .is_err()
            {
                self.fail("Desktop scope notification callback panicked.".into());
                break;
            }
        }
    }
    fn dispatch_foreground(
        &self,
        callbacks: Vec<(
            Arc<ForegroundSubscriber>,
            super::foreground::ForegroundSnapshot,
        )>,
    ) {
        for (subscriber, snapshot) in callbacks {
            if !subscriber.closed() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    (subscriber.wake)(snapshot)
                }));
            }
        }
    }
    pub(super) fn fail(&self, error: String) {
        let (callbacks, foreground_callbacks) = {
            let mut state = self.lock();
            if state.terminal.is_some() || state.closing {
                return;
            }
            state.terminal = Some(error.clone());
            let callbacks = state
                .subscribers
                .values()
                .filter_map(Weak::upgrade)
                .filter_map(|subscriber| {
                    let mut sub = subscriber.lock();
                    if sub.closed {
                        return None;
                    }
                    sub.terminal = Some(error.clone());
                    sub.snapshot.eligible = false;
                    sub.snapshot.error = Some(error.clone());
                    // Terminal errors cannot rearm, even if counters are exhausted.
                    sub.snapshot.sequence = sub.snapshot.sequence.saturating_add(1);
                    sub.snapshot.loss_epoch = sub.snapshot.loss_epoch.saturating_add(1);
                    Some((subscriber.clone(), sub.snapshot.clone()))
                })
                .collect::<Vec<_>>();
            let foreground_callbacks = state
                .foreground_subscribers
                .values()
                .filter_map(Weak::upgrade)
                .filter_map(|subscriber| {
                    subscriber
                        .fail(error.clone())
                        .map(|snapshot| (subscriber, snapshot))
                })
                .collect::<Vec<_>>();
            (callbacks, foreground_callbacks)
        };
        // A broken callback must not recursively redispatch the terminal fault.
        for (subscriber, snapshot) in callbacks {
            if !subscriber.lock().closed {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    (subscriber.wake)(snapshot)
                }));
            }
        }
        self.dispatch_foreground(foreground_callbacks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_exhaustion_is_terminal_and_never_wraps() {
        let shared = Shared::new();
        shared.observe(Observation::unavailable(), false);
        let watch = shared
            .subscribe(1, Scope::DesktopWide, Arc::new(|_| {}))
            .unwrap();
        watch.lock().snapshot.sequence = u64::MAX;
        shared.observe(Observation::unavailable(), true);
        assert!(watch.snapshot().unwrap_err().contains("exhausted"));
        assert_eq!(watch.lock().snapshot.sequence, u64::MAX);
        shared.observe(Observation::unavailable(), false);
        assert_eq!(watch.lock().snapshot.sequence, u64::MAX);
    }
}
