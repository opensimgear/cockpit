//! A process inventory is reusable only inside the exact live event scope.
use crate::game_discovery::{Platform, ProcessIdentity, ProcessLookup};
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

fn selection(game: &Value) -> [Value; 6] {
    ["id", "provider", "integrationKey", "integrationCompatible", "executable", "processExecutables"]
        .map(|key| game[key].clone())
}

impl Snapshot {
    pub(super) fn process(&self, game: &Value, epoch: (u64, u64)) -> Option<Option<ProcessIdentity>> {
        if self.epoch != epoch || game["processMonitoringSupported"] != true { return None; }
        let selected = selection(game);
        let index = self.selection.iter().position(|entry| *entry == selected)?;
        self.rows[index].as_ref().ok().cloned()
    }
}

impl Cache {
    pub(super) fn read(&mut self, platform: &dyn Platform, games: &[&Value], epoch: Option<(u64, u64)>) -> Vec<ProcessLookup> {
        let epoch = epoch.filter(|_| games.iter().all(|game| game["processMonitoringSupported"] == true));
        let selection: Vec<_> = games.iter().map(|game| selection(game)).collect();
        if let Some(snapshot) = self.snapshot.as_ref().filter(|snapshot|
            Some(snapshot.epoch) == epoch && snapshot.selection == selection)
        {
            return snapshot.rows.clone();
        }
        let rows = platform.integration_processes(games);
        self.snapshot = epoch.filter(|_| rows.len() == games.len() && rows.iter().all(Result::is_ok))
            .map(|epoch| Arc::new(Snapshot { epoch, selection, rows: rows.clone() }));
        rows
    }

    pub(super) fn is_current(&self, epoch: (u64, u64)) -> bool {
        self.snapshot.as_ref().is_some_and(|snapshot| snapshot.epoch == epoch)
    }

    pub(super) fn snapshot(&self) -> Option<Arc<Snapshot>> { self.snapshot.clone() }
}
