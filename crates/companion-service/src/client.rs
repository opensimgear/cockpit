//! Native client: credentials stay sealed, epochs reject stale UI, and UI stalls close the lease.
use crate::{
    server::{
        receive_json, send_json, websocket_config, BudgetIo, DeadlineTcp, PairingPayload,
        SERVICE_TYPE, SUBPROTOCOL,
    },
    tls,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rustls::{ClientConnection, StreamOwned};
use rustls_pki_types::ServerName;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tungstenite::{client::IntoClientRequest, WebSocket};
use zeroize::Zeroize;
type Socket = WebSocket<BudgetIo<StreamOwned<ClientConnection, DeadlineTcp>>>;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedCredential {
    companion_id: String,
    name: String,
    token: String,
    endpoint: String,
    host_id: String,
    ca_certificate: String,
    ca_spki_sha256: String,
    assignments: Value,
}
impl Drop for SavedCredential {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientStatus {
    pub phase: String,
    pub protected_store_available: bool,
    pub busy: bool,
    pub epoch: u64,
    pub paired_host: Option<Value>,
    pub request_id: Option<String>,
    pub assignments: Value,
    pub session: Option<Value>,
    pub publication_hash: Option<String>,
    pub last_error: Option<String>,
    pub ack: Option<Value>,
    pub feedback: Vec<Value>,
    pub feedback_revision: u64,
}
impl Default for ClientStatus {
    fn default() -> Self {
        Self {
            phase: "unpaired".into(),
            protected_store_available: cfg!(windows),
            busy: false,
            epoch: 0,
            paired_host: None,
            request_id: None,
            assignments: json!([]),
            session: None,
            publication_hash: None,
            last_error: None,
            ack: None,
            feedback: Vec::new(),
            feedback_revision: 0,
        }
    }
}
struct State {
    status: ClientStatus,
    credential: Option<SavedCredential>,
    publication: Option<Value>,
    sender: Option<SyncSender<QueuedInput>>,
    queued_transitions: Arc<AtomicUsize>,
    feedback_sequence: Option<String>,
    feedback_received_ms: u64,
}
// A reservation lives through the network ACK and drops on every error/cancellation path.
// Each session gets its own counter, so stale worker cleanup cannot subtract a new epoch.
struct InputReservation {
    counter: Arc<AtomicUsize>,
    count: usize,
}
impl InputReservation {
    fn reserve(counter: Arc<AtomicUsize>, count: usize) -> Result<Self, String> {
        counter
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                current.checked_add(count).filter(|next| *next <= 256)
            })
            .map_err(|_| "Native input queue exceeds 256 transitions".to_string())?;
        Ok(Self { counter, count })
    }
}
impl Drop for InputReservation {
    fn drop(&mut self) {
        self.counter.fetch_sub(self.count, Ordering::SeqCst);
    }
}
struct QueuedInput {
    frame: Value,
    _reservation: InputReservation,
}
struct Shared {
    state: Mutex<State>,
    cancel: AtomicBool,
    running: AtomicBool,
    epoch: AtomicU64,
    ui_tick: AtomicU64,
    started: Instant,
    path: PathBuf,
    companion_id: String,
}
pub struct ClientRuntime {
    shared: Arc<Shared>,
}
impl ClientRuntime {
    pub fn open(data_dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&data_dir).map_err(|error| error.to_string())?;
        let path = data_dir.join("companion-client.dpapi");
        let credential = if path.exists() {
            let bytes = tls::read_sealed(&path, 65536)?;
            if bytes.len() > 65536 {
                return Err("Stored client credential exceeds budget".into());
            }
            let mut plain = crate::persistence::unseal(&bytes)?;
            let parsed = serde_json::from_slice::<SavedCredential>(&plain)
                .map_err(|_| "Stored companion credential is invalid".to_string());
            plain.zeroize();
            Some(parsed?)
        } else {
            None
        };
        let companion_id = credential
            .as_ref()
            .map(|value| value.companion_id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut status = ClientStatus::default();
        if let Some(value) = &credential {
            status.phase = "paired".into();
            status.paired_host = Some(public_host(value));
            status.assignments = value.assignments.clone();
        }
        Ok(Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    status,
                    credential,
                    publication: None,
                    sender: None,
                    queued_transitions: Arc::new(AtomicUsize::new(0)),
                    feedback_sequence: None,
                    feedback_received_ms: 0,
                }),
                cancel: AtomicBool::new(false),
                running: AtomicBool::new(false),
                epoch: AtomicU64::new(0),
                ui_tick: AtomicU64::new(0),
                started: Instant::now(),
                path,
                companion_id,
            }),
        })
    }
    pub fn status(&self) -> ClientStatus {
        self.shared
            .state
            .lock()
            .map(|state| {
                let mut status = state.status.clone();
                status.busy = self.shared.running.load(Ordering::SeqCst);
                let elapsed = (self.shared.started.elapsed().as_millis() as u64)
                    .saturating_sub(state.feedback_received_ms);
                for sample in &mut status.feedback {
                    if let Some(age) = sample["sourceAgeMs"].as_u64() {
                        sample["sourceAgeMs"] = json!(age.saturating_add(elapsed));
                    }
                }
                status
            })
            .unwrap_or_else(|_| ClientStatus {
                phase: "error".into(),
                last_error: Some("Client state lock poisoned".into()),
                ..Default::default()
            })
    }
    pub fn invalidate(&self, reason: &str) {
        self.shared.cancel.store(true, Ordering::SeqCst);
        let epoch = self.shared.epoch.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut state) = self.shared.state.lock() {
            state.status.epoch = epoch;
            state.status.phase = if state.credential.is_some() {
                "disconnected"
            } else {
                "unpaired"
            }
            .into();
            state.status.last_error = Some(reason.into());
            clear_session_status(&mut state);
            state.publication = None;
            state.sender = None;
        }
    }
    pub fn command(&self, request: Value) -> Result<Value, String> {
        let command = request["type"]
            .as_str()
            .ok_or("Missing companion client command")?;
        if matches!(command, "pair-start" | "connect" | "refresh-assignments") && !cfg!(windows) {
            return Err(
                "Native pairing is disabled until protected storage is qualified on this platform"
                    .into(),
            );
        }
        match command {
            "status" => serde_json::to_value(self.status()).map_err(|error| error.to_string()),
            "publication" => {
                let state = self
                    .shared
                    .state
                    .lock()
                    .map_err(|_| "Client state lock poisoned")?;
                state
                    .publication
                    .clone()
                    .ok_or_else(|| "Connect to an assigned publication first".into())
            }
            "pair-start" => {
                let payload: PairingPayload = if let Some(text) = request["payload"].as_str() {
                    if text.len() > 8192 {
                        return Err("Pairing payload exceeds budget".into());
                    }
                    serde_json::from_str(text)
                } else {
                    serde_json::from_value(request["payload"].clone())
                }
                .map_err(|_| "Invalid out-of-band pairing payload")?;
                if payload.version != 1
                    || payload.secret.is_empty()
                    || payload.secret.len() > 256
                    || payload.host_id.len() > 64
                {
                    return Err("Unsupported pairing payload".into());
                }
                tls::verified_ca(&payload.ca_certificate, &payload.ca_spki_sha256)?;
                let name = request["name"]
                    .as_str()
                    .filter(|name| !name.trim().is_empty() && name.len() <= 80)
                    .ok_or("Choose a companion display name")?
                    .to_string();
                self.start_worker("pairing", move |shared, epoch| {
                    pair_worker(shared, epoch, payload, name)
                })?;
                Ok(json!({"ok":true}))
            }
            "refresh-assignments" => {
                let credential = self
                    .shared
                    .state
                    .lock()
                    .map_err(|_| "Client state lock poisoned")?
                    .credential
                    .clone()
                    .ok_or("Pair with a host first")?;
                self.start_worker("checking-assignments", move |shared, epoch| {
                    refresh_worker(shared, epoch, credential)
                })?;
                Ok(json!({"ok":true}))
            }
            "connect" => {
                let instance = request["instanceId"]
                    .as_str()
                    .filter(|id| !id.is_empty() && id.len() <= 64)
                    .ok_or("Choose an assigned controller instance")?
                    .to_string();
                if request["takeover"] == true {
                    return Err("Takeover requires an explicit Cockpit host operator action".into());
                }
                let takeover = false;
                let credential = self
                    .shared
                    .state
                    .lock()
                    .map_err(|_| "Client state lock poisoned")?
                    .credential
                    .clone()
                    .ok_or("Pair with a host first")?;
                self.start_worker("connecting", move |shared, epoch| {
                    session_worker(shared, epoch, credential, instance, takeover)
                })?;
                Ok(json!({"ok":true}))
            }
            "arm" => {
                let epoch = checked_epoch(&self.shared, &request)?;
                let mut state = self
                    .shared
                    .state
                    .lock()
                    .map_err(|_| "Client state lock poisoned")?;
                if state.status.phase != "ready" || state.status.epoch != epoch {
                    return Err("A fresh baseline acknowledgement is required before Start".into());
                }
                state.status.phase = "armed".into();
                self.shared.ui_tick.store(
                    self.shared.started.elapsed().as_millis() as u64,
                    Ordering::SeqCst,
                );
                Ok(json!({"ok":true,"epoch":epoch}))
            }
            "keepalive" => {
                checked_epoch(&self.shared, &request)?;
                self.shared.ui_tick.store(
                    self.shared.started.elapsed().as_millis() as u64,
                    Ordering::SeqCst,
                );
                Ok(json!({"ok":true}))
            }
            "input" => {
                checked_epoch(&self.shared, &request)?;
                let state = self
                    .shared
                    .state
                    .lock()
                    .map_err(|_| "Client state lock poisoned")?;
                if state.status.phase != "armed" {
                    return Err("Remote panel is disarmed".into());
                }
                let changes = request["changes"]
                    .as_array()
                    .filter(|changes| !changes.is_empty() && changes.len() <= 64)
                    .ok_or("Input batch exceeds control budget")?;
                let sender = state
                    .sender
                    .clone()
                    .ok_or("No connected companion session")?;
                let reservation =
                    InputReservation::reserve(state.queued_transitions.clone(), changes.len());
                let reservation = match reservation {
                    Ok(value) => value,
                    Err(error) => {
                        drop(state);
                        self.invalidate(&error);
                        return Err(error);
                    }
                };
                let frame = QueuedInput {
                    frame: json!({"changes":changes,"createdMs":self.shared.started.elapsed().as_millis() as u64}),
                    _reservation: reservation,
                };
                drop(state);
                if sender.try_send(frame).is_err() {
                    self.invalidate("Client input queue overflow; reconnect and Start explicitly");
                    return Err("Input queue is full; session stopped".into());
                }
                Ok(json!({"ok":true}))
            }
            "disconnect" | "neutralize" => {
                self.invalidate("Stopped by operator or lifecycle");
                Ok(json!({"ok":true}))
            }
            "forget" => {
                self.invalidate("Pairing removed");
                let mut state = self
                    .shared
                    .state
                    .lock()
                    .map_err(|_| "Client state lock poisoned")?;
                state.credential = None;
                state.status = ClientStatus {
                    epoch: self.shared.epoch.load(Ordering::SeqCst),
                    ..Default::default()
                };
                if self.shared.path.exists() {
                    std::fs::remove_file(&self.shared.path).map_err(|error| error.to_string())?;
                }
                Ok(json!({"ok":true}))
            }
            "discover" => discover(Duration::from_millis(
                request["durationMs"]
                    .as_u64()
                    .unwrap_or(1200)
                    .clamp(200, 3000),
            )),
            _ => Err("Unsupported companion client command".into()),
        }
    }
    fn start_worker(
        &self,
        phase: &str,
        work: impl FnOnce(Arc<Shared>, u64) -> Result<(), String> + Send + 'static,
    ) -> Result<(), String> {
        if self
            .shared
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(
                "A connection or pairing operation is still active; Stop and wait before retrying"
                    .into(),
            );
        }
        self.shared.cancel.store(false, Ordering::SeqCst);
        let epoch = self.shared.epoch.fetch_add(1, Ordering::SeqCst) + 1;
        {
            let mut state = self
                .shared
                .state
                .lock()
                .map_err(|_| "Client state lock poisoned")?;
            state.status.phase = phase.into();
            state.status.epoch = epoch;
            state.status.last_error = None;
            clear_session_status(&mut state);
            state.status.request_id = None;
            state.publication = None;
        }
        let shared = self.shared.clone();
        thread::spawn(move || {
            let result = work(shared.clone(), epoch);
            if let Err(error) = result {
                set_state(&shared, epoch, |state| {
                    state.status.phase = "error".into();
                    state.status.last_error = Some(error);
                    clear_session_status(state);
                    state.sender = None;
                });
            }
            shared.running.store(false, Ordering::SeqCst);
        });
        Ok(())
    }
}
impl Drop for ClientRuntime {
    fn drop(&mut self) {
        self.invalidate("Companion application exited");
    }
}
fn clear_session_status(state: &mut State) {
    state.status.session = None;
    state.status.publication_hash = None;
    state.status.ack = None;
    state.status.feedback.clear();
    state.status.feedback_revision = state.status.feedback_revision.saturating_add(1);
    state.feedback_sequence = None;
    state.feedback_received_ms = 0;
}
fn apply_ack(state: &mut State, reply: Value, now_ms: u64) {
    if let (Some(sequence), Some(values)) = (
        reply["feedbackSequence"].as_str(),
        reply["feedback"].as_array(),
    ) {
        if state.feedback_sequence.as_deref() != Some(sequence) {
            state.feedback_sequence = Some(sequence.to_owned());
            state.feedback_received_ms = now_ms;
            state.status.feedback = values.clone();
            state.status.feedback_revision = state.status.feedback_revision.saturating_add(1);
        }
    }
    state.status.ack = Some(reply);
}
fn public_host(value: &SavedCredential) -> Value {
    json!({"hostId":value.host_id,"endpoint":value.endpoint,"caSpkiSha256":value.ca_spki_sha256,"companionId":value.companion_id,"name":value.name})
}
fn checked_epoch(shared: &Shared, request: &Value) -> Result<u64, String> {
    let epoch = request["epoch"]
        .as_u64()
        .ok_or("Missing native session epoch")?;
    if epoch != shared.epoch.load(Ordering::SeqCst) {
        return Err("Stale native companion epoch".into());
    }
    Ok(epoch)
}
fn set_state(shared: &Shared, epoch: u64, apply: impl FnOnce(&mut State)) {
    if shared.epoch.load(Ordering::SeqCst) != epoch {
        return;
    }
    if let Ok(mut state) = shared.state.lock() {
        if state.status.epoch == epoch {
            apply(&mut state);
        }
    }
}
fn current(shared: &Shared, epoch: u64) -> bool {
    !shared.cancel.load(Ordering::SeqCst) && shared.epoch.load(Ordering::SeqCst) == epoch
}
pub(crate) fn connect(endpoint: &str, ca: &str, pin: &str) -> Result<Socket, String> {
    let config = tls::pinned_client_config(ca, pin)?; // Out-of-band CA pin precedes DNS/socket/authentication.
    if endpoint.len() > 512 {
        return Err("Endpoint exceeds budget".into());
    }
    let uri: tungstenite::http::Uri = endpoint.parse().map_err(|_| "Invalid WSS endpoint")?;
    if uri.scheme_str() != Some("wss")
        || uri.path() != "/companion"
        || uri.query().is_some()
        || uri.authority().is_none()
        || uri
            .authority()
            .is_some_and(|value| value.as_str().contains('@'))
    {
        return Err("Only a narrow wss://host:port/companion endpoint is supported".into());
    }
    let host = uri
        .host()
        .ok_or("Missing WSS host")?
        .trim_matches(['[', ']'])
        .to_string();
    let port = uri.port_u16().unwrap_or(443);
    let addresses = resolve_lan(&host, port)?;
    let mut connected = None;
    for address in &addresses {
        if let Ok(stream) = TcpStream::connect_timeout(&address, Duration::from_secs(2)) {
            connected = Some(stream);
            break;
        }
    }
    let stream = connected.ok_or(
        "Cannot reach a LAN/loopback companion endpoint; check interface, port and firewall",
    )?;
    stream
        .set_nodelay(true)
        .map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_millis(500)))
        .map_err(|error| error.to_string())?;
    let name = ServerName::try_from(host).map_err(|_| "Invalid TLS server name")?;
    let mut connection = ClientConnection::new(config, name).map_err(|error| error.to_string())?;
    connection.set_buffer_limit(Some(65536));
    let stream = BudgetIo::new(StreamOwned::new(connection, DeadlineTcp::new(stream)));
    let mut request = endpoint
        .into_client_request()
        .map_err(|error| error.to_string())?;
    request.headers_mut().insert(
        "sec-websocket-protocol",
        SUBPROTOCOL.parse().expect("static protocol"),
    );
    let (mut socket, response) =
        tungstenite::client::client_with_config(request, stream, Some(websocket_config()))
            .map_err(|error| format!("Pinned TLS/WebSocket connection failed: {error}"))?;
    if response
        .headers()
        .get("sec-websocket-protocol")
        .and_then(|value| value.to_str().ok())
        != Some(SUBPROTOCOL)
    {
        return Err("Host did not negotiate the companion protocol".into());
    }
    socket.get_mut().upgraded();
    socket.get_mut().inner.sock.upgraded();
    socket
        .get_mut()
        .inner
        .sock
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|error| error.to_string())?;
    Ok(socket)
}
// System DNS can block independently of socket deadlines. Limit abandoned resolutions
// globally and bound how long a native client operation waits for one.
static DNS_WORKERS: AtomicUsize = AtomicUsize::new(0);
struct DnsReservation;
impl Drop for DnsReservation {
    fn drop(&mut self) {
        DNS_WORKERS.fetch_sub(1, Ordering::SeqCst);
    }
}
fn resolve_lan(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(if lan_address(ip) {
            vec![SocketAddr::new(ip, port)]
        } else {
            Vec::new()
        });
    }
    DNS_WORKERS
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
            (count < 4).then_some(count + 1)
        })
        .map_err(|_| "DNS resolver is busy; wait before retrying".to_string())?;
    let host = host.to_owned();
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _slot = DnsReservation;
        let result = (host.as_str(), port)
            .to_socket_addrs()
            .map(|addresses| {
                addresses
                    .filter(|address| lan_address(address.ip()))
                    .take(4)
                    .collect::<Vec<_>>()
            })
            .map_err(|error| format!("Cannot resolve companion host: {error}"));
        let _ = sender.send(result);
    });
    receiver
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| "Companion host resolution exceeded two seconds".to_string())?
}
fn lan_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
        IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local(),
    }
}
fn request(socket: &mut Socket, value: Value, budget: Duration) -> Result<Value, String> {
    send_json(socket, &value)?;
    let reply = receive_json(socket, budget)?;
    if reply["ok"] != true {
        return Err(reply["error"]
            .as_str()
            .unwrap_or("Host rejected companion request")
            .into());
    }
    Ok(reply)
}
fn refresh_worker(
    shared: Arc<Shared>,
    epoch: u64,
    mut credential: SavedCredential,
) -> Result<(), String> {
    let mut socket = connect(
        &credential.endpoint,
        &credential.ca_certificate,
        &credential.ca_spki_sha256,
    )?;
    if !current(&shared, epoch) {
        return Ok(());
    }
    let reply = request(
        &mut socket,
        json!({"type":"auth","token":credential.token}),
        Duration::from_secs(3),
    )?;
    if reply["hostId"] != credential.host_id || reply["companionId"] != credential.companion_id {
        return Err("Assigned host identity mismatch".into());
    }
    credential.assignments = reply["assignments"].clone();
    if current(&shared, epoch) {
        let mut state = shared
            .state
            .lock()
            .map_err(|_| "Client state lock poisoned")?;
        if state.status.epoch == epoch {
            let mut plain = serde_json::to_vec(&credential).map_err(|error| error.to_string())?;
            let sealed = crate::persistence::seal(&plain)?;
            plain.zeroize();
            crate::persistence::atomic_write(&shared.path, &sealed)?;
            state.status.phase = "paired".into();
            state.status.assignments = credential.assignments.clone();
            state.credential = Some(credential);
        }
    }
    let _ = socket.close(None);
    Ok(())
}
fn session_request(socket: &mut Socket, frame: Value, budget: Duration) -> Result<Value, String> {
    let reply = request(socket, frame.clone(), budget)?;
    for key in ["clientId", "sessionId", "generation"] {
        if reply[key] != frame[key] {
            return Err("Stale or inconsistent native session acknowledgement".into());
        }
    }
    if frame["type"] != "close" && reply["sequence"] != frame["sequence"] {
        return Err("Acknowledgement sequence differs from submitted frame".into());
    }
    if reply["ackStage"] != "source-accepted" {
        return Err("Host did not acknowledge source acceptance".into());
    }
    Ok(reply)
}
fn pair_worker(
    shared: Arc<Shared>,
    epoch: u64,
    mut payload: PairingPayload,
    name: String,
) -> Result<(), String> {
    let secret = zeroize::Zeroizing::new(std::mem::take(&mut payload.secret));
    let mut socket = connect(
        &payload.endpoint,
        &payload.ca_certificate,
        &payload.ca_spki_sha256,
    )?;
    if !current(&shared, epoch) {
        return Ok(());
    }
    let mut reply = request(
        &mut socket,
        json!({"type":"pair","secret":secret.as_str(),"companionId":shared.companion_id,"name":name}),
        Duration::from_secs(3),
    )?;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if !current(&shared, epoch) {
            let _ = socket.close(None);
            return Ok(());
        }
        if reply["type"] == "paired" {
            if reply["hostId"] != payload.host_id || reply["companionId"] != shared.companion_id {
                return Err(
                    "Pairing approval identity does not match the pinned host/device".into(),
                );
            }
            let token = reply["token"]
                .as_str()
                .filter(|token| !token.is_empty() && token.len() <= 512)
                .ok_or("Host returned invalid native credential")?
                .to_string();
            let credential = SavedCredential {
                companion_id: shared.companion_id.clone(),
                name,
                token,
                endpoint: payload.endpoint,
                host_id: payload.host_id,
                ca_certificate: payload.ca_certificate,
                ca_spki_sha256: payload.ca_spki_sha256,
                assignments: reply["assignments"].clone(),
            };
            let mut plain = serde_json::to_vec(&credential).map_err(|error| error.to_string())?;
            let sealed = crate::persistence::seal(&plain)?;
            plain.zeroize();
            if !current(&shared, epoch) {
                return Ok(());
            }
            let mut state = shared
                .state
                .lock()
                .map_err(|_| "Client state lock poisoned")?;
            if state.status.epoch != epoch {
                return Ok(());
            }
            crate::persistence::atomic_write(&shared.path, &sealed)?;
            state.status.phase = "paired".into();
            state.status.paired_host = Some(public_host(&credential));
            state.status.assignments = credential.assignments.clone();
            state.status.request_id = None;
            state.credential = Some(credential);
            let _ = socket.close(None);
            return Ok(());
        }
        let id = reply["requestId"]
            .as_str()
            .filter(|id| id.len() <= 64)
            .ok_or("Host did not return a pairing request ID")?
            .to_string();
        set_state(&shared, epoch, |state| {
            state.status.phase = "awaiting-approval".into();
            state.status.request_id = Some(id.clone());
        });
        if Instant::now() > deadline {
            return Err("Pairing approval expired; create a new offer".into());
        }
        thread::sleep(Duration::from_millis(400));
        if !current(&shared, epoch) {
            return Ok(());
        }
        reply = request(
            &mut socket,
            json!({"type":"pair-status","requestId":id,"secret":secret.as_str()}),
            Duration::from_secs(3),
        )?;
    }
}
#[derive(Clone)]
struct Primitive {
    kind: String,
    min: f64,
    fallback: Value,
}
fn verify_publication(publication: &Value) -> Result<(Value, BTreeMap<String, Primitive>), String> {
    let canonical = publication["canonicalDocument"]
        .as_str()
        .filter(|value| value.len() <= 1024 * 1024)
        .ok_or("Publication document exceeds budget")?;
    let hash = publication["publicationHash"]
        .as_str()
        .ok_or("Missing publication digest")?;
    if tls::sha256(canonical.as_bytes()) != hash {
        return Err("Published document bytes do not match their authenticated hash".into());
    }
    let document: Value =
        serde_json::from_str(canonical).map_err(|_| "Published document is invalid JSON")?;
    if document["schemaVersion"] != 2 || document["ui"]["schemaVersion"] != 1 {
        return Err("Unsupported published document version".into());
    }
    let controls = document["ui"]["controls"]
        .as_array()
        .filter(|values| !values.is_empty() && values.len() <= 128)
        .ok_or("Invalid publication control budget")?;
    let mut primitives = BTreeMap::new();
    for control in controls {
        let id = control["id"]
            .as_str()
            .filter(|value| !value.is_empty() && value.len() <= 64)
            .ok_or("Invalid control ID")?;
        let kind = control["kind"].as_str().ok_or("Invalid control kind")?;
        let (min, fallback) = match kind {
            "button" => (0.0, json!(false)),
            "hat" => (-1.0, json!(-1)),
            "axis" => {
                let min = if control["axisMode"] == "unipolar" {
                    0.0
                } else if control["axisMode"] == "bipolar" {
                    -1.0
                } else {
                    return Err("Invalid axis range".into());
                };
                let value = control["fallback"]
                    .as_f64()
                    .filter(|value| value.is_finite() && *value >= min && *value <= 1.0)
                    .ok_or("Invalid axis fallback")?;
                (min, json!(value))
            }
            _ => return Err("Unsupported input primitive".into()),
        };
        if primitives
            .insert(
                id.into(),
                Primitive {
                    kind: kind.into(),
                    min,
                    fallback,
                },
            )
            .is_some()
        {
            return Err("Duplicate publication control".into());
        }
    }
    let manifest = document["ui"]["assets"]
        .as_array()
        .ok_or("Missing asset manifest")?;
    let assets = publication["assets"]
        .as_array()
        .filter(|assets| assets.len() == manifest.len() && assets.len() <= 64)
        .ok_or("Invalid publication assets")?;
    let mut total = 0;
    let mut seen = std::collections::HashSet::new();
    for asset in assets {
        let id = asset["assetId"].as_str().ok_or("Invalid asset ID")?;
        if !seen.insert(id) {
            return Err("Duplicate asset payload".into());
        }
        let entry = manifest
            .iter()
            .find(|entry| entry["id"] == id)
            .ok_or("Unknown asset payload")?;
        let encoded = asset["base64"]
            .as_str()
            .filter(|value| value.len() <= 2800000)
            .ok_or("Asset exceeds budget")?;
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| "Invalid asset encoding")?;
        total += bytes.len();
        if bytes.len() > 2 * 1024 * 1024
            || total > 8 * 1024 * 1024
            || entry["sizeBytes"].as_u64() != Some(bytes.len() as u64)
            || entry["sha256"].as_str() != Some(tls::sha256(&bytes).trim_start_matches("sha256:"))
        {
            return Err("Asset bytes do not match their authenticated manifest".into());
        }
    }
    Ok((
        json!({"canonicalDocument":canonical,"document":document,"assets":assets,"publicationHash":hash,"capabilityFingerprint":publication["capabilityFingerprint"]}),
        primitives,
    ))
}
fn changes_valid(changes: &Value, primitives: &BTreeMap<String, Primitive>) -> bool {
    changes.as_array().is_some_and(|changes| {
        !changes.is_empty()
            && changes.len() <= 64
            && changes.iter().all(|change| {
                let Some(control) = change["controlId"]
                    .as_str()
                    .and_then(|id| primitives.get(id))
                else {
                    return false;
                };
                match control.kind.as_str() {
                    "button" => change["value"].is_boolean(),
                    "hat" => change["value"]
                        .as_i64()
                        .is_some_and(|value| (-1..=7).contains(&value)),
                    _ => change["value"].as_f64().is_some_and(|value| {
                        value.is_finite() && value >= control.min && value <= 1.0
                    }),
                }
            })
    })
}
fn fallback_batches(primitives: &BTreeMap<String, Primitive>) -> Vec<Vec<Value>> {
    primitives
        .iter()
        .map(|(id, control)| json!({"controlId":id,"value":control.fallback}))
        .collect::<Vec<_>>()
        .chunks(64)
        .map(|chunk| chunk.to_vec())
        .collect()
}
fn session_worker(
    shared: Arc<Shared>,
    epoch: u64,
    credential: SavedCredential,
    instance: String,
    takeover: bool,
) -> Result<(), String> {
    let mut socket = connect(
        &credential.endpoint,
        &credential.ca_certificate,
        &credential.ca_spki_sha256,
    )?;
    if !current(&shared, epoch) {
        return Ok(());
    }
    request(
        &mut socket,
        json!({"type":"auth","token":credential.token}),
        Duration::from_secs(3),
    )?;
    if !current(&shared, epoch) {
        return Ok(());
    }
    let opened = request(
        &mut socket,
        json!({"type":"open","instanceId":instance,"takeover":takeover}),
        Duration::from_secs(15),
    )?;
    if opened["hostId"] != credential.host_id || opened["companionId"] != credential.companion_id {
        return Err("Session identity differs from the paired host/device".into());
    }
    let (publication, primitives) = verify_publication(&opened["publication"])?;
    let base = json!({"version":1,"clientId":opened["clientId"],"sessionId":opened["sessionId"],"generation":opened["generation"],"publicationHash":publication["publicationHash"]});
    let mut baseline = base.clone();
    baseline["type"] = json!("baseline");
    baseline["sequence"] = json!("0");
    baseline["values"] = json!(primitives
        .iter()
        .map(|(id, control)| (id.clone(), control.fallback.clone()))
        .collect::<BTreeMap<_, _>>());
    if !current(&shared, epoch) {
        return Ok(());
    }
    let ack = session_request(&mut socket, baseline, Duration::from_millis(700))?;
    let (tx, rx) = mpsc::sync_channel::<QueuedInput>(128);
    let queued_transitions = Arc::new(AtomicUsize::new(0));
    set_state(&shared, epoch, |state| {
        state.status.phase = "ready".into();
        state.status.publication_hash = publication["publicationHash"].as_str().map(str::to_string);
        state.status.session = Some(
            json!({"clientId":opened["clientId"],"sessionId":opened["sessionId"],"generation":opened["generation"],"controllerInstanceId":opened["controllerInstanceId"],"publicationHash":opened["publicationHash"]}),
        );
        apply_ack(state, ack, shared.started.elapsed().as_millis() as u64);
        state.publication = Some(publication);
        state.sender = Some(tx);
        state.queued_transitions = queued_transitions;
    });
    let mut sequence = 0u64;
    let mut heartbeat = 0u64;
    let mut last_heartbeat = Instant::now();
    while current(&shared, epoch) {
        let armed = shared
            .state
            .lock()
            .map(|state| state.status.phase == "armed")
            .unwrap_or(false);
        if armed
            && (shared.started.elapsed().as_millis() as u64)
                .saturating_sub(shared.ui_tick.load(Ordering::SeqCst))
                > 600
        {
            break;
        }
        if let Ok(queued) = rx.try_recv() {
            let input = &queued.frame;
            if !armed {
                continue;
            }
            let age = (shared.started.elapsed().as_millis() as u64).saturating_sub(
                input["createdMs"]
                    .as_u64()
                    .ok_or("Missing native queue age")?,
            );
            if age > 250 {
                return Err("Native input backlog expired; reconnect with a fresh baseline".into());
            }
            if !changes_valid(&input["changes"], &primitives) {
                return Err("Input does not match the authenticated publication".into());
            }
            sequence = sequence
                .checked_add(1)
                .ok_or("Input counter exhausted; reconnect")?;
            let mut frame = base.clone();
            frame["type"] = json!("input");
            frame["sequence"] = json!(sequence.to_string());
            frame["changes"] = input["changes"].clone();
            let reply = session_request(&mut socket, frame, Duration::from_millis(700))?;
            set_state(&shared, epoch, |state| {
                apply_ack(state, reply, shared.started.elapsed().as_millis() as u64);
            });
        }
        if last_heartbeat.elapsed() >= Duration::from_millis(250) {
            heartbeat = heartbeat
                .checked_add(1)
                .ok_or("Heartbeat counter exhausted; reconnect")?;
            let mut frame = base.clone();
            frame["type"] = json!("heartbeat");
            frame["sequence"] = json!(heartbeat.to_string());
            let reply = session_request(&mut socket, frame, Duration::from_millis(700))?;
            set_state(&shared, epoch, |state| {
                apply_ack(state, reply, shared.started.elapsed().as_millis() as u64);
            });
            last_heartbeat = Instant::now();
        }
        thread::sleep(Duration::from_millis(5));
    }
    for changes in fallback_batches(&primitives) {
        sequence = sequence
            .checked_add(1)
            .ok_or("Input counter exhausted; reconnect")?;
        let mut neutral = base.clone();
        neutral["type"] = json!("input");
        neutral["sequence"] = json!(sequence.to_string());
        neutral["changes"] = json!(changes);
        let _ = session_request(&mut socket, neutral, Duration::from_millis(400));
    }
    let mut close = base;
    close["type"] = json!("close");
    let _ = session_request(&mut socket, close, Duration::from_millis(400));
    let _ = socket.close(None);
    set_state(&shared, epoch, |state| {
        state.status.phase = "disconnected".into();
        state.status.last_error =
            Some("Session stopped or UI lease expired; Connect and Start explicitly".into());
        clear_session_status(state);
        state.sender = None;
    });
    Ok(())
}
pub fn discover(duration: Duration) -> Result<Value, String> {
    let daemon = mdns_sd::ServiceDaemon::new().map_err(|error| error.to_string())?;
    let receiver = daemon
        .browse(SERVICE_TYPE)
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + duration;
    let mut hints = BTreeMap::new();
    while Instant::now() < deadline {
        if let Ok(mdns_sd::ServiceEvent::ServiceResolved(info)) =
            receiver.recv_timeout(Duration::from_millis(100))
        {
            hints.insert(info.get_fullname().to_string(),json!({"name":info.get_fullname(),"hostname":info.get_hostname(),"port":info.get_port(),"addresses":info.get_addresses().iter().map(ToString::to_string).collect::<Vec<_>>(),"hostId":info.get_property_val_str("hostId"),"trusted":false,"requiresOutOfBandPairing":true}));
            if hints.len() >= 32 {
                break;
            }
        }
    }
    let _ = daemon.shutdown();
    Ok(json!({"hints":hints.into_values().collect::<Vec<_>>(),"trusted":false}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        serde_json::from_str(include_str!("../tests/fixtures/c1-flight-publication.json")).unwrap()
    }
    #[test]
    fn native_publication_and_input_validation_rejects_shape_hash_asset_and_range_confusion() {
        let publication = fixture();
        let (valid, controls) = verify_publication(&publication).unwrap();
        assert_eq!(valid["publicationHash"], publication["publicationHash"]);
        for (id, control) in &controls {
            let good = json!([{"controlId":id,"value":control.fallback}]);
            assert!(changes_valid(&good, &controls));
            assert!(!changes_valid(
                &json!([{"controlId":id,"value":null}]),
                &controls
            ));
        }
        assert!(!changes_valid(&json!([]), &controls));
        assert!(!changes_valid(
            &json!([{"controlId":"unknown","value":true}]),
            &controls
        ));
        assert!(!changes_valid(
            &json!(vec![json!({"controlId":"unknown","value":true}); 65]),
            &controls
        ));
        let mut bad = publication.clone();
        bad["publicationHash"] = json!("sha256:bad");
        assert!(verify_publication(&bad).is_err());
        let mut bad = publication.clone();
        bad["canonicalDocument"] = json!("bad-json");
        bad["publicationHash"] = json!(tls::sha256(b"bad-json"));
        assert!(verify_publication(&bad).is_err());
        for alter in 0..5 {
            let mut bad = publication.clone();
            let mut doc: Value =
                serde_json::from_str(bad["canonicalDocument"].as_str().unwrap()).unwrap();
            match alter {
                0 => doc["schemaVersion"] = json!(3),
                1 => doc["ui"]["controls"] = json!([]),
                2 => doc["ui"]["controls"][0]["kind"] = json!("unknown"),
                3 => doc["ui"]["controls"][0]["id"] = json!(""),
                _ => {
                    let duplicated = doc["ui"]["controls"][0].clone();
                    doc["ui"]["controls"]
                        .as_array_mut()
                        .unwrap()
                        .push(duplicated);
                }
            }
            let canonical = serde_json_canonicalizer::to_string(&doc).unwrap();
            bad["canonicalDocument"] = json!(canonical);
            bad["publicationHash"] = json!(tls::sha256(canonical.as_bytes()));
            assert!(verify_publication(&bad).is_err());
        }
        let mut bad = publication;
        bad["assets"] = json!([{"assetId":"unknown","base64":"YQ=="}]);
        assert!(verify_publication(&bad).is_err());
        assert!(lan_address("127.0.0.1".parse().unwrap()));
        assert!(lan_address("192.168.0.1".parse().unwrap()));
        assert!(lan_address("::1".parse().unwrap()));
        assert!(!lan_address("8.8.8.8".parse().unwrap()));
    }
    #[cfg(windows)]
    #[test]
    fn local_ipc_is_epoch_bound_bounded_and_never_exposes_or_accepts_unauthorized_input() {
        let directory = tempfile::tempdir().unwrap();
        let client = ClientRuntime::open(directory.path().to_path_buf()).unwrap();
        assert_eq!(
            client.command(json!({"type":"status"})).unwrap()["phase"],
            "unpaired"
        );
        for request in [
            json!({}),
            json!({"type":"unknown"}),
            json!({"type":"publication"}),
            json!({"type":"connect"}),
            json!({"type":"connect","instanceId":"x"}),
            json!({"type":"refresh-assignments"}),
            json!({"type":"arm"}),
            json!({"type":"arm","epoch":0}),
            json!({"type":"keepalive","epoch":99}),
            json!({"type":"input","epoch":0,"changes":[{"controlId":"x","value":true}]}),
            json!({"type":"pair-start","payload":"x".repeat(8193),"name":"x"}),
            json!({"type":"pair-start","payload":{},"name":"x"}),
        ] {
            assert!(client.command(request).is_err());
        }
        let (tx, _rx) = mpsc::sync_channel(1);
        {
            let mut state = client.shared.state.lock().unwrap();
            state.status.phase = "ready".into();
            state.sender = Some(tx);
        }
        client.command(json!({"type":"arm","epoch":0})).unwrap();
        client
            .command(json!({"type":"keepalive","epoch":0}))
            .unwrap();
        assert!(client
            .command(json!({"type":"input","epoch":0,"changes":[]}))
            .is_err());
        client
            .command(json!({"type":"input","epoch":0,"changes":[{"controlId":"x","value":true}]}))
            .unwrap();
        assert!(client
            .command(json!({"type":"input","epoch":0,"changes":[{"controlId":"x","value":false}]}))
            .is_err());
        assert!(client.status().epoch > 0);
        assert_ne!(client.status().phase, "armed");
        client.command(json!({"type":"neutralize"})).unwrap();
        client.command(json!({"type":"disconnect"})).unwrap();
        client.command(json!({"type":"forget"})).unwrap();
        assert_eq!(client.status().phase, "unpaired");
        std::fs::write(directory.path().join("companion-client.dpapi"), b"invalid").unwrap();
        assert!(ClientRuntime::open(directory.path().to_path_buf()).is_err());
    }
    #[test]
    fn wrong_pin_is_rejected_before_even_an_invalid_endpoint_is_resolved() {
        let identity =
            crate::tls::NativeIdentity::generate("host", vec!["localhost".into()]).unwrap();
        let error = match connect(
            "not-an-endpoint",
            &identity.ca_base64(),
            &format!("sha256:{}", "0".repeat(64)),
        ) {
            Ok(_) => panic!("unexpected connection"),
            Err(error) => error,
        };
        assert!(error.contains("fingerprint"));
        assert!(connect(
            "http://127.0.0.1/companion",
            &identity.ca_base64(),
            &identity.pin().unwrap()
        )
        .is_err());
        assert!(connect(
            "wss://127.0.0.1/gateway",
            &identity.ca_base64(),
            &identity.pin().unwrap()
        )
        .is_err());
    }
    #[test]
    fn transition_reservations_include_inflight_and_release_only_their_session() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut queue = Vec::new();
        for _ in 0..4 {
            queue.push(InputReservation::reserve(counter.clone(), 64).unwrap());
        }
        assert_eq!(counter.load(Ordering::SeqCst), 256);
        assert!(InputReservation::reserve(counter.clone(), 1).is_err());
        let inflight = queue.pop().unwrap();
        drop(queue);
        assert_eq!(counter.load(Ordering::SeqCst), 64);
        let next = Arc::new(AtomicUsize::new(0));
        let next_reservation = InputReservation::reserve(next.clone(), 64).unwrap();
        drop(inflight);
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        assert_eq!(next.load(Ordering::SeqCst), 64);
        drop(next_reservation);
        assert_eq!(next.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn fallback_for_128_primitives_is_split_into_legal_ordered_batches() {
        let mut controls = BTreeMap::new();
        for index in 0..128 {
            controls.insert(
                format!("control-{index:03}"),
                Primitive {
                    kind: "axis".into(),
                    min: 0.0,
                    fallback: json!(0.25),
                },
            );
        }
        let batches = fallback_batches(&controls);
        assert_eq!(
            batches.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![64, 64]
        );
        assert_eq!(batches[0][0]["controlId"], "control-000");
        assert_eq!(batches[1][63]["controlId"], "control-127");
        assert!(batches
            .iter()
            .flatten()
            .all(|change| change["value"] == 0.25));
    }
    #[cfg(windows)]
    #[test]
    fn native_ipc_caps_cumulative_transitions_and_channel_drop_clears_reservations() {
        let directory = tempfile::tempdir().unwrap();
        let client = ClientRuntime::open(directory.path().to_path_buf()).unwrap();
        let (sender, receiver) = mpsc::sync_channel(128);
        let counter = {
            let mut state = client.shared.state.lock().unwrap();
            state.status.phase = "ready".into();
            state.sender = Some(sender);
            state.queued_transitions.clone()
        };
        client.command(json!({"type":"arm","epoch":0})).unwrap();
        let changes = vec![json!({"controlId":"trigger","value":true}); 64];
        for _ in 0..4 {
            client
                .command(json!({"type":"input","epoch":0,"changes":changes}))
                .unwrap();
        }
        assert_eq!(counter.load(Ordering::SeqCst), 256);
        assert!(client
            .command(json!({"type":"input","epoch":0,"changes":changes}))
            .is_err());
        assert_ne!(client.status().phase, "armed");
        assert!(client.status().epoch > 0);
        assert!(client
            .command(json!({"type":"input","epoch":0,"changes":changes}))
            .is_err());
        drop(receiver);
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn lan_resolution_bounds_addresses_and_never_accepts_public_destinations() {
        assert_eq!(
            resolve_lan("127.0.0.1", 443).unwrap(),
            vec!["127.0.0.1:443".parse::<SocketAddr>().unwrap()]
        );
        assert!(resolve_lan("8.8.8.8", 443).unwrap().is_empty());
        let local = resolve_lan("localhost", 443).unwrap();
        assert!(!local.is_empty());
        assert!(local.len() <= 4);
        assert!(local.iter().all(|address| lan_address(address.ip())));
    }
    #[cfg(windows)]
    #[test]
    fn feedback_is_deduplicated_ages_from_its_source_and_clears_between_session_epochs() {
        let directory = tempfile::tempdir().unwrap();
        let client = ClientRuntime::open(directory.path().to_path_buf()).unwrap();
        {
            let mut state = client.shared.state.lock().unwrap();
            let reply = json!({"feedbackSequence":"1","feedback":[{"channelId":"engine","sourceAgeMs":50}],"ackStage":"source-accepted"});
            apply_ack(&mut state, reply.clone(), 0);
            let revision = state.status.feedback_revision;
            let mut repeated = reply.clone();
            repeated["feedback"][0]["sourceAgeMs"] = json!(500);
            apply_ack(&mut state, repeated, 450);
            assert_eq!(state.status.feedback_revision, revision);
            assert_eq!(state.status.feedback[0]["sourceAgeMs"], 50);
            let mut changed = reply;
            changed["feedbackSequence"] = json!("2");
            apply_ack(&mut state, changed, 0);
            assert!(state.status.feedback_revision > revision);
        }
        thread::sleep(Duration::from_millis(2));
        assert!(client.status().feedback[0]["sourceAgeMs"].as_u64().unwrap() >= 52);
        client.invalidate("new lifecycle epoch");
        let state = client.status();
        assert!(state.feedback.is_empty());
        assert!(state.ack.is_none());
        assert!(state.publication_hash.is_none());
    }
}
