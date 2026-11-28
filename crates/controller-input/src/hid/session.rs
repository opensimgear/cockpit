use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
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
        })
    }
}

struct Entry {
    seq: u64,
    json: Box<str>,
}
#[derive(Default)]
struct History {
    entries: VecDeque<Entry>,
    bytes: usize,
    sequence: u64,
    terminal: Option<Value>,
}
impl History {
    fn push(&mut self, micros: u128, raw: &[u8], projection: Value, provenance: Option<Value>) {
        self.sequence += 1;
        let mut row = json!({"sequence":self.sequence.to_string(),"receivedUs":micros.to_string(),
            "samples":projection});
        if provenance.as_ref().is_none_or(|p| p["wireAvailable"] == true) {
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
        let mut encoded = row.to_string();
        if encoded.len() > 512 * 1024 {
            row["samples"] =
                json!({"unavailable":"Decoded projection exceeds capture page budget"});
            encoded = row.to_string();
        }
        self.bytes += encoded.len();
        self.entries.push_back(Entry {
            seq: self.sequence,
            json: encoded.into_boxed_str(),
        });
        while self.entries.len() > 4096 || self.bytes > 8 * 1024 * 1024 {
            self.bytes -= self.entries.pop_front().unwrap().json.len();
        }
    }
    fn page(&self, after: u64) -> Result<Value, String> {
        if after > self.sequence {
            return Err("HID cursor is ahead of this generation".into());
        }
        let oldest = self.entries.front().map_or(1, |e| e.seq);
        let mut bytes = 0;
        let reports: Vec<_> = self
            .entries
            .iter()
            .filter(|e| e.seq > after)
            .take(128)
            .take_while(|e| {
                bytes += e.json.len();
                bytes <= 900 * 1024
            })
            .map(|e| serde_json::from_str::<Value>(&e.json).expect("entry serialized on insertion"))
            .collect();
        Ok(
            json!({"reports":reports,"oldest":oldest.to_string(),"latest":self.sequence.to_string(),
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
    pub fn start(
        open: impl FnOnce() -> Result<Box<dyn Reader>, String> + Send + 'static,
    ) -> Result<Self, String> {
        let (ready, started) = mpsc::sync_channel(1);
        let (tx, rx) = mpsc::sync_channel::<Command>(1);
        let history = Arc::new(Mutex::new(History::default()));
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
                let clock = Instant::now();
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
                                    )
                                )
                            })
                            .cloned();
                        pending.store(false, Ordering::Release);
                        if let Some(error) = removed {
                            log.lock().unwrap().terminal = Some(error);
                            let _ = reply.send(value);
                            break;
                        }
                        let _ = reply.send(value);
                    }
                    match reader.poll(Duration::from_millis(10)) {
                        Ok(Some(raw)) => {
                            let at = clock.elapsed().as_micros();
                            let projection = reader.project(&raw);
                            log.lock().unwrap().push(at, &raw, projection, reader.provenance());
                        }
                        Ok(None) => {}
                        Err(error) => {
                            log.lock().unwrap().terminal = Some(error);
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
    pub fn page(&self, after: u64) -> Result<Value, String> {
        self.history.lock().unwrap().page(after)
    }
    pub fn submit(&self, get: Get) -> Result<Reply, String> {
        if self.busy.swap(true, Ordering::AcqRel) {
            return Err("Busy: explicit HID read already outstanding".into());
        }
        let (tx, rx) = mpsc::channel();
        let result = Reply {
            rx,
            expires: get.expires,
            id: get.id.clone(),
            stop: self.stop.clone(),
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
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
