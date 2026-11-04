use super::*;
use osg_streams::NativeWake;

pub(crate) struct AssignmentSubscription {
    hub: Hub,
    primary: String,
    companion: String,
    credential_hash: String,
    wake: Arc<NativeWake>,
    previous: Option<Value>,
}

fn changed_assignments(inner: &Inner, companion: &str, previous: &mut Option<Value>) -> Option<Value> {
    let rows = json!(inner.record.instances.iter().filter(|row| row.companion_id == companion)
        .map(|row| json!({"instanceId":row.instance_id,"publicationHash":row.publication_hash,"name":row.definition.name}))
        .collect::<Vec<_>>());
    if previous.as_ref() == Some(&rows) { return None; }
    *previous = Some(rows.clone());
    Some(rows)
}

impl AssignmentSubscription {
    pub(crate) fn take(&mut self) -> Result<Option<Value>, String> {
        let inner = self.hub.inner.lock().unwrap_or_else(|p| p.into_inner());
        ensure(inner.connections.get(&self.primary) == Some(&self.companion)
            && inner.record.credentials.iter().any(|row| row.companion_id == self.companion
                && equal_hash(&row.token_hash, &self.credential_hash))
            && inner.assignment_wakes.get(&self.primary).is_some_and(|wake| wake.ptr_eq(&Arc::downgrade(&self.wake))),
            "Assignment subscription is obsolete.")?;
        Ok(changed_assignments(&inner, &self.companion, &mut self.previous))
    }
}

/// One passive reader for the exact panel session. Its cursor is independent
/// of request acknowledgements and it never renews a session lease.
pub struct FeedbackSubscription {
    hub: Hub,
    companion_id: String,
    credential_hash: String,
    session_id: String,
    generation: u64,
    wake: Arc<NativeWake>,
    cursor: crate::feedback::FeedbackCursor,
    assignments: Option<Value>,
    resync_sequence: Option<u64>,
}

impl Hub {
    pub(crate) fn subscribe_assignments(
        &self, connection: &str, primary: &str, credential: &str, wake: Arc<NativeWake>,
    ) -> Result<AssignmentSubscription, String> {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        ensure(!inner.connections.contains_key(connection), "Use a dedicated assignment connection.")?;
        ensure(secret_valid(credential), "Invalid installation credential.")?;
        let digest = hash(credential.as_bytes());
        let companion = inner.connections.get(primary).ok_or("Primary connection ended.")?.clone();
        ensure(inner.record.credentials.iter().any(|row| row.companion_id == companion
            && equal_hash(&row.token_hash, &digest)), "Installation credential is invalid or revoked.")?;
        Self::retire_assignment_reader(&mut inner, primary);
        inner.assignment_wakes.insert(primary.into(), Arc::downgrade(&wake));
        self.observe_record_changes(wake.clone());
        Ok(AssignmentSubscription {
            hub: self.clone(), primary: primary.into(), companion, credential_hash: digest, wake, previous: None,
        })
    }

    pub(super) fn retire_assignment_reader(inner: &mut Inner, primary: &str) {
        if let Some(wake) = inner.assignment_wakes.remove(primary).and_then(|wake| wake.upgrade()) {
            wake.notify();
        }
    }
    pub fn subscribe_feedback(
        &self, connection: &str, session_id: &str, generation: u64,
        publication_hash: &str, credential: &str, wake: Arc<NativeWake>, stream_resync: bool,
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
        session.feedback_stream_resync = stream_resync.then_some(session.feedback_stream_resync.unwrap_or(0));
        let resync_sequence = session.feedback_stream_resync;
        let primary = session.connection.clone();
        Self::retire_assignment_reader(&mut inner, &primary);
        self.observe_record_changes(wake.clone());
        Ok(FeedbackSubscription {
            hub: self.clone(), companion_id: companion, credential_hash: digest, session_id: session_id.into(),
            generation, wake, cursor: Default::default(), assignments: None, resync_sequence,
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
    pub(crate) fn take_assignments(&mut self) -> Option<Value> {
        let inner = self.hub.inner.lock().unwrap_or_else(|p| p.into_inner());
        if !inner.record.credentials.iter().any(|credential| credential.companion_id == self.companion_id
            && equal_hash(&credential.token_hash, &self.credential_hash)) { return None; }
        changed_assignments(&inner, &self.companion_id, &mut self.assignments)
    }

    pub(crate) fn resync_sequence(&self) -> Option<u64> { self.resync_sequence }

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
        if self.resync_sequence != session.feedback_stream_resync {
            self.resync_sequence = session.feedback_stream_resync;
            self.cursor = Default::default();
        }
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
