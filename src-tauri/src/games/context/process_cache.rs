//! A process inventory is reusable only inside the exact live event scope.
use crate::game_discovery::{process_selection, ProcessLookup, ProcessLookupError};
use serde_json::Value;
use std::sync::Arc;

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
            Some(snapshot.epoch) == epoch && snapshot.selection == selection
                && snapshot.rows.iter().all(|row| !matches!(row, Err(ProcessLookupError::Unavailable(_))))
        )
        {
            return snapshot.rows.clone();
        }
        let rows = capture(games);
        self.snapshot = epoch.filter(|_| rows.len() == games.len())
            .map(|epoch| Arc::new(Snapshot { epoch, selection, rows: rows.clone() }));
        rows
    }

    pub(super) fn is_current(&self, epoch: (u64, u64)) -> bool {
        self.snapshot.as_ref().is_some_and(|snapshot| snapshot.epoch == epoch
            && snapshot.rows.iter().all(|row| !matches!(row, Err(ProcessLookupError::Unavailable(_)))))
    }

    pub(super) fn snapshot(&self) -> Option<Arc<Snapshot>> { self.snapshot.clone() }
}

#[cfg(test)]
#[path = "process_cache_tests.rs"]
mod tests;
