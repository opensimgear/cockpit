//! A process inventory is reusable only inside the exact live event scope.
use crate::game_discovery::{Platform, ProcessLookup};
use serde_json::Value;

#[derive(Default)]
pub(super) struct Cache {
    epoch: Option<(u64, u64)>,
    selection: Vec<[Value; 6]>,
    rows: Vec<ProcessLookup>,
}

impl Cache {
    pub(super) fn read(&mut self, platform: &dyn Platform, games: &[&Value], epoch: Option<(u64, u64)>) -> Vec<ProcessLookup> {
        let epoch = epoch.filter(|_| games.iter().all(|game| game["processMonitoringSupported"] == true));
        let selection: Vec<_> = games.iter().map(|game| {
            ["id", "provider", "integrationKey", "integrationCompatible", "executable", "processExecutables"]
                .map(|key| game[key].clone())
        }).collect();
        if epoch.is_some() && self.epoch == epoch && self.selection == selection {
            return self.rows.clone();
        }
        let rows = platform.integration_processes(games);
        self.epoch = epoch.filter(|_| rows.len() == games.len() && rows.iter().all(Result::is_ok));
        self.selection = selection;
        self.rows = rows.clone();
        rows
    }

    pub(super) fn is_current(&self, epoch: (u64, u64)) -> bool { self.epoch == Some(epoch) }
}
