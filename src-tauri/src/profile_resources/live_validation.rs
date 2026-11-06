//! Live readers use the last committed answer while inventory writers work.
//! Every mutable state guard publishes revocations before releasing the state.
use super::*;
use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};
use std::sync::{LockResult, MutexGuard, PoisonError, TryLockError, TryLockResult};

const CAPACITY: usize = 2048;
const OWNER_CAPACITY: usize = 128;
type Answer = Result<Route, String>;

#[derive(Default)]
struct Confirmed {
    entries: BTreeMap<String, (ResourceToken, Answer)>,
    order: VecDeque<String>,
    // Commit state owns this table. Failed queries cannot evict an admitted reader.
    game_readers: BTreeMap<String, String>,
    retired_readers: VecDeque<(String, String)>,
}

pub(super) struct StateMutex {
    state: Mutex<State>,
    confirmed: Mutex<Confirmed>,
    pub(super) learning_wake: Mutex<std::sync::Weak<osg_streams::NativeWake>>,
    pub(super) reconnect_wake: Mutex<std::sync::Weak<osg_streams::NativeWake>>,
    pub(super) activation_wake: Mutex<std::sync::Weak<osg_streams::NativeWake>>,
    pub(super) routing_wake: Mutex<std::sync::Weak<osg_streams::NativeWake>>,
    reconnect_waiting: AtomicBool,
    pub(super) catalogue_wake: Arc<osg_streams::NativeWake>,
}

pub(super) struct StateGuard<'a> {
    owner: &'a StateMutex,
    state: MutexGuard<'a, State>,
    dirty: bool,
    // Fields drop in declaration order, so this runs after the mutex unlocks.
    _released: RegistryReleased<'a>,
}

struct RegistryReleased<'a>(&'a StateMutex);

impl Drop for RegistryReleased<'_> {
    fn drop(&mut self) {
        if self.0.reconnect_waiting.swap(false, Ordering::AcqRel) {
            self.0.wake_reconnect();
        }
    }
}

impl StateMutex {
    pub(super) fn new(state: State) -> Self {
        Self { state: Mutex::new(state), confirmed: Mutex::new(Confirmed::default()), catalogue_wake: Arc::default(), activation_wake: Mutex::default(), routing_wake: Mutex::default(), learning_wake: Mutex::default(), reconnect_wake: Mutex::default(), reconnect_waiting: AtomicBool::new(false) }
    }
    fn guard<'a>(&'a self, state: MutexGuard<'a, State>) -> StateGuard<'a> {
        StateGuard { owner: self, state, dirty: false, _released: RegistryReleased(self) }
    }
    pub(super) fn lock(&self) -> LockResult<StateGuard<'_>> {
        self.state.lock().map(|state| self.guard(state))
            .map_err(|error| PoisonError::new(self.guard(error.into_inner())))
    }
    pub(super) fn try_lock(&self) -> TryLockResult<StateGuard<'_>> {
        match self.state.try_lock() {
            Ok(state) => Ok(self.guard(state)),
            Err(TryLockError::WouldBlock) => Err(TryLockError::WouldBlock),
            Err(TryLockError::Poisoned(error)) => Err(TryLockError::Poisoned(
                PoisonError::new(self.guard(error.into_inner())))),
        }
    }
    pub(super) fn try_lock_for_reconnect(&self) -> TryLockResult<StateGuard<'_>> {
        let first = self.try_lock();
        if !matches!(first, Err(TryLockError::WouldBlock)) {
            return first;
        }
        self.reconnect_waiting.store(true, Ordering::Release);
        // Close the race with an unlock between the first attempt and registration.
        let retried = self.try_lock();
        if !matches!(retried, Err(TryLockError::WouldBlock)) {
            self.reconnect_waiting.store(false, Ordering::Release);
        }
        retried
    }
    pub(super) fn wake_reconnect(&self) {
        if let Some(wake) = self.reconnect_wake.lock().unwrap_or_else(|p| p.into_inner()).upgrade() {
            wake.notify();
        }
    }
    pub(super) fn confirmed(&self, token: &ResourceToken) -> Result<Option<Route>, String> {
        let key = serde_json::to_string(token).map_err(|error| error.to_string())?;
        self.confirmed.lock().unwrap_or_else(|p| p.into_inner())
            .entries.get(&key).map(|(_, answer)| answer.clone()).transpose()
    }
    pub(super) fn confirmed_game_owner(&self, game: &str, reader: &str) -> Result<Option<()>, String> {
        let confirmed = self.confirmed.lock().unwrap_or_else(|p| p.into_inner());
        if confirmed.game_readers.get(game).is_some_and(|session| session == reader) {
            return Ok(Some(()));
        }
        if confirmed.retired_readers.iter().any(|(id, session)| id == game && session == reader) {
            return Err("The native game reader lost its current inventory scope or verified metadata.".into());
        }
        Ok(None)
    }
}

impl StateGuard<'_> {
    pub(super) fn confirm(&self, token: &ResourceToken, answer: &Answer) {
        let Ok(key) = serde_json::to_string(token) else { return; };
        let mut confirmed = self.owner.confirmed.lock().unwrap_or_else(|p| p.into_inner());
        if !confirmed.entries.contains_key(&key) {
            if confirmed.entries.len() == CAPACITY {
                if let Some(oldest) = confirmed.order.pop_front() { confirmed.entries.remove(&oldest); }
            }
            confirmed.order.push_back(key.clone());
        }
        confirmed.entries.insert(key, (token.clone(), answer.clone()));
    }
}

impl Deref for StateGuard<'_> {
    type Target = State;
    fn deref(&self) -> &State { &self.state }
}
impl DerefMut for StateGuard<'_> {
    fn deref_mut(&mut self) -> &mut State { self.dirty = true; &mut self.state }
}
impl Drop for StateGuard<'_> {
    fn drop(&mut self) {
        if !self.dirty { return; }
        // Validation can scan registry state. Never do that while owning the
        // small cache mutex: contended readers must still obtain the old commit.
        let tokens = {
            let confirmed = self.owner.confirmed.lock().unwrap_or_else(|p| p.into_inner());
            confirmed.entries.iter().map(|(key, (token, _))| (key.clone(), token.clone())).collect::<Vec<_>>()
        };
        let answers = tokens.into_iter().map(|(key, token)| {
            let answer = Runtime::validate_delivery_locked(&self.state, &token);
            (key, (token, answer))
        }).collect();
        let game_readers: BTreeMap<_, _> = self.state.game_deliveries.iter()
            .filter(|_| !self.state.read_only)
            .map(|(game, batch)| (game.clone(), batch.session.clone())).collect();
        // State remains locked until this guard's fields drop. New state can
        // therefore never be observed alongside answers from an older commit.
        let old = {
            let mut confirmed = self.owner.confirmed.lock().unwrap_or_else(|p| p.into_inner());
            let previous = std::mem::replace(&mut confirmed.game_readers, game_readers);
            for (game, reader) in previous {
                if confirmed.game_readers.get(&game) != Some(&reader) {
                    confirmed.retired_readers.retain(|(id, session)| id != &game || session != &reader);
                    if confirmed.retired_readers.len() == OWNER_CAPACITY {
                        confirmed.retired_readers.pop_front();
                    }
                    confirmed.retired_readers.push_back((game, reader));
                }
            }
            std::mem::replace(&mut confirmed.entries, answers)
        };
        drop(old);
        self.owner.catalogue_wake.notify();
        if let Some(wake) = self.owner.routing_wake.lock().unwrap_or_else(|p| p.into_inner()).upgrade() {
            wake.notify();
        }
        if let Some(wake) = self.owner.activation_wake.lock().unwrap_or_else(|p| p.into_inner()).upgrade() {
            wake.notify();
        }
        if let Some(wake) = self.owner.learning_wake.lock().unwrap_or_else(|p| p.into_inner()).upgrade() {
            wake.notify();
        }
    }
}

#[cfg(test)]
#[path = "live_validation_tests.rs"]
mod tests;
