//! Local typed visual routing. This worker never invokes input, device or game commands.
use osg_companion_service::{
    FeedbackBinding, FeedbackKind, FeedbackMode, FeedbackProvenance, FeedbackSample,
    FeedbackSource, FeedbackStatus, FeedbackValue, Hub, Publication,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
pub type HubGetter = Arc<dyn Fn() -> Result<Hub, String> + Send + Sync>;
pub type Snapshot = Arc<dyn Fn() -> Value + Send + Sync>;
pub type Submission = Arc<dyn Fn(&str) -> Result<Value, String> + Send + Sync>;
const LIMIT: usize = 128 * 1024;
// C5 inspection uses the existing 128 KiB browser response budget; leave room
// for the private IPC wrapper. Request limits and C4 response limits stay unchanged.
const INSPECT_REPLY_BYTES: usize = 128 * 1024 - 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Source {
    Requested { control_id: String },
    HostAccepted { control_id: String },
    HostSubmitted { control_id: String },
    Integration { binding_id: String },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Binding {
    channel_id: String,
    output_id: String,
    source: Source,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    offset: Option<f64>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Route {
    instance_id: String,
    publication_hash: String,
    bindings: Vec<Binding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_revision: Option<String>,
}
#[derive(Default)]
struct State {
    routes: BTreeMap<String, Route>,
    errors: BTreeMap<String, String>,
    revision: u64,
    corrupt: Option<String>,
    suppressed: BTreeMap<String, String>,
    publications: BTreeMap<String, Arc<Publication>>,
}
struct Shared {
    state: Mutex<State>,
    operation: Mutex<()>,
    hub: HubGetter,
    telemetry: Snapshot,
    submission: Submission,
    path: Option<PathBuf>,
    stop: AtomicBool,
}
pub struct Runtime {
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
}
fn id(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn digest(s: &str) -> bool {
    s.len() == 71
        && s.starts_with("sha256:")
        && s.as_bytes()[7..]
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
}
fn schema(route: &Route) -> Result<(), String> {
    if !id(&route.instance_id, 64) || !digest(&route.publication_hash) || route.bindings.len() > 64
    {
        return Err("Invalid feedback route identity/revision/budget.".into());
    }
    let mut seen = BTreeSet::new();
    for b in &route.bindings {
        if !id(&b.channel_id, 64)
            || !id(&b.output_id, 64)
            || !seen.insert(&b.channel_id)
            || b.scale.is_some_and(|v| !v.is_finite() || v.abs() > 1e6)
            || b.offset.is_some_and(|v| !v.is_finite() || v.abs() > 1e9)
        {
            return Err("Invalid feedback binding or transform.".into());
        }
        match &b.source {
            Source::Integration { binding_id } => {
                if !id(binding_id, 31) {
                    return Err("Invalid telemetry binding identity.".into());
                }
            }
            Source::Requested { control_id }
            | Source::HostAccepted { control_id }
            | Source::HostSubmitted { control_id } => {
                if !id(control_id, 64) {
                    return Err("Invalid source control identity.".into());
                }
            }
        }
    }
    Ok(())
}
fn load(path: &Option<PathBuf>) -> State {
    let result = (|| {
        let Some(path) = path else {
            return Ok(BTreeMap::new());
        };
        match std::fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Ok(m) if !m.is_file() || m.file_type().is_symlink() => {
                return Err("Feedback settings must be a regular file.".to_string())
            }
            Err(e) => return Err(e.to_string()),
            _ => {}
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take((LIMIT + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > LIMIT {
            return Err("Feedback settings exceed the byte budget.".into());
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Saved {
            schema: u8,
            routes: Vec<Route>,
        }
        let saved: Saved = serde_json::from_slice(&bytes)
            .map_err(|_| "Invalid saved feedback routes.".to_string())?;
        if saved.schema != 1 || saved.routes.len() > 32 {
            return Err("Invalid feedback schema/budget.".into());
        }
        let mut routes = BTreeMap::new();
        for route in saved.routes {
            schema(&route)?;
            if routes.insert(route.instance_id.clone(), route).is_some() {
                return Err("Duplicate feedback instance.".into());
            }
        }
        Ok(routes)
    })();
    match result {
        Ok(routes) => State {
            routes,
            ..State::default()
        },
        Err(e) => State {
            corrupt: Some(e),
            ..State::default()
        },
    }
}
fn persist(path: &Option<PathBuf>, routes: &BTreeMap<String, Route>) -> Result<(), String> {
    let bytes =
        serde_json::to_vec(&json!({"schema":1,"routes":routes.values().collect::<Vec<_>>()}))
            .map_err(|e| e.to_string())?;
    if routes.len() > 32 || bytes.len() > LIMIT {
        return Err("Feedback route storage budget exceeded.".into());
    }
    let Some(path) = path else { return Ok(()) };
    let parent = path.parent().ok_or("Invalid feedback store path.")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp = parent.join(format!(".feedback-{}.tmp", uuid::Uuid::new_v4()));
    let write = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut f = options.open(&temp).map_err(|e| e.to_string())?;
        f.write_all(&bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&temp, path).map_err(|e| e.to_string())
    })();
    if write.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    write
}
fn native_bindings(route: &Route) -> Vec<FeedbackBinding> {
    route
        .bindings
        .iter()
        .map(|b| FeedbackBinding {
            channel_id: b.channel_id.clone(),
            output_id: b.output_id.clone(),
        })
        .collect()
}
fn document(p: &Publication) -> Result<Value, String> {
    serde_json::from_str(&p.canonical_document).map_err(|_| "Invalid immutable publication.".into())
}
fn sources(telemetry: &Value) -> Vec<Value> {
    telemetry["sources"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    let mut r = row.clone();
                    r["confirmed"] = json!(telemetry["sourceKind"] == "simulator");
                    r
                })
                .collect()
        })
        .unwrap_or_default()
}
fn source_description(b: &Binding, p: &Publication, t: &Value) -> Result<Value, String> {
    let control = match &b.source {
        Source::Integration { binding_id } => {
            return sources(t)
                .into_iter()
                .find(|r| r["bindingId"] == *binding_id)
                .ok_or("Start or validate the integration's named telemetry source first.".into())
        }
        Source::Requested { control_id }
        | Source::HostAccepted { control_id }
        | Source::HostSubmitted { control_id } => control_id,
    };
    let c = p
        .controls
        .iter()
        .find(|c| &c.id == control)
        .ok_or("Control is absent from the pinned publication.")?;
    use osg_companion_service::{AxisMode, SourceKind};
    Ok(match c.kind {
        SourceKind::Button => json!({"kind":"boolean","unit":"state","minimum":0,"maximum":1}),
        SourceKind::Hat => json!({"kind":"number","unit":"direction","minimum":-1,"maximum":7}),
        SourceKind::Axis { axis_mode, .. } => {
            json!({"kind":"number","unit":"ratio","minimum":if axis_mode==AxisMode::Bipolar{-1}else{0},"maximum":1})
        }
    })
}
fn validate(route: &Route, p: &Publication, t: &Value) -> Result<(), String> {
    schema(route)?;
    if p.publication_hash != route.publication_hash {
        return Err(
            "Explicitly reconcile feedback bindings with the assigned immutable revision.".into(),
        );
    }
    let doc = document(p)?;
    for b in &route.bindings {
        let d = p
            .feedback
            .iter()
            .find(|d| d.channel_id == b.channel_id)
            .ok_or("Unknown published feedback channel.")?;
        if !doc["ui"]["feedbackWidgets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["id"] == b.output_id && o["channelId"] == b.channel_id)
        {
            return Err("Output identity is not bound to the published feedback channel.".into());
        }
        let source = source_description(b, p, t)?;
        let kind = source["kind"].as_str().unwrap_or("");
        let compatible = match d.kind {
            FeedbackKind::Boolean => kind == "boolean",
            FeedbackKind::Text => kind == "string",
            FeedbackKind::Number => matches!(kind, "number" | "integer"),
        };
        if !compatible {
            return Err("Source type differs from the published output type.".into());
        }
        let unit = d
            .unit
            .as_deref()
            .unwrap_or(if d.kind == FeedbackKind::Text {
                "text"
            } else {
                "state"
            });
        if source["unit"] != unit
            && !(d.kind == FeedbackKind::Number && (b.scale.is_some() || b.offset.is_some()))
        {
            return Err("Units differ; an explicit numeric conversion is required.".into());
        }
        if d.kind != FeedbackKind::Number
            && (b.scale.unwrap_or(1.) != 1. || b.offset.unwrap_or(0.) != 0.)
        {
            return Err("Only numeric feedback supports transforms.".into());
        }
        if let (Some(min), Some(max), Some(range)) = (
            source["minimum"].as_f64(),
            source["maximum"].as_f64(),
            d.range.as_ref(),
        ) {
            if [min, max].iter().any(|v| {
                let n = v * b.scale.unwrap_or(1.) + b.offset.unwrap_or(0.);
                !n.is_finite() || n < range.min || n > range.max
            }) {
                return Err("Converted source range exceeds the immutable output range.".into());
            }
        }
    }
    Ok(())
}
fn signature(owner: &Value, t: &Value, submitted: &Value) -> String {
    json!([
        owner["sessionId"],
        owner["values"],
        t["epoch"],
        t["values"],
        submitted
    ])
    .to_string()
}
fn samples(
    route: &Route,
    p: &Publication,
    owner: &Value,
    t: &Value,
    submitted: &Value,
    suppressed: bool,
) -> Result<Vec<FeedbackSample>, String> {
    let mut values = Vec::new();
    for b in &route.bindings {
        let d = p
            .feedback
            .iter()
            .find(|d| d.channel_id == b.channel_id)
            .ok_or("Unknown published feedback channel.")?;
        let mut age = 0;
        let mut source_kind = FeedbackSource::Host;
        let mut provenance = FeedbackProvenance::Requested;
        let mut fresh = owner["armed"] == true;
        let mut value;
        let disconnected = owner["connected"] != true;
        match &b.source {
            Source::Integration { binding_id } => {
                age = t["sourceAgeMs"].as_u64().unwrap_or(0).min(86_400_000);
                fresh = t["running"] == true && t["fresh"] == true;
                if t["sourceKind"] == "simulator" {
                    source_kind = FeedbackSource::Simulator;
                    provenance = FeedbackProvenance::GameConfirmed;
                } else {
                    provenance = FeedbackProvenance::Local;
                }
                value = t["values"][binding_id].clone();
            }
            Source::Requested { control_id } | Source::HostAccepted { control_id } => {
                if matches!(b.source, Source::HostAccepted { .. }) {
                    provenance = FeedbackProvenance::HostAccepted;
                }
                value = owner["values"][control_id].clone();
            }
            Source::HostSubmitted { control_id } => {
                provenance = FeedbackProvenance::HostSubmitted;
                fresh = submitted["confirmed"] == true
                    && submitted["publicationHash"] == route.publication_hash;
                value = submitted["values"][control_id].clone();
            }
        }
        if let Some(n) = value.as_f64() {
            value = json!(n * b.scale.unwrap_or(1.) + b.offset.unwrap_or(0.));
        }
        // Never clamp or re-label incompatible source data as game confirmation.
        let valid = match d.kind {
            FeedbackKind::Boolean => value.is_boolean(),
            FeedbackKind::Text => value.as_str().is_some_and(|s| {
                s.encode_utf16().count() <= 256 && !s.chars().any(|c| c <= '\u{1f}')
            }),
            FeedbackKind::Number => value.as_f64().is_some_and(|n| {
                n.is_finite() && d.range.as_ref().is_some_and(|r| n >= r.min && n <= r.max)
            }),
        };
        let status = if disconnected {
            FeedbackStatus::Disconnected
        } else if suppressed || !valid {
            FeedbackStatus::Unknown
        } else if !fresh || age >= 1000 {
            FeedbackStatus::Stale
        } else {
            FeedbackStatus::Fresh
        };
        let value = if matches!(
            status,
            FeedbackStatus::Unknown | FeedbackStatus::Disconnected
        ) {
            None
        } else {
            serde_json::from_value::<FeedbackValue>(value).ok()
        };
        values.push(FeedbackSample {
            channel_id: b.channel_id.clone(),
            output_id: b.output_id.clone(),
            value,
            status,
            provenance,
            source_kind,
            source_age_ms: age,
            unit: d.unit.clone().unwrap_or_else(|| {
                if d.kind == FeedbackKind::Text {
                    "text".into()
                } else {
                    "state".into()
                }
            }),
            range: d.range.clone(),
        });
    }
    Ok(values)
}
impl Runtime {
    pub fn new(
        path: Option<PathBuf>,
        hub: HubGetter,
        telemetry: Snapshot,
        submission: Submission,
    ) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(load(&path)),
            operation: Mutex::new(()),
            hub,
            telemetry,
            submission,
            path,
            stop: AtomicBool::new(false),
        });
        let own = shared.clone();
        let worker = thread::Builder::new()
            .name("companion-feedback".into())
            .spawn(move || {
                while !own.stop.load(Ordering::Acquire) {
                    own.tick();
                    thread::sleep(Duration::from_millis(36));
                }
            })
            .ok();
        Self {
            shared,
            worker: Mutex::new(worker),
        }
    }
    pub fn request(&self, payload: &Value) -> Result<Value, String> {
        if self.shared.stop.load(Ordering::Acquire) {
            return Err("Companion feedback has closed.".into());
        }
        let _operation = self
            .shared
            .operation
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let action = payload["action"].as_str().unwrap_or("inspect");
        let hub = (self.shared.hub)()?;
        let telemetry = (self.shared.telemetry)();
        if action != "inspect" {
            let instance = payload["instanceId"]
                .as_str()
                .ok_or("Choose a feedback instance.")?;
            let p = hub.assigned_publication(instance)?;
            if payload["publicationHash"] != p.publication_hash {
                return Err("Refresh the current immutable feedback revision.".into());
            }
            if action == "save" {
                let route = Route {
                    instance_id: instance.into(),
                    publication_hash: p.publication_hash.clone(),
                    bindings: serde_json::from_value(payload["bindings"].clone())
                        .map_err(|_| "Invalid typed feedback bindings.")?,
                    source_revision: telemetry["revision"].as_str().map(str::to_owned),
                };
                validate(&route, &p, &telemetry)?;
                let mut state = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(e) = &state.corrupt {
                    return Err(format!("Preserve and repair invalid saved feedback: {e}"));
                }
                let mut routes = state.routes.clone();
                routes.insert(instance.into(), route.clone());
                let previous = state.routes.get(instance).cloned();
                hub.configure_feedback(
                    instance,
                    &route.publication_hash,
                    &native_bindings(&route),
                )?;
                hub.clear_feedback(instance, &route.publication_hash, hub.now_ms())?;
                if let Err(error) = persist(&self.shared.path, &routes) {
                    let old = previous
                        .as_ref()
                        .filter(|r| r.publication_hash == route.publication_hash)
                        .map(native_bindings)
                        .unwrap_or_default();
                    let _ = hub.configure_feedback(instance, &route.publication_hash, &old);
                    let _ = hub.clear_feedback(instance, &route.publication_hash, hub.now_ms());
                    return Err(error);
                }
                let mut cached = p.clone();
                cached.assets.clear();
                state.publications.insert(instance.into(), Arc::new(cached));
                state.routes = routes;
                state.revision = state
                    .revision
                    .checked_add(1)
                    .ok_or("Feedback settings revision exhausted.")?;
                state.errors.remove(instance);
                state.suppressed.remove(instance);
            } else if action == "reset" || action == "resync" {
                let status = hub.command(&json!({"command":"companion_status"}))?;
                let owner = status["instances"]
                    .as_array()
                    .and_then(|rows| rows.iter().find(|r| r["instanceId"] == instance))
                    .ok_or("Feedback instance disappeared.")?;
                hub.clear_feedback(instance, &p.publication_hash, hub.now_ms())?;
                let mut state = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
                if !state.routes.contains_key(instance) {
                    return Err("Save feedback routes first.".into());
                }
                if action == "reset" {
                    let submitted = (self.shared.submission)(instance).unwrap_or(Value::Null);
                    state
                        .suppressed
                        .insert(instance.into(), signature(owner, &telemetry, &submitted));
                } else {
                    state.suppressed.remove(instance);
                }
            } else {
                return Err("Unknown companion feedback route action.".into());
            }
        }
        self.shared
            .inspect(&hub, &telemetry, payload["instanceId"].as_str())
    }
    pub(crate) fn preflight_assignments(&self, capture: &Value) -> Result<(), String> {
        let capture = crate::game_mapping::companions::Capture::parse(capture)?;
        let state = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(error) = &state.corrupt {
            return Err(format!("Repair companion feedback settings before activating a captured game profile: {error}"));
        }
        for assignment in capture.assignments {
            if state
                .routes
                .get(&assignment.instance_id)
                .is_some_and(|r| r.publication_hash != assignment.publication_hash)
            {
                return Err("Captured companion revision differs from saved visual routes; reconcile explicitly before game activation.".into());
            }
        }
        Ok(())
    }
    /// Reject a captured integration revision that cannot drive this game's
    /// pinned visual routes before the coordinator changes any owned state.
    pub(crate) fn preflight_integration(
        &self,
        capture: &Value,
        desired_revision: &str,
    ) -> Result<(), String> {
        if capture.is_null() {
            return Ok(());
        }
        let capture = crate::game_mapping::companions::Capture::parse(capture)?;
        let state = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(error) = &state.corrupt {
            return Err(format!(
                "Repair companion feedback settings before integration preflight: {error}"
            ));
        }
        for assignment in capture.assignments {
            if let Some(route) = state.routes.get(&assignment.instance_id) {
                if route
                    .bindings
                    .iter()
                    .any(|binding| matches!(binding.source, Source::Integration { .. }))
                    && route.source_revision.as_deref() != Some(desired_revision)
                {
                    return Err("Captured integration differs from the saved companion telemetry routes. Save matching visual routes before activating this game profile.".into());
                }
            }
        }
        Ok(())
    }
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = worker.join();
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.stop();
    }
}
impl Shared {
    fn tick(&self) {
        let routes = {
            self.state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .routes
                .clone()
        };
        if routes.is_empty() {
            return;
        }
        let Ok(hub) = (self.hub)() else { return };
        let telemetry = (self.telemetry)();
        for (id, route) in routes {
            let _operation = self.operation.lock().unwrap_or_else(|p| p.into_inner());
            let current = self
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .routes
                .get(&id)
                .map(|r| serde_json::to_value(r).unwrap());
            if current != Some(serde_json::to_value(&route).unwrap()) {
                continue;
            }
            let result = (|| {
                let status = hub.command(&json!({"command":"companion_status"}))?;
                let owner = status["instances"]
                    .as_array()
                    .and_then(|rows| rows.iter().find(|r| r["instanceId"] == id))
                    .ok_or("Feedback instance unavailable.")?;
                if owner["publicationHash"] != route.publication_hash {
                    return Err(
                        "Feedback revision changed; explicitly save the new immutable assignment."
                            .into(),
                    );
                }
                if route
                    .bindings
                    .iter()
                    .any(|b| matches!(b.source, Source::Integration { .. }))
                    && route.source_revision.as_deref() != telemetry["revision"].as_str()
                {
                    return Err("Telemetry configuration changed; inspect and save the visual routes explicitly.".into());
                }
                let cached = {
                    self.state
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .publications
                        .get(&id)
                        .filter(|p| p.publication_hash == route.publication_hash)
                        .cloned()
                };
                let publication = if let Some(p) = cached {
                    p
                } else {
                    let mut p = hub.assigned_publication(&id)?;
                    validate(&route, &p, &telemetry)?;
                    p.assets.clear();
                    self.state
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .publications
                        .insert(id.clone(), Arc::new(p.clone()));
                    Arc::new(p)
                };
                hub.configure_feedback(&id, &route.publication_hash, &native_bindings(&route))?;
                let token = hub.feedback_token(&id)?;
                let submitted = if route
                    .bindings
                    .iter()
                    .any(|b| matches!(b.source, Source::HostSubmitted { .. }))
                {
                    (self.submission)(&id).unwrap_or(Value::Null)
                } else {
                    Value::Null
                };
                let suppressed = {
                    let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
                    let sig = signature(owner, &telemetry, &submitted);
                    if state.suppressed.get(&id).is_some_and(|old| old != &sig) {
                        state.suppressed.remove(&id);
                    }
                    state.suppressed.contains_key(&id)
                };
                let values = samples(
                    &route,
                    &publication,
                    owner,
                    &telemetry,
                    &submitted,
                    suppressed,
                )?;
                hub.publish_feedback(&token, &values, FeedbackMode::Snapshot, hub.now_ms())?;
                Ok::<_, String>(())
            })();
            if let Err(error) = &result {
                let changed = self
                    .state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .errors
                    .get(&id)
                    != Some(error);
                if changed {
                    let _ = hub.clear_feedback(&id, &route.publication_hash, hub.now_ms());
                }
            }
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            match result {
                Ok(()) => {
                    state.errors.remove(&id);
                }
                Err(e) => {
                    state.errors.insert(id, e.chars().take(256).collect());
                }
            }
        }
    }
    fn inspect(
        &self,
        hub: &Hub,
        telemetry: &Value,
        selected: Option<&str>,
    ) -> Result<Value, String> {
        let status = hub.command(&json!({"command":"companion_status"}))?;
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut instances = Vec::new();
        for (index, owner) in status["instances"]
            .as_array()
            .ok_or("Missing companion assignment list.")?
            .iter()
            .enumerate()
        {
            let instance = owner["instanceId"].as_str().unwrap();
            let saved = state.routes.get(instance);
            let detail = selected.map_or(index == 0, |s| s == instance);
            let mut outputs = Vec::new();
            let mut controls = Value::Array(vec![]);
            let mut error = state
                .errors
                .get(instance)
                .cloned()
                .or(state.corrupt.clone());
            if detail {
                match hub.assigned_publication(instance) {
                    Ok(p) => {
                        let doc = document(&p)?;
                        controls=json!(doc["ui"]["controls"].as_array().unwrap().iter().map(|c|{
                    let mut row=json!({"id":c["id"],"label":c["label"].as_str().unwrap_or("").chars().take(16).collect::<String>(),"kind":c["kind"]});
                    if let Some(mode)=c.get("axisMode"){row["axisMode"]=mode.clone();}row}).collect::<Vec<_>>());
                        let mut channels = BTreeSet::new();
                        for out in doc["ui"]["feedbackWidgets"].as_array().unwrap() {
                            let d = p
                                .feedback
                                .iter()
                                .find(|d| d.channel_id == out["channelId"])
                                .unwrap();
                            if !channels.insert(d.channel_id.clone()) {
                                continue;
                            }
                            let selected_output =
                                saved
                                    .and_then(|r| {
                                        r.bindings.iter().find(|b| b.channel_id == d.channel_id)
                                    })
                                    .map(|b| json!(b.output_id))
                                    .filter(|id| {
                                        doc["ui"]["feedbackWidgets"].as_array().unwrap().iter().any(
                                            |w| w["id"] == *id && w["channelId"] == d.channel_id,
                                        )
                                    })
                                    .unwrap_or_else(|| out["id"].clone());
                            outputs.push(json!({"outputId":selected_output,"channelId":d.channel_id,"kind":out["kind"],"unit":d.unit.as_deref().unwrap_or(if d.kind==FeedbackKind::Text{"text"}else{"state"}),"range":d.range}));
                        }
                    }
                    Err(e) => error = Some(e),
                }
            }
            instances.push(json!({"instanceId":instance,"publicationHash":owner["publicationHash"],"outputs":outputs,"controls":controls,
                "bindings":if detail{saved.map(|r|serde_json::to_value(&r.bindings).unwrap()).unwrap_or(json!([]))}else{json!([])},
                "configured":saved.is_some(),"pinned":saved.is_some_and(|r|r.publication_hash==owner["publicationHash"]),"error":error}));
        }
        let response = json!({"feedbackRoutes":{"instances":instances,"sources":sources(telemetry),"revision":state.revision,"error":state.corrupt}});
        if serde_json::to_vec(&response)
            .map_err(|e| e.to_string())?
            .len()
            > INSPECT_REPLY_BYTES
        {
            return Err("Selected feedback inspection exceeds its bounded reply; reduce the published descriptor.".into());
        }
        Ok(response)
    }
}
#[cfg(test)]
#[path = "companion_feedback_tests.rs"]
mod tests;
