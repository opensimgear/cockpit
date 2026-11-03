use super::{catalogue, parser::parse, Snapshot, FRESHNESS_MS};
use serde_json::Value;
use std::{
    collections::{BTreeMap, VecDeque},
    time::Instant,
};

pub(crate) struct Session {
    producer_id: String,
    fields: BTreeMap<String, String>,
    epoch: u64,
    session: Option<String>,
    retired: VecDeque<String>,
    aircraft: Option<String>,
    plane: Option<u64>,
    model_time: Option<f64>,
    sequence: Option<u32>,
    capture_sequence: u64,
    capture_exhausted: bool,
    captured_at: Option<Instant>,
    received_ms: Option<u64>,
    status: String,
    values: BTreeMap<String, Value>,
    malformed_frames: u64,
    error: Option<String>,
}
impl Session {
    pub(super) fn invalidate(&mut self, message: &str) {
        self.epoch = self.epoch.saturating_add(1);
        self.values.clear();
        self.model_time = None;
        self.status = "error".into();
        self.error = Some(message.to_owned());
    }
    pub(crate) fn new() -> Self {
        Self {
            producer_id: uuid::Uuid::new_v4().to_string(),
            fields: catalogue()
                .iter()
                .map(|v| {
                    (
                        v["id"].as_str().unwrap().to_owned(),
                        v["type"].as_str().unwrap().to_owned(),
                    )
                })
                .collect(),
            epoch: 1,
            session: None,
            retired: VecDeque::new(),
            aircraft: None,
            plane: None,
            model_time: None,
            sequence: None,
            capture_sequence: 0,
            capture_exhausted: false,
            captured_at: None,
            received_ms: None,
            status: "waiting".into(),
            values: BTreeMap::new(),
            malformed_frames: 0,
            error: None,
        }
    }
    pub(crate) fn feed(
        &mut self,
        bytes: &[u8],
        captured_at: Instant,
        capture_ms: u64,
    ) -> Result<bool, String> {
        let packet = match parse(bytes, &self.fields) {
            Ok(value) => value,
            Err(error) => {
                self.values.clear();
                self.status = "error".into();
                self.error = Some(error.clone());
                self.model_time = None;
                self.malformed_frames = self.malformed_frames.saturating_add(1);
                return Err(error);
            }
        };
        if self.retired.contains(&packet.session) {
            return Ok(false);
        }
        if self.capture_exhausted {
            return Err("DCS ownship capture sequence exhausted; source is fail-closed.".into());
        }
        let replacing_session = self.session.as_ref() != Some(&packet.session);
        if !replacing_session
            && self.sequence.is_some_and(|previous| {
                let delta = packet.sequence.wrapping_sub(previous);
                delta == 0 || delta >= 1 << 31
            })
        {
            return Ok(false);
        }
        let Some(capture_sequence) = self.capture_sequence.checked_add(1) else {
            self.capture_exhausted = true;
            self.invalidate("DCS ownship capture sequence exhausted; source is fail-closed.");
            return Err("DCS ownship capture sequence exhausted; source is fail-closed.".into());
        };
        if replacing_session {
            if let Some(previous) = self.session.take() {
                self.retired.push_back(previous);
                if self.retired.len() > 16 {
                    self.retired.pop_front();
                }
            }
            self.session = Some(packet.session.clone());
            self.epoch = self.epoch.saturating_add(1);
            self.model_time = None;
            self.sequence = None;
            self.aircraft = None;
            self.plane = None;
        }
        if self
            .received_ms
            .is_some_and(|at| capture_ms.saturating_sub(at) > FRESHNESS_MS)
        {
            self.invalidate("DCS ownship stream resumed; waiting for an advancing baseline.");
        }
        if self
            .sequence
            .is_some_and(|previous| packet.sequence.wrapping_sub(previous) > 1)
        {
            self.invalidate("DCS ownship sequence gap; waiting for an advancing baseline.");
        }
        let changed = self.aircraft != packet.aircraft || self.plane != packet.plane;
        let reversed = self
            .model_time
            .is_some_and(|previous| packet.model_time < previous);
        let advancing = !changed
            && !reversed
            && self
                .model_time
                .is_some_and(|previous| packet.model_time > previous);
        let frozen = !changed && self.model_time == Some(packet.model_time);
        if changed || reversed {
            self.epoch = self.epoch.saturating_add(1);
        }
        self.sequence = Some(packet.sequence);
        self.capture_sequence = capture_sequence;
        self.captured_at = Some(captured_at);
        self.received_ms = Some(capture_ms);
        self.aircraft = packet.aircraft;
        self.plane = packet.plane;
        self.model_time = Some(packet.model_time);
        self.error = None;
        self.values.clear();
        self.status = if packet.stopped {
            "stopped"
        } else if !packet.allowed {
            "export-denied"
        } else if packet.paused || frozen {
            "paused"
        } else if self.aircraft.is_none()
            || self.plane.is_none()
            || !advancing
            || packet.values.is_empty()
        {
            "waiting"
        } else {
            self.values = packet.values;
            "live"
        }
        .into();
        Ok(true)
    }
    pub(super) fn expire(&mut self) {
        self.values.clear();
        if self.status != "stopped" {
            self.status = "stale".into();
        }
    }
    pub(super) fn captured_at(&self) -> Option<Instant> {
        self.captured_at
    }
    pub(crate) fn snapshot(&self, now_ms: u64) -> Snapshot {
        let age = self.received_ms.map(|at| now_ms.saturating_sub(at));
        let expired = age.is_some_and(|age| age > FRESHNESS_MS);
        let fresh = !expired && self.status == "live";
        Snapshot {
            producer_id: self.producer_id.clone(),
            epoch: self.epoch,
            session: self.session.clone(),
            aircraft: self.aircraft.clone(),
            plane_id: self.plane,
            model_time: self.model_time,
            sequence: self.sequence,
            capture_sequence: self.capture_sequence,
            fresh,
            status: if expired && self.status != "stopped" {
                "stale".into()
            } else {
                self.status.clone()
            },
            age_ms: age,
            captured_at: self.captured_at,
            values: if fresh {
                self.values.clone()
            } else {
                BTreeMap::new()
            },
            malformed_frames: self.malformed_frames,
            error: self.error.clone(),
        }
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
