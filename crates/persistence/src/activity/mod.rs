//! Durable activity history, separate from operational records and credentials.
use crate::{failure, Database};
use rusqlite::{params, Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

mod query;

const DAY_MS: i64 = 86_400_000;
const MAX_DATE: i64 = 253_402_300_799_999;

pub(crate) fn initialize(connection: &mut Connection, expected_epoch: &str) -> Result<(), String> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(failure)?;
    if crate::epoch::read(&tx)? != expected_epoch {
        return Err(
            "The database was restored or replaced. Restart Cockpit before continuing.".into(),
        );
    }
    tx.execute_batch(include_str!("schema.sql"))
        .map_err(failure)?;
    tx.commit().map_err(failure)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEvent {
    pub id: u64,
    pub occurred_at: i64,
    pub category: String,
    pub event_type: String,
    pub level: String,
    pub message: String,
    pub detail: String,
    pub entity_id: Option<String>,
    pub entity_name: Option<String>,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewActivityEvent {
    pub occurred_at: i64,
    pub category: String,
    pub event_type: String,
    pub level: String,
    pub message: String,
    pub detail: String,
    pub entity_id: Option<String>,
    pub entity_name: Option<String>,
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ActivityQuery {
    pub search: String,
    pub categories: Vec<String>,
    pub levels: Vec<String>,
    pub event_types: Vec<String>,
    pub entity_id: Option<String>,
    pub since: Option<i64>,
    pub until: Option<i64>,
    /// Highest event ID admitted by this browsing session. None starts live.
    pub through_id: Option<u64>,
    pub offset: usize,
    pub limit: usize,
}
impl Default for ActivityQuery {
    fn default() -> Self {
        Self {
            search: String::new(),
            categories: vec![],
            levels: vec![],
            event_types: vec![],
            entity_id: None,
            since: None,
            until: None,
            through_id: None,
            offset: 0,
            limit: 100,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityPage {
    pub events: Vec<ActivityEvent>,
    pub through_id: u64,
    pub total: usize,
    pub categories: Vec<String>,
    pub levels: Vec<String>,
    pub event_types: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ActivitySettings {
    pub retention_days: u32,
}
impl Default for ActivitySettings {
    fn default() -> Self {
        Self { retention_days: 7 }
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(MAX_DATE as u128) as i64
}
fn valid_date(value: i64) -> bool {
    (0..=MAX_DATE).contains(&value)
}
fn settings(connection: &Connection) -> Result<ActivitySettings, String> {
    connection
        .query_row(
            "SELECT retention_days FROM activity_settings WHERE singleton=1",
            [],
            |row| {
                Ok(ActivitySettings {
                    retention_days: row.get(0)?,
                })
            },
        )
        .map_err(failure)
}
fn prune(connection: &Connection) -> Result<usize, String> {
    let cutoff = now().saturating_sub(i64::from(settings(connection)?.retention_days) * DAY_MS);
    connection.execute("DELETE FROM activity_fts WHERE rowid IN (SELECT id FROM activity_events WHERE occurred_at<?1)", [cutoff]).map_err(failure)?;
    connection
        .execute("DELETE FROM activity_events WHERE occurred_at<?1", [cutoff])
        .map_err(failure)
}
fn validate_event(event: &NewActivityEvent) -> Result<(), String> {
    let fields = [
        &event.category,
        &event.event_type,
        &event.level,
        &event.source,
    ];
    if !valid_date(event.occurred_at)
        || fields
            .iter()
            .any(|s| s.trim().is_empty() || s.len() > 4096 || s.chars().any(char::is_control))
        || event.message.trim().is_empty()
        || event.message.len() > 65536
        || event.detail.len() > 1_048_576
        || [event.entity_id.as_ref(), event.entity_name.as_ref()]
            .into_iter()
            .flatten()
            .any(|s| s.len() > 4096)
    {
        return Err("Invalid activity event.".into());
    }
    Ok(())
}

impl Database {
    pub fn activity_append(&self, events: &[NewActivityEvent]) -> Result<(), String> {
        if events.len() > 1024 {
            return Err("Invalid activity batch size.".into());
        }
        for event in events {
            validate_event(event)?;
        }
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        self.verify_epoch(&tx)?;
        for event in events {
            tx.execute("INSERT INTO activity_events(occurred_at,category,event_type,level,message,detail,entity_id,entity_name,source) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![event.occurred_at,event.category,event.event_type,event.level,event.message,event.detail,event.entity_id,event.entity_name,event.source]).map_err(failure)?;
            tx.execute("INSERT INTO activity_fts(rowid,message,detail,entity_name,entity_id,event_type,category) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![tx.last_insert_rowid(),event.message,event.detail,event.entity_name,event.entity_id,event.event_type,event.category]).map_err(failure)?;
        }
        prune(&tx)?;
        tx.commit().map_err(failure)
    }
    /// Call periodically from the native storage worker, even while history is closed.
    pub fn activity_prune(&self) -> Result<usize, String> {
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        self.verify_epoch(&tx)?;
        let deleted = prune(&tx)?;
        tx.commit().map_err(failure)?;
        Ok(deleted)
    }
    pub fn activity_clear(&self) -> Result<usize, String> {
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        self.verify_epoch(&tx)?;
        tx.execute("DELETE FROM activity_fts", [])
            .map_err(failure)?;
        let deleted = tx
            .execute("DELETE FROM activity_events", [])
            .map_err(failure)?;
        tx.commit().map_err(failure)?;
        Ok(deleted)
    }
    pub fn activity_settings(&self) -> Result<ActivitySettings, String> {
        self.activity_prune()?;
        settings(&self.connection()?)
    }
    pub fn activity_settings_save(&self, retention_days: u32) -> Result<ActivitySettings, String> {
        if !(1..=3650).contains(&retention_days) {
            return Err("Activity retention must be between 1 and 3650 days.".into());
        }
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        self.verify_epoch(&tx)?;
        tx.execute(
            "UPDATE activity_settings SET retention_days=?1 WHERE singleton=1",
            [retention_days],
        )
        .map_err(failure)?;
        prune(&tx)?;
        tx.commit().map_err(failure)?;
        Ok(ActivitySettings { retention_days })
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod query_tests;
