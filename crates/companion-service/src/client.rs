//! Native client: credentials stay sealed, epochs reject stale UI, and UI stalls close the lease.
use crate::{
    persistence::{self, StateStorage},
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
use zeroize::{Zeroize, Zeroizing};
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
    pub authentication_available: bool,
    pub authentication_unavailable_reason: Option<String>,
    pub storage_error: Option<String>,
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
            protected_store_available: false,
            authentication_available: false,
            authentication_unavailable_reason: Some(
                "Native protected storage has not been probed.".into(),
            ),
            storage_error: None,
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
    companion_id: String,
    storage_loaded: bool,
    credential: Option<SavedCredential>,
    publication: Option<Value>,
    sender: Option<SyncSender<QueuedInput>>,
    queued_transitions: Arc<AtomicUsize>,
    feedback_schema: Vec<crate::FeedbackDescriptor>,
    feedback_accumulator: crate::feedback::FeedbackAccumulator,
    feedback_reset_pending: bool,
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
    feedback_resync: AtomicU64,
    started: Instant,
    storage: Arc<dyn StateStorage>,
}
pub struct ClientRuntime {
    shared: Arc<Shared>,
}
impl ClientRuntime {
    pub fn open(data_dir: PathBuf) -> Result<Self, String> {
        let storage = Arc::new(persistence::SealedSqliteStore::new(
            data_dir.join("companion-client.dpapi"),
        ));
        Self::with_storage(data_dir, storage)
    }
    /// Native injection boundary for Android Keystore and deterministic tests.
    /// The adapter reads/writes plaintext only inside native code and must protect
    /// persisted bytes itself; never supply web/localStorage or a plaintext file.
    pub fn with_storage(data_dir: PathBuf, storage: Arc<dyn StateStorage>) -> Result<Self, String> {
        std::fs::create_dir_all(&data_dir).map_err(|error| error.to_string())?;
        let availability = storage.available();
        let loaded = if availability.is_ok() {
            load_credential(storage.as_ref())
        } else {
            Ok(None)
        };
        let storage_error = loaded.as_ref().err().cloned();
        let storage_loaded = availability.is_ok() && storage_error.is_none();
        let credential = loaded.ok().flatten();
        let companion_id = credential
            .as_ref()
            .map(|value| value.companion_id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut status = ClientStatus::default();
        status.storage_error = storage_error;
        set_storage_status(&mut status, availability);
        if let Some(value) = &credential {
            status.phase = "paired".into();
            status.paired_host = Some(public_host(value));
            status.assignments = value.assignments.clone();
        }
        let runtime = Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    status,
                    companion_id,
                    storage_loaded,
                    credential,
                    publication: None,
                    sender: None,
                    queued_transitions: Arc::new(AtomicUsize::new(0)),
                    feedback_schema: vec![],
                    feedback_accumulator: Default::default(),
                    feedback_reset_pending: false,
                    feedback_received_ms: 0,
                }),
                cancel: AtomicBool::new(false),
                running: AtomicBool::new(false),
                epoch: AtomicU64::new(0),
                ui_tick: AtomicU64::new(0),
                feedback_resync: AtomicU64::new(0),
                started: Instant::now(),
                storage,
            }),
        };
        start_presence(Arc::downgrade(&runtime.shared));
        Ok(runtime)
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
                        let age = age.saturating_add(elapsed).min(86_400_000);
                        sample["sourceAgeMs"] = json!(age);
                        if age >= crate::FEEDBACK_STALE_MS && sample["status"] == "fresh" {
                            sample["status"] = json!("stale");
                        }
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
        if matches!(
            command,
            "pair-start" | "pair-code-start" | "connect" | "refresh-assignments" | "storage-status"
        ) {
            let availability = self.shared.storage.available();
            let mut state = self
                .shared
                .state
                .lock()
                .map_err(|_| "Client state lock poisoned")?;
            if availability.is_ok() && !state.storage_loaded {
                match load_credential(self.shared.storage.as_ref()) {
                    Ok(credential) => {
                        if let Some(saved) = &credential {
                            state.companion_id = saved.companion_id.clone();
                        }
                        state.status.phase = if credential.is_some() {
                            "paired"
                        } else {
                            "unpaired"
                        }
                        .into();
                        state.status.paired_host = credential.as_ref().map(public_host);
                        state.status.assignments = credential
                            .as_ref()
                            .map(|saved| saved.assignments.clone())
                            .unwrap_or_else(|| json!([]));
                        state.credential = credential;
                        state.storage_loaded = true;
                        state.status.storage_error = None;
                        state.status.last_error = None;
                    }
                    Err(error) => state.status.storage_error = Some(error),
                }
            }
            set_storage_status(&mut state.status, availability);
            if command != "storage-status"
                && !(if matches!(command, "pair-start" | "pair-code-start") {
                    state.status.protected_store_available && state.status.storage_error.is_none()
                } else {
                    state.status.authentication_available
                })
            {
                return Err(state
                    .status
                    .authentication_unavailable_reason
                    .clone()
                    .unwrap_or_else(|| "Native authentication is unavailable".into()));
            }
        }
        match command {
            "status" | "storage-status" => {
                serde_json::to_value(self.status()).map_err(|error| error.to_string())
            }
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
            "pair-code-start" => {
                let code = Zeroizing::new(crate::pairing_code::normalize(
                    request["code"].as_str().unwrap_or(""),
                )?);
                let endpoint = request["endpoint"]
                    .as_str()
                    .ok_or("Choose a Cockpit host or enter its LAN address.")?
                    .to_owned();
                let name = request["name"]
                    .as_str()
                    .filter(|n| !n.trim().is_empty() && n.len() <= 80)
                    .ok_or("Choose a companion name.")?
                    .to_owned();
                self.start_worker("pairing", move |shared, epoch| {
                    let payload = resolve_pairing_code(&endpoint, &code)?;
                    pair_worker(shared, epoch, payload, name)
                })?;
                Ok(json!({"ok":true}))
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
            "feedback-resync" => {
                checked_epoch(&self.shared, &request)?;
                let mut state = self
                    .shared
                    .state
                    .lock()
                    .map_err(|_| "Client state lock poisoned")?;
                if state.status.session.is_none() {
                    return Err("Connect before requesting a feedback snapshot.".into());
                }
                self.shared
                    .feedback_resync
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_add(1))
                    .map_err(|_| "Feedback request counter exhausted; reconnect.")?;
                state.status.feedback.clear();
                state.status.feedback_revision = state.status.feedback_revision.saturating_add(1);
                state.feedback_accumulator = Default::default();
                state.feedback_reset_pending = true;
                Ok(json!({"ok":true,"feedbackRevision":state.status.feedback_revision}))
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
                self.shared.storage.delete()?;
                let availability = self.shared.storage.available();
                state.credential = None;
                state.companion_id = uuid::Uuid::new_v4().to_string();
                state.storage_loaded = true;
                state.status = ClientStatus {
                    epoch: self.shared.epoch.load(Ordering::SeqCst),
                    ..Default::default()
                };
                set_storage_status(&mut state.status, availability);
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
    state.feedback_schema.clear();
    state.feedback_accumulator = Default::default();
    state.feedback_reset_pending = false;
    state.feedback_received_ms = 0;
}
fn apply_ack(state: &mut State, reply: Value, now_ms: u64, resync: bool) -> Result<(), String> {
    if !state.feedback_reset_pending || resync {
        if let Some(frame) = reply.get("feedback") {
            let identity = state
                .status
                .session
                .as_ref()
                .ok_or("Feedback arrived without a current session.")?;
            if resync && frame["mode"] != "snapshot" {
                return Err("Feedback resync requires a complete snapshot.".into());
            }
            if let Some(samples) =
                state
                    .feedback_accumulator
                    .apply(frame, identity, &state.feedback_schema, now_ms)?
            {
                state.feedback_received_ms = now_ms;
                state.status.feedback = samples
                    .iter()
                    .map(|s| serde_json::to_value(s).unwrap())
                    .collect();
                state.status.feedback_revision = state.status.feedback_revision.saturating_add(1);
            }
            if resync {
                state.feedback_reset_pending = false;
            }
        }
    }
    let mut acknowledgement = reply;
    if let Some(object) = acknowledgement.as_object_mut() {
        object.remove("feedback");
    }
    state.status.ack = Some(acknowledgement);
    Ok(())
}
fn apply_worker_ack(shared: &Shared, epoch: u64, reply: Value, resync: bool) -> Result<(), String> {
    let mut state = shared
        .state
        .lock()
        .map_err(|_| "Client state lock poisoned")?;
    if state.status.epoch == epoch && current(shared, epoch) {
        apply_ack(
            &mut state,
            reply,
            shared.started.elapsed().as_millis() as u64,
            resync,
        )?;
    }
    Ok(())
}
fn load_credential(storage: &dyn StateStorage) -> Result<Option<SavedCredential>, String> {
    match storage.read()? {
        Some(bytes) => {
            let plain = Zeroizing::new(bytes);
            if plain.len() > 65536 {
                return Err(
                    "Stored client credential exceeds budget. Explicitly Forget host to remove it."
                        .into(),
                );
            }
            serde_json::from_slice::<SavedCredential>(&plain).map(Some).map_err(|_| "Stored companion credential is invalid. Restore the original protected state or explicitly Forget host; no automatic reset was attempted.".into())
        }
        None => Ok(None),
    }
}
fn set_storage_status(status: &mut ClientStatus, availability: Result<(), String>) {
    let unavailable = availability.err();
    status.protected_store_available = unavailable.is_none();
    status.authentication_unavailable_reason = authentication_reason(
        unavailable,
        cfg!(any(target_os = "android", target_os = "ios")),
    );
    if let Some(error) = &status.storage_error {
        status.phase = "storage-error".into();
        status.authentication_unavailable_reason =
            Some(format!("Saved native credential cannot be opened: {error}"));
    }
    status.authentication_available = status.authentication_unavailable_reason.is_none();
}
fn authentication_reason(unavailable: Option<String>, mobile: bool) -> Option<String> {
    if mobile {
        Some("Mobile authentication remains disabled pending native protected-store and lifecycle qualification (#55/#56); local preview and protected-store probing remain available.".into())
    } else {
        unavailable.map(|reason| {
            format!("Native authentication requires an available protected store: {reason}")
        })
    }
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
    let config = tls::pinned_client_config(ca, pin)?; // A verified CA always precedes credential transmission.
    connect_with_config(endpoint, config)
}
fn connect_with_config(
    endpoint: &str,
    config: Arc<rustls::ClientConfig>,
) -> Result<Socket, String> {
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
// This bootstrap socket carries only a SPAKE2 message. No identity, code, secret or
// credential is sent until the authenticated offer has been decrypted and its CA pinned.
fn resolve_pairing_code(endpoint: &str, code: &str) -> Result<PairingPayload, String> {
    let (state, message) = crate::pairing_code::start(code);
    let mut socket = connect_with_config(endpoint, tls::pairing_bootstrap_config())?;
    send_json(
        &mut socket,
        &json!({"type":"pair-code","message":STANDARD.encode(message)}),
    )?;
    let reply = receive_json(&mut socket, Duration::from_secs(3))?;
    let _ = socket.close(None);
    let value = crate::pairing_code::finish(state, &reply)?;
    let payload: PairingPayload =
        serde_json::from_value(value).map_err(|_| "Invalid authenticated pairing offer")?;
    tls::verified_ca(&payload.ca_certificate, &payload.ca_spki_sha256)?;
    if payload.endpoint != endpoint {
        return Err("Pairing host address changed. Choose its advertised address.".into());
    }
    Ok(payload)
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
            let plain =
                Zeroizing::new(serde_json::to_vec(&credential).map_err(|error| error.to_string())?);
            shared.storage.write(&plain)?;
            state.status.phase = "paired".into();
            state.status.assignments = credential.assignments.clone();
            state.credential = Some(credential);
        }
    }
    let _ = socket.close(None);
    Ok(())
}
// Credential-only presence never creates an input source or renews its lease.
fn start_presence(owner: std::sync::Weak<Shared>) {
    thread::spawn(move || {
        let mut socket: Option<Socket> = None;
        let mut identity = Zeroizing::new(String::new());
        loop {
            let Some(shared) = owner.upgrade() else {
                break;
            };
            let credential = shared.state.lock().ok().and_then(|s| {
                if s.status.protected_store_available {
                    s.credential.clone()
                } else {
                    None
                }
            });
            if let Some(credential) = credential {
                if identity.as_str() != credential.token {
                    socket = None;
                    *identity = credential.token.clone();
                }
                if socket.is_none() {
                    socket = connect(
                        &credential.endpoint,
                        &credential.ca_certificate,
                        &credential.ca_spki_sha256,
                    )
                    .ok()
                    .and_then(|mut link| {
                        let reply = request(
                            &mut link,
                            json!({"type":"auth","token":credential.token}),
                            Duration::from_secs(2),
                        )
                        .ok()?;
                        if reply["hostId"] != credential.host_id
                            || reply["companionId"] != credential.companion_id
                        {
                            return None;
                        }
                        Some(link)
                    });
                }
                if let Some(link) = socket.as_mut() {
                    match request(link, json!({"type":"presence"}), Duration::from_secs(2)) {
                        Ok(reply) => {
                            if let Ok(mut state) = shared.state.lock() {
                                if state
                                    .credential
                                    .as_ref()
                                    .is_some_and(|c| c.token == credential.token)
                                    && !shared.running.load(Ordering::SeqCst)
                                {
                                    state.status.assignments = reply["assignments"].clone();
                                }
                            }
                        }
                        Err(_) => {
                            socket = None;
                        }
                    }
                }
            } else {
                socket = None;
                identity.clear();
            }
            drop(shared);
            thread::sleep(Duration::from_secs(1));
        }
    });
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
    let companion_id = shared
        .state
        .lock()
        .map_err(|_| "Client state lock poisoned")?
        .companion_id
        .clone();
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
        json!({"type":"pair","secret":secret.as_str(),"companionId":companion_id,"name":name}),
        Duration::from_secs(3),
    )?;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if !current(&shared, epoch) {
            let _ = socket.close(None);
            return Ok(());
        }
        if reply["type"] == "paired" {
            if reply["hostId"] != payload.host_id || reply["companionId"] != companion_id {
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
                companion_id: companion_id.clone(),
                name,
                token,
                endpoint: payload.endpoint,
                host_id: payload.host_id,
                ca_certificate: payload.ca_certificate,
                ca_spki_sha256: payload.ca_spki_sha256,
                assignments: reply["assignments"].clone(),
            };
            let plain =
                Zeroizing::new(serde_json::to_vec(&credential).map_err(|error| error.to_string())?);
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
            shared.storage.write(&plain)?;
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
#[path="client/cache_validation.rs"]
pub(crate) mod cache_validation;
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
    crate::feedback_descriptors(&document)?;
    let controls = document["ui"]["controls"]
        .as_array()
        .filter(|values| values.len() <= 128)
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
    let feedback_schema = crate::feedback_descriptors(&publication["document"])?;
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
        state.publication = Some(publication);
        state.feedback_schema = feedback_schema;
        state.sender = Some(tx);
        state.queued_transitions = queued_transitions;
    });
    apply_worker_ack(&shared, epoch, ack, false)?;
    let mut resync_seen = 0;
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
        let resync = shared.feedback_resync.load(Ordering::SeqCst);
        if resync != resync_seen {
            let mut frame = base.clone();
            frame["type"] = json!("feedback-resync");
            frame["sequence"] = json!(resync.to_string());
            let reply = session_request(&mut socket, frame, Duration::from_millis(700))?;
            let current_resync = shared.feedback_resync.load(Ordering::SeqCst) == resync;
            apply_worker_ack(&shared, epoch, reply, current_resync)?;
            resync_seen = resync;
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
            apply_worker_ack(&shared, epoch, reply, false)?;
        }
        if last_heartbeat.elapsed() >= Duration::from_millis(250) {
            heartbeat = heartbeat
                .checked_add(1)
                .ok_or("Heartbeat counter exhausted; reconnect")?;
            let mut frame = base.clone();
            frame["type"] = json!("heartbeat");
            frame["sequence"] = json!(heartbeat.to_string());
            let reply = session_request(&mut socket, frame, Duration::from_millis(700))?;
            apply_worker_ack(&shared, epoch, reply, false)?;
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
    struct Adapter {
        value: Mutex<Option<Vec<u8>>>,
        available: AtomicBool,
        delete_fails: AtomicBool,
        probes: AtomicUsize,
    }
    impl StateStorage for Adapter {
        fn read(&self) -> Result<Option<Vec<u8>>, String> {
            Ok(self.value.lock().unwrap().clone())
        }
        fn write(&self, bytes: &[u8]) -> Result<(), String> {
            *self.value.lock().unwrap() = Some(bytes.to_vec());
            Ok(())
        }
        fn available(&self) -> Result<(), String> {
            self.probes.fetch_add(1, Ordering::SeqCst);
            if self.available.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err("Native store is locked".into())
            }
        }
        fn delete(&self) -> Result<(), String> {
            if self.delete_fails.load(Ordering::SeqCst) {
                Err("Delete denied".into())
            } else {
                self.value.lock().unwrap().take();
                Ok(())
            }
        }
    }
    fn adapter() -> Arc<Adapter> {
        Arc::new(Adapter {
            value: Mutex::new(None),
            available: AtomicBool::new(true),
            delete_fails: AtomicBool::new(false),
            probes: AtomicUsize::new(0),
        })
    }
    #[test]
    fn native_adapter_probe_and_mobile_qualification_are_distinct_admission_boundaries() {
        assert!(authentication_reason(None, false).is_none());
        assert!(authentication_reason(Some("Locked".into()), false)
            .unwrap()
            .contains("Locked"));
        assert!(authentication_reason(None, true)
            .unwrap()
            .contains("#55/#56"));
        assert!(authentication_reason(Some("Locked".into()), true).is_some());
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let storage = adapter();
        storage.available.store(false, Ordering::SeqCst);
        let client =
            ClientRuntime::with_storage(root.path().to_path_buf(), storage.clone()).unwrap();
        assert!(!client.status().protected_store_available);
        assert!(!client.status().authentication_available);
        assert!(client
            .command(json!({"type":"pair-start","payload":"never transmitted"}))
            .unwrap_err()
            .contains("locked"));
        assert!(client
            .command(json!({"type":"connect","instanceId":"a"}))
            .is_err());
        storage.available.store(true, Ordering::SeqCst);
        assert_eq!(
            client.command(json!({"type":"storage-status"})).unwrap()["protectedStoreAvailable"],
            true
        );
        let probes = storage.probes.load(Ordering::SeqCst);
        let _ = client.status();
        assert_eq!(storage.probes.load(Ordering::SeqCst), probes);
    }
    #[test]
    fn injected_adapter_restores_only_authority_and_failed_forget_preserves_saved_credential() {
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let storage = adapter();
        let saved = SavedCredential {
            companion_id: "saved-instance".into(),
            name: "Panel".into(),
            token: "native-secret".into(),
            endpoint: "wss://127.0.0.1:54437/companion".into(),
            host_id: "host".into(),
            ca_certificate: "certificate".into(),
            ca_spki_sha256: "pin".into(),
            assignments: json!([]),
        };
        storage.write(&serde_json::to_vec(&saved).unwrap()).unwrap();
        let client =
            ClientRuntime::with_storage(root.path().to_path_buf(), storage.clone()).unwrap();
        assert_eq!(client.status().phase, "paired");
        assert!(client.status().session.is_none());
        assert!(!client.status().busy);
        assert_eq!(
            client.shared.state.lock().unwrap().companion_id,
            "saved-instance"
        );
        storage.delete_fails.store(true, Ordering::SeqCst);
        assert!(client.command(json!({"type":"forget"})).is_err());
        assert!(storage.read().unwrap().is_some());
        assert!(client.shared.state.lock().unwrap().credential.is_some());
        storage.delete_fails.store(false, Ordering::SeqCst);
        client.command(json!({"type":"forget"})).unwrap();
        assert!(storage.read().unwrap().is_none());
        assert!(client.shared.state.lock().unwrap().credential.is_none());
        storage.write(b"broken authority").unwrap();
        let corrupt =
            ClientRuntime::with_storage(root.path().to_path_buf(), storage.clone()).unwrap();
        assert_eq!(corrupt.status().phase, "storage-error");
        assert!(!corrupt.status().authentication_available);
        assert!(corrupt
            .command(json!({"type":"connect","instanceId":"a"}))
            .is_err());
        assert_eq!(
            corrupt.command(json!({"type":"storage-status"})).unwrap()["phase"],
            "storage-error"
        );
        assert_eq!(storage.read().unwrap().unwrap(), b"broken authority");
        corrupt.command(json!({"type":"forget"})).unwrap();
        assert!(corrupt.status().storage_error.is_none());
        assert_eq!(corrupt.status().phase, "unpaired");
        storage.write(&vec![0u8; 65537]).unwrap();
        assert_eq!(
            ClientRuntime::with_storage(root.path().to_path_buf(), storage)
                .unwrap()
                .status()
                .phase,
            "storage-error"
        );
    }
    #[test]
    fn temporarily_locked_store_retries_without_losing_the_saved_pairing_or_resuming_input() {
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let storage = adapter();
        let saved = SavedCredential {
            companion_id: "persistent-install".into(),
            name: "Saved".into(),
            token: "sealed-only".into(),
            endpoint: "wss://127.0.0.1:54437/companion".into(),
            host_id: "host".into(),
            ca_certificate: "certificate".into(),
            ca_spki_sha256: "pin".into(),
            assignments: json!([{"instanceId":"retained"}]),
        };
        let bytes = serde_json::to_vec(&saved).unwrap();
        storage.write(&bytes).unwrap();
        storage.available.store(false, Ordering::SeqCst);
        let client =
            ClientRuntime::with_storage(root.path().to_path_buf(), storage.clone()).unwrap();
        assert!(!client.status().authentication_available);
        assert!(client.status().paired_host.is_none());
        assert_eq!(storage.read().unwrap().unwrap(), bytes);
        storage.available.store(true, Ordering::SeqCst);
        client.command(json!({"type":"storage-status"})).unwrap();
        assert_eq!(client.status().phase, "paired");
        assert!(client.status().session.is_none());
        assert!(!client.status().busy);
        assert_eq!(
            client.shared.state.lock().unwrap().companion_id,
            "persistent-install"
        );
        assert_eq!(storage.read().unwrap().unwrap(), bytes);
        client.command(json!({"type":"forget"})).unwrap();
        assert_ne!(
            client.shared.state.lock().unwrap().companion_id,
            "persistent-install"
        );
        assert!(storage.read().unwrap().is_none());
    }
    fn fixture() -> Value {
        serde_json::from_str(include_str!("../tests/fixtures/c1-flight-publication.json")).unwrap()
    }
    #[test]
    fn publications_without_controls_keep_static_and_feedback_only_panels_valid() {
        for feedback in [false, true] {
            let bounds = json!({"x":0.1,"y":0.1,"width":0.8,"height":0.8});
            let mut document = json!({"schemaVersion":2,"ui":{"schemaVersion":1,"id":"display","title":"Display","controls":[],"feedbackWidgets":[],"pages":[{"id":"main","title":"Main","items":[]}],"assets":[]},"presentation":{"widgets":[],"groups":[],"viewports":[{"pageId":"main","width":480,"height":320}],"grid":{"columns":24,"rows":16,"snap":true}},"targets":[],"bindings":[]});
            let mut assets = Vec::new();
            if feedback {
                document["ui"]["feedbackWidgets"] = json!([{"id":"speed-output","label":"Speed","kind":"gauge","channelId":"speed"}]);
                document["ui"]["pages"][0]["items"] = json!([{"kind":"feedback","refId":"speed-output","bounds":bounds}]);
                document["presentation"]["widgets"] = json!([{"id":"speed-widget","pageId":"main","kind":"numeric","label":"Speed","bounds":bounds,"controlIds":[],"feedbackIds":["speed-output"],"config":{"min":0,"max":2000,"unit":"km/h","precision":0}}]);
            } else {
                let mut png = std::io::Cursor::new(Vec::new());
                image::DynamicImage::new_rgb8(1, 1).write_to(&mut png, image::ImageFormat::Png).unwrap();
                let bytes = png.into_inner();
                document["ui"]["assets"] = json!([{"id":"logo","sha256":crate::hash(&bytes),"sizeBytes":bytes.len(),"mime":"image/png"}]);
                document["presentation"]["widgets"] = json!([{"id":"logo-widget","pageId":"main","kind":"image","label":"Logo","bounds":bounds,"controlIds":[],"feedbackIds":[],"config":{"assetId":"logo"}}]);
                assets.push(crate::AssetPayload {asset_id:"logo".into(),base64:STANDARD.encode(&bytes)});
            }
            let canonical = serde_json_canonicalizer::to_string(&document).unwrap();
            let upload = crate::PublicationUpload {
                publication_hash: tls::sha256(canonical.as_bytes()),
                canonical_document: canonical,
                capability_fingerprint: None,
                assets,
            };
            let publication = crate::Publication::validate_legacy_test_fixture(upload.clone()).unwrap();
            let (verified, primitives) = verify_publication(&serde_json::to_value(upload).unwrap()).unwrap();
            assert_eq!(verified["document"], document);
            assert!(primitives.is_empty());
            assert!(fallback_batches(&primitives).is_empty());
            assert!(!changes_valid(&json!([]), &primitives));
            assert!(!changes_valid(&json!([{"controlId":"invented","value":true}]), &primitives));
            assert_eq!(publication.feedback.len(), usize::from(feedback));
            if feedback {
                assert_eq!(publication.feedback[0].unit.as_deref(), Some("km/h"));
                assert_eq!(publication.feedback[0].range.as_ref().unwrap().max, 2000.0);
            }
        }
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
        for alter in 0..6 {
            let mut bad = publication.clone();
            let mut doc: Value =
                serde_json::from_str(bad["canonicalDocument"].as_str().unwrap()).unwrap();
            match alter {
                0 => doc["schemaVersion"] = json!(3),
                1 => doc["ui"]["controls"] = Value::Null,
                2 => doc["ui"]["controls"][0]["kind"] = json!("unknown"),
                3 => doc["ui"]["controls"][0]["id"] = json!(""),
                4 => {
                    let duplicated = doc["ui"]["controls"][0].clone();
                    doc["ui"]["controls"]
                        .as_array_mut()
                        .unwrap()
                        .push(duplicated);
                }
                _ => doc["ui"]["controls"] = json!((0..129).map(|index| json!({"id":format!("control-{index}"),"kind":"button","label":"Button"})).collect::<Vec<_>>()),
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
        let directory = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
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
        let anchor=directory.path().join("companion-client.dpapi");
        std::fs::write(&anchor,b"retired invalid file").unwrap();
        let database=osg_persistence::Database::for_namespace(&anchor).unwrap();
        database.commit(&[osg_persistence::Change{domain:"secrets",key:"companion-client",expected_revision:database.revision("secrets","companion-client").unwrap(),payload:Some(b"invalid")}]).unwrap();
        let reopened = ClientRuntime::open(directory.path().to_path_buf()).unwrap();
        assert!(!reopened.status().authentication_available);
        if reopened.status().protected_store_available {
            assert_eq!(reopened.status().phase, "storage-error");
        }
        assert_eq!(
            database.read("secrets","companion-client").unwrap().unwrap().payload,
            b"invalid"
        );
        reopened.command(json!({"type":"forget"})).unwrap();
        assert!(database.read("secrets","companion-client").unwrap().is_none());
        assert_eq!(std::fs::read(anchor).unwrap(),b"retired invalid file");
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
        let directory = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
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
    #[test]
    fn feedback_is_validated_deduplicated_and_resynced_without_input_or_arm_changes() {
        use crate::feedback::*;
        let directory = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let client = ClientRuntime::with_storage(directory.path().into(), adapter()).unwrap();
        assert!(client
            .command(json!({"type":"feedback-resync","epoch":0}))
            .is_err());
        let p = crate::Publication::validate_legacy_test_fixture(
            serde_json::from_str(include_str!("../tests/fixtures/c1-flight-publication.json"))
                .unwrap(),
        )
        .unwrap();
        let descriptor = p
            .feedback
            .iter()
            .find(|d| d.kind == FeedbackKind::Number)
            .unwrap();
        let binding = FeedbackBinding {
            channel_id: descriptor.channel_id.clone(),
            output_id: descriptor.output_ids[0].clone(),
        };
        let sample = FeedbackSample {
            channel_id: binding.channel_id.clone(),
            output_id: binding.output_id.clone(),
            value: Some(FeedbackValue::Number(42.)),
            status: FeedbackStatus::Fresh,
            provenance: FeedbackProvenance::GameConfirmed,
            source_kind: FeedbackSource::Simulator,
            source_age_ms: 50,
            unit: descriptor.unit.clone().unwrap(),
            range: descriptor.range.clone(),
        };
        let identity = json!({"clientId":"client","sessionId":"session","generation":"1","controllerInstanceId":"instance","publicationHash":p.publication_hash});
        let mut book = FeedbackBook::default();
        book.configure(p.feedback.clone(), &[binding], 0).unwrap();
        book.publish(&[sample.clone()], FeedbackMode::Snapshot, 0)
            .unwrap();
        let mut cursor = FeedbackCursor::default();
        let reply =
            json!({"feedback":book.deliver(&mut cursor,&identity,0),"ackStage":"source-accepted"});
        {
            let mut state = client.shared.state.lock().unwrap();
            state.publication = Some(
                verify_publication(&serde_json::to_value(&p).unwrap())
                    .unwrap()
                    .0,
            );
            state.feedback_schema = p.feedback.clone();
            state.status.session = Some(identity.clone());
            state.status.publication_hash = Some(p.publication_hash.clone());
            state.status.phase = "armed".into();
            apply_ack(&mut state, reply.clone(), 0, false).unwrap();
            let revision = state.status.feedback_revision;
            let mut repeated = reply.clone();
            repeated["feedback"]["values"][0]["sourceAgeMs"] = json!(500);
            apply_ack(&mut state, repeated, 450, false).unwrap();
            assert_eq!(state.status.feedback_revision, revision);
            assert_eq!(state.status.feedback[0]["sourceAgeMs"], 50);
            let mut stale = reply.clone();
            stale["feedback"]["sessionId"] = json!("old-session");
            assert!(apply_ack(&mut state, stale, 500, false).is_err());
            assert_eq!(state.status.feedback_revision, revision);
        }
        let before = client.status().feedback_revision;
        client
            .command(json!({"type":"feedback-resync","epoch":0}))
            .unwrap();
        assert!(client.status().feedback.is_empty());
        assert!(client.status().feedback_revision > before);
        assert_eq!(client.status().phase, "armed");
        assert_eq!(
            client
                .shared
                .state
                .lock()
                .unwrap()
                .queued_transitions
                .load(Ordering::SeqCst),
            0
        );
        {
            let mut state = client.shared.state.lock().unwrap();
            apply_ack(&mut state, reply.clone(), 500, false).unwrap();
            assert!(state.status.feedback.is_empty());
            let mut reset = FeedbackCursor::default();
            let reply = json!({"feedback":book.deliver(&mut reset,&identity,1000)});
            apply_ack(&mut state, reply, 0, true).unwrap();
            assert_eq!(state.status.feedback[0]["status"], "stale");
            assert!(!state.feedback_reset_pending);
        }
        assert_eq!(client.status().feedback[0]["status"], "stale");
        client.invalidate("new lifecycle epoch");
        assert!(client.status().feedback.is_empty());
        assert!(client.status().ack.is_none());
    }
}
