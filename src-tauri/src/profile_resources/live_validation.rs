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
    owners: BTreeMap<(String, String), Result<(), String>>,
    owner_order: VecDeque<(String, String)>,
}

pub(super) struct StateMutex {
    state: Mutex<State>,
    confirmed: Mutex<Confirmed>,
}

pub(super) struct StateGuard<'a> {
    owner: &'a StateMutex,
    state: MutexGuard<'a, State>,
    dirty: bool,
}

impl StateMutex {
    pub(super) fn new(state: State) -> Self {
        Self { state: Mutex::new(state), confirmed: Mutex::new(Confirmed::default()) }
    }
    fn guard<'a>(&'a self, state: MutexGuard<'a, State>) -> StateGuard<'a> {
        StateGuard { owner: self, state, dirty: false }
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
    pub(super) fn confirmed(&self, token: &ResourceToken) -> Result<Option<Route>, String> {
        let key = serde_json::to_string(token).map_err(|error| error.to_string())?;
        self.confirmed.lock().unwrap_or_else(|p| p.into_inner())
            .entries.get(&key).map(|(_, answer)| answer.clone()).transpose()
    }
    pub(super) fn confirmed_game_owner(&self, game: &str, reader: &str) -> Result<Option<()>, String> {
        self.confirmed.lock().unwrap_or_else(|p| p.into_inner()).owners
            .get(&(game.into(), reader.into())).cloned().transpose()
    }
}

impl StateGuard<'_> {
    pub(super) fn confirm_game_owner(&self, game: &str, reader: &str, answer: &Result<(), String>) {
        let key = (game.to_owned(), reader.to_owned());
        let mut confirmed = self.owner.confirmed.lock().unwrap_or_else(|p| p.into_inner());
        if !confirmed.owners.contains_key(&key) {
            if confirmed.owners.len() == OWNER_CAPACITY {
                if let Some(oldest) = confirmed.owner_order.pop_front() { confirmed.owners.remove(&oldest); }
            }
            confirmed.owner_order.push_back(key.clone());
        }
        confirmed.owners.insert(key, answer.clone());
    }
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
        let (tokens, owners) = {
            let confirmed = self.owner.confirmed.lock().unwrap_or_else(|p| p.into_inner());
            (confirmed.entries.iter().map(|(key, (token, _))| (key.clone(), token.clone())).collect::<Vec<_>>(),
                confirmed.owners.keys().cloned().collect::<Vec<_>>())
        };
        let answers = tokens.into_iter().map(|(key, token)| {
            let answer = Runtime::validate_delivery_locked(&self.state, &token);
            (key, (token, answer))
        }).collect();
        let owner_answers = owners.into_iter().map(|(game, reader)| {
            let answer = Runtime::validate_game_delivery_owner(&self.state, &game, &reader);
            ((game, reader), answer)
        }).collect();
        // State remains locked until this guard's fields drop. New state can
        // therefore never be observed alongside answers from an older commit.
        let old = {
            let mut confirmed = self.owner.confirmed.lock().unwrap_or_else(|p| p.into_inner());
            (std::mem::replace(&mut confirmed.entries, answers),
                std::mem::replace(&mut confirmed.owners, owner_answers))
        };
        drop(old);
    }
}

#[cfg(test)]
#[path = "live_validation_tests.rs"]
mod tests;
