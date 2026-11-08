//! A process inventory is reusable only inside the exact live event scope.
use super::Inner;
use crate::game_discovery::{process_selection, ProcessLookup, ProcessLookupError};
use serde_json::Value;
use std::sync::{atomic::Ordering, Arc};

#[derive(Default)]
pub(super) struct Cache {
    snapshot: Option<Arc<Snapshot>>,
}

pub(in crate::games) struct Snapshot {
    epoch: (u64, u64),
    selection: Vec<[Value; 6]>,
    rows: Vec<ProcessLookup>,
}

impl Snapshot {
    pub(super) fn process(&self, game: &Value, epoch: (u64, u64)) -> Option<ProcessLookup> {
        if self.epoch != epoch || game["processMonitoringSupported"] != true { return None; }
        let selected = process_selection(game);
        let index = self.selection.iter().position(|entry| *entry == selected)?;
        self.rows.get(index).cloned()
    }
}

impl Cache {
    #[cfg(test)]
    pub(super) fn read(&mut self, platform: &dyn crate::game_discovery::Platform, games: &[&Value], epoch: Option<(u64, u64)>) -> Vec<ProcessLookup> {
        self.read_using(games, epoch, |games| platform.integration_processes(games))
    }
    pub(super) fn read_using(&mut self, games: &[&Value], epoch: Option<(u64, u64)>,
        capture: impl FnOnce(&[&Value]) -> Vec<ProcessLookup>) -> Vec<ProcessLookup> {
        let epoch = epoch.filter(|_| games.iter().all(|game| game["processMonitoringSupported"] == true));
        let selection: Vec<_> = games.iter().map(|game| process_selection(game)).collect();
        if let Some(snapshot) = self.snapshot.as_ref().filter(|snapshot|
            Some(snapshot.epoch) == epoch && snapshot.selection == selection)
        {
            let pending: Vec<_> = snapshot.rows.iter().enumerate().filter_map(|(index, row)|
                matches!(row, Err(ProcessLookupError::Unavailable(_))).then_some(index)).collect();
            let mut rows = snapshot.rows.clone();
            if pending.is_empty() {
                return rows;
            }
            let selected: Vec<_> = pending.iter().map(|&index| games[index]).collect();
            let retried = capture(&selected);
            if retried.len() == pending.len() {
                for (index, row) in pending.into_iter().zip(retried) {
                    rows[index] = row;
                }
                self.snapshot = Some(Arc::new(Snapshot { epoch: snapshot.epoch, selection, rows: rows.clone() }));
            }
            return rows;
        }
        let rows = capture(games);
        if rows.len() != games.len() {
            self.snapshot = None;
            return unavailable_rows(games, "Game process inventory was incomplete.");
        }
        self.snapshot = epoch.map(|epoch| Arc::new(Snapshot { epoch, selection, rows: rows.clone() }));
        rows
    }

    pub(super) fn is_current(&self, epoch: (u64, u64)) -> bool {
        self.snapshot.as_ref().is_some_and(|snapshot| snapshot.epoch == epoch
            && snapshot.rows.iter().all(|row| !matches!(row, Err(ProcessLookupError::Unavailable(_)))))
    }

    pub(super) fn snapshot(&self) -> Option<Arc<Snapshot>> { self.snapshot.clone() }
}

fn unavailable_rows(games: &[&Value], reason: &str) -> Vec<ProcessLookup> {
    games.iter().map(|_| Err(ProcessLookupError::Unavailable(reason.into()))).collect()
}

impl Inner {
    pub(super) fn read_context_inventory(
        &self,
        cache: &mut Cache,
        epoch: (u64, u64),
        games: &[&Value],
        capture: impl FnOnce(&[&Value]) -> Vec<ProcessLookup>,
    ) -> Vec<ProcessLookup> {
        let current = || !self.closed.load(Ordering::Acquire)
            && (self.changes.context_wake.observed(), self.changes.context_process_wake.observed()) == epoch;
        let watched = self.current_process_epoch() == Some(epoch);
        let rows = current().then(|| cache.read_using(games, watched.then_some(epoch), capture));
        let still_watched = self.current_process_epoch() == Some(epoch);
        let rows = rows.filter(|_| current() && (!watched || still_watched));
        if rows.is_none() {
            cache.snapshot = None;
        }
        *self.context_process_inventory.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            if still_watched { cache.snapshot() } else { None };
        rows.unwrap_or_else(|| unavailable_rows(games, "Game process observation was superseded."))
    }
}

#[cfg(test)]
#[path = "process_cache_tests.rs"]
mod tests;
