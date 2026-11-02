//! Explicit permission jobs; passive status reads never create native watchers.
use super::process_events::Monitor;
use crate::game_discovery::ProcessIdentity;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};
use tokio::sync::watch;

struct Entry {
    game: String,
    operation: String,
    began: Instant,
    state: &'static str,
    error: Option<String>,
    monitor: Option<Arc<Monitor>>,
    cancel: watch::Sender<bool>,
}
static ENTRIES: OnceLock<Mutex<BTreeMap<String, Entry>>> = OnceLock::new();
fn entries() -> &'static Mutex<BTreeMap<String, Entry>> {
    ENTRIES.get_or_init(Default::default)
}
fn key(expected: &ProcessIdentity) -> String {
    format!(
        "{}:{}:{}",
        expected.pid,
        expected.epoch,
        crate::game_discovery::normalized_path(&expected.path)
    )
}

pub(super) fn acquire(game: &str, expected: &ProcessIdentity) -> Result<Arc<Monitor>, String> {
    {
        let registry = entries().lock().unwrap_or_else(|p| p.into_inner());
        if let Some(entry) = registry.get(&key(expected)) {
            if let Some(monitor) = &entry.monitor {
                if let Some(error) = monitor.error() {
                    return Err(error);
                }
                return Ok(monitor.clone());
            }
            return Err(match entry.state {
                "checking" | "starting" => "Preparing native process monitoring.",
                "disabled" => "Process monitoring was stopped. Choose Allow to enable it again.",
                _ => {
                    "Windows process notification permission is needed. Choose Allow for this game."
                }
            }
            .into());
        }
    }
    begin(game, expected.clone(), false)?;
    Err("Preparing native process monitoring.".into())
}

pub(crate) fn enable(game: &str, expected: ProcessIdentity) -> Result<(), String> {
    if !matches!(game, "x-plane-11" | "x-plane-12") {
        return Err("Choose a supported X-Plane game.".into());
    }
    if !cfg!(windows) {
        return Err("Native process notification permission requires Windows.".into());
    }
    {
        let registry = entries().lock().unwrap_or_else(|p| p.into_inner());
        if let Some(entry) = registry.get(&key(&expected)) {
            if entry.state == "starting" {
                return Err("A process monitoring permission request is already pending.".into());
            }
            if entry
                .monitor
                .as_ref()
                .is_some_and(|monitor| monitor.error().is_none())
            {
                return Ok(());
            }
        }
    }
    begin(game, expected, true)
}
fn begin(game: &str, expected: ProcessIdentity, elevated: bool) -> Result<(), String> {
    begin_with(game, expected, elevated, move |expected, cancelled| {
        if elevated {
            Monitor::start_elevated_with_cancel(&expected, cancelled)
        } else {
            Monitor::start(&expected)
        }
    })
    .map(|_| ())
}
fn begin_with(
    game: &str,
    expected: ProcessIdentity,
    elevated: bool,
    factory: impl FnOnce(ProcessIdentity, watch::Receiver<bool>) -> Result<Monitor, String>
        + Send
        + 'static,
) -> Result<Option<std::thread::JoinHandle<()>>, String> {
    let identity = key(&expected);
    let operation = uuid::Uuid::new_v4().to_string();
    let (cancel, cancelled) = watch::channel(false);
    {
        let mut registry = entries().lock().unwrap_or_else(|p| p.into_inner());
        if let Some(entry) = registry.get(&identity) {
            if !elevated {
                return Ok(None);
            }
            if entry.state == "starting" {
                return Err("A process monitoring permission request is already pending.".into());
            }
            if entry
                .monitor
                .as_ref()
                .is_some_and(|monitor| monitor.error().is_none())
            {
                return Ok(None);
            }
        }
        if !registry.contains_key(&identity) && registry.len() >= 16 {
            registry.retain(|_, entry| {
                matches!(entry.state, "checking" | "starting")
                    || entry
                        .monitor
                        .as_ref()
                        .is_some_and(|monitor| monitor.error().is_none())
            });
            if registry.len() >= 16 {
                return Err("Native process monitor limit reached.".into());
            }
        }
        if let Some(previous) = registry.remove(&identity) {
            previous.cancel.send_replace(true);
            if let Some(monitor) = previous.monitor {
                monitor.cancel();
            }
        }
        registry.insert(
            identity.clone(),
            Entry {
                game: game.into(),
                operation: operation.clone(),
                began: Instant::now(),
                state: if elevated { "starting" } else { "checking" },
                error: None,
                monitor: None,
                cancel,
            },
        );
    }
    let job_identity = identity.clone();
    let job_operation = operation.clone();
    let spawned = std::thread::Builder::new()
        .name("process-monitor-permission".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                factory(expected, cancelled)
            }))
            .unwrap_or_else(|_| Err("Native process monitor startup panicked.".into()));
            let mut registry = entries().lock().unwrap_or_else(|p| p.into_inner());
            let current = registry
                .get_mut(&job_identity)
                .filter(|entry| entry.operation == job_operation && !*entry.cancel.borrow());
            let Some(entry) = current else {
                if let Ok(monitor) = result {
                    monitor.cancel();
                }
                return;
            };
            match result {
                Ok(monitor) => {
                    entry.monitor = Some(Arc::new(monitor));
                    entry.state = "ready";
                }
                Err(error) => {
                    let denied = error.contains("80041003")
                        || error.to_lowercase().contains("permission denied")
                        || error.to_lowercase().contains("access is denied");
                    entry.state = if !elevated && denied {
                        "needs-permission"
                    } else {
                        "failed"
                    };
                    entry.error = Some(error.chars().take(1024).collect());
                }
            }
        });
    match spawned {
        Ok(worker) => Ok(Some(worker)),
        Err(error) => {
            if let Some(entry) = entries()
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get_mut(&identity)
                .filter(|entry| entry.operation == operation)
            {
                entry.state = "failed";
                entry.error = Some(error.to_string());
            }
            Err(error.to_string())
        }
    }
}

pub(crate) fn cancel(game: &str) {
    let mut registry = entries().lock().unwrap_or_else(|p| p.into_inner());
    for entry in registry.values_mut().filter(|entry| entry.game == game) {
        entry.cancel.send_replace(true);
        if let Some(monitor) = &entry.monitor {
            monitor.cancel();
        }
        entry.state = "disabled";
        entry.error = None;
    }
}
pub(crate) fn cancel_all() {
    let mut registry = entries().lock().unwrap_or_else(|p| p.into_inner());
    for entry in registry.values_mut() {
        entry.cancel.send_replace(true);
        if let Some(monitor) = &entry.monitor {
            monitor.cancel();
        }
        entry.state = "disabled";
    }
}
pub(crate) fn status(game: &str) -> Value {
    let registry = entries().lock().unwrap_or_else(|p| p.into_inner());
    let Some(entry) = registry
        .values()
        .filter(|entry| entry.game == game)
        .max_by_key(|entry| entry.began)
    else {
        return json!({"state":"waiting","message":"Start the game to prepare native process monitoring."});
    };
    let failure = entry.monitor.as_ref().and_then(|monitor| monitor.error());
    let state = if entry.state == "ready" && failure.is_some() {
        "failed"
    } else {
        entry.state
    };
    let message = match state {
        "ready" => "Native process monitoring is active for this game session.",
        "checking" => "Checking native process notification permission.",
        "starting" => "Waiting for Windows permission to monitor this game session.",
        "disabled" => "Process monitoring is stopped.",
        _ => "Allow Windows process monitoring to receive this game's telemetry.",
    };
    json!({"state":state,"message":message,"details":failure.or_else(|| entry.error.clone())})
}

#[cfg(test)]
mod tests;
