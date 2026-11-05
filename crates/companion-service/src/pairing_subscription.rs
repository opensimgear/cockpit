use super::*;
use osg_streams::NativeWake;

pub(super) struct PairingReader {
    request_id: String,
    secret_hash: String,
    wake: std::sync::Weak<NativeWake>,
    terminal: Option<&'static str>,
}

/// A passive one-shot reader. Observing approval never authenticates a connection
/// or grants input authority; the native client must explicitly connect later.
pub struct PairingSubscription {
    hub: Hub,
    connection: String,
    wake: Arc<NativeWake>,
    expires: u64,
}

impl Hub {
    pub fn subscribe_pairing(
        &self, frame: &Value, connection: &str, wake: Arc<NativeWake>,
    ) -> Result<PairingSubscription, String> {
        ensure(id(connection) && serde_json::to_vec(frame).map_err(|_| "Invalid frame.")?.len() <= FRAME_BYTES,
            "Frame exceeds budget.")?;
        ensure(frame.get("version").is_none_or(|version| version == 1), "Unsupported protocol version.")?;
        exact(frame, &["type", "requestId", "secret"], &["version"])?;
        ensure(frame["type"] == "pair-subscribe", "Invalid pairing subscription type.")?;
        let request_id = field(frame, "requestId")?;
        let secret = field(frame, "secret")?;
        ensure(secret_valid(secret), "Pairing request owner mismatch.")?;
        // A caller may reuse its worker wake after disconnect. Each retained
        // reader still needs a distinct identity so an old Drop cannot retire it.
        let wake = Arc::new(NativeWake::child(wake));
        let digest = hash(secret.as_bytes());
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        self.expire(&mut inner, self.now_ms())?;
        ensure(!inner.connections.contains_key(connection), "Use an unauthenticated pairing connection.")?;
        let pending = inner.pending.get(request_id).ok_or("Pairing request expired or missing.")?;
        ensure(equal_hash(&pending.secret_hash, &digest), "Pairing request owner mismatch.")?;
        let expires = pending.expires;
        inner.pairing_readers.retain(|_, reader| reader.wake.strong_count() > 0);
        ensure(inner.pairing_readers.len() < 16, "Pairing subscription budget exhausted.")?;
        ensure(!inner.pairing_readers.contains_key(connection)
            && !inner.pairing_readers.values().any(|reader| reader.request_id == request_id),
            "Pairing request already has a reader.")?;
        inner.pairing_readers.insert(connection.into(), PairingReader {
            request_id: request_id.into(), secret_hash: digest, wake: Arc::downgrade(&wake), terminal: None,
        });
        Ok(PairingSubscription { hub: self.clone(), connection: connection.into(), wake, expires })
    }

    pub(super) fn notify_pairing(inner: &mut Inner, request_id: &str, terminal: Option<&'static str>) {
        for reader in inner.pairing_readers.values_mut().filter(|reader| reader.request_id == request_id) {
            if reader.terminal.is_none() { reader.terminal = terminal; }
            if let Some(wake) = reader.wake.upgrade() { wake.notify(); }
        }
    }

    pub(super) fn retire_pairing_reader(inner: &mut Inner, connection: &str) {
        if let Some(reader) = inner.pairing_readers.remove(connection) {
            if let Some(wake) = reader.wake.upgrade() { wake.notify(); }
        }
    }

    pub(super) fn retire_pairing_readers(inner: &mut Inner) {
        for reader in std::mem::take(&mut inner.pairing_readers).into_values() {
            if let Some(wake) = reader.wake.upgrade() { wake.notify(); }
        }
    }
}

impl PairingSubscription {
    pub fn observed(&self) -> u64 { self.wake.observed() }
    pub fn expires_at_ms(&self) -> u64 { self.expires }
    pub fn remaining_ms(&self) -> u64 { self.expires.saturating_sub(self.hub.now_ms()) }
    pub fn take_current(&mut self) -> Result<Value, String> {
        self.take_with_clock(Hub::now_ms)
    }

    fn take_with_clock(&mut self, now_ms: impl FnOnce(&Hub) -> u64) -> Result<Value, String> {
        let mut inner = self.hub.inner.lock().unwrap_or_else(|p| p.into_inner());
        // Time spent waiting for the Hub lock counts against the offer's expiry.
        self.hub.expire(&mut inner, now_ms(&self.hub))?;
        let reader = inner.pairing_readers.get(&self.connection)
            .filter(|reader| reader.wake.ptr_eq(&Arc::downgrade(&self.wake)))
            .ok_or("Pairing subscription ended.")?;
        if let Some(error) = reader.terminal { return Err(error.into()); }
        let pending = inner.pending.get(&reader.request_id).ok_or("Pairing request expired or missing.")?;
        ensure(equal_hash(&pending.secret_hash, &reader.secret_hash), "Pairing subscription is obsolete.")?;
        if let Some(token) = &pending.token {
            let digest = hash(token.as_bytes());
            ensure(inner.record.credentials.iter().any(|credential|
                credential.companion_id == pending.companion_id && equal_hash(&credential.token_hash, &digest)),
                "Pairing approval was revoked or replaced.")?;
        }
        Ok(pairing_reply(&inner, &reader.request_id, pending))
    }
}

impl Drop for PairingSubscription {
    fn drop(&mut self) {
        let mut inner = self.hub.inner.lock().unwrap_or_else(|p| p.into_inner());
        if inner.pairing_readers.get(&self.connection)
            .is_some_and(|reader| reader.wake.ptr_eq(&Arc::downgrade(&self.wake))) {
            inner.pairing_readers.remove(&self.connection);
        }
    }
}

#[cfg(test)]
#[path = "pairing_subscription_tests.rs"]
mod tests;
