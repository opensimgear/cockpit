//! Bounded native supervisor for the bundled, separately entitled CoreHID helper.
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAX_REQUEST: usize = 8192;
const MAX_RESPONSE: usize = 65_536;
const REQUEST_WAIT: Duration = Duration::from_secs(12);
const HELPER_WAIT: Duration = Duration::from_secs(10);
const LEASE_TIME: Duration = Duration::from_secs(15);

enum Operation {
    Request {
        request: Value,
        deadline: Instant,
        reply: SyncSender<Value>,
    },
    Shutdown,
}

pub struct CoreHidService {
    sender: SyncSender<Operation>,
    closed: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl CoreHidService {
    pub fn new(resource_dir: PathBuf) -> Self {
        let (sender, receiver) = mpsc::sync_channel(16);
        let closed = Arc::new(AtomicBool::new(false));
        let worker_closed = closed.clone();
        let worker = thread::spawn(move || run_worker(resource_dir, receiver, worker_closed));
        Self {
            sender,
            closed,
            worker: Mutex::new(Some(worker)),
        }
    }

    pub fn request(&self, request: Value) -> Value {
        if self.closed.load(Ordering::Acquire) {
            return error_result("Cockpit has closed.", None);
        }
        if let Err(error) = validate_request(&request) {
            return error_result(&error, None);
        }
        let (reply, response) = mpsc::sync_channel(1);
        let operation = Operation::Request {
            request,
            deadline: Instant::now() + REQUEST_WAIT,
            reply,
        };
        match self.sender.try_send(operation) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                return error_result(
                    "The CoreHID request queue is full. Retry when the gateway is ready.",
                    None,
                )
            }
            Err(TrySendError::Disconnected(_)) => {
                return error_result("The CoreHID service has stopped.", None)
            }
        }
        response.recv_timeout(REQUEST_WAIT).unwrap_or_else(|_| {
            error_result(
                "The CoreHID request timed out. Retry when the gateway is ready.",
                None,
            )
        })
    }

    pub fn shutdown(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = self.sender.try_send(Operation::Shutdown);
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
        }
    }
}

impl Drop for CoreHidService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn error_result(message: &str, state: Option<&Value>) -> Value {
    let mut result = json!({"ok": false, "error": message});
    if let Some(state) = state {
        result["state"] = state.clone();
    }
    result
}

fn validate_request(request: &Value) -> Result<(), String> {
    if !request.is_object()
        || serde_json::to_vec(request)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_REQUEST
    {
        return Err("The CoreHID request must be an object of at most 8192 bytes.".into());
    }
    let client = request["clientId"].as_str().unwrap_or("");
    if !(16..=80).contains(&client.len())
        || !client
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'-'))
    {
        return Err("Missing simulator client identity. Reload Cockpit.".into());
    }
    if !matches!(
        request["command"].as_str(),
        Some(
            "status"
                | "start"
                | "stop"
                | "input"
                | "register"
                | "reconnect"
                | "presence"
                | "configure"
                | "claim"
                | "release"
        )
    ) {
        return Err("Unknown simulator command.".into());
    }
    if matches!(
        request["command"].as_str(),
        Some("claim" | "release" | "configure")
    ) && request["moduleId"]
        .as_str()
        .is_none_or(|id| id.is_empty() || id.len() > 80)
    {
        return Err("A module identity is required for setup access.".into());
    }
    Ok(())
}

struct Lease {
    client: String,
    last_seen: Instant,
}

#[derive(Default)]
struct Claims(HashMap<String, Lease>);

impl Claims {
    fn refresh(&mut self, client: &str, now: Instant) {
        self.0
            .retain(|_, lease| now.saturating_duration_since(lease.last_seen) <= LEASE_TIME);
        for lease in self.0.values_mut().filter(|lease| lease.client == client) {
            lease.last_seen = now;
        }
    }

    fn guard(&self, command: &str, module: &str, client: &str) -> Result<(), String> {
        let owner = self.0.get(module).map(|lease| lease.client.as_str());
        if command == "claim" && owner.is_some_and(|owner| owner != client) {
            return Err("Another Cockpit window owns this module’s setup session.".into());
        }
        if command == "configure" && owner != Some(client) {
            return Err("Start a setup session before saving this module.".into());
        }
        if matches!(command, "register" | "reconnect" | "stop") && !self.0.is_empty() {
            return Err("End active setup sessions before changing the gateway layout.".into());
        }
        Ok(())
    }

    fn decorate(&self, state: &mut Value, client: &str) {
        let mut modules: Vec<&str> = self
            .0
            .iter()
            .filter(|(_, lease)| lease.client == client)
            .map(|(id, _)| id.as_str())
            .collect();
        modules.sort_unstable();
        state["claimedModules"] = json!(modules);
        state["backend"] = json!("corehid");
    }
}

struct Supervisor {
    resources: PathBuf,
    helper: Option<Helper>,
    claims: Claims,
    state: Option<Value>,
}

impl Supervisor {
    fn new(resources: PathBuf) -> Self {
        Self {
            resources,
            helper: None,
            claims: Claims::default(),
            state: None,
        }
    }

    fn discard_helper(&mut self) {
        self.helper = None;
        self.claims.0.clear();
        self.state = None;
    }

    fn send(
        &mut self,
        request: Value,
        deadline: Instant,
        closed: &AtomicBool,
    ) -> Result<Value, String> {
        if self.helper.is_none() {
            self.claims.0.clear();
            self.helper = Some(Helper::launch(&helper_path(&self.resources)?)?);
        }
        let result = self
            .helper
            .as_mut()
            .unwrap()
            .send(request, deadline, closed);
        match result {
            Ok(result) => {
                self.state = Some(result["state"].clone());
                Ok(result)
            }
            Err(error) => {
                self.discard_helper();
                Err(error)
            }
        }
    }

    fn handle(&mut self, request: Value, deadline: Instant, closed: &AtomicBool) -> Value {
        // A dead helper invalidates setup ownership before any layout/configuration guards.
        if self
            .helper
            .as_mut()
            .is_some_and(|helper| !matches!(helper.child.try_wait(), Ok(None)))
        {
            self.discard_helper();
        }
        let client = request["clientId"].as_str().unwrap().to_owned();
        let command = request["command"].as_str().unwrap().to_owned();
        let module = request["moduleId"].as_str().unwrap_or("").to_owned();
        self.claims.refresh(&client, Instant::now());
        if let Err(error) = self.claims.guard(&command, &module, &client) {
            return self.error(&error, &client);
        }
        if command == "release"
            && self
                .claims
                .0
                .get(&module)
                .is_some_and(|lease| lease.client == client)
        {
            self.claims.0.remove(&module);
        }
        let native_request = if matches!(command.as_str(), "claim" | "release") {
            json!({"command": "status"})
        } else {
            request.clone()
        };
        let mut result = match self.send(native_request, deadline, closed) {
            Ok(result) => result,
            Err(error) => return self.error(&error, &client),
        };
        if result["ok"].as_bool() == Some(true) {
            if command == "claim" {
                let online = result["state"]["modules"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| {
                        item["id"].as_str() == Some(&module)
                            && item["online"].as_bool() == Some(true)
                    });
                if !online {
                    return self.error("Reconnect this module before starting setup.", &client);
                }
                self.claims.0.insert(
                    module.clone(),
                    Lease {
                        client: client.clone(),
                        last_seen: Instant::now(),
                    },
                );
            } else if command == "presence" && request["online"].as_bool() == Some(false) {
                self.claims.0.remove(&module);
            }
        }
        self.claims.decorate(&mut result["state"], &client);
        result
    }

    fn error(&self, message: &str, client: &str) -> Value {
        let mut state = self.state.clone();
        if let Some(state) = state.as_mut() {
            self.claims.decorate(state, client);
        }
        error_result(message, state.as_ref())
    }
}

fn run_worker(resources: PathBuf, receiver: Receiver<Operation>, closed: Arc<AtomicBool>) {
    let mut supervisor = Supervisor::new(resources);
    while !closed.load(Ordering::Acquire) {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(Operation::Request {
                request,
                deadline,
                reply,
            }) => {
                let result = if Instant::now() >= deadline || closed.load(Ordering::Acquire) {
                    error_result("The CoreHID request expired before it could run.", None)
                } else {
                    supervisor.handle(request, deadline, &closed)
                };
                let _ = reply.send(result);
            }
            Ok(Operation::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    // Close stdin so Swift can neutralize reports and release its virtual controllers.
    if let Some(helper) = supervisor.helper.as_mut() {
        helper.stop_gracefully();
    }
    supervisor.discard_helper();
    while let Ok(Operation::Request { reply, .. }) = receiver.try_recv() {
        let _ = reply.send(error_result("Cockpit has closed.", None));
    }
}

fn helper_path(resources: &Path) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("OSG_HID_HELPER") {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(
                "OSG_HID_HELPER must be the absolute path to an Apple-authorized signed helper."
                    .into(),
            );
        }
        return Ok(path);
    }
    Ok(resources.join("resources/corehid/osg-gateway-simulator"))
}

struct Helper {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<Result<Vec<u8>, String>>,
    diagnostics: Arc<Mutex<Vec<u8>>>,
}

impl Helper {
    fn launch(path: &Path) -> Result<Self, String> {
        if !cfg!(target_os = "macos") {
            return Err("CoreHID simulation requires macOS 15 or later and an Apple-authorized signed helper.".into());
        }
        if !path.is_absolute() || !path.is_file() {
            return Err("The bundled CoreHID helper is unavailable. Rebuild Cockpit, or set OSG_HID_HELPER to an Apple-authorized signed helper.".into());
        }
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("Could not launch the CoreHID helper: {error}"))?;
        let stdin = child.stdin.take();
        if let Err(error) = nonblocking_input(stdin.as_ref().unwrap()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let (sender, lines) = mpsc::sync_channel(2);
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match bounded_line(&mut reader) {
                    Ok(Some(line)) => {
                        if sender.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => {
                        let _ = sender.send(Err("CoreHID helper exited before responding.".into()));
                        break;
                    }
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            }
        });
        let diagnostics = Arc::new(Mutex::new(Vec::new()));
        let output = diagnostics.clone();
        thread::spawn(move || {
            let mut buffer = [0_u8; 512];
            while let Ok(size) = stderr.read(&mut buffer) {
                if size == 0 {
                    break;
                }
                if let Ok(mut output) = output.lock() {
                    output.extend_from_slice(&buffer[..size]);
                    if output.len() > 3000 {
                        let excess = output.len() - 3000;
                        output.drain(..excess);
                    }
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            lines,
            diagnostics,
        })
    }

    fn send(
        &mut self,
        mut request: Value,
        deadline: Instant,
        closed: &AtomicBool,
    ) -> Result<Value, String> {
        let id = uuid::Uuid::new_v4().to_string();
        request["id"] = json!(id);
        // Ownership exists only in Cockpit; the helper accepts no client identity.
        request.as_object_mut().unwrap().remove("clientId");
        let mut data = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
        data.push(b'\n');
        let response_deadline = std::cmp::min(deadline, Instant::now() + HELPER_WAIT);
        let mut written = 0;
        while written < data.len() {
            if closed.load(Ordering::Acquire) {
                return Err("Cockpit has closed.".into());
            }
            if Instant::now() >= response_deadline {
                return Err("CoreHID helper input timed out and was stopped.".into());
            }
            match self
                .stdin
                .as_mut()
                .ok_or("CoreHID helper input is closed.")?
                .write(&data[written..])
            {
                Ok(0) => return Err("CoreHID helper input has closed.".into()),
                Ok(size) => written += size,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Err(format!("Could not send to the CoreHID helper: {error}")),
            }
        }
        loop {
            if closed.load(Ordering::Acquire) {
                return Err("Cockpit has closed.".into());
            }
            let remaining = response_deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("CoreHID helper did not respond and was stopped. Retry to start a fresh helper.".into());
            }
            match self
                .lines
                .recv_timeout(remaining.min(Duration::from_millis(100)))
            {
                Ok(Ok(line)) => return parse_response(&line, &id),
                Ok(Err(error)) => return Err(self.with_diagnostics(&error)),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(self.with_diagnostics("CoreHID helper output has closed."))
                }
            }
        }
    }

    fn with_diagnostics(&self, message: &str) -> String {
        let diagnostics = self
            .diagnostics
            .lock()
            .ok()
            .map(|output| String::from_utf8_lossy(&output).trim().to_owned())
            .unwrap_or_default();
        if diagnostics.is_empty() {
            message.into()
        } else {
            format!("{message} {diagnostics}")
        }
    }

    fn stop_gracefully(&mut self) {
        self.stdin = None;
        let deadline = Instant::now() + Duration::from_millis(750);
        while Instant::now() < deadline {
            if !matches!(self.child.try_wait(), Ok(None)) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        self.stdin = None;
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

#[cfg(target_os = "macos")]
fn nonblocking_input(stdin: &ChildStdin) -> Result<(), String> {
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn fcntl(fd: i32, command: i32, ...) -> i32;
    }
    // Darwin F_GETFL/F_SETFL and O_NONBLOCK, confined to the macOS-only helper.
    let flags = unsafe { fcntl(stdin.as_raw_fd(), 3) };
    if flags == -1 || unsafe { fcntl(stdin.as_raw_fd(), 4, flags | 0x0004) } == -1 {
        return Err(format!(
            "Could not configure the CoreHID helper input: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn nonblocking_input(_: &ChildStdin) -> Result<(), String> {
    Err("CoreHID simulation requires macOS.".into())
}

fn bounded_line(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, String> {
    let mut line = Vec::new();
    let size = reader
        .take((MAX_RESPONSE + 1) as u64)
        .read_until(b'\n', &mut line)
        .map_err(|error| format!("Could not read CoreHID helper output: {error}"))?;
    if size == 0 {
        return Ok(None);
    }
    if size > MAX_RESPONSE || line.last() != Some(&b'\n') {
        return Err("CoreHID helper returned an oversized or incomplete response.".into());
    }
    Ok(Some(line))
}

fn parse_response(line: &[u8], id: &str) -> Result<Value, String> {
    let result: Value = serde_json::from_slice(line)
        .map_err(|_| "CoreHID helper returned malformed JSON.".to_owned())?;
    let state = &result["state"];
    if result["id"].as_str() != Some(id)
        || !result["ok"].is_boolean()
        || (!result["error"].is_null() && !result["error"].is_string())
        || !state["running"].is_boolean()
        || !state["needsReconnect"].is_boolean()
        || !state["modules"].is_array()
    {
        return Err("CoreHID helper returned an invalid or uncorrelated response.".into());
    }
    if state["modules"].as_array().unwrap().len() > 32
        || state["modules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|module| !valid_module(module))
    {
        return Err("CoreHID helper returned invalid module state.".into());
    }
    Ok(result)
}

fn valid_module(module: &Value) -> bool {
    let Some(button_count) = module["buttonCount"].as_u64().filter(|count| *count <= 32) else {
        return false;
    };
    let Some(axis_count) = module["axisCount"].as_u64().filter(|count| *count <= 6) else {
        return false;
    };
    let Some(buttons) = module["buttons"].as_array() else {
        return false;
    };
    let Some(axes) = module["axes"].as_array() else {
        return false;
    };
    module["id"]
        .as_str()
        .is_some_and(|id| !id.is_empty() && id.len() <= 80)
        && module["name"].is_string()
        && matches!(
            module["kind"].as_str(),
            Some("shifter" | "pedals" | "panel" | "handbrake" | "wheel")
        )
        && module["online"].is_boolean()
        && module["active"].is_boolean()
        && module["registered"].is_boolean()
        && module["serialNumber"].is_string()
        && (module["registryID"].is_null() || module["registryID"].is_string())
        && buttons.len() <= button_count as usize
        && buttons.iter().all(|button| {
            button
                .as_u64()
                .is_some_and(|button| button > 0 && button <= button_count)
        })
        && axes.len() == axis_count as usize
        && axes
            .iter()
            .all(|axis| axis.as_u64().is_some_and(|axis| axis <= 65_535))
        && module["settings"]["label"]
            .as_str()
            .is_some_and(|label| !label.trim().is_empty() && label.chars().count() <= 80)
        && module["settings"]["debounce"]
            .as_u64()
            .is_some_and(|debounce| debounce <= 100)
        && module["settings"]["inverted"].is_boolean()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ownership_blocks_configuration_and_layout_changes_until_release_or_expiry() {
        let mut claims = Claims::default();
        let now = Instant::now();
        claims.0.insert(
            "module".into(),
            Lease {
                client: "owner".into(),
                last_seen: now,
            },
        );
        assert!(claims.guard("configure", "module", "owner").is_ok());
        assert!(claims.guard("configure", "module", "other").is_err());
        assert!(claims.guard("claim", "module", "other").is_err());
        for command in ["stop", "reconnect", "register"] {
            assert!(claims.guard(command, "module", "owner").is_err());
        }
        claims.refresh("other", now + LEASE_TIME + Duration::from_millis(1));
        assert!(claims.guard("claim", "module", "other").is_ok());
        assert!(claims.guard("reconnect", "module", "other").is_ok());
    }

    #[test]
    fn refresh_and_snapshot_expose_only_the_requesting_clients_claims() {
        let mut claims = Claims::default();
        let now = Instant::now();
        claims.0.insert(
            "one".into(),
            Lease {
                client: "owner".into(),
                last_seen: now,
            },
        );
        claims.0.insert(
            "two".into(),
            Lease {
                client: "other".into(),
                last_seen: now,
            },
        );
        claims.refresh("owner", now + Duration::from_secs(14));
        claims.refresh("owner", now + Duration::from_secs(16));
        let mut state = json!({});
        claims.decorate(&mut state, "owner");
        assert_eq!(state["claimedModules"], json!(["one"]));
        assert_eq!(claims.0.len(), 1);
    }

    #[test]
    fn bounds_lines_and_rejects_malformed_or_wrong_id_responses() {
        let good = json!({"id": "expected", "ok": true, "state": {"running": false, "needsReconnect": false, "modules": []}});
        assert!(parse_response(&serde_json::to_vec(&good).unwrap(), "expected").is_ok());
        assert!(parse_response(&serde_json::to_vec(&good).unwrap(), "other").is_err());
        assert!(parse_response(b"not JSON", "expected").is_err());
        assert!(parse_response(
            b"{\"id\":\"expected\",\"ok\":true,\"state\":{}}",
            "expected"
        )
        .is_err());
        assert!(bounded_line(&mut std::io::Cursor::new(vec![b'x'; MAX_RESPONSE + 1])).is_err());
        assert!(bounded_line(&mut std::io::Cursor::new(b"{}".to_vec())).is_err());
    }

    #[test]
    fn crash_discards_claims_and_cached_state() {
        let mut supervisor = Supervisor::new(PathBuf::from("/unused"));
        supervisor.claims.0.insert(
            "one".into(),
            Lease {
                client: "owner".into(),
                last_seen: Instant::now(),
            },
        );
        supervisor.state = Some(json!({"running": true}));
        supervisor.discard_helper();
        assert!(supervisor.claims.0.is_empty());
        assert!(supervisor.state.is_none());
    }

    #[test]
    fn rejects_invalid_ownership_and_oversized_requests_before_launch() {
        assert!(validate_request(
            &json!({"command": "status", "clientId": "test-client-identity"})
        )
        .is_ok());
        assert!(validate_request(&json!({"command": "status", "clientId": "short"})).is_err());
        assert!(validate_request(
            &json!({"command": "configure", "clientId": "test-client-identity"})
        )
        .is_err());
        assert!(validate_request(&json!({"command": "status", "clientId": "test-client-identity", "padding": "x".repeat(MAX_REQUEST)})).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mock_helper_correlates_stdio_and_exits_when_input_closes() {
        use std::os::unix::fs::PermissionsExt;
        let directory =
            std::env::temp_dir().join(format!("osg-corehid-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("mock-helper");
        std::fs::write(&path, r#"#!/bin/sh
while IFS= read -r line; do
  id=${line#*\"id\":\"}
  id=${id%%\"*}
  printf '{"id":"%s","ok":true,"state":{"running":false,"needsReconnect":false,"modules":[]}}\n' "$id"
done
"#).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut helper = Helper::launch(&path).unwrap();
        let closed = AtomicBool::new(false);
        let result = helper
            .send(
                json!({"command":"status"}),
                Instant::now() + Duration::from_secs(2),
                &closed,
            )
            .unwrap();
        assert_eq!(result["ok"], json!(true));
        assert!(uuid::Uuid::parse_str(result["id"].as_str().unwrap()).is_ok());
        helper.stop_gracefully();
        assert!(helper.child.try_wait().unwrap().is_some());
        drop(helper);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
