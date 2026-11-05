use super::*;
use osg_companion_service::PairingSubscription;

/// Retained once per USB link; unrelated worker wakes do not query pairing state.
#[derive(Default)]
pub(super) struct Pairing {
    reader: Option<PairingSubscription>,
    observed: Option<u64>,
}

impl Pairing {
    pub(super) fn clear(&mut self) { self.reader = None; self.observed = None; }

    pub(super) fn subscribe(&mut self, hub: &Hub, frame: &Value, connection: &str, wake: &Arc<NativeWake>) -> Value {
        let result = if self.reader.is_some() {
            Err("Pairing request already has a reader.".into())
        } else {
            hub.subscribe_pairing(frame, connection, wake.clone())
        };
        match result {
            Ok(reader) => {
                self.reader = Some(reader);
                self.observed = None;
                json!({"ok":true,"type":"pair-subscribed"})
            }
            Err(error) => json!({"ok":false,"error":error}),
        }
    }

    pub(super) fn delay(&self) -> Option<Duration> {
        self.reader.as_ref().map(|reader| Duration::from_millis(reader.remaining_ms()))
    }

    pub(super) fn take(&mut self) -> Option<Value> {
        let reader = self.reader.as_mut()?;
        let observed = reader.observed();
        if self.observed == Some(observed) && reader.remaining_ms() > 0 { return None; }
        self.observed = Some(observed);
        let terminal = match reader.take_current() {
            Ok(value) if value["type"] == "paired" => value,
            Ok(_) => return None,
            Err(error) => json!({"ok":false,"error":error}),
        };
        self.clear();
        Some(terminal)
    }
}

#[cfg(test)]
#[path = "pairing_tests.rs"]
mod tests;
