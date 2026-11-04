use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub(super) struct StatusObserver(Option<Arc<dyn Fn() + Send + Sync>>);

impl StatusObserver {
    pub(super) fn new(callback: impl Fn() + Send + Sync + 'static) -> Self {
        Self(Some(Arc::new(callback)))
    }

    pub(super) fn notify(&self) {
        if let Some(callback) = &self.0 {
            callback();
        }
    }

    pub(super) fn set(&self, state: &Mutex<Value>, key: &str, value: Value) {
        self.set_fields(state, [(key, value)]);
    }

    pub(super) fn set_fields<const N: usize>(&self, state: &Mutex<Value>, fields: [(&str, Value); N]) {
        let mut state = state.lock().unwrap_or_else(|p| p.into_inner());
        let mut changed = false;
        for (key, value) in fields {
            changed |= update_field(&mut state, key, value);
        }
        if changed {
            drop(state);
            self.notify();
        }
    }

    pub(super) fn verified_status(
        &self, state: &Mutex<Value>, device: Value, stage: Value,
        recovery: Option<Value>, observed_ms: u64, cleared_error: bool,
    ) {
        let mut state = state.lock().unwrap_or_else(|p| p.into_inner());
        // The timestamp advances for every verified report, independently of changes.
        state["_statusObservedMs"] = json!(observed_ms);
        let mut changed = update_field(&mut state, "ready", json!(true));
        changed |= update_field(&mut state, "device", device);
        changed |= update_field(&mut state, "stage", stage);
        changed |= update_recovery(&mut state, recovery);
        drop(state);
        if changed || cleared_error {
            self.notify();
        }
    }
}

fn update_field(state: &mut Value, key: &str, value: Value) -> bool {
    if state[key] == value {
        return false;
    }
    state[key] = value;
    true
}

fn update_recovery(state: &mut Value, recovery: Option<Value>) -> bool {
    let current = &state["recovery"];
    let changed = match &recovery {
        Some(next) => ["phase", "attempt", "message"].iter().any(|key| current[key] != next[key]),
        None => !current.is_null(),
    };
    if let Some(recovery) = recovery {
        state["recovery"] = recovery;
    } else if let Some(state) = state.as_object_mut() {
        state.remove("recovery");
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn unchanged_state_stays_quiet_and_callback_can_read_committed_state() {
        let state = Arc::new(Mutex::new(json!({"ready":false,"scanError":"old"})));
        let observed = state.clone();
        let count = Arc::new(AtomicUsize::new(0));
        let calls = count.clone();
        let observer = StatusObserver::new(move || {
            let state = observed.lock().unwrap();
            assert_eq!(state["ready"], true);
            assert_eq!(state["scanError"], Value::Null);
            calls.fetch_add(1, Ordering::Relaxed);
        });
        observer.set(&state, "ready", json!(false));
        observer.set_fields(&state, [("ready", json!(true)), ("scanError", Value::Null)]);
        observer.set(&state, "ready", json!(true));
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn recovery_wakes_for_transitions_but_not_countdown_changes() {
        let mut state = json!({});
        let waiting = |ms| json!({"phase":"waiting","attempt":1,"message":"Waiting","nextRetryMs":ms});
        assert!(update_recovery(&mut state, Some(waiting(500))));
        assert!(!update_recovery(&mut state, Some(waiting(250))));
        assert_eq!(state["recovery"]["nextRetryMs"], 250);
        assert!(update_recovery(&mut state, None));
        assert!(!update_recovery(&mut state, None));
    }

    #[test]
    fn verified_reports_wake_for_visible_changes_and_keep_freshness_separate() {
        let count = Arc::new(AtomicUsize::new(0));
        let calls = count.clone();
        let observer = StatusObserver::new(move || { calls.fetch_add(1, Ordering::Relaxed); });
        let state = Mutex::new(json!({}));
        let device = json!({"status":"awaiting-assignment"});
        let stage = json!("Paired");
        observer.verified_status(&state, device.clone(), stage.clone(), None, 1, false);
        observer.verified_status(&state, device.clone(), stage.clone(), None, 2, false);
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert_eq!(state.lock().unwrap()["_statusObservedMs"], 2);
        observer.verified_status(&state, device, stage, None, 3, true);
        assert_eq!(count.load(Ordering::Relaxed), 2);
    }
}
