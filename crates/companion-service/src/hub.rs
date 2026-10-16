use crate::{
    persistence::{SealedFileStore, StateStorage},
    publication::{digest_id, ensure, hash, id, label},
    *,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::Path,
    sync::{Arc, Mutex},
    time::Instant,
};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

pub const LEASE_MS: u64 = 1500;
pub const PAIRING_MS: u64 = 60_000;
pub const MAX_CLIENTS: usize = 8;
pub const MAX_SESSIONS: usize = 512;
pub const MAX_INSTANCES: usize = 32;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Credential {
    companion_id: String,
    name: String,
    token_hash: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Assignment {
    instance_id: String,
    companion_id: String,
    publication_hash: String,
    definition: SourceDefinition,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    schema_version: u8,
    host_id: String,
    credentials: Vec<Credential>,
    instances: Vec<Assignment>,
}
struct Offer {
    secret_hash: String,
    expires: u64,
    consumed: bool,
}
struct Pending {
    companion_id: String,
    name: String,
    secret_hash: String,
    expires: u64,
    token: Option<Zeroizing<String>>,
}
#[derive(Clone)]
struct Session {
    connection: String,
    client_id: String,
    companion_id: String,
    token: SourceToken,
    armed: bool,
    exhausted: bool,
    sequence: u64,
    heartbeat: u64,
    last_seen: u64,
    batches: VecDeque<u64>,
    values: BTreeMap<String, InputValue>,
}
struct Feedback {
    sequence: u64,
    updated: u64,
    values: Vec<Value>,
}
struct Inner {
    record: Record,
    publications: BTreeMap<String, Publication>,
    offer: Option<Offer>,
    pending: BTreeMap<String, Pending>,
    connections: BTreeMap<String, String>,
    sessions: BTreeMap<String, Session>,
    used_sessions: BTreeSet<String>,
    feedback: BTreeMap<String, Feedback>,
    last_now: u64,
    diagnostic: Option<String>,
}

#[derive(Clone)]
pub struct Hub {
    inner: Arc<Mutex<Inner>>,
    sink: Arc<dyn SourceSink>,
    storage: Option<Arc<dyn StateStorage>>,
    started: Arc<Instant>,
}
pub struct NullSourceSink;
impl SourceSink for NullSourceSink {
    fn register(&self, _: SourceDefinition) -> Result<(), String> {
        Ok(())
    }
    fn baseline(&self, _: &SourceToken, _: u64, _: &[SourceChange]) -> Result<(), String> {
        Ok(())
    }
    fn changes(&self, _: &SourceToken, _: u64, _: &[SourceChange]) -> Result<(), String> {
        Ok(())
    }
    fn loss(&self, _: &SourceToken) -> Result<(), String> {
        Ok(())
    }
}

fn random_secret() -> Result<String, String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| "Secure randomness is unavailable.")?;
    let value = URL_SAFE_NO_PAD.encode(bytes);
    bytes.zeroize();
    Ok(value)
}
fn secret_valid(secret: &str) -> bool {
    secret.len() == 43
        && URL_SAFE_NO_PAD
            .decode(secret)
            .is_ok_and(|v| v.len() == 32 && URL_SAFE_NO_PAD.encode(v) == secret)
}
fn equal_hash(a: &str, b: &str) -> bool {
    bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}
fn counter(value: &Value) -> Result<u64, String> {
    let s = value.as_str().ok_or("Counter must be a decimal string.")?;
    ensure(
        !s.is_empty()
            && s.len() <= 20
            && (s == "0" || !s.starts_with('0'))
            && s.bytes().all(|b| b.is_ascii_digit()),
        "Invalid decimal counter.",
    )?;
    s.parse().map_err(|_| "Counter overflow.".into())
}
fn field<'a>(v: &'a Value, name: &str) -> Result<&'a str, String> {
    v[name].as_str().ok_or_else(|| format!("Missing {name}."))
}
fn exact(v: &Value, required: &[&str], optional: &[&str]) -> Result<(), String> {
    let map = v.as_object().ok_or("Expected message object.")?;
    ensure(
        required.iter().all(|k| map.contains_key(*k))
            && map
                .keys()
                .all(|k| required.contains(&k.as_str()) || optional.contains(&k.as_str())),
        "Unknown or missing message field.",
    )
}

impl Hub {
    pub fn new(host_id: String, sink: Arc<dyn SourceSink>) -> Self {
        Self::from_record(
            Record {
                schema_version: 1,
                host_id,
                credentials: vec![],
                instances: vec![],
            },
            sink,
            None,
        )
        .expect("new host identity must be valid")
    }
    pub fn open(path: impl AsRef<Path>, sink: Arc<dyn SourceSink>) -> Result<Self, String> {
        Self::with_storage(
            Arc::new(SealedFileStore::new(
                path.as_ref().join("companion-host-state.dpapi"),
            )),
            sink,
        )
    }
    pub fn with_storage(
        storage: Arc<dyn StateStorage>,
        sink: Arc<dyn SourceSink>,
    ) -> Result<Self, String> {
        let record = match storage.read()? {
            Some(mut bytes) => {
                let result = serde_json::from_slice(&bytes)
                    .map_err(|_| "Invalid protected companion host state.");
                bytes.zeroize();
                result?
            }
            None => {
                let record = Record {
                    schema_version: 1,
                    host_id: uuid::Uuid::new_v4().to_string(),
                    credentials: vec![],
                    instances: vec![],
                };
                storage.write(&serde_json::to_vec(&record).map_err(|e| e.to_string())?)?;
                record
            }
        };
        Self::from_record(record, sink, Some(storage))
    }
    fn from_record(
        record: Record,
        sink: Arc<dyn SourceSink>,
        storage: Option<Arc<dyn StateStorage>>,
    ) -> Result<Self, String> {
        ensure(
            record.schema_version == 1
                && id(&record.host_id)
                && record.credentials.len() <= 32
                && record.instances.len() <= MAX_INSTANCES,
            "Invalid companion state version/budget.",
        )?;
        let mut clients = BTreeSet::new();
        let mut token_hashes = BTreeSet::new();
        for c in &record.credentials {
            ensure(
                id(&c.companion_id)
                    && label(&c.name)
                    && c.token_hash.len() == 64
                    && c.token_hash
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    && token_hashes.insert(&c.token_hash)
                    && clients.insert(&c.companion_id),
                "Invalid saved companion credential.",
            )?;
        }
        let mut instances = BTreeSet::new();
        for a in &record.instances {
            ensure(
                id(&a.instance_id)
                    && instances.insert(&a.instance_id)
                    && clients.contains(&a.companion_id)
                    && digest_id(&a.publication_hash)
                    && a.definition.source_id == a.instance_id
                    && a.definition.publication_hash == a.publication_hash
                    && label(&a.definition.name)
                    && !a.definition.controls.is_empty()
                    && a.definition.controls.len() <= 128,
                "Invalid saved companion assignment.",
            )?;
            let mut controls = BTreeSet::new();
            for control in &a.definition.controls {
                ensure(
                    id(&control.id)
                        && controls.insert(&control.id)
                        && control.accepts(&control.fallback()),
                    "Invalid saved companion control descriptor.",
                )?;
            }
            sink.register(a.definition.clone())?;
        }
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                record,
                publications: BTreeMap::new(),
                offer: None,
                pending: BTreeMap::new(),
                connections: BTreeMap::new(),
                sessions: BTreeMap::new(),
                used_sessions: BTreeSet::new(),
                feedback: BTreeMap::new(),
                last_now: 0,
                diagnostic: None,
            })),
            sink,
            storage,
            started: Arc::new(Instant::now()),
        })
    }

    /// Trusted local mapping boundary. This is deliberately not a wire command.
    /// Retained source descriptors alone do not grant mapping authority after revoke.
    pub fn assigned_publication(&self, instance_id: &str) -> Result<Publication, String> {
        ensure(id(instance_id), "Invalid controller instance.")?;
        let inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let assignment = inner
            .record
            .instances
            .iter()
            .find(|assignment| assignment.instance_id == instance_id)
            .ok_or("The controller instance is not assigned to a trusted companion.")?;
        ensure(
            inner
                .record
                .credentials
                .iter()
                .any(|credential| credential.companion_id == assignment.companion_id),
            "The companion installation is no longer authorized.",
        )?;
        let publication = inner
            .publications
            .get(&assignment.publication_hash)
            .ok_or("Make the assigned panel revision available again before mapping it.")?;
        ensure(
            publication.definition(instance_id) == assignment.definition,
            "The assigned publication descriptor changed; reconcile the assignment explicitly.",
        )?;
        Ok(publication.clone())
    }

    pub fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
    pub fn host_id(&self) -> String {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .record
            .host_id
            .clone()
    }
    fn persist(&self, record: &Record) -> Result<(), String> {
        if let Some(storage) = &self.storage {
            let mut bytes = serde_json::to_vec(record).map_err(|e| e.to_string())?;
            let result = storage.write(&bytes);
            bytes.zeroize();
            result
        } else {
            Ok(())
        }
    }
    fn disarm(&self, inner: &mut Inner, session_id: &str, reason: &str, remove: bool) {
        if let Some(mut session) = inner.sessions.remove(session_id) {
            if session.armed {
                if let Err(error) = self.sink.loss(&session.token) {
                    inner.diagnostic = Some(format!("Source cleanup failed: {error}"));
                }
            }
            session.armed = false;
            session.sequence = 0;
            session.values.clear();
            if let Some(next) = session.token.generation.checked_add(1) {
                session.token.generation = next;
            } else {
                session.exhausted = true;
            }
            if !remove {
                inner.sessions.insert(session_id.into(), session);
            }
            if reason == "source-rejected" {
                inner.diagnostic =
                    Some("Companion source rejected input; fresh baseline required.".into());
            }
        }
    }
    fn expire(&self, inner: &mut Inner, now: u64) -> Result<(), String> {
        // Native workers may sample the monotonic clock before contending for this lock.
        // Reordering those trusted samples is not a protocol fault or client timestamp.
        let now = now.max(inner.last_now);
        inner.last_now = now;
        inner.pending.retain(|_, p| p.expires > now);
        if inner.offer.as_ref().is_some_and(|o| o.expires <= now) {
            inner.offer = None;
        }
        let expired: Vec<_> = inner
            .sessions
            .iter()
            .filter(|(_, s)| now.saturating_sub(s.last_seen) >= LEASE_MS)
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            self.disarm(inner, &id, "lease-expired", true);
        }
        Ok(())
    }
    pub fn tick(&self, now_ms: u64) {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let _ = self.expire(&mut inner, now_ms);
    }
    pub fn disconnect_connection(&self, connection_id: &str, now_ms: u64) {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let _ = self.expire(&mut inner, now_ms);
        inner.connections.remove(connection_id);
        let ids: Vec<_> = inner
            .sessions
            .iter()
            .filter(|(_, s)| s.connection == connection_id)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.disarm(&mut inner, &id, "connection-lost", true);
        }
    }
    pub fn disconnect_all(&self) {
        let ids: Vec<_> = self
            .inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .connections
            .keys()
            .cloned()
            .collect();
        for id in ids {
            self.disconnect_connection(&id, self.now_ms());
        }
    }
    pub fn command(&self, request: &Value) -> Result<Value, String> {
        // Publication verification may hash assets; it runs before acquiring session state.
        let published = if request["command"] == "companion_publish" {
            let upload = PublicationUpload {
                canonical_document: field(request, "canonicalDocument")?.into(),
                publication_hash: field(request, "publicationHash")?.into(),
                capability_fingerprint: request["capabilityFingerprint"]
                    .as_str()
                    .map(str::to_owned),
                assets: serde_json::from_value(request["assets"].clone())
                    .map_err(|_| "Invalid publication assets.")?,
            };
            Some(Publication::validate(upload)?)
        } else {
            None
        };
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let now = self.now_ms().max(inner.last_now);
        self.expire(&mut inner, now)?;
        match request["command"].as_str().unwrap_or("") {
            "companion_status" => Ok(Self::status(&inner)),
            "companion_pair_offer" => {
                let secret = random_secret()?;
                let expires = now.saturating_add(PAIRING_MS);
                inner.offer = Some(Offer {
                    secret_hash: hash(secret.as_bytes()),
                    expires,
                    consumed: false,
                });
                Ok(
                    json!({"secret":secret,"expiresAtMs":expires,"ttlMs":PAIRING_MS,"hostId":inner.record.host_id}),
                )
            }
            "companion_approve" | "companion_pair_approve" => {
                let request_id = field(request, "requestId")?;
                let pending = inner
                    .pending
                    .get(request_id)
                    .ok_or("Pairing request expired or missing.")?;
                if let Some(expected) = request["companionId"].as_str() {
                    ensure(
                        expected == pending.companion_id,
                        "Pairing installation mismatch.",
                    )?;
                }
                if pending.token.is_some() {
                    return Ok(json!({"approved":true}));
                }
                let token = Zeroizing::new(random_secret()?);
                let credential = Credential {
                    companion_id: pending.companion_id.clone(),
                    name: pending.name.clone(),
                    token_hash: hash(token.as_bytes()),
                };
                let mut record = inner.record.clone();
                record
                    .credentials
                    .retain(|c| c.companion_id != credential.companion_id);
                ensure(
                    record.credentials.len() < 32,
                    "Paired companion budget exhausted.",
                )?;
                record.credentials.push(credential.clone());
                self.persist(&record)?;
                let sessions: Vec<_> = inner
                    .sessions
                    .iter()
                    .filter(|(_, s)| s.companion_id == credential.companion_id)
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in sessions {
                    self.disarm(&mut inner, &id, "credential-rotated", true);
                }
                inner
                    .connections
                    .retain(|_, c| c != &credential.companion_id);
                inner.record = record;
                inner.pending.get_mut(request_id).unwrap().token = Some(token);
                Ok(json!({"approved":true,"companionId":credential.companion_id}))
            }
            "companion_reject" => {
                ensure(
                    inner.pending.remove(field(request, "requestId")?).is_some(),
                    "Unknown pairing request.",
                )?;
                Ok(json!({"rejected":true}))
            }
            "companion_revoke" => {
                let companion = field(request, "companionId")?;
                let mut record = inner.record.clone();
                ensure(
                    record
                        .credentials
                        .iter()
                        .any(|c| c.companion_id == companion),
                    "Unknown companion.",
                )?;
                record.credentials.retain(|c| c.companion_id != companion);
                record.instances.retain(|a| a.companion_id != companion);
                self.persist(&record)?;
                let sessions: Vec<_> = inner
                    .sessions
                    .iter()
                    .filter(|(_, s)| s.companion_id == companion)
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in sessions {
                    self.disarm(&mut inner, &id, "revoked", true);
                }
                inner.connections.retain(|_, c| c != companion);
                inner.pending.retain(|_, p| p.companion_id != companion);
                inner.record = record;
                Ok(json!({"revoked":true}))
            }
            "companion_publish" => {
                let p = published.unwrap();
                let existing = inner
                    .publications
                    .get(&p.publication_hash)
                    .map_or(0, Publication::bytes);
                ensure(
                    inner.publications.len() < 32 || existing > 0,
                    "Native publication budget exhausted.",
                )?;
                ensure(
                    inner
                        .publications
                        .values()
                        .map(Publication::bytes)
                        .sum::<usize>()
                        - existing
                        + p.bytes()
                        <= 64 * 1024 * 1024,
                    "Native publication bytes exceed 64 MiB.",
                )?;
                let summary = json!({"publicationHash":p.publication_hash,"capabilityFingerprint":p.capability_fingerprint,"uiId":p.ui_id,"title":p.title});
                inner.publications.insert(p.publication_hash.clone(), p);
                Ok(summary)
            }
            "companion_assign" => {
                let instance = field(request, "instanceId")?;
                let companion = field(request, "companionId")?;
                let publication = field(request, "publicationHash")?;
                ensure(
                    id(instance)
                        && inner
                            .record
                            .credentials
                            .iter()
                            .any(|c| c.companion_id == companion),
                    "Unknown companion or invalid instance.",
                )?;
                let p = inner
                    .publications
                    .get(publication)
                    .ok_or("Publish the assigned panel before assignment.")?;
                let assignment = Assignment {
                    instance_id: instance.into(),
                    companion_id: companion.into(),
                    publication_hash: publication.into(),
                    definition: p.definition(instance),
                };
                let mut record = inner.record.clone();
                record.instances.retain(|a| a.instance_id != instance);
                ensure(
                    record.instances.len() < MAX_INSTANCES,
                    "Instance budget exhausted.",
                )?;
                record.instances.push(assignment.clone());
                // Validate source shape before committing authority; existing active ownership is released first.
                let ids: Vec<_> = inner
                    .sessions
                    .iter()
                    .filter(|(_, s)| s.token.source_id == instance)
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in ids {
                    self.disarm(&mut inner, &id, "assignment-changed", true);
                }
                self.sink.register(assignment.definition.clone())?;
                self.persist(&record)?;
                inner.record = record;
                inner.feedback.remove(instance);
                Ok(json!({"assigned":true,"instanceId":instance}))
            }
            "companion_takeover" => {
                let instance = field(request, "instanceId")?;
                let prior = inner
                    .record
                    .instances
                    .iter()
                    .find(|a| a.instance_id == instance)
                    .ok_or("Unknown controller instance.")?;
                let owner = request["companionId"]
                    .as_str()
                    .unwrap_or(&prior.companion_id);
                ensure(
                    inner
                        .record
                        .credentials
                        .iter()
                        .any(|c| c.companion_id == owner),
                    "Pair the new installation before takeover.",
                )?;
                let mut record = inner.record.clone();
                record
                    .instances
                    .iter_mut()
                    .find(|a| a.instance_id == instance)
                    .unwrap()
                    .companion_id = owner.into();
                self.persist(&record)?;
                let ids: Vec<_> = inner
                    .sessions
                    .iter()
                    .filter(|(_, s)| s.token.source_id == instance)
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in ids {
                    self.disarm(&mut inner, &id, "operator-takeover", true);
                }
                inner.record = record;
                Ok(json!({"takenOver":true,"instanceId":instance}))
            }
            "companion_disconnect" => {
                let instance = request["instanceId"].as_str();
                let companion = request["companionId"].as_str();
                ensure(
                    instance.is_some() || companion.is_some(),
                    "Choose an instance or companion.",
                )?;
                let ids: Vec<_> = inner
                    .sessions
                    .iter()
                    .filter(|(_, s)| {
                        if let Some(i) = instance {
                            s.token.source_id == i
                        } else {
                            companion.is_some_and(|c| s.companion_id == c)
                        }
                    })
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in ids {
                    self.disarm(&mut inner, &id, "operator-disconnect", true);
                }
                Ok(json!({"disconnected":true}))
            }
            "companion_feedback" => {
                let instance = field(request, "instanceId")?;
                let assignment = inner
                    .record
                    .instances
                    .iter()
                    .find(|a| a.instance_id == instance)
                    .ok_or("Unknown controller instance.")?;
                let p = inner
                    .publications
                    .get(&assignment.publication_hash)
                    .ok_or("Publication unavailable.")?;
                let values = request["values"]
                    .as_array()
                    .ok_or("Feedback values must be an array.")?
                    .clone();
                validate_feedback(p, &values)?;
                ensure(
                    inner
                        .feedback
                        .get(instance)
                        .is_none_or(|f| now.saturating_sub(f.updated) >= 34),
                    "Feedback rate exceeds 30 Hz.",
                )?;
                let sequence = inner
                    .feedback
                    .get(instance)
                    .map_or(Some(1), |f| f.sequence.checked_add(1))
                    .ok_or("Feedback counter exhausted.")?;
                inner.feedback.insert(
                    instance.into(),
                    Feedback {
                        sequence,
                        updated: now,
                        values,
                    },
                );
                Ok(json!({"accepted":true,"feedbackSequence":sequence.to_string()}))
            }
            _ => Err("Unknown local companion command.".into()),
        }
    }
    fn status(inner: &Inner) -> Value {
        json!({"hostId":inner.record.host_id,"diagnostic":inner.diagnostic,"clients":inner.record.credentials.iter().map(|c|json!({"companionId":c.companion_id,"name":c.name,"connected":inner.connections.values().any(|v|v==&c.companion_id)})).collect::<Vec<_>>(),"pending":inner.pending.iter().map(|(id,p)|json!({"requestId":id,"companionId":p.companion_id,"name":p.name,"expiresAtMs":p.expires,"approved":p.token.is_some()})).collect::<Vec<_>>(),"instances":inner.record.instances.iter().map(|a|{let s=inner.sessions.values().find(|s|s.token.source_id==a.instance_id);json!({"instanceId":a.instance_id,"sourceId":a.instance_id,"companionId":a.companion_id,"publicationHash":a.publication_hash,"name":a.definition.name,"connected":s.is_some(),"armed":s.is_some_and(|s|s.armed),"generation":s.map(|s|s.token.generation.to_string()),"sessionId":s.map(|s|&s.token.session_id),"values":s.map(|s|&s.values)})}).collect::<Vec<_>>(),"publications":inner.publications.values().map(|p|json!({"publicationHash":p.publication_hash,"capabilityFingerprint":p.capability_fingerprint,"uiId":p.ui_id,"title":p.title,"controlCount":p.controls.len(),"assetCount":p.assets.len()})).collect::<Vec<_>>()})
    }
    pub fn handle_wire(&self, frame: Value, connection_id: &str, now_ms: u64) -> Value {
        let result = self.wire(&frame, connection_id, now_ms);
        match result {
            Ok(value) => value,
            Err(error) => json!({"ok":false,"type":"error","error":error}),
        }
    }
    fn wire(&self, frame: &Value, connection_id: &str, now: u64) -> Result<Value, String> {
        ensure(
            id(connection_id)
                && serde_json::to_vec(frame)
                    .map_err(|_| "Invalid frame.")?
                    .len()
                    <= FRAME_BYTES,
            "Frame exceeds budget.",
        )?;
        if let Some(version) = frame.get("version") {
            ensure(version == 1, "Unsupported protocol version.")?;
        }
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let now = now.max(inner.last_now).max(self.now_ms());
        self.expire(&mut inner, now)?;
        match field(frame, "type")? {
            "pair" => {
                exact(
                    frame,
                    &["type", "secret", "companionId", "name"],
                    &["version"],
                )?;
                let secret = field(frame, "secret")?;
                let companion = field(frame, "companionId")?;
                let name = field(frame, "name")?;
                ensure(
                    secret_valid(secret)
                        && id(companion)
                        && label(name)
                        && inner.pending.len() < 16,
                    "Invalid pairing request/budget.",
                )?;
                let offer = inner
                    .offer
                    .as_mut()
                    .ok_or("Open a fresh pairing offer on Cockpit.")?;
                ensure(
                    !offer.consumed
                        && offer.expires > now
                        && equal_hash(&offer.secret_hash, &hash(secret.as_bytes())),
                    "Pairing offer is invalid, consumed or expired.",
                )?;
                offer.consumed = true;
                let pending = Pending {
                    companion_id: companion.into(),
                    name: name.into(),
                    secret_hash: offer.secret_hash.clone(),
                    expires: offer.expires,
                    token: None,
                };
                let request_id = uuid::Uuid::new_v4().to_string();
                inner.pending.insert(request_id.clone(), pending);
                Ok(json!({"ok":true,"type":"pair-pending","requestId":request_id}))
            }
            "pair-status" => {
                exact(frame, &["type", "requestId", "secret"], &["version"])?;
                let request_id = field(frame, "requestId")?;
                let p = inner
                    .pending
                    .get(request_id)
                    .ok_or("Pairing request expired or missing.")?;
                let secret = field(frame, "secret")?;
                ensure(
                    secret_valid(secret) && equal_hash(&p.secret_hash, &hash(secret.as_bytes())),
                    "Pairing request owner mismatch.",
                )?;
                if let Some(token) = &p.token {
                    Ok(
                        json!({"ok":true,"type":"paired","requestId":request_id,"token":token.as_str(),"hostId":inner.record.host_id,"companionId":p.companion_id,"assignments":inner.record.instances.iter().filter(|a|a.companion_id==p.companion_id).map(|a|json!({"instanceId":a.instance_id,"publicationHash":a.publication_hash,"name":a.definition.name})).collect::<Vec<_>>()}),
                    )
                } else {
                    Ok(json!({"ok":true,"type":"pair-pending","requestId":request_id}))
                }
            }
            "auth" => {
                exact(frame, &["type", "token"], &["version"])?;
                ensure(
                    inner.connections.contains_key(connection_id)
                        || inner.connections.len() < MAX_CLIENTS,
                    "Connected client budget exhausted.",
                )?;
                let token = field(frame, "token")?;
                ensure(secret_valid(token), "Invalid installation credential.")?;
                let digest = hash(token.as_bytes());
                let credential = inner
                    .record
                    .credentials
                    .iter()
                    .find(|c| equal_hash(&c.token_hash, &digest))
                    .ok_or("Installation credential is invalid or revoked.")?;
                let companion = credential.companion_id.clone();
                ensure(
                    inner
                        .connections
                        .values()
                        .filter(|c| *c == &companion)
                        .count()
                        < 2,
                    "This installation already uses its two connection slots.",
                )?;
                ensure(
                    !inner.connections.contains_key(connection_id),
                    "Connection is already authenticated.",
                )?;
                inner
                    .connections
                    .insert(connection_id.into(), companion.clone());
                Ok(
                    json!({"ok":true,"type":"authenticated","hostId":inner.record.host_id,"companionId":companion,"assignments":inner.record.instances.iter().filter(|a|a.companion_id==companion).map(|a|json!({"instanceId":a.instance_id,"publicationHash":a.publication_hash,"name":a.definition.name})).collect::<Vec<_>>()}),
                )
            }
            _ => {
                let companion = inner
                    .connections
                    .get(connection_id)
                    .cloned()
                    .ok_or("Authenticate this connection first.")?;
                ensure(
                    inner
                        .record
                        .credentials
                        .iter()
                        .any(|c| c.companion_id == companion),
                    "Installation was revoked.",
                )?;
                if frame["type"] == "open" {
                    exact(
                        frame,
                        &["type", "instanceId"],
                        &["version", "publicationHash", "takeover"],
                    )?;
                    let instance = field(frame, "instanceId")?;
                    let assignment = inner
                        .record
                        .instances
                        .iter()
                        .find(|a| a.instance_id == instance && a.companion_id == companion)
                        .cloned()
                        .ok_or("This installation is not assigned that controller instance.")?;
                    if let Some(hash) = frame.get("publicationHash") {
                        ensure(
                            hash.as_str() == Some(assignment.publication_hash.as_str()),
                            "Assigned publication revision mismatch.",
                        )?;
                    }
                    if let Some(takeover) = frame.get("takeover") {
                        ensure(
                            takeover.is_boolean(),
                            "Takeover must be an explicit boolean.",
                        )?;
                    }
                    ensure(!inner.sessions.values().any(|s|s.token.source_id==instance),"Instance already has a writer; an operator must disconnect/take over first.")?;
                    ensure(
                        !inner
                            .sessions
                            .values()
                            .any(|s| s.connection == connection_id),
                        "Close the current panel before opening another.",
                    )?;
                    ensure(
                        inner.sessions.len() < MAX_CLIENTS
                            && inner.used_sessions.len() < MAX_SESSIONS,
                        "Session identity budget exhausted.",
                    )?;
                    let p=inner.publications.get(&assignment.publication_hash).ok_or("Assigned publication is unavailable; republish its immutable revision.")?.clone();
                    let session_id = uuid::Uuid::new_v4().to_string();
                    ensure(
                        inner.used_sessions.insert(session_id.clone()),
                        "Session identity collision.",
                    )?;
                    let client_id = uuid::Uuid::new_v4().to_string();
                    inner.sessions.insert(
                        session_id.clone(),
                        Session {
                            connection: connection_id.into(),
                            client_id: client_id.clone(),
                            companion_id: companion.clone(),
                            token: SourceToken {
                                source_id: instance.into(),
                                session_id: session_id.clone(),
                                generation: 1,
                                publication_hash: p.publication_hash.clone(),
                            },
                            armed: false,
                            exhausted: false,
                            sequence: 0,
                            heartbeat: 0,
                            last_seen: now,
                            batches: VecDeque::new(),
                            values: BTreeMap::new(),
                        },
                    );
                    return Ok(
                        json!({"ok":true,"type":"session-open","hostId":inner.record.host_id,"companionId":companion,"controllerInstanceId":instance,"clientId":client_id,"sessionId":session_id,"generation":"1","publicationHash":p.publication_hash,"publication":p}),
                    );
                }
                self.session_frame(&mut inner, frame, connection_id, &companion, now)
            }
        }
    }
    fn session_frame(
        &self,
        inner: &mut Inner,
        frame: &Value,
        connection: &str,
        companion: &str,
        now: u64,
    ) -> Result<Value, String> {
        let session_id = field(frame, "sessionId")?;
        let generation = counter(&frame["generation"])?;
        let session = inner
            .sessions
            .get(session_id)
            .ok_or("Session is obsolete; explicitly reconnect.")?
            .clone();
        ensure(
            session.connection == connection
                && session.companion_id == companion
                && session.token.generation == generation
                && Some(session.client_id.as_str()) == frame["clientId"].as_str()
                && Some(session.token.publication_hash.as_str())
                    == frame["publicationHash"].as_str(),
            "Stale or unauthorized session identity.",
        )?;
        ensure(frame["version"] == 1, "Unsupported protocol version.")?;
        let feedback = inner.feedback.get(&session.token.source_id).map(|f| {
            let mut values = f.values.clone();
            for value in &mut values {
                let age = value["sourceAgeMs"].as_u64().unwrap();
                value["sourceAgeMs"] = json!(age
                    .saturating_add(now.saturating_sub(f.updated))
                    .min(86_400_000));
            }
            (f.sequence.to_string(), values)
        });
        let reply = |seq: Value, kind: &str| json!({"ok":true,"type":kind,"clientId":session.client_id,"sessionId":session_id,"generation":generation.to_string(),"sequence":seq,"ackStage":"source-accepted","submitted":false,"feedbackSequence":feedback.as_ref().map(|(s,_)|s),"feedback":feedback.as_ref().map(|(_,v)|v)});
        if frame["type"] == "close" || frame["type"] == "cancel" {
            exact(
                frame,
                &[
                    "version",
                    "type",
                    "clientId",
                    "sessionId",
                    "generation",
                    "publicationHash",
                ],
                &["reason"],
            )?;
            self.disarm(inner, session_id, "remote-close", true);
            return Ok(reply(Value::Null, "closed"));
        }
        let p = inner
            .publications
            .get(&session.token.publication_hash)
            .ok_or("Publication unavailable.")?
            .clone();
        if frame["type"] == "heartbeat" {
            exact(
                frame,
                &[
                    "version",
                    "type",
                    "clientId",
                    "sessionId",
                    "generation",
                    "publicationHash",
                    "sequence",
                ],
                &[],
            )?;
            let seq = counter(&frame["sequence"])?;
            ensure(seq > session.heartbeat, "Stale heartbeat sequence.")?;
            if seq == u64::MAX {
                self.disarm(inner, session_id, "counter-exhausted", true);
                return Err("Heartbeat counter exhausted; reconnect.".into());
            }
            if session.armed {
                if let Err(error) = self.sink.heartbeat(&session.token) {
                    self.disarm(inner, session_id, "source-rejected", false);
                    return Err(error);
                }
            }
            let s = inner.sessions.get_mut(session_id).unwrap();
            s.heartbeat = seq;
            s.last_seen = now;
            return Ok(reply(frame["sequence"].clone(), "ack"));
        }
        if frame["type"] == "baseline" {
            exact(
                frame,
                &[
                    "version",
                    "type",
                    "clientId",
                    "sessionId",
                    "generation",
                    "publicationHash",
                    "sequence",
                    "values",
                ],
                &[],
            )?;
            ensure(
                counter(&frame["sequence"])? == 0 && !session.armed && !session.exhausted,
                "Baseline requires a disarmed session and sequence zero.",
            )?;
            let values: BTreeMap<String, InputValue> =
                serde_json::from_value(frame["values"].clone())
                    .map_err(|_| "Invalid baseline values.")?;
            ensure(
                values.len() == p.controls.len()
                    && p.controls
                        .iter()
                        .all(|c| values.get(&c.id) == Some(&c.fallback())),
                "Baseline must contain every control at its declared fallback.",
            )?;
            let changes: Vec<_> = values
                .iter()
                .map(|(id, value)| SourceChange {
                    control_id: id.clone(),
                    value: value.clone(),
                })
                .collect();
            if let Err(error) = self.sink.baseline(&session.token, 0, &changes) {
                self.disarm(inner, session_id, "source-rejected", false);
                return Err(error);
            }
            let s = inner.sessions.get_mut(session_id).unwrap();
            s.armed = true;
            s.values = values;
            s.sequence = 0;
            s.last_seen = now;
            return Ok(reply(json!("0"), "ack"));
        }
        if frame["type"] != "input" {
            return Err("Unknown companion protocol message.".into());
        }
        let input = (|| {
            exact(
                frame,
                &[
                    "version",
                    "type",
                    "clientId",
                    "sessionId",
                    "generation",
                    "publicationHash",
                    "sequence",
                    "changes",
                ],
                &[],
            )?;
            let seq = counter(&frame["sequence"])?;
            let changes: Vec<SourceChange> = serde_json::from_value(frame["changes"].clone())
                .map_err(|_| "Invalid input changes.")?;
            ensure(
                !changes.is_empty()
                    && changes.len() <= 64
                    && changes.iter().all(|change| {
                        p.controls
                            .iter()
                            .any(|c| c.id == change.control_id && c.accepts(&change.value))
                    }),
                "Unknown control or invalid input value.",
            )?;
            Ok::<_, String>((seq, changes))
        })();
        let (seq, changes) = match input {
            Ok(v) => v,
            Err(error) => {
                self.disarm(inner, session_id, "invalid-input", false);
                return Err(error);
            }
        };
        ensure(
            session.armed,
            "Fresh neutral baseline required before input.",
        )?;
        ensure(seq > session.sequence, "Stale input sequence.")?;
        if seq == u64::MAX || seq != session.sequence + 1 {
            self.disarm(inner, session_id, "sequence-gap", false);
            return Err("Input gap/exhaustion; fresh generation and baseline required.".into());
        }
        let s = inner.sessions.get_mut(session_id).unwrap();
        while s
            .batches
            .front()
            .is_some_and(|t| now.saturating_sub(*t) >= 1000)
        {
            s.batches.pop_front();
        }
        if s.batches.len() >= 120 {
            self.disarm(inner, session_id, "rate-limit", false);
            return Err("Input rate exceeded; fresh generation and baseline required.".into());
        }
        if let Err(error) = self.sink.changes(&session.token, seq, &changes) {
            self.disarm(inner, session_id, "source-rejected", false);
            return Err(error);
        }
        let s = inner.sessions.get_mut(session_id).unwrap();
        for c in changes {
            s.values.insert(c.control_id, c.value);
        }
        s.batches.push_back(now);
        s.sequence = seq;
        s.last_seen = now;
        Ok(reply(frame["sequence"].clone(), "ack"))
    }
}

fn validate_feedback(publication: &Publication, values: &[Value]) -> Result<(), String> {
    ensure(values.len() <= 64, "Feedback value budget exceeded.")?;
    let doc: Value = serde_json::from_str(&publication.canonical_document)
        .map_err(|_| "Invalid retained publication.")?;
    let widgets = doc["ui"]["feedbackWidgets"].as_array().unwrap();
    let mut channels = BTreeSet::new();
    for v in values {
        exact(
            v,
            &[
                "channelId",
                "value",
                "provenance",
                "sourceKind",
                "sourceAgeMs",
                "unit",
            ],
            &["range"],
        )?;
        let channel = field(v, "channelId")?;
        ensure(channels.insert(channel), "Repeated feedback channel.")?;
        let bound: Vec<_> = widgets
            .iter()
            .filter(|w| w["channelId"] == channel)
            .collect();
        ensure(!bound.is_empty(), "Unknown feedback channel.")?;
        ensure(
            matches!(
                v["provenance"].as_str(),
                Some("requested" | "local" | "host-accepted" | "host-submitted" | "game-confirmed")
            ) && matches!(
                v["sourceKind"].as_str(),
                Some("host" | "simulator" | "script")
            ),
            "Invalid feedback provenance.",
        )?;
        ensure(
            v["sourceAgeMs"].as_u64().is_some_and(|n| n <= 86_400_000)
                && v["unit"].as_str().is_some_and(|s| {
                    !s.trim().is_empty()
                        && s.encode_utf16().count() <= 32
                        && !s.chars().any(|c| c <= '\u{1f}')
                }),
            "Invalid feedback age/unit.",
        )?;
        for widget in bound {
            ensure(
                match widget["kind"].as_str().unwrap() {
                    "text" => v["value"].as_str().is_some_and(|s| {
                        !s.trim().is_empty()
                            && s.encode_utf16().count() <= 256
                            && !s.chars().any(|c| c <= '\u{1f}')
                    }),
                    "indicator" => v["value"].is_boolean(),
                    _ => v["value"].as_f64().is_some_and(f64::is_finite),
                },
                "Feedback value does not match widget type.",
            )?;
        }
        if let Some(range) = v.get("range") {
            exact(range, &["min", "max"], &[])?;
            let min = range["min"]
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or("Invalid feedback range.")?;
            let max = range["max"]
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or("Invalid feedback range.")?;
            ensure(
                min < max
                    && v["value"]
                        .as_f64()
                        .is_some_and(|n| (min..=max).contains(&n)),
                "Feedback exceeds declared range.",
            )?;
        }
    }
    Ok(())
}
