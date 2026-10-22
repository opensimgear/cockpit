//! Trusted native feedback route admission. Operator/WSS commands cannot publish samples.
use super::*;
impl Hub {
    pub fn configure_feedback(
        &self,
        instance: &str,
        publication_hash: &str,
        bindings: &[FeedbackBinding],
    ) -> Result<(), String> {
        self.configure_feedback_authorized(instance, publication_hash, bindings, None)
    }
    pub(super) fn configure_feedback_authorized(
        &self,
        instance: &str,
        publication_hash: &str,
        bindings: &[FeedbackBinding],
        receipt: Option<&profile::AssignmentReceipt>,
    ) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        profile::feedback_access(&inner, instance, receipt)?;
        ensure(
            inner
                .record
                .instances
                .iter()
                .any(|a| a.instance_id == instance && a.publication_hash == publication_hash),
            "Unknown/stale feedback assignment.",
        )?;
        let schema = inner
            .publications
            .get(publication_hash)
            .ok_or("Feedback publication is unavailable.")?
            .feedback
            .clone();
        let now = inner.last_now.max(self.now_ms());
        let mut book = inner.feedback.get(instance).cloned().unwrap_or_default();
        book.configure(schema, bindings, now)?;
        inner.feedback.insert(instance.into(), book);
        Ok(())
    }
    pub fn feedback_token(&self, instance: &str) -> Result<FeedbackToken, String> {
        self.feedback_token_authorized(instance, None)
    }
    pub(super) fn feedback_token_authorized(
        &self,
        instance: &str,
        receipt: Option<&profile::AssignmentReceipt>,
    ) -> Result<FeedbackToken, String> {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        profile::feedback_access(&inner, instance, receipt)?;
        self.expire(&mut inner, self.now_ms())?;
        let session = inner
            .sessions
            .values()
            .find(|s| s.token.source_id == instance)
            .ok_or("Feedback companion is disconnected.")?;
        ensure(
            receipt.is_none() || session.armed,
            "Profile feedback requires a fresh actual companion input baseline.",
        )?;
        let book = inner
            .feedback
            .get(instance)
            .ok_or("Configure native feedback bindings first.")?;
        Ok(FeedbackToken {
            instance_id: instance.into(),
            publication_hash: session.token.publication_hash.clone(),
            session_id: session.token.session_id.clone(),
            generation: session.token.generation,
            feedback_generation: book.generation,
        })
    }
    pub fn publish_feedback(
        &self,
        token: &FeedbackToken,
        values: &[FeedbackSample],
        mode: FeedbackMode,
        now_ms: u64,
    ) -> Result<u64, String> {
        self.publish_feedback_authorized(token, values, mode, now_ms, None)
    }
    pub(super) fn publish_feedback_authorized(
        &self,
        token: &FeedbackToken,
        values: &[FeedbackSample],
        mode: FeedbackMode,
        now_ms: u64,
        receipt: Option<&profile::AssignmentReceipt>,
    ) -> Result<u64, String> {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        profile::feedback_access(&inner, &token.instance_id, receipt)?;
        self.expire(&mut inner, now_ms)?;
        let now = inner.last_now;
        let session = inner
            .sessions
            .get(&token.session_id)
            .ok_or("Feedback session is obsolete/disconnected.")?;
        ensure(
            session.token.source_id == token.instance_id
                && session.token.publication_hash == token.publication_hash
                && session.token.generation == token.generation
                && (receipt.is_none() || session.armed),
            "Stale or wrongly routed native feedback token.",
        )?;
        let book = inner
            .feedback
            .get_mut(&token.instance_id)
            .ok_or("Feedback routes were cleared.")?;
        ensure(
            book.generation == token.feedback_generation,
            "Feedback binding generation changed.",
        )?;
        book.publish(values, mode, now)
    }
    pub fn clear_feedback(
        &self,
        instance: &str,
        publication_hash: &str,
        now_ms: u64,
    ) -> Result<(), String> {
        self.clear_feedback_authorized(instance, publication_hash, now_ms, None)
    }
    pub(super) fn clear_feedback_authorized(
        &self,
        instance: &str,
        publication_hash: &str,
        now_ms: u64,
        receipt: Option<&profile::AssignmentReceipt>,
    ) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        profile::feedback_clear_access(&inner, instance, receipt)?;
        self.expire(&mut inner, now_ms)?;
        ensure(
            inner
                .record
                .instances
                .iter()
                .any(|a| a.instance_id == instance && a.publication_hash == publication_hash),
            "Unknown/stale feedback assignment.",
        )?;
        let now = inner.last_now;
        if let Some(book) = inner.feedback.get_mut(instance) {
            book.reset(now)?;
        }
        Ok(())
    }
    pub(super) fn attach_feedback(
        &self,
        inner: &mut Inner,
        frame: &Value,
        reply: &mut Value,
        now: u64,
    ) -> Result<(), String> {
        let session_id = field(frame, "sessionId")?;
        let session = inner
            .sessions
            .get(session_id)
            .ok_or("Feedback session ended.")?;
        // Image/font preparation runs before the device installs its new runtime.
        // It validates these ACKs without applying feedback to the old panel.
        // Preserve the cursor so the first rendered-panel ACK is a snapshot.
        if !session.armed && frame["type"] == "heartbeat"
            && matches!(frame["operation"]["phase"].as_str(), Some("checking-cache" | "downloading")) {
            reply["feedback"] = Value::Null;
            reply["feedbackSequence"] = Value::Null;
            return Ok(());
        }
        let book = inner
            .feedback
            .get(&session.token.source_id)
            .cloned()
            .unwrap_or_default();
        let identity = json!({"clientId":session.client_id,"sessionId":session_id,"generation":session.token.generation.to_string(),"controllerInstanceId":session.token.source_id,"publicationHash":session.token.publication_hash});
        let mut cursor = session.feedback_cursor.clone();
        let feedback = book.deliver(&mut cursor, &identity, now);
        ensure(
            serde_json::to_vec(&feedback)
                .map_err(|e| e.to_string())?
                .len()
                <= FEEDBACK_BYTES,
            "Feedback transfer exceeds its bounded budget.",
        )?;
        inner.sessions.get_mut(session_id).unwrap().feedback_cursor = cursor;
        reply["feedback"] = feedback;
        Ok(())
    }
}
