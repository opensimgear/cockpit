use super::values;
#[path = "session/get_result.rs"]
mod get_result;
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, Weak,
    },
    thread,
    time::{Duration, Instant},
};

pub(super) trait Reader {
    fn poll(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>, Value>;
    fn project(&self, bytes: &[u8]) -> Value;
    fn provenance(&self) -> Option<Value> {
        None
    }
    fn get(&mut self, request: &Get, remaining: Duration) -> Result<Value, Value>;
}

pub(super) struct Get {
    pub id: String,
    pub feature: bool,
    pub report_id: u8,
    pub max_bytes: usize,
    bounded: bool,
    expires: Instant,
}
impl Get {
    pub fn parse(req: &Value) -> Result<Self, String> {
        let ms = req["deadlineMs"]
            .as_u64()
            .filter(|n| (1..=2000).contains(n))
            .ok_or("deadlineMs must be 1..2000")?;
        Ok(Self {
            id: super::required(req, "requestId")?.to_owned(),
            feature: match req["kind"].as_str() {
                Some("feature") => true,
                Some("input") => false,
                _ => return Err("GET requires input or feature".into()),
            },
            report_id: req["reportId"]
                .as_u64()
                .and_then(|n| n.try_into().ok())
                .ok_or("Expected HID report ID 0..255 (zero selects unnumbered)")?,
            max_bytes: req["maxBytes"]
                .as_u64()
                .filter(|n| (1..=65536).contains(n))
                .ok_or("Expected maxBytes 1..65536")? as usize,
            expires: Instant::now() + Duration::from_millis(ms),
            bounded: req.get("bounded").map_or(Ok(false), |value| {
                value.as_bool().ok_or("HID bounded must be boolean")
            })?,
        })
    }
}

struct Entry {
    seq: u64,
    received: u128,
    report_id: Option<u8>,
    json: Box<str>,
}
#[derive(Default)]
struct History {
    entries: VecDeque<Entry>,
    bytes: usize,
    sequence: u64,
    terminal: Option<Value>,
    values: values::Current,
    detail: Value,
    recording: Option<Weak<super::recording::Archive>>,
    clock: Option<Instant>,
    get_token: Option<String>,
    last_get: Option<get_result::Retained>,
}
impl History {
    fn end(&mut self, reason: Value) {
        self.get_token = None;
        self.last_get = None;
        self.values.clear();
        if self.terminal.is_none() {
            if let Some(recording) = self.recording.as_ref().and_then(Weak::upgrade) {
                let loss = reason["kind"] == "overflow";
                recording.end(
                    reason.clone(),
                    self.sequence,
                    self.clock.map_or(0, |c| c.elapsed().as_micros()),
                    loss,
                );
            }
            self.terminal = Some(reason);
        }
    }
    fn state(&self) -> Value {
        self.values.state(self.sequence, self.terminal.as_ref())
    }
    fn push(&mut self, micros: u128, raw: &[u8], projection: Value, provenance: Option<Value>) {
        if self.terminal.is_some() {
            return;
        }
        let Some(sequence) = self.sequence.checked_add(1) else {
            self.end(json!({"kind":"overflow","message":"HID report sequence exhausted"}));
            return;
        };
        self.sequence = sequence;
        self.values
            .observe(self.sequence, micros, &projection, provenance.as_ref());
        let mut row = json!({"sequence":self.sequence.to_string(),"receivedUs":micros.to_string(),
            "samples":projection});
        if provenance
            .as_ref()
            .is_none_or(|p| p["wireAvailable"] == true)
        {
            row["wireHex"] = json!(hex(raw));
        }
        #[cfg(windows)]
        if provenance.is_none() {
            row["windowsReportId"] = json!(raw.first());
        }
        #[cfg(target_os = "linux")]
        {
            row["reportId"] = row["samples"]["report_id"].clone();
        }
        if let Some(source) = provenance {
            row["reportId"] = source["reportId"].clone();
            row["provenance"] = source;
        }
        let encoded = row.to_string();
        if let Some(recording) = self.recording.as_ref().and_then(Weak::upgrade) {
            recording.observe_value(self.sequence, micros, &encoded, &row);
        }
        self.bytes += encoded.len();
        self.entries.push_back(Entry {
            seq: self.sequence,
            received: micros,
            report_id: row["reportId"]
                .as_u64()
                .or_else(|| row["windowsReportId"].as_u64())
                .and_then(|id| id.try_into().ok()),
            json: encoded.into_boxed_str(),
        });
        while self.entries.len() > 4096 || self.bytes + self.get_bytes() > 8 * 1024 * 1024 {
            self.bytes -= self.entries.pop_front().unwrap().json.len();
        }
    }
    #[cfg(test)]
    fn page(&self, after: u64) -> Result<Value, String> {
        self.page_limit(after, 128)
    }
    fn page_limit(&self, after: u64, limit: usize) -> Result<Value, String> {
        if after > self.sequence {
            return Err("HID cursor is ahead of this generation".into());
        }
        let oldest = self
            .entries
            .front()
            .map_or(self.sequence.saturating_add(1), |e| e.seq);
        let mut bytes = 0;
        let mut reports = Vec::new();
        for entry in self.entries.iter().filter(|e| e.seq > after).take(limit) {
            let row = if entry.json.len() >= 900 * 1024 {
                json!({"sequence":entry.seq.to_string(),"receivedUs":entry.received.to_string(),
                    "reportId":entry.report_id,"samples":{"unavailable":"Report exceeds page budget; read report slice for exact JSON"},"reportSlice":{
                    "totalBytes":entry.json.len(),"encoding":"json-utf8"}})
            } else {
                serde_json::from_str::<Value>(&entry.json).expect("entry serialized on insertion")
            };
            let size = row.to_string().len() + 1;
            if bytes + size > 900 * 1024 {
                break;
            }
            bytes += size;
            reports.push(row);
        }
        let next = reports
            .last()
            .and_then(|row| row["sequence"].as_str())
            .and_then(|sequence| sequence.parse::<u64>().ok())
            .unwrap_or(after.max(oldest.saturating_sub(1)));
        Ok(
            json!({"reports":reports,"oldest":oldest.to_string(),"latest":self.sequence.to_string(),
            "next":next.to_string(),"hasMore":self.entries.back().is_some_and(|entry|entry.seq>next),
            "gap":after.saturating_add(1)<oldest,"terminal":self.terminal}),
        )
    }
}
pub(super) fn hex(raw: &[u8]) -> String {
    use std::fmt::Write;
    let mut text = String::with_capacity(raw.len() * 2);
    for &b in raw {
        write!(text, "{b:02x}").unwrap();
    }
    text
}
type Response = Result<Value, Value>;
type Command = (Get, mpsc::Sender<Response>);
pub(super) struct Reply {
    rx: mpsc::Receiver<Response>,
    expires: Instant,
    id: String,
    stop: Arc<AtomicBool>,
    history: Arc<Mutex<History>>,
    token: String,
    bounded: bool,
}
impl Reply {
    pub fn wait(self, target: &str, generation: &str) -> Result<Value, String> {
        let response = self
            .rx
            .recv_timeout(self.expires.saturating_duration_since(Instant::now()));
        let result = if self.stop.load(Ordering::Acquire) {
            json!({"error":{"kind":"offline"}})
        } else {
            match response {
                Ok(Ok(v)) => v,
                Ok(Err(e)) => json!({"error":e}),
                Err(mpsc::RecvTimeoutError::Timeout) => json!({"error":{"kind":"timeout"}}),
                Err(mpsc::RecvTimeoutError::Disconnected) => json!({"error":{"kind":"offline"}}),
            }
        };
        let result = self
            .history
            .lock()
            .map_err(|_| "HID history lock unavailable")?
            .retain_get(&self.id, &self.token, result, self.bounded);
        Ok(
            json!({"interfaceId":target,"generation":generation,"requestId":self.id,"result":result}),
        )
    }
}

pub(super) struct Session {
    history: Arc<Mutex<History>>,
    tx: mpsc::SyncSender<Command>,
    stop: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Session {
    #[cfg(test)]
    pub fn start(
        open: impl FnOnce() -> Result<Box<dyn Reader>, String> + Send + 'static,
    ) -> Result<Self, String> {
        Self::start_with_detail(Value::Null, open)
    }
    pub fn start_with_detail(
        detail: Value,
        open: impl FnOnce() -> Result<Box<dyn Reader>, String> + Send + 'static,
    ) -> Result<Self, String> {
        let (ready, started) = mpsc::sync_channel(1);
        let (tx, rx) = mpsc::sync_channel::<Command>(1);
        let clock = Instant::now();
        let history = Arc::new(Mutex::new(History {
            values: values::Current::new(detail.clone()),
            detail,
            clock: Some(clock),
            ..History::default()
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let busy = Arc::new(AtomicBool::new(false));
        let (log, done, pending) = (history.clone(), stop.clone(), busy.clone());
        let worker = thread::Builder::new()
            .name("hid-receiver".into())
            .spawn(move || {
                let mut reader = match open() {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = ready.send(Err(e));
                        return;
                    }
                };
                if ready.send(Ok(())).is_err() {
                    return;
                }
                while !done.load(Ordering::Acquire) {
                    if let Ok((get, reply)) = rx.try_recv() {
                        let remaining = get.expires.saturating_duration_since(Instant::now());
                        let value = if remaining.is_zero() {
                            Err(json!({"kind":"timeout"}))
                        } else {
                            let result = reader.get(&get, remaining);
                            if result.is_ok() && Instant::now() > get.expires {
                                Err(json!({"kind":"timeout"}))
                            } else {
                                result
                            }
                        };
                        let removed = value
                            .as_ref()
                            .err()
                            .filter(|e| {
                                matches!(
                                    e["kind"].as_str(),
                                    Some(
                                        "removed"
                                            | "stale-generation"
                                            | "permission-denied"
                                            | "access-denied"
                                            | "eof"
                                            | "overflow"
                                            | "offline"
                                    )
                                )
                            })
                            .cloned();
                        pending.store(false, Ordering::Release);
                        if let Some(error) = removed {
                            log.lock().unwrap().end(error);
                            done.store(true, Ordering::Release);
                            let _ = reply.send(value);
                            break;
                        }
                        let _ = reply.send(value);
                    }
                    match reader.poll(Duration::from_millis(10)) {
                        Ok(Some(raw)) => {
                            if done.load(Ordering::Acquire) {
                                break;
                            }
                            let at = clock.elapsed().as_micros();
                            let projection = reader.project(&raw);
                            log.lock()
                                .unwrap()
                                .push(at, &raw, projection, reader.provenance());
                        }
                        Ok(None) => {}
                        Err(error) => {
                            log.lock().unwrap().end(error);
                            done.store(true, Ordering::Release);
                            break;
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        started
            .recv()
            .map_err(|_| "HID capture worker stopped during open")??;
        Ok(Self {
            history,
            tx,
            stop,
            busy,
            worker: Some(worker),
        })
    }
    #[cfg(test)]
    pub fn page(&self, after: u64) -> Result<Value, String> {
        self.history.lock().unwrap().page(after)
    }
    pub fn page_limit(&self, after: u64, limit: usize) -> Result<Value, String> {
        self.history.lock().unwrap().page_limit(after, limit)
    }
    pub fn status(&self) -> Value {
        let history = self.history.lock().unwrap();
        json!({"status":if history.terminal.is_some() {"offline"} else {"open"},
            "latest":history.sequence.to_string(),
            "oldest":history.entries.front().map_or(history.sequence.saturating_add(1),|entry|entry.seq).to_string(),
            "retainedRecords":history.entries.len(),"retainedBytes":history.bytes,"terminal":history.terminal})
    }
    pub fn report_slice(&self, request: &Value) -> Result<Value, String> {
        let sequence = super::required(request, "sequence")?
            .parse::<u64>()
            .map_err(|_| "Invalid HID report sequence")?;
        if request["sequence"].as_str().unwrap() != sequence.to_string() {
            return Err(
                "Invalid HID report sequence; expected canonical unsigned decimal string".into(),
            );
        }
        let history = self
            .history
            .lock()
            .map_err(|_| "HID history lock unavailable")?;
        let entry = history.entries.iter().find(|e| e.seq == sequence).ok_or(
            "HID report sequence is not retained; page reports again to recover the cursor",
        )?;
        let mut result = super::inspection::slice(entry.json.as_bytes(), request)?;
        result["sequence"] = json!(sequence.to_string());
        Ok(result)
    }
    pub fn get_slice(&self, request: &Value) -> Result<Value, String> {
        self.history
            .lock()
            .map_err(|_| "HID history lock unavailable")?
            .get_slice(request)
    }
    pub fn state(&self) -> Value {
        self.history.lock().unwrap().state()
    }
    pub fn record_start(
        &self,
        store: &mut super::recording::Store,
        request: &Value,
    ) -> Result<Value, String> {
        let mut history = self
            .history
            .lock()
            .map_err(|_| "HID history lock unavailable")?;
        if history.terminal.is_some() || self.stop.load(Ordering::Acquire) {
            return Err("Offline: HID generation has ended".into());
        }
        let recording = store.start(
            request,
            history.detail.clone(),
            history.sequence,
            history.clock.map_or(0, |c| c.elapsed().as_micros()),
        )?;
        history.recording = Some(Arc::downgrade(&recording));
        store.request("controller_hid_record_status", request)
    }
    pub fn is_terminal(&self) -> bool {
        self.history.lock().unwrap().terminal.is_some()
    }
    pub fn revoke(&self, reason: Value) {
        self.stop.store(true, Ordering::Release);
        self.history.lock().unwrap().end(reason);
    }
    pub fn submit(&self, get: Get) -> Result<Reply, String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Offline: HID generation has ended".into());
        }
        if self.busy.swap(true, Ordering::AcqRel) {
            return Err("Busy: explicit HID read already outstanding".into());
        }
        let (tx, rx) = mpsc::channel();
        let token = uuid::Uuid::new_v4().to_string();
        {
            let mut history = self.history.lock().unwrap();
            history.last_get = None;
            history.get_token = Some(token.clone());
        }
        let result = Reply {
            rx,
            expires: get.expires,
            id: get.id.clone(),
            stop: self.stop.clone(),
            history: self.history.clone(),
            token,
            bounded: get.bounded,
        };
        if self.tx.try_send((get, tx)).is_err() {
            self.busy.store(false, Ordering::Release);
            return Err("Offline: HID generation has ended".into());
        }
        Ok(result)
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.revoke(json!({"kind":"closed"}));
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "session_bounds_tests.rs"]
mod bounds_tests;
