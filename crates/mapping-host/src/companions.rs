//! Retained companion source definitions and bounded, isolated native mapping feeds.
//! No socket, client timestamp, HID discovery or UI polling participates in this clock.
use osg_desktop_output::audit::{self, AliasKind, Kind as AuditKind};
use osg_mapping::{
    engine::{Assignment, Change, Counter, Frame, Source},
    profile::{self, Kind, Origin, Profile, Value},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Mutex},
};
pub const MAX_REGISTERED: usize = 128;
pub const QUEUE_LIMIT: usize = 512;
pub const LEASE_US: u64 = 1_500_000;
const MAX_SUBSCRIPTIONS: usize = 16;
type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlDefinition {
    pub id: String,
    pub kind: Kind,
    pub fallback: Value,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Definition {
    pub source_id: String,
    pub name: String,
    pub publication_hash: String,
    pub controls: Vec<ControlDefinition>,
}
#[derive(Clone, Debug)]
pub struct ResourceDescriptor {
    pub definition: Definition,
    pub online: bool,
    pub baseline_ready: bool,
    pub generation: u64,
    pub fault: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputToken {
    pub source_id: String,
    pub session_id: String,
    pub generation: u64,
    pub publication_hash: String,
}
struct Instance {
    definition: Definition,
    owner: Option<InputToken>,
    wire_sequence: u64,
    last_seen_us: u64,
    generation: u64,
    sessions: BTreeMap<String, u64>,
    values: BTreeMap<String, Value>,
    fault: Option<String>,
}
#[derive(Default)]
struct Queue {
    controls: BTreeSet<String>,
    publication_hash: String,
    explicit_restart: bool,
    restart_required: bool,
    frames: VecDeque<NativeFrame>,
    loss: Option<NativeFrame>,
    audit_domain: u64,
    started: bool,
    generation: u64,
    sequence: u64,
}
#[derive(Default)]
struct Subscription {
    sources: BTreeMap<String, Queue>,
    next: usize,
}
#[derive(Default)]
struct State {
    instances: BTreeMap<String, Instance>,
    subscriptions: BTreeMap<u64, Subscription>,
    serial: u64,
    audit_provider: u64,
}
#[derive(Clone)]
pub struct CompanionProvider {
    state: Arc<Mutex<State>>,
    clock: Clock,
    audit_provider: u64,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// In-process diagnostics only; the portable source frame and its clock stay intact.
#[derive(Clone, Debug)]
pub struct NativeFrame {
    pub frame: Frame,
    pub diagnostic: Option<NativeFrameDiagnostic>,
}
#[derive(Clone, Copy, Debug)]
pub struct NativeFrameDiagnostic {
    pub frame_id: u64,
    pub provider: u64,
    pub subscription: u64,
    pub queue_domain: u64,
    pub physical_source: u64,
    /// 1 baseline, 2 current-state renewal, 3 loss, 4 non-WSS change, 0 capture.
    pub kind: u64,
}
impl From<Frame> for NativeFrame {
    fn from(frame: Frame) -> Self {
        Self {
            frame,
            diagnostic: None,
        }
    }
}
impl std::ops::Deref for NativeFrame {
    type Target = Frame;
    fn deref(&self) -> &Frame {
        &self.frame
    }
}
fn audit_native_frame(
    provider: u64,
    subscription: u64,
    queue: &Queue,
    id: &str,
    frame: Frame,
    capture: Option<(u64, usize)>,
    kind: u64,
) -> NativeFrame {
    if audit::enabled() && (provider == 0 || queue.audit_domain == 0) {
        audit::fault(audit::Fault::Provenance);
    }
    let diagnostic = if audit::enabled() && provider != 0 && queue.audit_domain != 0 {
        audit::next_id()
            .zip(audit::alias(AliasKind::NativeSource, id.as_bytes()))
            .map(|(frame_id, physical_source)| NativeFrameDiagnostic {
                frame_id,
                provider,
                subscription,
                queue_domain: queue.audit_domain,
                physical_source,
                kind,
            })
    } else {
        None
    };
    if let Some(d) = diagnostic {
        if let Some((capture, change_index)) = capture {
            let port = frame
                .changes
                .first()
                .and_then(|change| audit::alias(AliasKind::Port, change.control.as_bytes()))
                .unwrap_or(0);
            audit::record(
                AuditKind::NativeEnqueue,
                &[
                    capture,
                    change_index as u64,
                    d.frame_id,
                    d.provider,
                    d.subscription,
                    d.queue_domain,
                    d.physical_source,
                    frame.generation.0,
                    frame.sequence.0,
                    port,
                    frame.captured_us.0,
                    queue.frames.len() as u64 + 1,
                ],
            );
        } else {
            audit::record(
                AuditKind::SyntheticFrame,
                &[
                    d.frame_id,
                    d.provider,
                    d.subscription,
                    d.queue_domain,
                    d.physical_source,
                    frame.generation.0,
                    frame.sequence.0,
                    kind,
                    frame.captured_us.0,
                ],
            );
        }
    }
    NativeFrame { frame, diagnostic }
}
fn audit_dequeue(frame: &NativeFrame, remaining: usize) {
    if let Some(d) = frame.diagnostic {
        audit::record(
            AuditKind::NativeDequeue,
            &[
                d.frame_id,
                d.provider,
                d.subscription,
                d.queue_domain,
                d.physical_source,
                frame.generation.0,
                frame.sequence.0,
                remaining as u64,
            ],
        );
    }
}
pub(crate) struct Prepared {
    pub controls: BTreeMap<Source, BTreeMap<String, Kind>>,
    pub feed: Feed,
}
pub(crate) struct Feed {
    provider: CompanionProvider,
    subscription: u64,
}
fn valid_hash(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value.as_bytes()[7..]
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
}
fn source(id: &str) -> Source {
    Source {
        origin: Origin::Companion,
        id: id.into(),
    }
}
fn validate_definition(value: &Definition) -> Result<(), String> {
    if !profile::identifier(&value.source_id)
        || value.name.trim().is_empty()
        || value.name.chars().count() > 120
        || value.name.chars().any(char::is_control)
        || !valid_hash(&value.publication_hash)
        || value.controls.is_empty()
        || value.controls.len() > 128
    {
        return Err("Invalid companion source definition.".into());
    }
    let mut ids = BTreeSet::new();
    for control in &value.controls {
        if !profile::identifier(&control.id)
            || !ids.insert(&control.id)
            || !control.kind.accepts(control.fallback)
            || matches!(control.fallback, Value::Button(true) | Value::Hat(Some(_)))
        {
            return Err("Invalid companion control or fallback.".into());
        }
    }
    Ok(())
}
fn token_valid(token: &InputToken) -> bool {
    profile::identifier(&token.source_id)
        && profile::identifier(&token.session_id)
        && token.generation > 0
        && valid_hash(&token.publication_hash)
}
fn loss_queue(queue: &mut Queue, id: &str, now: u64, provider: u64, subscription: u64) {
    queue.frames.clear();
    if queue.started && queue.generation > 0 && queue.explicit_restart {
        queue.restart_required = true;
    }
    if queue.started && queue.generation > 0 && queue.loss.is_none() {
        if let Some(sequence) = queue.sequence.checked_add(1) {
            queue.sequence = sequence;
            let frame = Frame {
                source: source(id),
                generation: Counter(queue.generation),
                sequence: Counter(sequence),
                captured_us: Counter(now),
                reason: osg_mapping::engine::Reason::Disconnect,
                changes: vec![],
            };
            queue.loss = Some(audit_native_frame(
                provider,
                subscription,
                queue,
                id,
                frame,
                None,
                3,
            ));
        }
    }
}
fn lose(state: &mut State, id: &str, now: u64, reason: &str) {
    if let Some(instance) = state.instances.get_mut(id) {
        instance.owner = None;
        instance.values.clear();
        instance.fault = Some(reason.into());
    }
    for (subscription_id, subscription) in &mut state.subscriptions {
        if let Some(queue) = subscription.sources.get_mut(id) {
            loss_queue(queue, id, now, state.audit_provider, *subscription_id);
            if state.audit_provider != 0 {
                let source = audit::alias(AliasKind::NativeSource, id.as_bytes()).unwrap_or(0);
                let code = if reason.contains("overflow") {
                    3
                } else if reason.contains("expired") {
                    1
                } else if reason.contains("gap") || reason.contains("invalid") {
                    2
                } else {
                    0
                };
                audit::record(
                    AuditKind::NativeGap,
                    &[source, state.audit_provider, *subscription_id, code],
                );
            }
        }
    }
}
fn expire(state: &mut State, now: u64) {
    let expired = state
        .instances
        .iter()
        .filter(|(_, instance)| {
            instance.owner.is_some() && now.saturating_sub(instance.last_seen_us) >= LEASE_US
        })
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    for id in expired {
        lose(
            state,
            &id,
            now,
            "Companion native lease expired; acquire a fresh baseline.",
        );
    }
}
impl CompanionProvider {
    /// Descriptors and liveness are captured together without subscribing or
    /// returning input values. A publication replacement changes its generation.
    pub fn resource_inventory(&self) -> Vec<ResourceDescriptor> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        expire(&mut state, (self.clock)());
        state
            .instances
            .values()
            .map(|instance| ResourceDescriptor {
                definition: instance.definition.clone(),
                online: instance.owner.is_some(),
                baseline_ready: instance.owner.is_some()
                    && instance.values.len() == instance.definition.controls.len(),
                generation: instance.generation,
                fault: instance.fault.clone(),
            })
            .collect()
    }

    pub fn new(clock: Clock) -> Self {
        let audit_provider = audit::next_id().unwrap_or(0);
        Self {
            state: Arc::new(Mutex::new(State {
                audit_provider,
                ..State::default()
            })),
            clock,
            audit_provider,
            wake: None,
        }
    }
    /// Wake the native consumer after source admission has committed and its
    /// provider lock has been released. This callback grants no source authority.
    pub fn with_wake(clock: Clock, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let mut provider = Self::new(clock);
        provider.wake = Some(wake);
        provider
    }
    fn notify_consumer(&self) {
        if let Some(wake) = &self.wake { wake(); }
    }
    fn now(&self) -> u64 {
        (self.clock)().max(1)
    }
    pub fn register_definition(&self, definition: Definition) -> Result<(), String> {
        validate_definition(&definition)?;
        let now = self.now();
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(prior) = state.instances.get(&definition.source_id) {
            if prior.definition == definition {
                return Ok(());
            }
            if prior.definition.controls != definition.controls
                && state
                    .subscriptions
                    .values()
                    .any(|s| s.sources.contains_key(&definition.source_id))
            {
                return Err(
                    "Stop or reassign this companion before changing its descriptor.".into(),
                );
            }
            let id = definition.source_id.clone();
            lose(
                &mut state,
                &id,
                now,
                "Companion publication changed; acquire a fresh baseline.",
            );
            state.instances.get_mut(&id).unwrap().definition = definition;
        } else {
            if state.instances.len() >= MAX_REGISTERED {
                return Err("Companion source registry is full.".into());
            }
            state.instances.insert(
                definition.source_id.clone(),
                Instance {
                    definition,
                    owner: None,
                    wire_sequence: 0,
                    last_seen_us: 0,
                    generation: 0,
                    sessions: BTreeMap::new(),
                    values: BTreeMap::new(),
                    fault: None,
                },
            );
        }
        Ok(())
    }
    pub fn baseline_native(
        &self,
        token: &InputToken,
        sequence: u64,
        changes: &[Change],
    ) -> Result<(), String> {
        if !token_valid(token) || sequence != 0 {
            return Err(
                "Companion baselines use a valid fresh identity and wire sequence zero.".into(),
            );
        }
        let now = self.now();
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        expire(&mut state, now);
        let instance = state
            .instances
            .get(&token.source_id)
            .ok_or("Unknown companion instance.")?;
        if token.publication_hash != instance.definition.publication_hash {
            return Err("Stale companion publication.".into());
        }
        if instance
            .sessions
            .get(&token.session_id)
            .is_some_and(|prior| *prior >= token.generation)
        {
            return Err("Stale companion session generation.".into());
        }
        if !instance.sessions.contains_key(&token.session_id) && instance.sessions.len() >= 512 {
            return Err("Companion session identity budget exhausted.".into());
        }
        let mut values = BTreeMap::new();
        for change in changes {
            if !instance
                .definition
                .controls
                .iter()
                .any(|control| control.id == change.control && control.fallback == change.value)
                || values
                    .insert(change.control.clone(), change.value)
                    .is_some()
            {
                return Err("A complete companion baseline must contain every control at its declared fallback.".into());
            }
        }
        if values.len() != instance.definition.controls.len() {
            return Err("Incomplete companion baseline.".into());
        }
        let generation = instance
            .generation
            .checked_add(1)
            .ok_or("Companion mapping generation exhausted.")?;
        // Superseding a live writer releases its old contributions before the new baseline.
        if instance.owner.is_some() {
            lose(
                &mut state,
                &token.source_id,
                now,
                "Companion writer changed.",
            );
        }
        let instance = state.instances.get_mut(&token.source_id).unwrap();
        instance
            .sessions
            .insert(token.session_id.clone(), token.generation);
        instance.owner = Some(token.clone());
        instance.wire_sequence = 0;
        instance.last_seen_us = now;
        instance.generation = generation;
        instance.values = values.clone();
        instance.fault = None;
        for (subscription_id, subscription) in &mut state.subscriptions {
            if let Some(queue) = subscription.sources.get_mut(&token.source_id).filter(|q| {
                q.started && !q.restart_required && q.publication_hash == token.publication_hash
            }) {
                queue.frames.clear();
                queue.generation = generation;
                queue.sequence = 1;
                queue.audit_domain = audit::next_id().unwrap_or(0);
                let frame = Frame {
                    source: source(&token.source_id),
                    generation: Counter(generation),
                    sequence: Counter(1),
                    captured_us: Counter(now),
                    reason: osg_mapping::engine::Reason::Baseline,
                    changes: values
                        .iter()
                        .filter(|(id, _)| queue.controls.contains(*id))
                        .map(|(control, value)| Change {
                            control: control.clone(),
                            value: *value,
                        })
                        .collect(),
                };
                queue.frames.push_back(audit_native_frame(
                    self.audit_provider,
                    *subscription_id,
                    queue,
                    &token.source_id,
                    frame,
                    None,
                    1,
                ));
            }
        }
        Ok(())
    }
    pub fn changes_native(
        &self,
        token: &InputToken,
        sequence: u64,
        changes: &[Change],
    ) -> Result<(), String> {
        self.changes_native_audited(token, sequence, changes, None)
    }
    fn changes_native_audited(
        &self,
        token: &InputToken,
        sequence: u64,
        changes: &[Change],
        capture: Option<u64>,
    ) -> Result<(), String> {
        let now = self.now();
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        expire(&mut state, now);
        let instance = state
            .instances
            .get(&token.source_id)
            .ok_or("Unknown companion instance.")?;
        if instance.owner.as_ref() != Some(token) {
            return Err("Stale companion writer.".into());
        }
        if sequence <= instance.wire_sequence {
            return Err("Stale companion input sequence.".into());
        }
        let valid = instance.wire_sequence.checked_add(1) == Some(sequence)
            && !changes.is_empty()
            && changes.len() <= 64
            && changes.iter().all(|change| {
                instance.definition.controls.iter().any(|control| {
                    control.id == change.control && control.kind.accepts(change.value)
                })
            });
        if !valid {
            lose(
                &mut state,
                &token.source_id,
                now,
                "Companion input gap or invalid values; acquire a fresh baseline.",
            );
            return Err("Companion input gap or invalid values.".into());
        }
        let overflow = state
            .subscriptions
            .values()
            .filter_map(|s| s.sources.get(&token.source_id))
            .filter(|q| {
                q.started && !q.restart_required && q.publication_hash == token.publication_hash
            })
            .any(|q| {
                let count = changes
                    .iter()
                    .filter(|c| q.controls.contains(&c.control))
                    .count();
                q.frames.len() + count > QUEUE_LIMIT
                    || q.sequence
                        .checked_add(count as u64)
                        .is_none_or(|next| next >= u64::MAX)
            });
        if overflow {
            lose(
                &mut state,
                &token.source_id,
                now,
                "Companion transition queue overflow; acquire a fresh baseline.",
            );
            return Err("Companion transition queue overflow.".into());
        }
        let instance = state.instances.get_mut(&token.source_id).unwrap();
        for change in changes {
            instance.values.insert(change.control.clone(), change.value);
        }
        instance.wire_sequence = sequence;
        instance.last_seen_us = now;
        for (subscription_id, subscription) in &mut state.subscriptions {
            if let Some(queue) = subscription.sources.get_mut(&token.source_id).filter(|q| {
                q.started && !q.restart_required && q.publication_hash == token.publication_hash
            }) {
                for (change_index, change) in changes
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| queue.controls.contains(&c.control))
                {
                    queue.sequence += 1;
                    let frame = Frame {
                        source: source(&token.source_id),
                        generation: Counter(queue.generation),
                        sequence: Counter(queue.sequence),
                        captured_us: Counter(now),
                        reason: osg_mapping::engine::Reason::Change,
                        changes: vec![change.clone()],
                    };
                    let origin = capture.map(|capture| (capture, change_index));
                    queue.frames.push_back(audit_native_frame(
                        self.audit_provider,
                        *subscription_id,
                        queue,
                        &token.source_id,
                        frame,
                        origin,
                        if origin.is_some() { 0 } else { 4 },
                    ));
                }
            }
        }
        Ok(())
    }
    pub fn heartbeat_native(&self, token: &InputToken) -> Result<(), String> {
        let now = self.now();
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        expire(&mut state, now);
        let instance = state
            .instances
            .get_mut(&token.source_id)
            .ok_or("Unknown companion instance.")?;
        if instance.owner.as_ref() != Some(token) {
            return Err("Stale or expired companion writer; acquire a fresh baseline.".into());
        }
        instance.last_seen_us = now;
        Ok(())
    }
    pub fn loss_native(&self, token: &InputToken) -> Result<(), String> {
        let now = self.now();
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let instance = state
            .instances
            .get(&token.source_id)
            .ok_or("Unknown companion instance.")?;
        if instance.owner.as_ref() != Some(token) {
            return Err("Stale companion writer.".into());
        }
        lose(
            &mut state,
            &token.source_id,
            now,
            "Companion session stopped.",
        );
        Ok(())
    }
    pub fn release(&self, source_id: &str) -> Result<(), String> {
        let now = self.now();
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if !state.instances.contains_key(source_id) {
            return Err("Unknown companion instance.".into());
        }
        lose(&mut state, source_id, now, "Companion instance released.");
        Ok(())
    }
    pub(crate) fn definition(&self, id: &str) -> Result<Definition, String> {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .instances
            .get(id)
            .map(|i| i.definition.clone())
            .ok_or_else(|| "Unknown companion instance.".into())
    }
    pub(crate) fn require_ready(&self, id: &str, hash: &str, neutral: bool) -> Result<u64, String> {
        let now = self.now();
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        expire(&mut state, now);
        let instance = state
            .instances
            .get(id)
            .ok_or("Unknown companion instance.")?;
        if instance.definition.publication_hash != hash {
            return Err(
                "Published companion revision changed; reconcile explicitly before Start.".into(),
            );
        }
        if instance.owner.is_none() || instance.values.len() != instance.definition.controls.len() {
            return Err("Connect the assigned companion and establish its complete neutral baseline before live Start.".into());
        }
        if neutral
            && instance
                .definition
                .controls
                .iter()
                .any(|c| instance.values.get(&c.id) != Some(&c.fallback))
        {
            return Err("Release controls or reconnect the companion to its declared fallback baseline before live Start.".into());
        }
        Ok(instance.generation)
    }
    pub fn list(&self) -> Result<serde_json::Value, String> {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        Ok(
            json!({"companionSources":state.instances.values().map(|instance| json!({
            "id":instance.definition.source_id,"name":instance.definition.name,"origin":"companion",
            "online":instance.owner.is_some(),"baselineReady":instance.owner.is_some(),
            "publicationHash":instance.definition.publication_hash,"controlCount":instance.definition.controls.len(),
            "generation":instance.generation.to_string(),"sequence":instance.wire_sequence.to_string(),"fault":instance.fault
        })).collect::<Vec<_>>()}),
        )
    }
    pub fn inspect(&self, id: &str) -> Result<serde_json::Value, String> {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let instance = state
            .instances
            .get(id)
            .ok_or("Unknown companion instance.")?;
        let controls = instance.definition.controls.iter().enumerate().map(|(index, control)| {
            let (kind, min, max, has_null) = match control.kind { Kind::Button => ("button",0,1,false), Kind::Axis { range } => ("axis",range.min(),32767,false), Kind::Hat => ("hat",0,7,true) };
            let value = instance.values.get(&control.id).map(|value| match value { Value::Button(v) => json!({"native":i32::from(*v),"normalized":i32::from(*v)}), Value::Axis(v) => json!({"native":v,"normalized":v}), Value::Hat(v) => json!({"native":v.map_or(-1,i32::from),"normalized":v}) });
            json!({"capability":{"id":control.id,"kind":kind,"usagePage":0,"usage":0,"reportId":0,"collection":0,"dataIndex":index,"bits":16,"min":min,"max":max,"hasNull":has_null},"value":value})
        }).collect::<Vec<_>>();
        Ok(
            json!({"source":{"id":id,"name":instance.definition.name,"origin":"companion","online":instance.owner.is_some(),"publicationHash":instance.definition.publication_hash},"companionControls":controls,"offset":0,"total":controls.len()}),
        )
    }
    pub(crate) fn prepare(
        &self,
        profile: &Profile,
        assignments: &[Assignment],
    ) -> Result<Prepared, String> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.subscriptions.len() >= MAX_SUBSCRIPTIONS {
            return Err("Too many pending companion mapping subscriptions.".into());
        }
        let mut controls: BTreeMap<Source, BTreeMap<String, Kind>> = BTreeMap::new();
        for assignment in assignments
            .iter()
            .filter(|a| a.source.origin == Origin::Companion)
        {
            let instance = state
                .instances
                .get(&assignment.source.id)
                .ok_or("Register the companion's published descriptor before mapping it.")?;
            let role = profile
                .roles
                .iter()
                .find(|r| r.id == assignment.role)
                .ok_or("Unknown companion role.")?;
            for binding in &assignment.controls {
                let expected = role
                    .controls
                    .iter()
                    .find(|control| control.id == binding.control)
                    .ok_or("Unknown logical companion input.")?;
                if !instance.definition.controls.iter().any(|control| {
                    control.id == binding.source_control && control.kind == expected.kind
                }) {
                    return Err("Companion descriptor changed; reconcile its mapping assignment explicitly.".into());
                }
                controls
                    .entry(assignment.source.clone())
                    .or_default()
                    .insert(binding.source_control.clone(), expected.kind);
            }
        }
        if controls.is_empty() || controls.len() > 8 {
            return Err("A mapping may reserve one to eight companion instances.".into());
        }
        state.serial = state
            .serial
            .checked_add(1)
            .ok_or("Companion subscription counter exhausted.")?;
        let id = state.serial;
        let subscription = Subscription {
            sources: controls
                .iter()
                .map(|(source, controls)| {
                    (
                        source.id.clone(),
                        Queue {
                            controls: controls.keys().cloned().collect(),
                            publication_hash: state.instances[&source.id]
                                .definition
                                .publication_hash
                                .clone(),
                            ..Queue::default()
                        },
                    )
                })
                .collect(),
            next: 0,
        };
        state.subscriptions.insert(id, subscription);
        Ok(Prepared {
            controls,
            feed: Feed {
                provider: self.clone(),
                subscription: id,
            },
        })
    }
}
impl Feed {
    pub(crate) fn require_explicit_rearm(&self) {
        let mut state = self
            .provider
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(subscription) = state.subscriptions.get_mut(&self.subscription) {
            for queue in subscription.sources.values_mut() {
                queue.explicit_restart = true;
            }
        }
    }
    pub(crate) fn require_live_ready(&self) -> Result<(), String> {
        let now = self.provider.now();
        let mut state = self
            .provider
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        expire(&mut state, now);
        let subscription = state
            .subscriptions
            .get(&self.subscription)
            .ok_or("Companion subscription ended.")?;
        for (id, queue) in &subscription.sources {
            let instance = &state.instances[id];
            if !queue.started
                || queue.restart_required
                || instance
                    .definition
                    .controls
                    .iter()
                    .any(|c| instance.values.get(&c.id) != Some(&c.fallback))
                || queue.generation == 0
                || instance.owner.is_none()
                || instance.generation != queue.generation
                || instance.definition.publication_hash != queue.publication_hash
            {
                return Err("Companion source identity, revision or baseline changed before output activation; Start again explicitly.".into());
            }
        }
        Ok(())
    }
    pub(crate) fn initial(&mut self) -> Result<Vec<Frame>, String> {
        Ok(self
            .initial_audited()?
            .into_iter()
            .map(|frame| frame.frame)
            .collect())
    }
    fn initial_audited(&mut self) -> Result<Vec<NativeFrame>, String> {
        let now = self.provider.now();
        let mut state = self
            .provider
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        expire(&mut state, now);
        let ids = state
            .subscriptions
            .get(&self.subscription)
            .ok_or("Companion subscription ended.")?
            .sources
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let mut frames = Vec::new();
        for id in ids {
            let instance = &state.instances[&id];
            let generation = instance.generation;
            let values = instance.owner.as_ref().map(|_| instance.values.clone());
            let publication_hash = instance.definition.publication_hash.clone();
            let queue = state
                .subscriptions
                .get_mut(&self.subscription)
                .unwrap()
                .sources
                .get_mut(&id)
                .unwrap();
            queue.started = true;
            queue.frames.clear();
            queue.loss = None;
            queue.generation = generation;
            queue.sequence = 0;
            queue.audit_domain = audit::next_id().unwrap_or(0);
            if let Some(values) = values.filter(|_| queue.publication_hash == publication_hash) {
                queue.sequence = 1;
                let frame = Frame {
                    source: source(&id),
                    generation: Counter(generation),
                    sequence: Counter(1),
                    captured_us: Counter(now),
                    reason: osg_mapping::engine::Reason::Baseline,
                    changes: values
                        .into_iter()
                        .filter(|(id, _)| queue.controls.contains(id))
                        .map(|(control, value)| Change { control, value })
                        .collect(),
                };
                frames.push(audit_native_frame(
                    self.provider.audit_provider,
                    self.subscription,
                    queue,
                    &id,
                    frame,
                    None,
                    1,
                ));
            }
        }
        Ok(frames)
    }
    pub(crate) fn reject(&self, source_id: &str) {
        let _ = self.provider.release(source_id);
    }
    pub(crate) fn drain(&mut self) -> Vec<Frame> {
        self.drain_audited()
            .into_iter()
            .map(|frame| frame.frame)
            .collect()
    }
    fn drain_audited(&mut self) -> Vec<NativeFrame> {
        let now = self.provider.now();
        let mut state = self
            .provider
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        expire(&mut state, now);
        let Some(subscription) = state.subscriptions.get_mut(&self.subscription) else {
            return vec![];
        };
        let ids = subscription.sources.keys().cloned().collect::<Vec<_>>();
        if ids.is_empty() {
            return vec![];
        }
        let mut frames = Vec::new();
        for offset in 0..ids.len() {
            let id = &ids[(subscription.next + offset) % ids.len()];
            let queue = subscription.sources.get_mut(id).unwrap();
            if let Some(loss) = queue.loss.take() {
                audit_dequeue(&loss, queue.frames.len());
                frames.push(loss);
            }
            for _ in 0..64 {
                if let Some(frame) = queue.frames.pop_front() {
                    audit_dequeue(&frame, queue.frames.len());
                    frames.push(frame);
                } else {
                    break;
                }
            }
        }
        subscription.next = (subscription.next + 1) % ids.len();
        frames
    }
}
impl Drop for Feed {
    fn drop(&mut self) {
        self.provider
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .subscriptions
            .remove(&self.subscription);
    }
}

impl CompanionProvider {
    fn convert_changes(
        &self,
        token: &InputToken,
        changes: &[osg_companion_service::SourceChange],
    ) -> Result<Vec<Change>, String> {
        use osg_companion_service::InputValue;
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let instance = state
            .instances
            .get(&token.source_id)
            .ok_or("Unknown companion instance.")?;
        changes
            .iter()
            .map(|change| {
                let control = instance
                    .definition
                    .controls
                    .iter()
                    .find(|control| control.id == change.control_id)
                    .ok_or("Unknown companion control.")?;
                let value = match (control.kind, &change.value) {
                    (Kind::Button, InputValue::Button(value)) => Value::Button(*value),
                    (Kind::Hat, InputValue::Number(value))
                        if value.is_finite()
                            && value.fract() == 0.0
                            && (-1.0..=7.0).contains(value) =>
                    {
                        Value::Hat(if *value < 0.0 {
                            None
                        } else {
                            Some(*value as u8)
                        })
                    }
                    (Kind::Axis { range }, InputValue::Number(value))
                        if value.is_finite()
                            && (if range == profile::AxisRange::Bipolar {
                                -1.0
                            } else {
                                0.0
                            }..=1.0)
                                .contains(value) =>
                    {
                        Value::Axis(osg_companion_service::quantize_axis(
                            *value,
                            if range == profile::AxisRange::Bipolar {
                                osg_companion_service::AxisMode::Bipolar
                            } else {
                                osg_companion_service::AxisMode::Unipolar
                            },
                            0.0,
                        )?)
                    }
                    _ => return Err("Companion input kind or range mismatch.".into()),
                };
                Ok(Change {
                    control: change.control_id.clone(),
                    value,
                })
            })
            .collect()
    }
}
fn native_token(token: &osg_companion_service::SourceToken) -> InputToken {
    InputToken {
        source_id: token.source_id.clone(),
        session_id: token.session_id.clone(),
        generation: token.generation,
        publication_hash: token.publication_hash.clone(),
    }
}
impl osg_companion_service::SourceSink for CompanionProvider {
    fn register(&self, definition: osg_companion_service::SourceDefinition) -> Result<(), String> {
        let result = native_definition(definition).and_then(|definition| self.register_definition(definition));
        self.notify_consumer();
        result
    }
    fn baseline(
        &self,
        token: &osg_companion_service::SourceToken,
        sequence: u64,
        changes: &[osg_companion_service::SourceChange],
    ) -> Result<(), String> {
        let token = native_token(token);
        let result = self.convert_changes(&token, changes)
            .and_then(|changes| self.baseline_native(&token, sequence, &changes));
        self.notify_consumer();
        result
    }
    fn changes(
        &self,
        token: &osg_companion_service::SourceToken,
        sequence: u64,
        changes: &[osg_companion_service::SourceChange],
    ) -> Result<(), String> {
        let capture = audit::stamp().and_then(|at| audit::next_id().map(|id| (at, id)));
        if let Some((at, id)) = capture {
            if self.audit_provider == 0 {
                audit::fault(audit::Fault::Provenance);
            }
            let source =
                audit::alias(AliasKind::NativeSource, token.source_id.as_bytes()).unwrap_or(0);
            let session =
                audit::alias(AliasKind::Subscription, token.session_id.as_bytes()).unwrap_or(0);
            let publication =
                audit::alias(AliasKind::Provider, token.publication_hash.as_bytes()).unwrap_or(0);
            audit::record_at(
                at,
                AuditKind::SourceCapture,
                &[
                    id,
                    self.audit_provider,
                    source,
                    session,
                    token.generation,
                    publication,
                    sequence,
                    changes.len() as u64,
                ],
            );
        }
        let token = native_token(token);
        let changes = match self.convert_changes(&token, changes) {
            Ok(changes) => changes,
            Err(error) => {
                let _ = self.loss_native(&token);
                if let Some((_, id)) = capture {
                    audit::record(AuditKind::SourceResult, &[id, 0, 1]);
                }
                self.notify_consumer();
                return Err(error);
            }
        };
        let result =
            self.changes_native_audited(&token, sequence, &changes, capture.map(|(_, id)| id));
        if let Some((_, id)) = capture {
            audit::record(
                AuditKind::SourceResult,
                &[
                    id,
                    u64::from(result.is_ok()),
                    if result.is_ok() { 0 } else { 2 },
                ],
            );
        }
        self.notify_consumer();
        result
    }
    fn heartbeat(&self, token: &osg_companion_service::SourceToken) -> Result<(), String> {
        let result = self.heartbeat_native(&native_token(token));
        self.notify_consumer();
        result
    }
    fn loss(&self, token: &osg_companion_service::SourceToken) -> Result<(), String> {
        let result = self.loss_native(&native_token(token));
        self.notify_consumer();
        result
    }
}

#[path = "companion_native.rs"]
mod native;
pub use native::{native_definition, NativeFeed, NativeSelection};

#[cfg(test)]
#[path = "companion_tests.rs"]
mod tests;
