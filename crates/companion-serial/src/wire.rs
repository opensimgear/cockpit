use super::*;
use osg_companion_service::{AssignmentSubscription, FeedbackSubscription};

pub(super) struct Wire {
    pub(super) pairing: pairing::Pairing,
    pub(super) wake: Arc<NativeWake>,
    pub(super) link: String,
    pub(super) connection: String,
    pub(super) sequence: u64,
    feedback: Option<FeedbackSubscription>,
    assignments: Option<AssignmentSubscription>,
    feedback_observed: Option<u64>,
    feedback_frames: std::collections::VecDeque<Value>,
}
impl Wire {
    pub(super) fn new(wake: Arc<NativeWake>) -> Self {
        Self {
            pairing: Default::default(),
            wake,
            link: String::new(),
            connection: uuid::Uuid::new_v4().to_string(),
            sequence: 0,
            feedback: None,
            assignments: None,
            feedback_observed: None,
            feedback_frames: Default::default(),
        }
    }
    pub(super) fn reset(&mut self, hub: &Hub, link: &str) {
        hub.disconnect_connection(&self.connection, hub.now_ms());
        self.connection = uuid::Uuid::new_v4().to_string();
        self.link = link.into();
        self.sequence = 0;
        self.clear_feedback();
    }
    pub(super) fn clear_feedback(&mut self) {
        self.pairing.clear();
        self.feedback = None;
        self.assignments = None;
        self.feedback_observed = None;
        self.feedback_frames.clear();
    }
    pub(super) fn take_feedback(&mut self) -> Result<Option<Value>, String> {
        if let Some(frame) = self.pairing.take() {
            return Ok(Some(json!({"command":"serial-feedback","linkId":self.link,"frame":frame})));
        }
        if self.feedback_frames.is_empty() {
            let observed = self.assignments.as_ref().map(AssignmentSubscription::observed)
                .or_else(|| self.feedback.as_ref().map(FeedbackSubscription::observed));
            if observed != self.feedback_observed {
                self.feedback_observed = observed;
                let update = if let Some(reader) = self.assignments.as_mut() {
                    reader.take().map(|rows| rows.map(|rows| json!({"ok":true,"type":"assignments-event","assignments":rows})))
                } else if let Some(reader) = self.feedback.as_mut() {
                    if let Some(rows) = reader.take_assignments() {
                        let event = json!({"ok":true,"type":"assignments-event","assignments":rows});
                        self.feedback_frames = reply_frames(&event)?.into();
                    }
                    reader.take().map(|value| value.map(|value| json!({"ok":true,"type":"feedback-event","feedback":value})))
                } else {
                    Ok(None)
                };
                match update {
                    Ok(Some(event)) => self.feedback_frames.extend(reply_frames(&event)?),
                    Ok(None) => {}
                    Err(_) => { self.feedback = None; self.assignments = None; }
                }
            }
        }
        Ok(self.feedback_frames.pop_front().map(|frame| json!({"command":"serial-feedback","linkId":self.link,"frame":frame})))
    }
    pub(super) fn handle(&mut self, hub: &Hub, v: &Value) -> Result<Vec<Value>, String> {
        let link = v["linkId"].as_str().ok_or("Missing link identity.")?;
        if link != self.link || link.is_empty() {
            return Err("Stale USB link.".into());
        }
        let id = v["requestId"].as_str().ok_or("Missing request identity.")?;
        let seq = id.parse::<u64>().map_err(|_| "Invalid request sequence.")?;
        if seq != self.sequence + 1 || seq == u64::MAX || seq.to_string() != id {
            return Err("Out of order USB request.".into());
        }
        self.sequence = seq;
        let frame = v["frame"].clone();
        if !frame.is_object() {
            return Err("Invalid USB frame.".into());
        }
        let ends_feedback = matches!(frame["type"].as_str(), Some("auth" | "open" | "close" | "cancel"))
            || frame["panelError"] == true;
        let start_assignments = v["pushAssignments"] == true && frame["type"] == "auth";
        let start_feedback = v["pushFeedback"] == true && frame["panelError"] != true
            && matches!(frame["type"].as_str(), Some("baseline" | "heartbeat"));
        let session = start_feedback.then(|| frame["sessionId"].clone());
        if matches!(frame["type"].as_str(), Some("pair" | "auth" | "close" | "cancel")) {
            self.pairing.clear();
        }
        let mut response = if frame["type"] == "pair-subscribe" {
            self.pairing.subscribe(hub, &frame, &self.connection, &self.wake)
        } else {
            hub.handle_serial_wire(frame, &self.connection, hub.now_ms())
        };
        if ends_feedback && response["ok"] == true { self.clear_feedback(); }
        if start_assignments && response["ok"] == true
            && response["assignments"].as_array().is_some_and(|rows| rows.len() != 1) {
            self.assignments = Some(hub.subscribe_serial_assignments(&self.connection, self.wake.clone())?);
            response["assignmentsPush"] = json!(true);
        }
        if let Some(session) = session.as_ref().and_then(Value::as_str).filter(|_| response["ok"] == true && self.feedback.is_none()) {
            self.clear_feedback();
            self.feedback = Some(hub.subscribe_serial_feedback(&self.connection, session, self.wake.clone())?);
            self.feedback_observed = None;
            response["feedbackPush"] = json!(true);
        }
        Ok(reply_frames(&response)?.into_iter().map(|frame|json!({"command":"serial-reply","linkId":link,"requestId":id,"frame":frame})).collect())
    }
}
