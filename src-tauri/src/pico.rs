//! Pico gateway service. One worker owns CDC; HID naming runs on a separate worker.
use crate::{
    protocol::{identifier, validate_state, REQUEST_LIMIT, RESPONSE_LIMIT},
    transport::{Connection, Factory, NativeFactory, PortInfo},
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

const CLOSED: &str = "Cockpit native gateway service has closed.";
const NO_CONNECTION: &str = "Connect the flashed Pico gateway first.";
const COMMAND_TIMEOUT: &str =
    "Pico did not respond. Its connection and setup sessions were closed; reconnect the board.";

#[derive(Clone)]
struct Timing {
    command: Duration,
    hello: Duration,
    reconnect: Duration,
    reconnect_delay: Duration,
    reconnect_poll: Duration,
    read_poll: Duration,
    heartbeat: Duration,
    ui_timeout: Duration,
    claim_timeout: Duration,
    naming_retry: Duration,
}
impl Default for Timing {
    fn default() -> Self {
        Self {
            command: Duration::from_secs(3),
            hello: Duration::from_secs(3),
            reconnect: Duration::from_secs(10),
            reconnect_delay: Duration::from_millis(400),
            reconnect_poll: Duration::from_millis(250),
            read_poll: Duration::from_millis(50),
            heartbeat: Duration::from_secs(2),
            ui_timeout: Duration::from_secs(5),
            claim_timeout: Duration::from_secs(15),
            naming_retry: Duration::from_secs(5),
        }
    }
}

enum Message {
    Request(Value, mpsc::Sender<Value>),
    Shutdown,
}

struct Snapshot {
    state: Value,
    owners: HashMap<String, String>,
}
fn empty_state() -> Value {
    json!({"backend":"pico","protocolVersion":1,"connected":false,"running":false,
        "needsReconnect":false,"reconnecting":false,"modules":[],"port":null,"ports":[]})
}

/// All hardware access is explicit. Construction and port enumeration do not open CDC.
pub struct PicoService {
    sender: mpsc::SyncSender<Message>,
    closed: Arc<AtomicBool>,
    queued: Arc<AtomicUsize>,
    snapshot: Arc<Mutex<Snapshot>>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}
impl Default for PicoService {
    fn default() -> Self {
        Self::new()
    }
}
impl PicoService {
    pub fn new() -> Self {
        Self::with_factory(Box::new(NativeFactory), Timing::default(), true)
    }

    fn with_factory(factory: Box<dyn Factory>, timing: Timing, naming: bool) -> Self {
        // Permit normal status/input/control overlaps while bounding renderer backlog.
        let (sender, receiver) = mpsc::sync_channel(8);
        let closed = Arc::new(AtomicBool::new(false));
        let queued = Arc::new(AtomicUsize::new(0));
        let cancel = closed.clone();
        let pending = queued.clone();
        let snapshot = Arc::new(Mutex::new(Snapshot {
            state: empty_state(),
            owners: HashMap::new(),
        }));
        let latest = snapshot.clone();
        let worker = thread::Builder::new()
            .name("osg-pico-cdc".into())
            .spawn(move || {
                let mut gateway = Gateway::new(factory, timing, cancel.clone(), naming);
                let mut next_heartbeat = Instant::now() + gateway.timing.heartbeat;
                let mut ui_seen = Instant::now();
                while !cancel.load(Ordering::Acquire) {
                    let wait = next_heartbeat.saturating_duration_since(Instant::now());
                    match receiver.recv_timeout(wait) {
                        Ok(Message::Request(request, response)) => {
                            let result = gateway.request(request);
                            gateway.cache(&latest);
                            pending.fetch_sub(1, Ordering::AcqRel);
                            ui_seen = Instant::now();
                            let _ = response.send(result);
                        }
                        Ok(Message::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            next_heartbeat = Instant::now() + gateway.timing.heartbeat;
                            if pending.load(Ordering::Acquire) == 0 {
                                gateway.heartbeat(ui_seen);
                                gateway.cache(&latest);
                            }
                        }
                    }
                }
                gateway.shutdown();
                gateway.cache(&latest);
                while let Ok(Message::Request(_, response)) = receiver.try_recv() {
                    pending.fetch_sub(1, Ordering::AcqRel);
                    let _ = response
                        .send(json!({"ok":false,"error":CLOSED,"state":gateway.decorate(None)}));
                }
            })
            .expect("could not start the Pico CDC worker");
        Self {
            sender,
            closed,
            queued,
            snapshot,
            worker: Mutex::new(Some(worker)),
        }
    }

    /// Invoke from Tauri spawn_blocking; no CDC operation runs on the UI thread.
    pub fn request(&self, request: Value) -> Value {
        let client = request["clientId"].as_str().map(str::to_owned);
        if self.closed.load(Ordering::Acquire) {
            return self.error(CLOSED, client.as_deref());
        }
        let (sender, receiver) = mpsc::channel();
        self.queued.fetch_add(1, Ordering::AcqRel);
        if let Err(error) = self.sender.try_send(Message::Request(request, sender)) {
            self.queued.fetch_sub(1, Ordering::AcqRel);
            let message = match error {
                mpsc::TrySendError::Full(_) => {
                    "The Pico gateway is busy. Wait for the current operation to finish."
                }
                mpsc::TrySendError::Disconnected(_) => CLOSED,
            };
            return self.error(message, client.as_deref());
        }
        // Each exchange and reconnect has a deadline; shutdown interrupts reads.
        receiver
            .recv()
            .unwrap_or_else(|_| self.error(CLOSED, client.as_deref()))
    }

    fn error(&self, error: &str, client: Option<&str>) -> Value {
        let snapshot = self
            .snapshot
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut state = snapshot.state.clone();
        let mut claims: Vec<_> = snapshot
            .owners
            .iter()
            .filter(|(_, owner)| client == Some(owner.as_str()))
            .map(|(id, _)| id.clone())
            .collect();
        claims.sort();
        state["claimedModules"] = json!(claims);
        json!({"ok":false,"error":error,"state":state})
    }

    pub fn shutdown(&self) {
        self.closed.store(true, Ordering::Release);
        let _ = self.sender.try_send(Message::Shutdown);
        if let Some(worker) = self
            .worker
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
        {
            let _ = worker.join();
        }
    }
}
impl Drop for PicoService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Claim {
    client: String,
    seen: Instant,
}
struct Board {
    info: PortInfo,
    serial: String,
}

struct Naming {
    enabled: bool,
    epoch: u64,
    signature: Option<String>,
    attempted: Option<Instant>,
    task: Option<mpsc::Receiver<(u64, Value)>>,
    task_cancel: Option<Arc<AtomicBool>>,
    state: Option<Value>,
    retry: Duration,
}
impl Naming {
    fn new(enabled: bool, retry: Duration) -> Self {
        Self {
            enabled,
            epoch: 0,
            signature: None,
            attempted: None,
            task: None,
            task_cancel: None,
            state: None,
            retry,
        }
    }
    fn clear(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        if let Some(cancel) = &self.task_cancel {
            cancel.store(true, Ordering::Release);
        }
        self.signature = None;
        self.attempted = None;
        self.state = None;
    }
    fn harvest(&mut self) {
        if let Some(task) = &self.task {
            match task.try_recv() {
                Ok((epoch, result)) => {
                    if epoch == self.epoch {
                        let supported = result["supported"] != false;
                        let applied = result["ok"] == true;
                        let mut state = json!({"supported":supported,"applied":applied});
                        if supported && !applied {
                            state["error"] = result.get("error").cloned().unwrap_or_else(|| {
                                json!("macOS could not apply all HID controller names.")
                            });
                        }
                        self.state = Some(state);
                    }
                    self.task = None;
                    self.task_cancel = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.task = None;
                    self.task_cancel = None;
                    self.state = Some(
                        json!({"supported":cfg!(target_os="macos"),"applied":false,"error":"macOS HID naming failed."}),
                    );
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
    }
    fn refresh(&mut self, state: &Value, force: bool) {
        if force {
            self.clear();
        }
        self.harvest();
        if !self.enabled || state["connected"] != true || state["reconnecting"] == true {
            return;
        }
        let modules: Vec<Value> = state["modules"].as_array().into_iter().flatten()
            .filter(|module| module["registered"] == true)
            .map(|module| json!({"id":module["id"],"hidInterface":module["hidInterface"],"label":module["settings"]["label"]})).collect();
        let payload = json!({"gatewaySerial":state["gatewaySerial"],"modules":modules});
        let signature = payload.to_string();
        if self
            .signature
            .as_ref()
            .is_some_and(|previous| previous != &signature)
        {
            self.clear();
        }
        // Cancel changed inventories immediately, but allow only one registry worker at a time.
        if self.task.is_some() {
            self.signature = Some(signature);
            return;
        }
        if self.signature.as_ref() == Some(&signature)
            && (self
                .state
                .as_ref()
                .is_some_and(|state| state["applied"] == true || state["supported"] == false)
                || self
                    .attempted
                    .is_some_and(|attempt| attempt.elapsed() < self.retry))
        {
            return;
        }
        self.signature = Some(signature);
        self.attempted = Some(Instant::now());
        self.state = Some(json!({"supported":cfg!(target_os="macos"),"applied":false}));
        let epoch = self.epoch;
        let cancel = Arc::new(AtomicBool::new(false));
        self.task_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.task = Some(receiver);
        thread::spawn(move || {
            let result = std::panic::catch_unwind(|| crate::hid_names::apply_cancellable(&payload, &cancel))
                .unwrap_or_else(|_| json!({"supported":cfg!(target_os="macos"),"ok":false,"error":"macOS HID naming failed."}));
            let _ = sender.send((epoch, result));
        });
    }
}

struct Gateway {
    factory: Box<dyn Factory>,
    connection: Option<Box<dyn Connection>>,
    board: Option<Board>,
    state: Value,
    buffer: Vec<u8>,
    claims: HashMap<String, Claim>,
    timing: Timing,
    cancel: Arc<AtomicBool>,
    naming: Naming,
}
impl Gateway {
    fn new(
        factory: Box<dyn Factory>,
        timing: Timing,
        cancel: Arc<AtomicBool>,
        naming: bool,
    ) -> Self {
        let manager = Naming::new(naming, timing.naming_retry);
        Self {
            factory,
            connection: None,
            board: None,
            state: empty_state(),
            buffer: Vec::new(),
            claims: HashMap::new(),
            timing,
            cancel,
            naming: manager,
        }
    }

    fn ports(&mut self) -> Result<Vec<PortInfo>, String> {
        let ports = self
            .factory
            .ports()?
            .into_iter()
            .filter(|port| crate::transport::allowed_path(&port.path))
            .collect::<Vec<_>>();
        self.state["ports"] = json!(ports.iter().map(PortInfo::value).collect::<Vec<_>>());
        Ok(ports)
    }

    fn close_port(&mut self) {
        self.naming.clear();
        if let Some(mut connection) = self.connection.take() {
            // Firmware neutralizes immediately on DTR loss, including command timeout.
            let _ = connection.set_control_lines(false, false);
        }
        self.buffer.clear();
        self.claims.clear();
        self.state["connected"] = json!(false);
        self.state["running"] = json!(false);
        self.state["reconnecting"] = json!(false);
        self.state["port"] = Value::Null;
        if let Some(modules) = self.state["modules"].as_array_mut() {
            for module in modules {
                module["online"] = json!(false);
                module["active"] = json!(false);
                module["buttons"] = json!([]);
                module["axes"] = json!(vec![0; module["axisCount"].as_u64().unwrap_or(0) as usize]);
            }
        }
    }

    fn accept(&mut self, mut state: Value) -> Result<(), String> {
        validate_state(
            &state,
            self.board.as_ref().map(|board| board.serial.as_str()),
        )?;
        state["ports"] = self.state["ports"].clone();
        state["connected"] = json!(true);
        state["port"] = self.state["port"].clone();
        self.state = state;
        Ok(())
    }

    fn send(&mut self, mut request: Value, timeout: Duration) -> Result<Value, String> {
        if self.connection.is_none() {
            return Err(NO_CONNECTION.into());
        }
        let id = Uuid::new_v4().to_string();
        request
            .as_object_mut()
            .ok_or("Invalid Pico gateway request.")?
            .remove("clientId");
        request["id"] = json!(id);
        let mut wire = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
        wire.push(b'\n');
        if wire.len() > REQUEST_LIMIT {
            return Err("Pico request exceeds the firmware’s 4095-byte serial frame limit.".into());
        }
        let deadline = Instant::now() + timeout;
        let result = (|| {
            let connection = self.connection.as_mut().unwrap();
            connection
                .write_all(&wire)
                .map_err(|error| format!("Pico serial write failed: {error}"))?;
            connection
                .flush()
                .map_err(|error| format!("Pico serial write failed: {error}"))?;
            loop {
                if self.cancel.load(Ordering::Acquire) {
                    return Err(CLOSED.into());
                }
                if Instant::now() >= deadline {
                    return Err(COMMAND_TIMEOUT.into());
                }
                while let Some(newline) = self.buffer.iter().position(|byte| *byte == b'\n') {
                    let line = self.buffer.drain(..=newline).collect::<Vec<_>>();
                    if line.iter().all(u8::is_ascii_whitespace) {
                        continue;
                    }
                    let response: Value = serde_json::from_slice(&line).map_err(|_| {
                        "Pico sent malformed serial data. Reconnect the board.".to_owned()
                    })?;
                    // Stale and unrelated IDs must never update state or satisfy a request.
                    if response["id"].as_str() == Some(id.as_str()) {
                        return Ok(response);
                    }
                }
                let mut chunk = [0u8; 4_096];
                match connection.read(&mut chunk) {
                    Ok(0) => return Err("Pico was unplugged. Connect it again to resume.".into()),
                    Ok(count) => {
                        if self.buffer.len() + count > RESPONSE_LIMIT {
                            return Err("Pico sent an oversized serial response.".into());
                        }
                        self.buffer.extend_from_slice(&chunk[..count]);
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::TimedOut
                                | io::ErrorKind::WouldBlock
                                | io::ErrorKind::Interrupted
                        ) => {}
                    Err(error) => return Err(format!("Pico serial connection failed: {error}")),
                }
            }
        })();
        if result.is_err() {
            self.close_port();
        }
        result
    }

    fn exchange(&mut self, request: Value, timeout: Duration) -> Result<Value, String> {
        let result = self.send(request, timeout)?;
        let accepted = (|| {
            if !result["ok"].is_boolean() || (result["ok"] == false && !result["error"].is_string())
            {
                return Err("Pico sent an invalid gateway response.".into());
            }
            if let Some(state) = result.get("state") {
                self.accept(state.clone())?;
            } else if result["ok"] == true {
                return Err("Pico response did not include its gateway state.".into());
            }
            Ok(result)
        })();
        if accepted.is_err() {
            self.close_port();
        }
        accepted
    }

    fn open(
        &mut self,
        info: PortInfo,
        expected: Option<String>,
        deadline: Option<Instant>,
    ) -> Result<(), String> {
        if self.cancel.load(Ordering::Acquire) {
            return Err(CLOSED.into());
        }
        let result = (|| {
            self.connection = Some(self.factory.open(&info, self.timing.read_poll)?);
            self.connection
                .as_mut()
                .unwrap()
                .set_control_lines(true, false)
                .map_err(|error| format!("Setting Pico serial control lines failed: {error}"))?;
            self.buffer.clear();
            self.state["port"] = json!(info.path);
            self.board = expected.map(|serial| Board {
                info: info.clone(),
                serial,
            });
            let timeout = deadline
                .map(|deadline| {
                    deadline
                        .saturating_duration_since(Instant::now())
                        .min(self.timing.hello)
                })
                .unwrap_or(self.timing.hello);
            let hello = self.exchange(json!({"command":"hello"}), timeout)?;
            if hello["ok"] != true {
                return Err(hello["error"]
                    .as_str()
                    .unwrap_or("Pico refused the gateway handshake.")
                    .to_owned());
            }
            self.board = Some(Board {
                info,
                serial: self.state["gatewaySerial"].as_str().unwrap().into(),
            });
            Ok(())
        })();
        if result.is_err() {
            self.close_port();
        }
        result
    }

    fn disconnect(&mut self) {
        if self.connection.is_some() {
            let _ = self.exchange(json!({"command":"stop"}), self.timing.command);
        }
        self.close_port();
    }

    fn sleep(&self, duration: Duration) -> Result<(), String> {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            if self.cancel.load(Ordering::Acquire) {
                return Err(CLOSED.into());
            }
            thread::sleep(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(self.timing.read_poll),
            );
        }
        Ok(())
    }

    fn reconnect(&mut self) -> Result<Value, String> {
        let identity = self.board.as_ref().ok_or(NO_CONNECTION)?;
        let old_info = identity.info.clone();
        let expected = identity.serial.clone();
        let ack = self.exchange(json!({"command":"reconnect"}), self.timing.command)?;
        if ack["ok"] != true {
            return Ok(ack);
        }
        if self.state["reconnecting"] != true
            || self.state["running"] != false
            || self.state["modules"]
                .as_array()
                .unwrap()
                .iter()
                .any(|module| module["active"] == true)
        {
            self.close_port();
            return Err(
                "Pico did not acknowledge stopped controller reports before USB re-enumeration."
                    .into(),
            );
        }
        self.claims.clear();
        self.sleep(self.timing.reconnect_delay)?;
        self.close_port();
        let deadline = Instant::now() + self.timing.reconnect;
        let mut last_error = None;
        while Instant::now() < deadline && !self.cancel.load(Ordering::Acquire) {
            let available = self.ports()?;
            let candidate = available.into_iter().find(|port| {
                if let Some(serial) = &old_info.serial_number {
                    port.serial_number.as_ref() == Some(serial)
                        && old_info
                            .vendor_id
                            .as_ref()
                            .is_none_or(|vendor| port.vendor_id.as_ref() == Some(vendor))
                } else {
                    port.path == old_info.path
                }
            });
            if let Some(candidate) = candidate {
                match self.open(candidate, Some(expected.clone()), Some(deadline)) {
                    Ok(()) => {
                        return self.exchange(
                            json!({"command":"status"}),
                            deadline
                                .saturating_duration_since(Instant::now())
                                .min(self.timing.command),
                        )
                    }
                    Err(error) => last_error = Some(error),
                }
            }
            self.sleep(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(self.timing.reconnect_poll),
            )?;
        }
        self.close_port();
        Err(format!(
            "Pico did not return after changing its USB layout. Connect the board again.{}",
            last_error
                .map(|error| format!(" {error}"))
                .unwrap_or_default()
        ))
    }

    fn expire_claims(&mut self, client: Option<&str>) {
        let timeout = self.timing.claim_timeout;
        self.claims.retain(|_, claim| {
            if claim.seen.elapsed() > timeout {
                return false;
            }
            if client == Some(claim.client.as_str()) {
                claim.seen = Instant::now();
            }
            true
        });
    }

    fn decorate(&mut self, client: Option<&str>) -> Value {
        self.naming.harvest();
        let mut state = self.state.clone();
        if let Some(naming) = &self.naming.state {
            state["hidNaming"] = naming.clone();
        }
        let mut claimed: Vec<_> = self
            .claims
            .iter()
            .filter(|(_, claim)| client == Some(claim.client.as_str()))
            .map(|(id, _)| id.clone())
            .collect();
        claimed.sort();
        state["claimedModules"] = json!(claimed);
        state
    }

    fn cache(&mut self, latest: &Mutex<Snapshot>) {
        let state = self.decorate(None);
        let owners = self
            .claims
            .iter()
            .map(|(id, claim)| (id.clone(), claim.client.clone()))
            .collect();
        *latest.lock().unwrap_or_else(|poison| poison.into_inner()) = Snapshot { state, owners };
    }

    fn request(&mut self, request: Value) -> Value {
        let client = request["clientId"].as_str().map(str::to_owned);
        match self.handle(&request) {
            Ok(mut result) => {
                self.naming.refresh(
                    &self.state,
                    matches!(request["command"].as_str(), Some("connect" | "reconnect")),
                );
                result["state"] = self.decorate(client.as_deref());
                result
            }
            Err(error) => {
                json!({"ok":false,"error":error,"state":self.decorate(client.as_deref())})
            }
        }
    }

    fn handle(&mut self, request: &Value) -> Result<Value, String> {
        if self.cancel.load(Ordering::Acquire) {
            return Err(CLOSED.into());
        }
        if !request.is_object() {
            return Err("Invalid Pico gateway request.".into());
        }
        if request.to_string().len() > 8_192 {
            return Err("Pico gateway request is too large.".into());
        }
        let client = request["clientId"]
            .as_str()
            .filter(|id| identifier(id, 16, 80))
            .ok_or("Missing Cockpit client identity. Reload Cockpit.")?;
        let command = request["command"]
            .as_str()
            .ok_or("Unknown Pico gateway command.")?;
        let module_id = request["moduleId"].as_str().unwrap_or("");
        self.expire_claims(Some(client));
        if matches!(
            command,
            "register" | "reconnect" | "stop" | "disconnect" | "connect"
        ) && !self.claims.is_empty()
        {
            return Err("End active setup sessions before changing the gateway layout.".into());
        }
        match command {
            "ports" => {
                self.ports()?;
                Ok(json!({"ok":true}))
            }
            "connect" => {
                let selected = self
                    .ports()?
                    .into_iter()
                    .find(|port| request["port"] == port.path)
                    .ok_or("Select an available USB serial port for the flashed Pico gateway.")?;
                let expected = request["gatewayId"]
                    .as_str()
                    .and_then(|id| id.strip_prefix("pico:"))
                    .map(str::to_owned);
                if expected
                    .as_ref()
                    .is_some_and(|id| selected.serial_number.as_ref() != Some(id))
                {
                    return Err("Gateway identity does not match the selected port.".into());
                }
                self.disconnect();
                self.open(selected, expected, None)?;
                let stopped = self.exchange(json!({"command":"stop"}), self.timing.command)?;
                if stopped["ok"] != true {
                    self.close_port();
                    return Err(stopped["error"]
                        .as_str()
                        .unwrap_or("Pico could not stop its controller reports.")
                        .into());
                }
                self.exchange(json!({"command":"status"}), self.timing.command)
            }
            "disconnect" => {
                self.disconnect();
                Ok(json!({"ok":true}))
            }
            "status" if self.connection.is_none() => {
                self.ports()?;
                Ok(json!({"ok":true}))
            }
            "claim" => {
                if self
                    .claims
                    .get(module_id)
                    .is_some_and(|claim| claim.client != client)
                {
                    return Err("Another Cockpit tab owns this module’s setup session.".into());
                }
                let result = self.exchange(json!({"command":"status"}), self.timing.command)?;
                let module = self.state["modules"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|module| module["id"] == module_id);
                if !module.is_some_and(|module| module["online"] == true) {
                    return Err("Reconnect this module before starting setup.".into());
                }
                self.claims.insert(
                    module_id.into(),
                    Claim {
                        client: client.into(),
                        seen: Instant::now(),
                    },
                );
                Ok(result)
            }
            "release" => {
                if self
                    .claims
                    .get(module_id)
                    .is_some_and(|claim| claim.client == client)
                {
                    self.claims.remove(module_id);
                }
                if self.connection.is_some() {
                    self.exchange(json!({"command":"status"}), self.timing.command)
                } else {
                    Ok(json!({"ok":true}))
                }
            }
            "status" | "start" | "stop" | "register" | "reconnect" | "configure" => {
                if command == "configure"
                    && !self
                        .claims
                        .get(module_id)
                        .is_some_and(|claim| claim.client == client)
                {
                    return Err("Start a setup session before saving this module.".into());
                }
                let result = if command == "reconnect" {
                    self.reconnect()?
                } else {
                    self.exchange(request.clone(), self.timing.command)?
                };
                Ok(result)
            }
            _ => Err("Unknown Pico gateway command.".into()),
        }
    }

    fn heartbeat(&mut self, ui_seen: Instant) {
        if self.connection.is_none() {
            return;
        }
        if ui_seen.elapsed() > self.timing.ui_timeout {
            self.close_port();
            return;
        }
        self.expire_claims(None);
        if self
            .exchange(json!({"command":"status"}), self.timing.command)
            .is_ok()
        {
            self.naming.refresh(&self.state, false);
        }
    }

    fn shutdown(&mut self) {
        if let Some(connection) = self.connection.as_mut() {
            let wire = format!(
                "{}\n",
                json!({"id":Uuid::new_v4().to_string(),"command":"stop"})
            );
            let _ = connection.write_all(wire.as_bytes());
            let _ = connection.flush();
        }
        self.close_port();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::VecDeque,
        io::{Read, Write},
    };

    const CLIENT_A: &str = "test-client-a-0123456789";
    const CLIENT_B: &str = "test-client-b-0123456789";
    const MODULE: &str = "OSG-SQ-0042";
    const PATH: &str = "/dev/cu.usbmodemOSG1";

    fn info(path: &str) -> PortInfo {
        PortInfo {
            path: path.into(),
            serial_number: Some("OSG-PICO-0123456789ABCDEF".into()),
            vendor_id: Some("cafe".into()),
            product_id: Some("4010".into()),
            manufacturer: None,
            friendly_name: None,
        }
    }
    fn timing() -> Timing {
        Timing {
            command: Duration::from_millis(20),
            hello: Duration::from_millis(20),
            reconnect: Duration::from_millis(60),
            reconnect_delay: Duration::from_millis(1),
            reconnect_poll: Duration::from_millis(1),
            read_poll: Duration::from_millis(1),
            heartbeat: Duration::from_millis(5),
            ui_timeout: Duration::from_millis(40),
            claim_timeout: Duration::from_millis(20),
            naming_retry: Duration::from_millis(10),
        }
    }
    struct Mock {
        state: Value,
        ports: Vec<PortInfo>,
        opened: Vec<String>,
        writes: Vec<Value>,
        lines: Vec<(bool, bool)>,
        timeout: Option<String>,
        malformed: Option<String>,
        oversized: Option<String>,
        fragment: usize,
        stale: bool,
        unplugged: bool,
        new_path: Option<String>,
        swap_identity: bool,
    }
    type Context = Arc<Mutex<Mock>>;
    struct MockFactory(Context);
    struct MockConnection {
        context: Context,
        data: VecDeque<u8>,
    }
    impl Factory for MockFactory {
        fn ports(&mut self) -> Result<Vec<PortInfo>, String> {
            Ok(self.0.lock().unwrap().ports.clone())
        }
        fn open(&mut self, port: &PortInfo, _: Duration) -> Result<Box<dyn Connection>, String> {
            self.0.lock().unwrap().opened.push(port.path.clone());
            Ok(Box::new(MockConnection {
                context: self.0.clone(),
                data: VecDeque::new(),
            }))
        }
    }
    impl Read for MockConnection {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            let context = self.context.lock().unwrap();
            if context.unplugged {
                return Ok(0);
            }
            let count = context.fragment.min(bytes.len()).min(self.data.len());
            drop(context);
            if count == 0 {
                thread::sleep(Duration::from_millis(1));
                return Err(io::Error::from(io::ErrorKind::TimedOut));
            }
            for byte in &mut bytes[..count] {
                *byte = self.data.pop_front().unwrap();
            }
            Ok(count)
        }
    }
    impl Write for MockConnection {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let request: Value = serde_json::from_slice(bytes).unwrap();
            let mut context = self.context.lock().unwrap();
            context.writes.push(request.clone());
            let command = request["command"].as_str().unwrap();
            if context.timeout.as_deref() == Some(command) {
                return Ok(bytes.len());
            }
            if context.malformed.as_deref() == Some(command) {
                self.data.extend(b"{broken-json\n");
                return Ok(bytes.len());
            }
            if context.oversized.as_deref() == Some(command) {
                self.data.extend(vec![b'x'; RESPONSE_LIMIT + 1]);
                return Ok(bytes.len());
            }
            let mut reply = None;
            match command {
                "start" => {
                    context.state["running"] = json!(true);
                    for module in context.state["modules"].as_array_mut().unwrap() {
                        module["active"] = module["registered"].clone();
                    }
                }
                "stop" => neutral_firmware(&mut context.state),
                "reconnect" => {
                    neutral_firmware(&mut context.state);
                    let mut ack = context.state.clone();
                    ack["reconnecting"] = json!(true);
                    reply = Some(ack);
                    context.state["needsReconnect"] = json!(false);
                    if let Some(path) = context.new_path.clone() {
                        context.ports[0].path = path;
                    }
                    if context.swap_identity {
                        context.state["gatewaySerial"] = json!("OSG-PICO-OTHER123456789");
                        for module in context.state["modules"].as_array_mut().unwrap() {
                            module["usbSerialNumber"] = json!("OSG-PICO-OTHER123456789");
                        }
                    }
                }
                _ => {}
            }
            if context.stale {
                self.data.extend(
                    b"{\"id\":\"old-request\",\"ok\":true,\"state\":{\"backend\":\"unrelated\"}}\n",
                );
            }
            let encoded = format!(
                "{}\n",
                json!({"id":request["id"],"ok":true,"state":reply.unwrap_or_else(|| context.state.clone())})
            );
            self.data.extend(encoded.bytes());
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Connection for MockConnection {
        fn set_control_lines(&mut self, dtr: bool, rts: bool) -> Result<(), String> {
            let mut context = self.context.lock().unwrap();
            context.lines.push((dtr, rts));
            if !dtr {
                neutral_firmware(&mut context.state);
            }
            Ok(())
        }
    }
    fn neutral_firmware(state: &mut Value) {
        state["running"] = json!(false);
        for module in state["modules"].as_array_mut().unwrap() {
            module["active"] = json!(false);
            module["buttons"] = json!([]);
            module["axes"] = json!(vec![0; module["axisCount"].as_u64().unwrap() as usize]);
        }
    }
    fn harness() -> (Gateway, Context) {
        let context = Arc::new(Mutex::new(Mock {
            state: crate::protocol::tests::state(),
            ports: vec![info(PATH)],
            opened: vec![],
            writes: vec![],
            lines: vec![],
            timeout: None,
            malformed: None,
            oversized: None,
            fragment: 4_096,
            stale: false,
            unplugged: false,
            new_path: None,
            swap_identity: false,
        }));
        let gateway = Gateway::new(
            Box::new(MockFactory(context.clone())),
            timing(),
            Arc::new(AtomicBool::new(false)),
            false,
        );
        (gateway, context)
    }
    fn request(gateway: &mut Gateway, command: &str, extra: Value, client: &str) -> Value {
        let mut value = extra;
        value["command"] = json!(command);
        value["clientId"] = json!(client);
        gateway.request(value)
    }
    fn connect(gateway: &mut Gateway) -> Value {
        request(gateway, "connect", json!({"port":PATH}), CLIENT_A)
    }

    #[test]
    fn explicit_connect_requires_valid_handshake_and_stops_existing_reports() {
        let (mut gateway, context) = harness();
        context
            .lock()
            .unwrap()
            .ports
            .push(info("/tmp/arbitrary-file"));
        assert_eq!(
            request(&mut gateway, "status", json!({}), CLIENT_A)["state"]["ports"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            request(
                &mut gateway,
                "connect",
                json!({"port":"/tmp/arbitrary-file"}),
                CLIENT_A
            )["ok"],
            false
        );
        assert!(context.lock().unwrap().opened.is_empty());
        assert_eq!(connect(&mut gateway)["state"]["running"], false);
        assert_eq!(
            context
                .lock()
                .unwrap()
                .writes
                .iter()
                .map(|request| request["command"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["hello", "stop", "status"]
        );
        assert!(context
            .lock()
            .unwrap()
            .writes
            .iter()
            .all(|request| request.get("clientId").is_none()));
        assert_eq!(
            request(&mut gateway, "start", json!({}), CLIENT_A)["state"]["running"],
            true
        );
        assert_eq!(
            request(&mut gateway, "disconnect", json!({}), CLIENT_A)["state"]["connected"],
            false
        );
        assert_eq!(context.lock().unwrap().lines.last(), Some(&(false, false)));
    }

    #[test]
    fn fragmented_utf8_and_stale_response_never_replace_trusted_state() {
        let (mut gateway, context) = harness();
        {
            let mut context = context.lock().unwrap();
            context.fragment = 1;
            context.stale = true;
            context.state["modules"][0]["settings"]["label"] = json!("OSG Pédales 🏁");
        }
        let result = connect(&mut gateway);
        assert_eq!(result["ok"], true);
        assert_eq!(
            result["state"]["modules"][0]["settings"]["label"],
            "OSG Pédales 🏁"
        );
        gateway.shutdown();
    }

    #[test]
    fn scoped_connection_validates_metadata_and_hello_before_controller_commands() {
        let (mut gateway, context) = harness();
        let wrong = "OSG-PICO-DIFFERENT1234";
        let result = request(
            &mut gateway,
            "connect",
            json!({"port":PATH,"gatewayId":format!("pico:{wrong}")}),
            CLIENT_A,
        );
        assert_eq!(result["ok"], false);
        assert!(context.lock().unwrap().opened.is_empty());
        {
            let mut context = context.lock().unwrap();
            context.state["gatewaySerial"] = json!(wrong);
            for module in context.state["modules"].as_array_mut().unwrap() {
                module["usbSerialNumber"] = json!(wrong);
            }
        }
        let result = request(
            &mut gateway,
            "connect",
            json!({"port":PATH,"gatewayId":"pico:OSG-PICO-0123456789ABCDEF"}),
            CLIENT_A,
        );
        assert_eq!(result["ok"], false);
        assert!(result["error"].as_str().unwrap().contains("different Pico"));
        assert_eq!(
            context
                .lock()
                .unwrap()
                .writes
                .iter()
                .map(|r| r["command"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>(),
            ["hello"]
        );
        assert_eq!(context.lock().unwrap().lines.last(), Some(&(false, false)));
    }

    #[test]
    fn wrong_protocol_closes_before_sending_controller_commands() {
        for (field, value) in [
            ("backend", json!("other")),
            ("protocolVersion", json!(2)),
            ("gatewaySerial", json!("wrong")),
        ] {
            let (mut gateway, context) = harness();
            context.lock().unwrap().state[field] = value;
            let result = connect(&mut gateway);
            assert_eq!(result["ok"], false);
            assert!(result["error"]
                .as_str()
                .unwrap()
                .contains("not a supported"));
            assert_eq!(context.lock().unwrap().writes.len(), 1);
            assert_eq!(context.lock().unwrap().lines.last(), Some(&(false, false)));
        }
    }

    #[test]
    fn timeout_malformed_and_oversized_responses_neutralize_and_release_leases() {
        for failure in ["timeout", "malformed", "oversized"] {
            let (mut gateway, context) = harness();
            assert_eq!(connect(&mut gateway)["ok"], true);
            request(&mut gateway, "start", json!({}), CLIENT_A);
            {
                let mut context = context.lock().unwrap();
                context.state["modules"][0]["buttons"] = json!([1]);
                context.state["modules"][0]["axes"] = json!([60_000]);
            }
            request(&mut gateway, "claim", json!({"moduleId":MODULE}), CLIENT_A);
            {
                let mut context = context.lock().unwrap();
                match failure {
                    "timeout" => context.timeout = Some("status".into()),
                    "malformed" => context.malformed = Some("status".into()),
                    _ => context.oversized = Some("status".into()),
                }
            }
            let result = request(&mut gateway, "status", json!({}), CLIENT_A);
            assert_eq!(result["ok"], false, "{failure}");
            assert_eq!(result["state"]["connected"], false);
            assert_eq!(result["state"]["modules"][0]["buttons"], json!([]));
            assert_eq!(result["state"]["modules"][0]["axes"], json!([0]));
            assert_eq!(result["state"]["claimedModules"], json!([]));
            assert_eq!(context.lock().unwrap().lines.last(), Some(&(false, false)));
            request(&mut gateway, "status", json!({}), CLIENT_A);
            assert_eq!(
                context.lock().unwrap().opened.len(),
                1,
                "status never automatically opens hardware"
            );
        }
    }

    #[test]
    fn setup_leases_protect_configuration_layout_and_other_clients() {
        let (mut gateway, context) = harness();
        connect(&mut gateway);
        assert_eq!(
            request(
                &mut gateway,
                "configure",
                json!({"moduleId":MODULE,"settings":{}}),
                CLIENT_A
            )["ok"],
            false
        );
        assert_eq!(
            request(&mut gateway, "claim", json!({"moduleId":MODULE}), CLIENT_A)["state"]
                ["claimedModules"],
            json!([MODULE])
        );
        assert_eq!(
            request(&mut gateway, "claim", json!({"moduleId":MODULE}), CLIENT_B)["ok"],
            false
        );
        for command in ["register", "reconnect", "stop", "disconnect", "connect"] {
            let result = request(
                &mut gateway,
                command,
                json!({"moduleId":MODULE,"port":PATH}),
                CLIENT_A,
            );
            assert!(result["error"]
                .as_str()
                .unwrap()
                .contains("End active setup"));
            assert_eq!(result["state"]["claimedModules"], json!([MODULE]));
        }
        assert_eq!(
            request(
                &mut gateway,
                "configure",
                json!({"moduleId":MODULE,"settings":{}}),
                CLIENT_B
            )["ok"],
            false
        );
        request(
            &mut gateway,
            "release",
            json!({"moduleId":MODULE}),
            CLIENT_B,
        );
        assert_eq!(
            request(
                &mut gateway,
                "configure",
                json!({"moduleId":MODULE,"settings":{}}),
                CLIENT_A
            )["ok"],
            true
        );
        context.lock().unwrap().state["modules"][0]["online"] = json!(false);
        assert_eq!(
            request(&mut gateway, "claim", json!({"moduleId":MODULE}), CLIENT_A)["ok"],
            false
        );
        gateway.shutdown();
    }

    #[test]
    fn expired_claims_can_be_owned_by_another_client() {
        let (mut gateway, _) = harness();
        connect(&mut gateway);
        request(&mut gateway, "claim", json!({"moduleId":MODULE}), CLIENT_A);
        gateway.claims.get_mut(MODULE).unwrap().seen = Instant::now() - Duration::from_millis(25);
        assert_eq!(
            request(&mut gateway, "claim", json!({"moduleId":MODULE}), CLIENT_B)["state"]
                ["claimedModules"],
            json!([MODULE])
        );
        gateway.shutdown();
    }

    #[test]
    fn reconnect_follows_usb_serial_to_new_port_and_rejects_a_different_board() {
        for swap in [false, true] {
            let (mut gateway, context) = harness();
            connect(&mut gateway);
            {
                let mut context = context.lock().unwrap();
                context.new_path = Some("/dev/cu.usbmodemOSG9".into());
                context.swap_identity = swap;
            }
            let result = request(&mut gateway, "reconnect", json!({}), CLIENT_A);
            if swap {
                assert_eq!(result["ok"], false);
                assert!(result["error"].as_str().unwrap().contains("different Pico"));
                assert_eq!(result["state"]["connected"], false);
            } else {
                assert_eq!(result["ok"], true);
                assert_eq!(result["state"]["port"], "/dev/cu.usbmodemOSG9");
                assert_eq!(result["state"]["running"], false);
                assert_eq!(
                    context.lock().unwrap().opened,
                    [PATH, "/dev/cu.usbmodemOSG9"]
                );
            }
            gateway.shutdown();
        }
    }

    #[test]
    fn ui_loss_and_unplug_leave_no_stale_inputs() {
        for unplug in [false, true] {
            let (mut gateway, context) = harness();
            connect(&mut gateway);
            request(&mut gateway, "start", json!({}), CLIENT_A);
            {
                let mut context = context.lock().unwrap();
                context.state["modules"][0]["buttons"] = json!([1]);
                context.state["modules"][0]["axes"] = json!([65_535]);
            }
            assert_eq!(
                request(&mut gateway, "status", json!({}), CLIENT_A)["state"]["modules"][0]
                    ["buttons"],
                json!([1])
            );
            if unplug {
                context.lock().unwrap().unplugged = true;
                assert_eq!(
                    request(&mut gateway, "status", json!({}), CLIENT_A)["ok"],
                    false
                );
            } else {
                gateway.heartbeat(Instant::now() - Duration::from_millis(50));
            }
            assert_eq!(gateway.state["connected"], false);
            assert_eq!(gateway.state["modules"][0]["buttons"], json!([]));
            assert_eq!(gateway.state["modules"][0]["axes"], json!([0]));
            assert_eq!(context.lock().unwrap().lines.last(), Some(&(false, false)));
        }
    }

    #[test]
    fn shutdown_interrupts_a_pending_timeout_and_drops_dtr() {
        let (_, context) = harness();
        let mut long = timing();
        long.command = Duration::from_secs(3);
        long.heartbeat = Duration::from_secs(1);
        let service = Arc::new(PicoService::with_factory(
            Box::new(MockFactory(context.clone())),
            long,
            false,
        ));
        assert_eq!(
            service.request(json!({"command":"connect","port":PATH,"clientId":CLIENT_A}))["ok"],
            true
        );
        context.lock().unwrap().timeout = Some("start".into());
        let pending = service.clone();
        let request = thread::spawn(move || {
            pending.request(
                json!({"command":"start","moduleId":MODULE,"buttons":[1],"clientId":CLIENT_A}),
            )
        });
        while !context
            .lock()
            .unwrap()
            .writes
            .iter()
            .any(|request| request["command"] == "start")
        {
            thread::sleep(Duration::from_millis(1));
        }
        let start = Instant::now();
        service.shutdown();
        assert!(start.elapsed() < Duration::from_millis(100));
        assert_eq!(request.join().unwrap()["ok"], false);
        assert_eq!(context.lock().unwrap().lines.last(), Some(&(false, false)));
    }

    #[test]
    fn changed_labels_cancel_inflight_naming_and_reject_its_completion() {
        let mut state = crate::protocol::tests::state();
        state["connected"] = json!(true);
        let payload = json!({"gatewaySerial":state["gatewaySerial"],"modules":[{
            "id":state["modules"][0]["id"],"hidInterface":2,"label":state["modules"][0]["settings"]["label"]}]});
        let mut naming = Naming::new(true, Duration::from_secs(5));
        naming.signature = Some(payload.to_string());
        naming.epoch = 7;
        let cancel = Arc::new(AtomicBool::new(false));
        naming.task_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        naming.task = Some(receiver);
        naming.state = Some(json!({"supported":true,"applied":false}));
        state["modules"][0]["settings"]["label"] = json!("OSG Updated name");
        naming.refresh(&state, false);
        assert!(cancel.load(Ordering::Acquire));
        assert_eq!(naming.epoch, 8);
        assert!(naming.state.is_none());
        sender
            .send((7, json!({"supported":true,"ok":true})))
            .unwrap();
        naming.harvest();
        assert!(
            naming.state.is_none(),
            "old registry result must not mark the new inventory applied"
        );
        assert!(naming.task.is_none());
    }

    #[test]
    fn full_queue_preserves_trusted_connection_and_client_specific_claims() {
        let (_, context) = harness();
        let mut durations = timing();
        durations.command = Duration::from_secs(3);
        durations.heartbeat = Duration::from_secs(1);
        let service = Arc::new(PicoService::with_factory(
            Box::new(MockFactory(context.clone())),
            durations,
            false,
        ));
        assert_eq!(
            service.request(json!({"command":"connect","port":PATH,"clientId":CLIENT_A}))["ok"],
            true
        );
        assert_eq!(
            service.request(json!({"command":"claim","moduleId":MODULE,"clientId":CLIENT_A}))["ok"],
            true
        );
        context.lock().unwrap().timeout = Some("start".into());
        let in_flight = service.clone();
        let mut callers = vec![thread::spawn(move || {
            in_flight.request(
                json!({"command":"start","moduleId":MODULE,"buttons":[1],"clientId":CLIENT_A}),
            )
        })];
        let deadline = Instant::now() + Duration::from_secs(1);
        while !context
            .lock()
            .unwrap()
            .writes
            .iter()
            .any(|request| request["command"] == "start")
        {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        for _ in 0..8 {
            let queued = service.clone();
            callers.push(thread::spawn(move || {
                queued.request(json!({"command":"status","clientId":CLIENT_A}))
            }));
        }
        while service.queued.load(Ordering::Acquire) != 9 {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        let owner = service.request(json!({"command":"status","clientId":CLIENT_A}));
        assert!(owner["error"].as_str().unwrap().contains("busy"));
        assert_eq!(owner["state"]["connected"], true);
        assert_eq!(owner["state"]["claimedModules"], json!([MODULE]));
        let other = service.request(json!({"command":"status","clientId":CLIENT_B}));
        assert_eq!(other["state"]["connected"], true);
        assert_eq!(other["state"]["claimedModules"], json!([]));
        service.shutdown();
        for caller in callers {
            assert_eq!(caller.join().unwrap()["ok"], false);
        }
    }

    #[test]
    fn client_rejects_input_and_inventory_injection_without_serial_writes() {
        let (mut gateway, context) = harness();
        connect(&mut gateway);
        let before = context.lock().unwrap().writes.len();
        for command in ["input", "presence", "add", "remove"] {
            let result = request(&mut gateway, command, json!({"moduleId":MODULE}), CLIENT_A);
            assert_eq!(result["ok"], false);
            assert!(result["error"]
                .as_str()
                .unwrap()
                .contains("Unknown Pico gateway command"));
        }
        assert_eq!(context.lock().unwrap().writes.len(), before);
        gateway.shutdown();
    }
}
