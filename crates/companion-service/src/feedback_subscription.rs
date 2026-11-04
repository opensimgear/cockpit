use super::*;
use osg_streams::NativeWake;

/// One passive reader for the exact panel session. Its cursor is independent
/// of request acknowledgements and it never renews a session lease.
pub struct FeedbackSubscription {
    hub: Hub,
    companion_id: String,
    session_id: String,
    generation: u64,
    wake: Arc<NativeWake>,
    cursor: crate::feedback::FeedbackCursor,
}

impl Hub {
    pub fn subscribe_feedback(
        &self, connection: &str, session_id: &str, generation: u64,
        publication_hash: &str, credential: &str, wake: Arc<NativeWake>,
    ) -> Result<FeedbackSubscription, String> {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        self.expire(&mut inner, self.now_ms())?;
        ensure(!inner.connections.contains_key(connection), "Use a dedicated feedback connection.")?;
        ensure(secret_valid(credential), "Invalid installation credential.")?;
        let digest = hash(credential.as_bytes());
        let companion = inner.record.credentials.iter().find(|row| equal_hash(&row.token_hash, &digest))
            .ok_or("Installation credential is invalid or revoked.")?.companion_id.clone();
        let session = inner.sessions.get_mut(session_id).ok_or("Feedback session ended.")?;
        ensure(
            session.companion_id == companion && session.token.generation == generation
                && session.token.publication_hash == publication_hash && session.armed,
            "Feedback subscription belongs to another or disarmed session.",
        )?;
        if let Some(previous) = session.feedback_wake.replace(Arc::downgrade(&wake)).and_then(|previous| previous.upgrade()) {
            previous.notify();
        }
        wake.notify();
        Ok(FeedbackSubscription {
            hub: self.clone(), companion_id: companion, session_id: session_id.into(),
            generation, wake, cursor: Default::default(),
        })
    }

    pub(super) fn wake_feedback(inner: &Inner, instance: &str) {
        for session in inner.sessions.values().filter(|session| session.token.source_id == instance) {
            if let Some(wake) = session.feedback_wake.as_ref().and_then(|wake| wake.upgrade()) {
                wake.notify();
            }
        }
    }
}

impl FeedbackSubscription {
    pub fn take(&mut self) -> Result<Option<Value>, String> {
        let inner = self.hub.inner.lock().unwrap_or_else(|p| p.into_inner());
        let session = inner.sessions.get(&self.session_id).ok_or("Feedback session ended.")?;
        ensure(
            session.token.generation == self.generation && session.armed
                && session.companion_id == self.companion_id
                && inner.connections.get(&session.connection) == Some(&self.companion_id)
                && session.feedback_wake.as_ref().is_some_and(|wake| wake.ptr_eq(&Arc::downgrade(&self.wake))),
            "Feedback subscription is obsolete.",
        )?;
        let empty = crate::feedback::FeedbackBook::default();
        let book = inner.feedback.get(&session.token.source_id).unwrap_or(&empty);
        if self.cursor.initialized && self.cursor.generation == book.generation && self.cursor.sequence == book.sequence {
            return Ok(None);
        }
        let identity = json!({
            "clientId":session.client_id,"sessionId":self.session_id,"generation":self.generation.to_string(),
            "controllerInstanceId":session.token.source_id,"publicationHash":session.token.publication_hash,
        });
        let mut cursor = self.cursor.clone();
        let feedback = book.deliver(&mut cursor, &identity, inner.last_now.max(self.hub.now_ms()));
        ensure(serde_json::to_vec(&feedback).map_err(|e| e.to_string())?.len() <= FEEDBACK_BYTES,
            "Feedback transfer exceeds its bounded budget.")?;
        self.cursor = cursor;
        Ok(Some(feedback))
    }
}
