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
    #[serde(default = "unknown_kind")]
    kind: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AssignmentOwner {
    owner: String,
    revision: u64,
    baseline_hash: String,
    baseline_publication_hash: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Assignment {
    instance_id: String,
    companion_id: String,
    publication_hash: String,
    definition: SourceDefinition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ownership: Option<AssignmentOwner>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    schema_version: u8,
    host_id: String,
    credentials: Vec<Credential>,
    instances: Vec<Assignment>,
}
fn unknown_kind() -> String {
    "unknown".into()
}
struct CodeOffer {
    code: Zeroizing<String>,
    payload: Value,
    expires: u64,
    attempts: u8,
}
struct Offer {
    secret_hash: String,
    expires: u64,
    consumed: bool,
    auto_approve: bool,
    kind: String,
}
struct Pending {
    kind: String,
    companion_id: String,
    name: String,
    secret_hash: String,
    expires: u64,
    token: Option<Zeroizing<String>>,
}
#[derive(Clone)]
struct Session {
    connection: String,
    transport: SourceTransport,
    client_id: String,
    companion_id: String,
    token: SourceToken,
    armed: bool,
    panel_error: bool,
    exhausted: bool,
    sequence: u64,
    heartbeat: u64,
    last_seen: u64,
    batches: VecDeque<u64>,
    values: BTreeMap<String, InputValue>,
    feedback_cursor: crate::feedback::FeedbackCursor,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceTransport { Network, Usb }
struct Inner {
    record: Record,
    publications: BTreeMap<String, Publication>,
    offer: Option<Offer>,
    code_offer: Option<CodeOffer>,
    pending: BTreeMap<String, Pending>,
    connections: BTreeMap<String, String>,
    sessions: BTreeMap<String, Session>,
    used_sessions: BTreeSet<String>,
    feedback: BTreeMap<String, crate::feedback::FeedbackBook>,
    last_now: u64,
    diagnostic: Option<String>,
}

#[derive(Clone)]
pub struct Hub {
    inner: Arc<Mutex<Inner>>,
    sink: Arc<dyn SourceSink>,
    storage: Option<Arc<dyn StateStorage>>,
    started: Arc<Instant>,
    runtime_identity: Arc<String>,
    record_revision: Arc<std::sync::atomic::AtomicU64>,
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
pub(crate) fn counter(value: &Value) -> Result<u64, String> {
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
pub(crate) fn exact(v: &Value, required: &[&str], optional: &[&str]) -> Result<(), String> {
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
    /// Inspect retained local metadata without creating a host identity or store.
    pub fn open_existing(path: impl AsRef<Path>, sink: Arc<dyn SourceSink>) -> Result<Option<Self>, String> {
        Self::with_existing_storage(Arc::new(SealedFileStore::new(
            path.as_ref().join("companion-host-state.dpapi"),
        )), sink)
    }
    pub fn with_existing_storage(storage: Arc<dyn StateStorage>, sink: Arc<dyn SourceSink>) -> Result<Option<Self>, String> {
        let Some(mut bytes) = storage.read()? else { return Ok(None); };
        let record = serde_json::from_slice(&bytes)
            .map_err(|_| "Invalid protected companion host state.");
        bytes.zeroize();
        Self::from_record(record?, sink, Some(storage)).map(Some)
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
                    && a.definition.controls.len() <= 128
                    && a.ownership.as_ref().is_none_or(|owner|
                        uuid::Uuid::parse_str(&owner.owner).is_ok_and(|id|id.to_string()==owner.owner)
                        && owner.revision > 0 && digest_id(&owner.baseline_hash)
                        && digest_id(&owner.baseline_publication_hash)),
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
                code_offer: None,
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
            runtime_identity: Arc::new(uuid::Uuid::new_v4().to_string()),
            record_revision: Arc::new(std::sync::atomic::AtomicU64::new(0)),
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

    /// Trusted local persistence boundary; never exposed as a wire operation.
    /// Metadata only: unlike operator status, this does not expire sessions or
    /// copy publication documents/assets while a profile editor is browsing.
    pub fn publication_summaries(&self) -> Vec<Value> {
        self.inner.lock().unwrap_or_else(|p|p.into_inner()).publications.values().map(|p|json!({
            "publicationHash":p.publication_hash,"capabilityFingerprint":p.capability_fingerprint,
            "title":p.title,"controlCount":p.controls.len(),"assetCount":p.assets.len()
        })).collect()
    }

    /// Trusted local persistence boundary; never exposed as a wire operation.
    pub fn publication_uploads(&self) -> Vec<PublicationUpload> {
        self.inner.lock().unwrap_or_else(|p|p.into_inner()).publications.values().map(|p|PublicationUpload {
            canonical_document:p.canonical_document.clone(),publication_hash:p.publication_hash.clone(),
            capability_fingerprint:Some(p.capability_fingerprint.clone()),assets:p.assets.clone(),
        }).collect()
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
        // Even an uncertain attempted native edit expires in-flight metadata
        // proofs. This fence grants no ownership and is never a wire field.
        self.record_revision.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
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
            if let Some(feedback) = inner.feedback.get_mut(&session.token.source_id) {
                feedback.disconnect(inner.last_now);
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
        if inner.code_offer.as_ref().is_some_and(|o| o.expires <= now) {
            inner.code_offer = None;
        }
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
    fn approve_pending(
        &self,
        inner: &mut Inner,
        request_id: &str,
        expected: Option<&str>,
    ) -> Result<Value, String> {
        let pending = inner
            .pending
            .get(request_id)
            .ok_or("Pairing request expired or missing.")?;
        if let Some(expected) = expected {
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
            kind: pending.kind.clone(),
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
            self.disarm(inner, &id, "credential-rotated", true);
        }
        inner
            .connections
            .retain(|_, c| c != &credential.companion_id);
        inner.record = record;
        inner.pending.get_mut(request_id).unwrap().token = Some(token);
        Ok(json!({"approved":true,"companionId":credential.companion_id}))
    }

    fn revoke_inner(&self,inner:&mut Inner,companion:&str)->Result<(),String>{
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
                    self.disarm(inner, &id, "revoked", true);
                }
                inner.connections.retain(|_, c| c != companion);
                inner.pending.retain(|_, p| p.companion_id != companion);
                inner.record = record;
                let retained=inner.record.instances.iter().map(|a|a.instance_id.clone()).collect::<BTreeSet<_>>();
                inner.feedback.retain(|id,_|retained.contains(id));
        Ok(())
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
                    auto_approve: request["autoApprove"] == true,
                    kind: request["kind"].as_str().filter(|s| ["browser","desktop","mobile","esp32"].contains(s)).unwrap_or("desktop").into(),
                });
                Ok(
                    json!({"secret":secret,"expiresAtMs":expires,"ttlMs":PAIRING_MS,"hostId":inner.record.host_id}),
                )
            }
            "companion_approve" | "companion_pair_approve" => {
                self.approve_pending(&mut inner, field(request, "requestId")?, request["companionId"].as_str())
            }
            "companion_pair_code" => {
                let payload = request["payload"].clone();
                ensure(payload["secret"].as_str().is_some_and(secret_valid), "Create a pairing offer first.")?;
                let code = crate::pairing_code::new_code()?;
                inner.code_offer = Some(CodeOffer { code: Zeroizing::new(code.clone()), payload, expires: now.saturating_add(PAIRING_MS), attempts: 0 });
                Ok(json!({"code":code,"ttlMs":PAIRING_MS}))
            }
            "companion_pair_cancel" => {
                inner.offer = None; inner.code_offer = None;
                inner.pending.retain(|_, p| p.token.is_some());
                Ok(json!({"cancelled":true}))
            }
            "companion_reject" => {
                ensure(
                    inner.pending.remove(field(request, "requestId")?).is_some(),
                    "Unknown pairing request.",
                )?;
                Ok(json!({"rejected":true}))
            }
            "companion_update" => {
                let companion=field(request,"companionId")?;
                let name=field(request,"name")?;
                ensure(label(name),"Enter a valid companion name.")?;
                let mut record=inner.record.clone();
                let client=record.credentials.iter_mut().find(|c|c.companion_id==companion).ok_or("Unknown companion.")?;
                client.name=name.into();
                self.persist(&record)?;inner.record=record;
                Ok(Self::status(&inner))
            }
            "companion_revoke" => {
                let companion=field(request,"companionId")?;
                self.revoke_inner(&mut inner,companion)?;
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
            "companion_prune" => {
                let hashes=request["hashes"].as_array().filter(|a|a.len()<=100).ok_or("Invalid panel retention list.")?;
                let ids=request["panelIds"].as_array().filter(|a|a.len()<=128).ok_or("Invalid panel identity list.")?;
                let mut managed=BTreeSet::new();
                for value in ids {let ui=value.as_str().filter(|s|id(s)).ok_or("Invalid panel identity.")?;managed.insert(ui.to_owned());}
                let mut retained=BTreeSet::new();
                for value in hashes { let hash=value.as_str().filter(|s|digest_id(s)).ok_or("Invalid panel retention hash.")?;retained.insert(hash.to_owned()); }
                retained.extend(inner.record.instances.iter().map(|a|a.publication_hash.clone()));
                retained.extend(inner.record.instances.iter().filter_map(|a|a.ownership.as_ref().map(|owner|owner.baseline_publication_hash.clone())));
                retained.extend(inner.sessions.values().map(|s|s.token.publication_hash.clone()));
                let before=inner.publications.len();
                inner.publications.retain(|hash,p|!managed.contains(&p.ui_id)||retained.contains(hash));
                Ok(json!({"removed":before-inner.publications.len()}))
            }
            "companion_unassign" => {
                let instance=field(request,"instanceId")?;
                let mut record=inner.record.clone();
                ensure(record.instances.iter().any(|a|a.instance_id==instance),"Unknown controller instance.")?;
                record.instances.retain(|a|a.instance_id!=instance);
                let sessions=inner.sessions.iter().filter(|(_,s)|s.token.source_id==instance).map(|(id,_)|id.clone()).collect::<Vec<_>>();
                for id in sessions { self.disarm(&mut inner,&id,"assignment-removed",true); }
                self.persist(&record)?;
                inner.record=record;
                inner.feedback.remove(instance);
                Ok(json!({"unassigned":true}))
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
                    ownership: None,
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
                record.instances.iter_mut().find(|a|a.instance_id==instance).unwrap().ownership = None;
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
            "companion_feedback" => Err("Raw feedback commands cannot assert output or simulator truth; use the configured native integration sink.".into()),
            _ => Err("Unknown local companion command.".into()),
        }
    }
    fn status(inner: &Inner) -> Value {
        json!({"hostId":inner.record.host_id,"diagnostic":inner.diagnostic,
            "clients":inner.record.credentials.iter().map(|client| {
                // The native adapter supplies each Session's transport. Kind,
                // display names and an authenticated connection cannot prove it.
                let transports=inner.sessions.values().filter(|session|session.companion_id==client.companion_id)
                    .map(|session|session.transport).collect::<BTreeSet<_>>();
                json!({"companionId":client.companion_id,"name":client.name,"kind":client.kind,
                    "connected":inner.connections.values().any(|value|value==&client.companion_id),"transports":transports,
                    "panelError":if inner.sessions.values().any(|s|s.companion_id==client.companion_id&&s.panel_error){Some("The screen cannot display the assigned panel. Remove unsupported content in the panel editor.")}else{None}})
            }).collect::<Vec<_>>(),
            "pending":inner.pending.iter().map(|(id,pending)|json!({"requestId":id,"companionId":pending.companion_id,"name":pending.name,"expiresAtMs":pending.expires,"approved":pending.token.is_some()})).collect::<Vec<_>>(),
            "instances":inner.record.instances.iter().map(|assignment| {
                let session=inner.sessions.values().find(|session|session.token.source_id==assignment.instance_id);
                json!({"instanceId":assignment.instance_id,"sourceId":assignment.instance_id,"companionId":assignment.companion_id,
                    "publicationHash":assignment.publication_hash,"name":assignment.definition.name,
                    "connected":session.is_some(),"transport":session.map(|session|session.transport),
                    "armed":session.is_some_and(|session|session.armed),"generation":session.map(|session|session.token.generation.to_string()),
                    "sessionId":session.map(|session|&session.token.session_id),"values":session.map(|session|&session.values)})
            }).collect::<Vec<_>>(),
            "publications":inner.publications.values().map(|publication|json!({"publicationHash":publication.publication_hash,"capabilityFingerprint":publication.capability_fingerprint,"uiId":publication.ui_id,"title":publication.title,"controlCount":publication.controls.len(),"assetCount":publication.assets.len()})).collect::<Vec<_>>()})
    }
    pub fn handle_wire(&self, frame: Value, connection_id: &str, now_ms: u64) -> Value {
        self.handle_native_wire(frame, connection_id, now_ms, SourceTransport::Network)
    }
    /// Only the native serial adapter selects this entry. Peer payloads cannot
    /// claim a transport or alter the transport of an existing source owner.
    pub fn handle_serial_wire(&self, frame: Value, connection_id: &str, now_ms: u64) -> Value {
        self.handle_native_wire(frame, connection_id, now_ms, SourceTransport::Usb)
    }
    pub fn source_transport(&self, source_id: &str) -> Option<SourceTransport> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner()).sessions.values()
            .find(|session| session.armed && session.token.source_id == source_id).map(|session| session.transport)
    }
    fn handle_native_wire(&self, frame: Value, connection_id: &str, now_ms: u64, transport: SourceTransport) -> Value {
        let result = self.wire(&frame, connection_id, now_ms, transport);
        match result {
            Ok(value) => value,
            Err(error) => json!({"ok":false,"type":"error","error":error}),
        }
    }
    fn wire(&self, frame: &Value, connection_id: &str, now: u64, transport: SourceTransport) -> Result<Value, String> {
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
            "pair-code" => {
                exact(frame, &["type", "message"], &["version"])?;
                let offer = inner
                    .code_offer
                    .as_mut()
                    .ok_or("Start pairing in Cockpit.")?;
                ensure(
                    offer.expires > now && offer.attempts < 5,
                    "Pairing code expired or attempt limit reached. Start again in Cockpit.",
                )?;
                offer.attempts += 1;
                crate::pairing_code::respond(&offer.code, field(frame, "message")?, &offer.payload)
            }
            "pair" => {
                exact(
                    frame,
                    &["type", "secret", "companionId", "name"],
                    &["version","replaceToken"],
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
                let browser_offer=offer.kind=="browser";
                let replace=if let Some(value)=frame.get("replaceToken"){
                    let token=value.as_str().ok_or("Invalid replacement credential.")?;
                    ensure(secret_valid(token)&&browser_offer,"Replacement requires a browser pairing offer.")?;
                    let digest=hash(token.as_bytes());
                    let old=inner.record.credentials.iter().find(|c|equal_hash(&c.token_hash,&digest));
                    if let Some(old)=old{ensure(old.kind=="browser","Only this browser pairing may be replaced.")?;}
                    old.map(|c|c.companion_id.clone())
                }else{None};
                if let Some(old)=replace{self.revoke_inner(&mut inner,&old)?;}
                let offer=inner.offer.as_mut().ok_or("Pairing offer is unavailable.")?;
                offer.consumed = true;
                let auto_approve = offer.auto_approve;
                let pending = Pending {
                    kind: offer.kind.clone(),
                    companion_id: companion.into(),
                    name: name.into(),
                    secret_hash: offer.secret_hash.clone(),
                    expires: offer.expires,
                    token: None,
                };
                let request_id = uuid::Uuid::new_v4().to_string();
                inner.pending.insert(request_id.clone(), pending);
                if auto_approve {
                    self.approve_pending(&mut inner, &request_id, Some(companion))?;
                    inner.code_offer = None;
                }
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
            "presence" => {
                exact(frame, &["type"], &["version"])?;
                let companion = inner
                    .connections
                    .get(connection_id)
                    .ok_or("Authenticate this companion first.")?;
                Ok(
                    json!({"ok":true,"type":"presence","assignments":inner.record.instances.iter().filter(|a| &a.companion_id==companion).map(|a|json!({"instanceId":a.instance_id,"publicationHash":a.publication_hash,"name":a.definition.name})).collect::<Vec<_>>()}),
                )
            }
            "auth" => {
                exact(frame, &["type", "token"], &["version"])?;
                ensure(
                    inner.connections.contains_key(connection_id)
                        || inner.connections.len() < MAX_CLIENTS * 2,
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
                    inner.connections.values().any(|c| c == &companion)
                        || inner.connections.values().collect::<BTreeSet<_>>().len() < MAX_CLIENTS,
                    "Connected companion budget exhausted.",
                )?;
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
                            transport,
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
                            panel_error: false,
                            last_seen: now,
                            batches: VecDeque::new(),
                            values: BTreeMap::new(),
                            feedback_cursor: Default::default(),
                        },
                    );
                    return Ok(
                        json!({"ok":true,"type":"session-open","hostId":inner.record.host_id,"companionId":companion,"controllerInstanceId":instance,"clientId":client_id,"sessionId":session_id,"generation":"1","publicationHash":p.publication_hash,"publication":p}),
                    );
                }
                let mut reply =
                    self.session_frame(&mut inner, frame, connection_id, &companion, now)?;
                if reply["type"] != "closed" {
                    self.attach_feedback(&mut inner, frame, &mut reply, now)?;
                }
                Ok(reply)
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
        let reply = |seq: Value, kind: &str| json!({"ok":true,"type":kind,"clientId":session.client_id,"sessionId":session_id,"generation":generation.to_string(),"publicationHash":session.token.publication_hash,"sequence":seq,"ackStage":"source-accepted","submitted":false});
        if frame["type"] == "feedback-resync" {
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
            counter(&frame["sequence"])?;
            inner.sessions.get_mut(session_id).unwrap().feedback_cursor = Default::default();
            return Ok(reply(frame["sequence"].clone(), "feedback-resync"));
        }
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
                &["panelError"],
            )?;
            ensure(frame.get("panelError").is_none_or(Value::is_boolean),"Invalid panel error status.")?;
            ensure(frame["panelError"]!=true || !session.armed,"Disarm inputs before reporting a panel error.")?;
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
            s.panel_error = frame["panelError"]==true;
            s.last_seen = now;
            return Ok(reply(frame["sequence"].clone(), "ack"));
        }
        if frame["type"] == "baseline" {
            ensure(!session.panel_error,"Repair the panel before arming inputs.")?;
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

#[path = "profile.rs"]
pub mod profile;

#[path = "hub_assignments.rs"]
mod assignments_api;
#[path = "hub_feedback.rs"]
mod feedback_api;
