//! Narrow, opt-in WSS transport. Every connection has its own bounded worker and timeouts.
use crate::{
    tls::{self, NativeIdentity},
    Hub,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rustls::{ServerConnection, StreamOwned};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tungstenite::{
    handshake::server::{ErrorResponse, Request, Response},
    protocol::WebSocketConfig,
    Message, WebSocket,
};

pub const FRAME_BYTES: usize = 16 * 1024;
pub const TRANSFER_BYTES: usize = 13 * 1024 * 1024;
pub const SUBPROTOCOL: &str = "opensimgear-companion-v1";
pub const SERVICE_TYPE: &str = "_osg-companion._tcp.local.";
pub fn websocket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .read_buffer_size(8192)
        .write_buffer_size(0)
        .max_write_buffer_size(65536)
        .max_message_size(Some(FRAME_BYTES))
        .max_frame_size(Some(FRAME_BYTES))
}
pub fn timed_out(error: &tungstenite::Error) -> bool {
    matches!(error, tungstenite::Error::Io(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
}

/// Absolute deadline wraps raw TCP, so slow TLS handshakes cannot hide inside rustls I/O loops.
pub struct DeadlineTcp {
    pub inner: TcpStream,
    deadline: Option<Instant>,
}
impl DeadlineTcp {
    pub fn new(inner: TcpStream) -> Self {
        Self {
            inner,
            deadline: Some(Instant::now() + Duration::from_secs(3)),
        }
    }
    pub fn upgraded(&mut self) {
        self.deadline = None;
    }
    pub fn set_read_timeout(&self, value: Option<Duration>) -> std::io::Result<()> {
        self.inner.set_read_timeout(value)
    }
    fn check(&self) -> std::io::Result<()> {
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() > deadline)
        {
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "TLS/HTTP handshake deadline exceeded",
            ))
        } else {
            Ok(())
        }
    }
}
impl Read for DeadlineTcp {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.check()?;
        self.inner.read(buffer)
    }
}
impl Write for DeadlineTcp {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.check()?;
        self.inner.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.check()?;
        self.inner.flush()
    }
}
struct Attempts {
    since: Instant,
    total: usize,
    peers: HashMap<IpAddr, usize>,
}
impl Attempts {
    fn new() -> Self {
        Self {
            since: Instant::now(),
            total: 0,
            peers: HashMap::new(),
        }
    }
    fn allow(&mut self, peer: IpAddr) -> bool {
        if self.since.elapsed() >= Duration::from_secs(60) {
            self.since = Instant::now();
            self.total = 0;
            self.peers.clear();
        }
        if self.total >= 20 || self.peers.get(&peer).copied().unwrap_or(0) >= 5 {
            return false;
        }
        self.total += 1;
        *self.peers.entry(peer).or_default() += 1;
        true
    }
}

/// Prevents an unterminated HTTP upgrade from growing beyond the handshake budget.
pub struct BudgetIo<S> {
    pub inner: S,
    remaining: Option<usize>,
}
impl<S> BudgetIo<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            remaining: Some(32768),
        }
    }
    pub fn upgraded(&mut self) {
        self.remaining = None;
    }
}
impl<S: Read> Read for BudgetIo<S> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let cap = self.remaining.unwrap_or(buffer.len()).min(buffer.len());
        if cap == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "HTTP upgrade exceeds budget",
            ));
        }
        let count = self.inner.read(&mut buffer[..cap])?;
        if let Some(remaining) = &mut self.remaining {
            *remaining -= count;
        }
        Ok(count)
    }
}
impl<S: Write> Write for BudgetIo<S> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.inner.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
pub fn send_json<S: Read + Write>(socket: &mut WebSocket<S>, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    if bytes.len() > TRANSFER_BYTES {
        return Err("Reply exceeds bounded publication transfer".into());
    }
    if bytes.len() <= FRAME_BYTES {
        return socket
            .send(Message::Text(
                String::from_utf8(bytes)
                    .map_err(|error| error.to_string())?
                    .into(),
            ))
            .map_err(|error| error.to_string());
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let id = uuid::Uuid::new_v4().to_string();
    socket.send(Message::Text(json!({"type":"transfer-start","id":id,"bytes":bytes.len(),"sha256":tls::sha256(&bytes)}).to_string().into())).map_err(|error|error.to_string())?;
    for (index, chunk) in bytes.chunks(8192).enumerate() {
        if Instant::now() > deadline {
            return Err("Publication send deadline exceeded".into());
        }
        socket.send(Message::Text(json!({"type":"transfer-chunk","id":id,"index":index,"base64":STANDARD.encode(chunk)}).to_string().into())).map_err(|error|error.to_string())?;
    }
    socket
        .send(Message::Text(
            json!({"type":"transfer-end","id":id}).to_string().into(),
        ))
        .map_err(|error| error.to_string())
}
pub fn receive_json<S: Read + Write>(
    socket: &mut WebSocket<S>,
    budget: Duration,
) -> Result<Value, String> {
    let deadline = Instant::now() + budget;
    let mut transfer: Option<(String, usize, String, Vec<u8>, usize)> = None;
    loop {
        if Instant::now() > deadline {
            return Err("Companion reply timed out".into());
        }
        let message = match socket.read() {
            Ok(message) => message,
            Err(error) if timed_out(&error) => continue,
            Err(error) => return Err(error.to_string()),
        };
        match message {
            Message::Ping(_) | Message::Pong(_) => continue,
            Message::Close(_) => return Err("Companion connection closed".into()),
            Message::Text(text) => {
                let value: Value =
                    serde_json::from_str(&text).map_err(|_| "Invalid companion JSON")?;
                match value.get("type").and_then(Value::as_str) {
                    Some("transfer-start") => {
                        if transfer.is_some() {
                            return Err("Nested publication transfer".into());
                        }
                        let bytes = value["bytes"]
                            .as_u64()
                            .filter(|size| *size > 0 && *size <= TRANSFER_BYTES as u64)
                            .ok_or("Invalid transfer size")?
                            as usize;
                        let id = value["id"]
                            .as_str()
                            .filter(|id| id.len() <= 64)
                            .ok_or("Invalid transfer ID")?
                            .to_string();
                        let hash = value["sha256"]
                            .as_str()
                            .filter(|value| value.len() == 71)
                            .ok_or("Invalid transfer digest")?
                            .to_string();
                        transfer = Some((id, bytes, hash, Vec::with_capacity(bytes.min(65536)), 0));
                    }
                    Some("transfer-chunk") => {
                        let state = transfer.as_mut().ok_or("Transfer chunk without start")?;
                        if value["id"] != state.0 || value["index"].as_u64() != Some(state.4 as u64)
                        {
                            return Err("Out-of-order transfer chunk".into());
                        }
                        let encoded = value["base64"]
                            .as_str()
                            .filter(|value| value.len() <= 11000)
                            .ok_or("Invalid transfer chunk")?;
                        let chunk = STANDARD
                            .decode(encoded)
                            .map_err(|_| "Invalid transfer chunk encoding")?;
                        if chunk.is_empty() || state.3.len() + chunk.len() > state.1 {
                            return Err("Transfer exceeds declared size".into());
                        }
                        state.3.extend(chunk);
                        state.4 += 1;
                    }
                    Some("transfer-end") => {
                        let state = transfer.take().ok_or("Transfer end without start")?;
                        if value["id"] != state.0
                            || state.3.len() != state.1
                            || tls::sha256(&state.3) != state.2
                        {
                            return Err("Incomplete or inconsistent publication transfer".into());
                        }
                        return serde_json::from_slice(&state.3)
                            .map_err(|_| "Invalid transferred JSON".into());
                    }
                    _ => {
                        if transfer.is_some() {
                            return Err("Interleaved publication transfer".into());
                        }
                        return Ok(value);
                    }
                }
            }
            _ => return Err("Only bounded text companion frames are supported".into()),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServerConfig {
    pub bind: SocketAddr,
    pub advertised_host: String,
    pub discovery: bool,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub enabled: bool,
    pub endpoint: Option<String>,
    pub ca_spki_sha256: String,
    pub active_connections: usize,
    pub last_error: Option<String>,
    pub discovery: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairingPayload {
    pub version: u8,
    pub endpoint: String,
    pub host_id: String,
    pub ca_certificate: String,
    pub ca_spki_sha256: String,
    pub secret: String,
    pub expires_at_ms: u64,
}
struct Running {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
    discovery: Option<mdns_sd::ServiceDaemon>,
}
struct Inner {
    identity: NativeIdentity,
    status: ServerStatus,
    running: Option<Running>,
}
pub struct ServerRuntime {
    hub: Hub,
    path: PathBuf,
    inner: Mutex<Inner>,
    active: Arc<AtomicUsize>,
}
impl ServerRuntime {
    pub fn new(hub: Hub, data_dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&data_dir).map_err(|error| error.to_string())?;
        let path = data_dir.join("companion-identity.dpapi");
        let identity = NativeIdentity::load_or_create(
            &path,
            &hub.host_id(),
            vec!["localhost".into(), "127.0.0.1".into()],
        )?;
        let status = ServerStatus {
            ca_spki_sha256: identity.pin()?,
            discovery: "disabled".into(),
            ..Default::default()
        };
        Ok(Self {
            hub,
            path,
            inner: Mutex::new(Inner {
                identity,
                status,
                running: None,
            }),
            active: Arc::new(AtomicUsize::new(0)),
        })
    }
    pub fn start(&self, config: ServerConfig) -> Result<ServerStatus, String> {
        let mut inner = self.inner.lock().map_err(|_| "Listener lock poisoned")?;
        if inner.running.is_some() {
            return Err("Companion listener is already enabled".into());
        }
        let host = config.advertised_host.trim();
        if host.is_empty() || host.len() > 253 || host.contains(['/', '@', '?', '#', ' ']) {
            return Err("Choose an interface IP or valid local hostname".into());
        }
        let listener = TcpListener::bind(config.bind).map_err(|error| {
            format!(
                "Cannot bind companion interface/port {}: {error}",
                config.bind
            )
        })?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let address = listener.local_addr().map_err(|error| error.to_string())?;
        inner.identity = NativeIdentity::load_or_create(
            &self.path,
            &self.hub.host_id(),
            vec![host.into(), "localhost".into(), "127.0.0.1".into()],
        )?;
        let tls = inner.identity.server_config()?;
        let endpoint = if host.contains(':') {
            format!("wss://[{host}]:{}/companion", address.port())
        } else {
            format!("wss://{host}:{}/companion", address.port())
        };
        let mut discovery = None;
        let mut discovery_status = "disabled".to_string();
        if config.discovery {
            match discovery_advertisement(&self.hub.host_id(), host, address.port()) {
                Ok(daemon) => {
                    discovery = Some(daemon);
                    discovery_status = "advertising (untrusted hints only)".into();
                }
                Err(error) => {
                    discovery_status =
                        format!("unavailable: {error}; manual pairing remains available")
                }
            }
        }
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let hub = self.hub.clone();
        let active = self.active.clone();
        let attempts = Arc::new(Mutex::new(Attempts::new()));
        let handle = thread::Builder::new()
            .name("companion-listener".into())
            .spawn(move || {
                let mut workers: Vec<JoinHandle<()>> = Vec::new();
                while !stopped.load(Ordering::SeqCst) {
                    hub.tick(hub.now_ms());
                    match listener.accept() {
                        Ok((stream, _)) => {
                            if active.load(Ordering::SeqCst) >= 16 {
                                drop(stream);
                                continue;
                            }
                            active.fetch_add(1, Ordering::SeqCst);
                            let hub = hub.clone();
                            let tls = tls.clone();
                            let active = active.clone();
                            let stop = stopped.clone();
                            let attempts = attempts.clone();
                            workers.push(thread::spawn(move || {
                                let id = uuid::Uuid::new_v4().to_string();
                                let _result =
                                    serve_connection(stream, tls, &hub, &id, &stop, &attempts);
                                #[cfg(test)]
                                if let Err(error) = _result {
                                    eprintln!("companion peer ended: {error}");
                                }
                                hub.disconnect_connection(&id, hub.now_ms());
                                active.fetch_sub(1, Ordering::SeqCst);
                            }));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(20))
                        }
                        Err(_) => break,
                    }
                    let mut index = 0;
                    while index < workers.len() {
                        if workers[index].is_finished() {
                            let _ = workers.swap_remove(index).join();
                        } else {
                            index += 1;
                        }
                    }
                }
                for worker in workers {
                    let _ = worker.join();
                }
                hub.tick(hub.now_ms());
            })
            .map_err(|error| error.to_string())?;
        inner.status = ServerStatus {
            enabled: true,
            endpoint: Some(endpoint),
            ca_spki_sha256: inner.identity.pin()?,
            active_connections: self.active.load(Ordering::SeqCst),
            last_error: None,
            discovery: discovery_status,
        };
        inner.running = Some(Running {
            stop,
            handle,
            discovery,
        });
        Ok(inner.status.clone())
    }
    pub fn stop(&self) {
        let running = {
            let Ok(mut inner) = self.inner.lock() else {
                return;
            };
            inner.status.enabled = false;
            inner.status.endpoint = None;
            inner.status.discovery = "disabled".into();
            inner.running.take()
        };
        if let Some(running) = running {
            running.stop.store(true, Ordering::SeqCst);
            self.hub.disconnect_all();
            if let Some(discovery) = running.discovery {
                let _ = discovery.shutdown();
            }
            let _ = running.handle.join();
        }
    }
    pub fn status(&self) -> ServerStatus {
        let Ok(inner) = self.inner.lock() else {
            return ServerStatus {
                last_error: Some("Listener lock poisoned".into()),
                ..Default::default()
            };
        };
        let mut value = inner.status.clone();
        value.active_connections = self.active.load(Ordering::SeqCst);
        value
    }
    pub fn pairing_payload(
        &self,
        secret: &str,
        expires_at_ms: u64,
    ) -> Result<PairingPayload, String> {
        let inner = self.inner.lock().map_err(|_| "Listener lock poisoned")?;
        if secret.len() > 256 {
            return Err("Invalid pairing offer".into());
        }
        Ok(PairingPayload {
            version: 1,
            endpoint: inner
                .status
                .endpoint
                .clone()
                .filter(|_| inner.status.enabled)
                .ok_or("Enable the listener before creating a pairing offer")?,
            host_id: self.hub.host_id(),
            ca_certificate: inner.identity.ca_base64(),
            ca_spki_sha256: inner.identity.pin()?,
            secret: secret.into(),
            expires_at_ms,
        })
    }
}
impl Drop for ServerRuntime {
    fn drop(&mut self) {
        self.stop();
    }
}
fn serve_connection(
    stream: TcpStream,
    tls: Arc<rustls::ServerConfig>,
    hub: &Hub,
    id: &str,
    stop: &AtomicBool,
    attempts: &Mutex<Attempts>,
) -> Result<(), String> {
    let peer = stream.peer_addr().map_err(|error| error.to_string())?.ip();
    stream
        .set_nonblocking(false)
        .map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_millis(500)))
        .map_err(|error| error.to_string())?;
    stream
        .set_nodelay(true)
        .map_err(|error| error.to_string())?;
    let mut connection = ServerConnection::new(tls).map_err(|error| error.to_string())?;
    connection.set_buffer_limit(Some(65536));
    let stream = BudgetIo::new(StreamOwned::new(connection, DeadlineTcp::new(stream)));
    let callback = |request: &Request, mut response: Response| -> Result<Response, ErrorResponse> {
        if request.uri().path() != "/companion"
            || request.uri().query().is_some()
            || request.headers().contains_key("origin")
            || request
                .headers()
                .get("sec-websocket-protocol")
                .and_then(|value| value.to_str().ok())
                != Some(SUBPROTOCOL)
        {
            return Err(tungstenite::http::Response::builder()
                .status(400)
                .body(Some("Unsupported companion endpoint/protocol".into()))
                .expect("valid rejection"));
        }
        response.headers_mut().insert(
            "sec-websocket-protocol",
            SUBPROTOCOL.parse().expect("static protocol"),
        );
        Ok(response)
    };
    let mut socket =
        tungstenite::accept_hdr_with_config(stream, callback, Some(websocket_config()))
            .map_err(|error| error.to_string())?;
    socket.get_mut().upgraded();
    socket.get_mut().inner.sock.upgraded();
    socket
        .get_mut()
        .inner
        .sock
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|error| error.to_string())?;
    let mut window = Instant::now();
    let mut frames = 0;
    let mut bytes = 0;
    let mut last_frame = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        if last_frame.elapsed() > Duration::from_secs(5) {
            break;
        }
        let message = match socket.read() {
            Ok(message) => message,
            Err(error) if timed_out(&error) => continue,
            Err(error) => return Err(error.to_string()),
        };
        if stop.load(Ordering::SeqCst) {
            break;
        }
        if window.elapsed() >= Duration::from_secs(1) {
            window = Instant::now();
            frames = 0;
            bytes = 0;
        }
        frames += 1;
        bytes += message.len();
        if frames > 128 || bytes > 2 * 1024 * 1024 {
            return Err("Companion frame/rate budget exceeded".into());
        }
        match message {
            Message::Text(text) => {
                last_frame = Instant::now();
                let frame: Value =
                    serde_json::from_str(&text).map_err(|_| "Invalid companion JSON")?;
                if frame["type"] == "pair"
                    && !attempts
                        .lock()
                        .map_err(|_| "Admission lock poisoned")?
                        .allow(peer)
                {
                    send_json(
                        &mut socket,
                        &json!({"ok":false,"error":"Pairing/authentication attempt budget exceeded"}),
                    )?;
                    return Err("Pre-authentication attempt budget exceeded".into());
                }
                let is_auth = frame["type"] == "auth";
                let response = hub.handle_wire(frame, id, hub.now_ms());
                if is_auth
                    && response["ok"] != true
                    && !attempts
                        .lock()
                        .map_err(|_| "Admission lock poisoned")?
                        .allow(peer)
                {
                    send_json(
                        &mut socket,
                        &json!({"ok":false,"error":"Authentication attempt budget exceeded"}),
                    )?;
                    return Err("Authentication attempt budget exceeded".into());
                }
                send_json(&mut socket, &response)?;
            }
            Message::Ping(_) | Message::Pong(_) => {}
            Message::Close(_) => break,
            _ => return Err("Companion endpoint accepts text JSON only".into()),
        }
    }
    let _ = socket.close(None);
    Ok(())
}
fn discovery_advertisement(
    host_id: &str,
    host: &str,
    port: u16,
) -> Result<mdns_sd::ServiceDaemon, String> {
    let daemon = mdns_sd::ServiceDaemon::new().map_err(|error| error.to_string())?;
    let hostname = format!(
        "osg-{}.local.",
        host_id
            .chars()
            .filter(|value| value.is_ascii_alphanumeric() || *value == '-')
            .take(48)
            .collect::<String>()
    );
    let address = host
        .parse::<IpAddr>()
        .map(|ip| ip.to_string())
        .unwrap_or_default();
    let properties = [
        ("hostId", host_id),
        ("version", "1"),
        ("path", "/companion"),
        ("trust", "out-of-band-required"),
    ];
    let info = mdns_sd::ServiceInfo::new(
        SERVICE_TYPE,
        &format!("Cockpit-{}", &host_id[..host_id.len().min(12)]),
        &hostname,
        address.as_str(),
        port,
        &properties[..],
    )
    .map_err(|error| error.to_string())?;
    daemon
        .register(if address.is_empty() {
            info.enable_addr_auto()
        } else {
            info
        })
        .map_err(|error| error.to_string())?;
    Ok(daemon)
}
pub fn local_interfaces() -> Result<Value, String> {
    let interfaces = if_addrs::get_if_addrs().map_err(|error| error.to_string())?;
    let values:Vec<Value>=interfaces.into_iter().filter(|item|!item.ip().is_unspecified()&&!item.ip().is_multicast()).map(|item|json!({"name":item.name,"ip":item.ip().to_string(),"loopback":item.is_loopback()})).collect();
    Ok(json!(values))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Error, ErrorKind};
    use tungstenite::protocol::Role;
    #[derive(Default)]
    struct MemoryIo {
        input: Cursor<Vec<u8>>,
        output: Vec<u8>,
    }
    impl Read for MemoryIo {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            let count = self.input.read(bytes)?;
            if count == 0 {
                Err(Error::new(ErrorKind::UnexpectedEof, "end of fixture"))
            } else {
                Ok(count)
            }
        }
    }
    impl Write for MemoryIo {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.output.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    fn encoded(values: Vec<Message>) -> WebSocket<MemoryIo> {
        let mut sender =
            WebSocket::from_raw_socket(MemoryIo::default(), Role::Server, Some(websocket_config()));
        for value in values {
            sender.send(value).unwrap();
        }
        WebSocket::from_raw_socket(
            MemoryIo {
                input: Cursor::new(sender.get_ref().output.clone()),
                output: Vec::new(),
            },
            Role::Client,
            Some(websocket_config()),
        )
    }
    fn text(value: Value) -> Message {
        Message::Text(value.to_string().into())
    }
    #[test]
    fn transfer_decoder_rejects_missing_duplicate_interleaved_and_corrupted_chunks() {
        let start = json!({"type":"transfer-start","id":"x","bytes":2,"sha256":tls::sha256(b"{}")});
        for frames in [
            vec![text(json!({"type":"transfer-chunk"}))],
            vec![text(json!({"type":"transfer-end"}))],
            vec![text(start.clone()), text(start.clone())],
            vec![text(start.clone()), text(json!({"ok":true}))],
            vec![text(
                json!({"type":"transfer-start","id":"x","bytes":TRANSFER_BYTES+1,"sha256":"x"}),
            )],
            vec![
                text(start.clone()),
                text(json!({"type":"transfer-chunk","id":"x","index":1,"base64":"e30="})),
            ],
            vec![
                text(start.clone()),
                text(json!({"type":"transfer-chunk","id":"x","index":0,"base64":"%%%%"})),
            ],
            vec![
                text(start.clone()),
                text(json!({"type":"transfer-chunk","id":"x","index":0,"base64":"e30ge30="})),
            ],
            vec![
                text(start.clone()),
                text(json!({"type":"transfer-end","id":"x"})),
            ],
            vec![Message::Binary(vec![1, 2].into())],
            vec![Message::Text("not json".into())],
            vec![Message::Close(None)],
        ] {
            assert!(receive_json(&mut encoded(frames), Duration::from_millis(50)).is_err());
        }
        let mut oversized = encoded(Vec::new());
        assert!(send_json(
            &mut oversized,
            &json!({"huge":"x".repeat(TRANSFER_BYTES+1)})
        )
        .is_err());
    }
    #[test]
    fn admission_budgets_expire_and_handshake_buffer_is_bounded() {
        let first: IpAddr = "127.0.0.1".parse().unwrap();
        let second: IpAddr = "127.0.0.2".parse().unwrap();
        let mut attempts = Attempts::new();
        for _ in 0..5 {
            assert!(attempts.allow(first));
        }
        assert!(!attempts.allow(first));
        for _ in 0..5 {
            assert!(attempts.allow(second));
        }
        attempts.total = 20;
        assert!(!attempts.allow("127.0.0.3".parse().unwrap()));
        attempts.since = Instant::now() - Duration::from_secs(61);
        assert!(attempts.allow(first));
        let mut io = BudgetIo::new(MemoryIo {
            input: Cursor::new(vec![1; 32769]),
            output: Vec::new(),
        });
        assert_eq!(io.read(&mut vec![0; 32768]).unwrap(), 32768);
        assert!(io.read(&mut [0]).is_err());
        io.upgraded();
        assert_eq!(io.read(&mut [0]).unwrap(), 1);
        io.write_all(b"x").unwrap();
        io.flush().unwrap();
        assert_eq!(io.inner.output, b"x");
        assert!(local_interfaces().unwrap().is_array());
    }
}
