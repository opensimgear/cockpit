//! Bounded generation-local archives, fed before the lossy display history.
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    ops::Deref,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard, TryLockError,
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[path = "recording/bounds.rs"]
mod bounds;
#[path = "recording/export.rs"]
mod export;
#[cfg(test)]
#[path = "recording/tests.rs"]
mod tests;

const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_RECORDS: usize = 1_000_000;
const RESERVE: usize = 8192;
const MAX_ARCHIVES: usize = 4;
pub(super) type Handle = Arc<Archive>;

pub(super) struct Archive {
    owner: Option<String>,
    live: Arc<AtomicBool>,
    inner: Mutex<Recording>,
}
impl Deref for Archive {
    type Target = Mutex<Recording>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl Archive {
    // A stopped archive may be exporting a large snapshot. Never wait for that
    // mutex on the capture thread, including when stop races this acquisition.
    fn active_lock(&self) -> Option<MutexGuard<'_, Recording>> {
        loop {
            if !self.live.load(Ordering::Acquire) {
                return None;
            }
            match self.inner.try_lock() {
                Ok(guard) => return Some(guard),
                Err(TryLockError::WouldBlock) => std::thread::yield_now(),
                Err(TryLockError::Poisoned(_)) => return None,
            }
        }
    }
    pub fn observe_value(&self, sequence: u64, micros: u128, encoded: &str, row: &Value) {
        if let Some(mut recording) = self.active_lock() {
            recording.observe_value(sequence, micros, encoded, row);
        }
    }
    #[cfg(test)]
    pub fn observe(&self, sequence: u64, micros: u128, encoded: &str) {
        let row = serde_json::from_str(encoded).unwrap();
        self.observe_value(sequence, micros, encoded, &row);
    }
    pub fn end(&self, reason: Value, sequence: u64, micros: u128, loss: bool) {
        if let Some(mut recording) = self.active_lock() {
            recording.end(reason, sequence, micros, loss);
        }
    }
}

#[derive(Default)]
pub(super) struct Store {
    recordings: HashMap<(String, String), Handle>,
    budget: Arc<Mutex<usize>>,
}

pub(super) struct Recording {
    metadata: Value,
    records: Vec<Box<str>>,
    positions: Vec<usize>,
    sealed: Option<(String, String)>,
    row_bytes: usize,
    initial_metadata_bytes: usize,
    initial_sequence_digits: usize,
    charged: usize,
    max_bytes: usize,
    max_records: usize,
    budget: Arc<Mutex<usize>>,
    clock: Instant,
    started_micros: u128,
    live: Arc<AtomicBool>,
}

impl Store {
    pub fn start(
        &mut self,
        req: &Value,
        detail: Value,
        sequence: u64,
        micros: u128,
    ) -> Result<Handle, String> {
        let id = super::required(req, "interfaceId")?.to_owned();
        let generation = super::required(req, "generation")?.to_owned();
        if !bounds::descriptor(&detail) {
            return Err("Recording descriptor exceeds replay decode limits (2 MiB, depth 48); inspect a smaller supported contract".into());
        }
        if self
            .recordings
            .contains_key(&(id.clone(), generation.clone()))
        {
            return Err(
                "Recording already exists; stop and discard it before starting again".into(),
            );
        }
        if self.recordings.len() >= MAX_ARCHIVES {
            return Err("Recording capacity reached; discard a stopped recording".into());
        }
        let limit = |name: &str, minimum: usize, maximum: usize| -> Result<usize, String> {
            req.get(name).map_or(Ok(maximum), |v| {
                v.as_u64()
                    .and_then(|v| usize::try_from(v).ok())
                    .filter(|v| (minimum..=maximum).contains(v))
                    .ok_or_else(|| format!("{name} must be {minimum}..{maximum}"))
            })
        };
        let mut recording = Recording {
            metadata: json!({"format":"osg-hid-recording","descriptor":detail,"interfaceId":id,
                "generation":generation,"startedUnixMs":SystemTime::now().duration_since(UNIX_EPOCH)
                    .unwrap_or_default().as_millis().to_string(),"startedReceivedUs":micros.to_string(),
                "startingSequence":sequence.to_string(),"lastSequence":sequence.to_string(),
                "status":"recording","reason":null,"recordCount":0,"bytes":0,"records":[]}),
            records: Vec::new(),
            positions: Vec::new(),
            sealed: None,
            row_bytes: 0,
            initial_metadata_bytes: 0,
            initial_sequence_digits: sequence.to_string().len(),
            charged: 0,
            max_bytes: limit("maxBytes", RESERVE * 2, MAX_BYTES)?,
            max_records: limit("maxRecords", 3, MAX_RECORDS)?,
            budget: self.budget.clone(),
            clock: Instant::now(),
            started_micros: micros,
            live: Arc::new(AtomicBool::new(true)),
        };
        recording.initial_metadata_bytes = recording.metadata.to_string().len();
        let initial = recording.initial_metadata_bytes + RESERVE + bounds::WRAPPER_BYTES;
        if initial > recording.max_bytes || !recording.charge(initial) {
            return Err(
                "Recording metadata or global memory capacity reached; discard a stopped recording"
                    .into(),
            );
        }
        let handle = Arc::new(Archive {
            owner: req["clientId"].as_str().map(str::to_owned),
            live: recording.live.clone(),
            inner: Mutex::new(recording),
        });
        self.recordings.insert((id, generation), handle.clone());
        Ok(handle)
    }

    pub(super) fn release_client(&self, client: &str) {
        for archive in self.recordings.values().filter(|a| a.owner.as_deref()==Some(client)) {
            if let Some(mut recording) = archive.active_lock() {
                let seq = recording.last_sequence();
                let at = recording.started_micros + recording.clock.elapsed().as_micros();
                recording.end(json!({"kind":"authorization-revoked"}), seq, at, false);
            }
        }
    }

    pub fn request(&mut self, command: &str, req: &Value) -> Result<Value, String> {
        if command == "controller_hid_record_list" {
            let mut recordings = Vec::with_capacity(self.recordings.len());
            for ((id, generation), handle) in &self.recordings {
                let recording = handle
                    .lock()
                    .map_err(|_| "HID recording lock unavailable")?;
                let mut status = recording.status();
                status["interfaceId"] = json!(id);
                status["generation"] = json!(generation);
                recordings.push(status);
            }
            recordings.sort_by(|a, b| {
                a["interfaceId"]
                    .as_str()
                    .cmp(&b["interfaceId"].as_str())
                    .then_with(|| a["generation"].as_str().cmp(&b["generation"].as_str()))
            });
            return Ok(json!({"recordings":recordings}));
        }
        let key = (
            super::required(req, "interfaceId")?.to_owned(),
            super::required(req, "generation")?.to_owned(),
        );
        let handle = self
            .recordings
            .get(&key)
            .ok_or("No recording retained for this HID generation")?
            .clone();
        let mut recording = handle
            .lock()
            .map_err(|_| "HID recording lock unavailable")?;
        let result = match command {
            "controller_hid_record_status" => recording.status(),
            "controller_hid_record_stop" => {
                let seq = recording.last_sequence();
                let at = recording.started_micros + recording.clock.elapsed().as_micros();
                recording.end(json!({"kind":"stopped"}), seq, at, false);
                recording.status()
            }
            "controller_hid_record_export" => recording.export(req)?,
            "controller_hid_record_discard" => {
                if recording.active() {
                    return Err("Stop recording before discarding it".into());
                }
                self.recordings.remove(&key);
                json!({"discarded":true})
            }
            _ => return Err("Unknown HID recording command".into()),
        };
        Ok(json!({"interfaceId":key.0,"generation":key.1,
            "contractFingerprint":recording.metadata["descriptor"]["contractFingerprint"],"result":result}))
    }
}

impl Recording {
    fn active(&self) -> bool {
        self.metadata["status"] == "recording"
    }
    fn last_sequence(&self) -> u64 {
        self.metadata["lastSequence"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    }
    fn charge(&mut self, bytes: usize) -> bool {
        let mut total = self.budget.lock().unwrap();
        if total.saturating_add(bytes) > MAX_BYTES {
            return false;
        }
        *total += bytes;
        self.charged += bytes;
        true
    }
    fn append(&mut self, row: String) {
        self.positions.push(self.row_bytes + self.records.len());
        self.row_bytes += row.len();
        self.records.push(row.into_boxed_str());
    }
    #[cfg(test)]
    pub fn observe(&mut self, sequence: u64, micros: u128, encoded: &str) {
        let row = serde_json::from_str(encoded).unwrap();
        self.observe_value(sequence, micros, encoded, &row);
    }
    fn observe_value(&mut self, sequence: u64, micros: u128, encoded: &str, row: &Value) {
        if !self.active() {
            return;
        }
        if encoded.len() > bounds::VALUE_BYTES {
            self.end(json!({"kind":"oversized-row","message":"Report exceeds the 2 MiB replay decode limit; recording stopped before this report; raw capture remains available"}), sequence, micros, true);
            return;
        }
        let cost = encoded.len() + 1;
        if self.records.len() + 3 > self.max_records
            || self.charged.saturating_add(cost) > self.max_bytes
            || !self.charge(cost)
        {
            self.end(json!({"kind":"capacity","message":"Recording limit reached; export or discard the retained prefix"}), sequence, micros, true);
            return;
        }
        if !bounds::row(row) {
            self.end(json!({"kind":"decode-budget","message":"Report exceeds replay structure or sample limits; recording stopped before this report; raw capture remains available"}), sequence, micros, true);
            return;
        }
        self.metadata["lastSequence"] = json!(sequence.to_string());
        self.append(encoded.to_owned());
    }
    pub fn end(&mut self, reason: Value, sequence: u64, micros: u128, gap: bool) {
        if !self.active() {
            return;
        }
        let reason = if bounds::reason(&reason) {
            reason
        } else {
            json!({"kind":"terminal","message":"Native terminal detail exceeded recording marker budget"})
        };
        if gap {
            self.append(json!({"kind":"gap","fromSequence":(self.last_sequence()+1).to_string(),
                "toSequence":if sequence > self.last_sequence() {json!(sequence.to_string())} else {Value::Null},
                "receivedUs":micros.to_string(),"reason":reason}).to_string());
        }
        self.append(json!({"kind":"terminal","sequence":sequence.to_string(),"receivedUs":micros.to_string(),"reason":reason}).to_string());
        self.metadata["lastSequence"] = json!(sequence.to_string());
        self.metadata["status"] = json!("stopped");
        self.metadata["reason"] = reason;
        self.live.store(false, Ordering::Release);
        self.sealed = Some(self.make_parts());
    }
    fn status(&self) -> Value {
        json!({"status":self.metadata["status"],"reason":self.metadata["reason"],
            "lastSequence":self.metadata["lastSequence"],"recordCount":self.records.len(),"bytes":self.encoded_size()})
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        *self.budget.lock().unwrap() -= self.charged;
    }
}
