//! Coordinated, bounded companion routes. Configuration is durable; execution never resumes.
use super::*;
use osg_companion_service::{quantize_axis, Publication, PublicationUpload, SourceKind};
use osg_mapping::{engine::validate_assignments, profile::Profile};
use osg_virtual_controller::vjoy::Binding;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
};
pub type BlueprintProvider = Arc<dyn Fn(&str) -> Result<Publication, String> + Send + Sync>;
const CONFIG_LIMIT: u64 = 4 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Configuration {
    instance_id: String,
    publication_hash: String,
    capability_fingerprint: String,
    profile: Profile,
    assignments: Vec<Assignment>,
    bindings: Vec<Binding>,
}
struct Route {
    profile: Profile,
    bindings: Vec<Binding>,
    router: Mutex<Router>,
    cancelled: AtomicBool,
    starting: AtomicBool,
    reserved: AtomicBool,
    recovery: AtomicBool,
    live: bool,
    worker: Mutex<Option<JoinHandle<()>>>,
}
pub struct Manager {
    root: Option<PathBuf>,
    configurations: BTreeMap<String, Configuration>,
    routes: BTreeMap<String, Arc<Route>>,
    pending: Option<(u64, Resources)>,
    serial: u64,
    epochs: BTreeMap<String, u64>,
    orphaned: BTreeMap<String, Resources>,
    error: Option<String>,
}
#[derive(Clone, Default)]
struct Resources {
    roles: usize,
    targets: usize,
    nodes: usize,
    slots: BTreeSet<u8>,
    live: bool,
}
impl Resources {
    fn of(profile: &Profile, bindings: &[Binding], live: bool) -> Self {
        Self {
            roles: profile.roles.len(),
            targets: profile.targets.len(),
            nodes: profile.nodes.len(),
            slots: if live {
                bindings.iter().map(|b| b.slot).collect()
            } else {
                BTreeSet::new()
            },
            live,
        }
    }
    fn add(&mut self, other: &Self) -> Result<(), String> {
        if self.slots.intersection(&other.slots).next().is_some() {
            return Err(
                "A local virtual target slot is already reserved by another mapping.".into(),
            );
        }
        self.roles += other.roles;
        self.targets += other.targets;
        self.nodes += other.nodes;
        self.slots.extend(&other.slots);
        self.live |= other.live;
        if self.roles > 16 || self.targets > 8 || self.nodes > 256 {
            return Err("Shared companion/physical mapping budget exceeds 16 roles, 8 targets or 256 nodes.".into());
        }
        Ok(())
    }
}
fn id_for(prefix: &str, value: &str) -> String {
    format!("{prefix}-{}", &osg_logic::worker::hash(value)[..32])
}
fn profile_id(value: &str) -> String {
    let hash = osg_logic::worker::hash(&format!("companion/{value}"));
    format!(
        "{}-{}-{}-{}-{}",
        &hash[..8],
        &hash[8..12],
        &hash[12..16],
        &hash[16..20],
        &hash[20..32]
    )
}
fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value.as_bytes()[7..]
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
}
fn validate_configuration(c: &Configuration) -> Result<(), String> {
    if !osg_mapping::profile::identifier(&c.instance_id)
        || !valid_digest(&c.publication_hash)
        || !valid_digest(&c.capability_fingerprint)
        || c.profile.id != profile_id(&c.instance_id)
        || c.profile.roles.len() != 1
        || c.profile.roles[0].origin != Origin::Companion
        || c.assignments.len() != 1
        || c.assignments[0].source
            != (Source {
                origin: Origin::Companion,
                id: c.instance_id.clone(),
            })
    {
        return Err("Invalid saved companion configuration.".into());
    }
    c.profile.compile()?;
    validate_assignments(&c.profile, &c.assignments)?;
    if !c.bindings.is_empty() {
        crate::settings::validate_bindings(&c.profile, &c.bindings)?;
    }
    Ok(())
}
impl Manager {
    pub(super) fn new(root: Option<PathBuf>) -> Self {
        let loaded = (|| {
            let Some(root) = &root else {
                return Ok(BTreeMap::new());
            };
            let path = root.join("companion-routes.json");
            match std::fs::symlink_metadata(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
                Ok(m) if !m.is_file() || m.file_type().is_symlink() => {
                    return Err("Companion routes must be a regular local file.".to_string())
                }
                Err(e) => return Err(e.to_string()),
                _ => {}
            }
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .map_err(|e| e.to_string())?
                .take(CONFIG_LIMIT + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() as u64 > CONFIG_LIMIT {
                return Err("Saved companion routes exceed their budget.".into());
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Saved {
                schema: u8,
                configurations: Vec<Configuration>,
            }
            let saved: Saved = serde_json::from_slice(&bytes)
                .map_err(|_| "Invalid saved companion routes.".to_string())?;
            if saved.schema != 1 || saved.configurations.len() > 32 {
                return Err("Invalid companion configuration version/budget.".into());
            }
            let mut values = BTreeMap::new();
            for c in saved.configurations {
                validate_configuration(&c)?;
                if values.insert(c.instance_id.clone(), c).is_some() {
                    return Err("Duplicate saved companion instance.".into());
                }
            }
            Ok(values)
        })();
        let (configurations, error) = match loaded {
            Ok(c) => (c, None),
            Err(e) => (BTreeMap::new(), Some(e)),
        };
        let mut manager = Self {
            root,
            configurations,
            routes: BTreeMap::new(),
            pending: None,
            serial: 0,
            epochs: BTreeMap::new(),
            orphaned: BTreeMap::new(),
            error,
        };
        // Interrupted ownership remains reserved after restart, without opening
        // a driver or restarting a mapping. An unreadable journal fails closed.
        for (id, c) in &manager.configurations {
            if let Ok(path) = manager.journal(id) {
                let journal = osg_virtual_controller::output_journal::inspect(&path);
                if journal
                    .as_ref()
                    .map_or(true, |r| r.as_ref().is_some_and(|r| r.pending()))
                {
                    manager.routes.insert(
                        id.clone(),
                        Arc::new(Route {
                            profile: c.profile.clone(),
                            bindings: journal
                                .ok()
                                .flatten()
                                .map(|r| r.bindings)
                                .unwrap_or_else(|| c.bindings.clone()),
                            router: Mutex::new(Router::default()),
                            cancelled: AtomicBool::new(true),
                            starting: AtomicBool::new(false),
                            reserved: AtomicBool::new(true),
                            recovery: AtomicBool::new(true),
                            live: true,
                            worker: Mutex::new(None),
                        }),
                    );
                }
            }
        }
        if let Some(root) = &manager.root {
            let scan = (|| -> Result<(), String> {
                let directory = root.join("companion-recovery");
                let entries = match std::fs::read_dir(directory) {
                    Ok(e) => e,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                    Err(e) => return Err(e.to_string()),
                };
                let known = manager
                    .configurations
                    .keys()
                    .map(|id| format!("{}.json", id_for("instance", id)))
                    .collect::<BTreeSet<_>>();
                let mut count = 0;
                for entry in entries {
                    let entry = entry.map_err(|e| e.to_string())?;
                    if entry.path().extension().and_then(|v| v.to_str()) != Some("json") {
                        continue;
                    }
                    count += 1;
                    if count > 32 {
                        return Err(
                            "Too many companion recovery journals; preserve and inspect ownership."
                                .into(),
                        );
                    }
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if known.contains(&name) {
                        continue;
                    }
                    if let Some(record) =
                        osg_virtual_controller::output_journal::inspect(&entry.path())?
                            .filter(|r| r.pending())
                    {
                        manager.orphaned.insert(
                            name,
                            Resources {
                                roles: 0,
                                targets: record.bindings.len(),
                                nodes: 0,
                                slots: record.bindings.iter().map(|b| b.slot).collect(),
                                live: true,
                            },
                        );
                    }
                }
                Ok(())
            })();
            if let Err(error) = scan {
                manager.error.get_or_insert(format!(
                    "Interrupted companion ownership cannot be read: {error}"
                ));
            }
        }
        manager
    }
    fn journal(&self, id: &str) -> Result<PathBuf, String> {
        self.root
            .as_ref()
            .map(|p| {
                p.join("companion-recovery")
                    .join(format!("{}.json", id_for("instance", id)))
            })
            .ok_or("Local storage is required for companion output recovery.".into())
    }
    fn resources(
        &self,
        manual: Option<&Resources>,
        except: Option<&str>,
    ) -> Result<Resources, String> {
        let mut total = Resources::default();
        for reservation in self.orphaned.values() {
            total.add(reservation)?;
        }
        if let Some((_, pending)) = &self.pending {
            total.add(pending)?;
        } else if let Some(manual) = manual {
            total.add(manual)?;
        }
        for (id, route) in &self.routes {
            if except != Some(id.as_str()) && route.reserved.load(Ordering::Acquire) {
                total.add(&Resources::of(&route.profile, &route.bindings, route.live))?;
            }
        }
        Ok(total)
    }
    fn check(
        &self,
        profile: &Profile,
        bindings: &[Binding],
        live: bool,
        manual: Option<&Resources>,
        except: Option<&str>,
    ) -> Result<(), String> {
        if live {
            if let Some(error) = &self.error {
                return Err(format!(
                    "Repair preserved companion configuration/recovery before live output: {error}"
                ));
            }
        }
        let mut total = self.resources(manual, except)?;
        if live && !cfg!(windows) && total.live {
            return Err("Multiple live feeder processes are not qualified for this host backend; recording preview remains available.".into());
        }
        total.add(&Resources::of(profile, bindings, live))
    }
}
fn manual(router: &Router, settings: &crate::settings::Settings) -> Option<Resources> {
    if router.runtime.token().is_err()
        && !router.status()["recoveryRequired"]
            .as_bool()
            .unwrap_or(false)
    {
        return None;
    }
    router.runtime.trace.trace.as_ref().map(|trace| {
        Resources::of(
            &trace.profile,
            settings
                .value
                .targets
                .get(&trace.profile.id)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            router.live(),
        )
    })
}
pub(super) struct ManualReservation {
    manager: Arc<Mutex<Manager>>,
    serial: u64,
}
impl Drop for ManualReservation {
    fn drop(&mut self) {
        let mut m = self.manager.lock().unwrap_or_else(|p| p.into_inner());
        if m.pending
            .as_ref()
            .is_some_and(|(serial, _)| *serial == self.serial)
        {
            m.pending = None;
        }
    }
}
impl Service {
    pub(super) fn reserve_manual(
        &self,
        profile: &Profile,
        bindings: &[Binding],
        live: bool,
    ) -> Result<ManualReservation, String> {
        let mut m = self
            .companion_runs
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if m.pending.is_some() {
            return Err("Another mapping start is already preparing resources.".into());
        }
        m.check(profile, bindings, live, None, None)?;
        m.serial = m
            .serial
            .checked_add(1)
            .ok_or("Mapping resource counter exhausted.")?;
        let serial = m.serial;
        m.pending = Some((serial, Resources::of(profile, bindings, live)));
        Ok(ManualReservation {
            manager: self.companion_runs.clone(),
            serial,
        })
    }
    pub(super) fn companion_sources(&self) -> Result<Value, String> {
        let mut response = self
            .companion_provider()
            .map_or_else(|_| Ok(json!({"companionSources":[]})), |p| p.list())?;
        let sources = response["companionSources"].as_array_mut().unwrap();
        let m = self
            .companion_runs
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        for c in m.configurations.values() {
            if !sources.iter().any(|s| s["id"] == c.instance_id) {
                sources.push(json!({"id":c.instance_id,"name":c.profile.name,"origin":"companion","online":false,"baselineReady":false,"publicationHash":c.publication_hash,"controlCount":c.profile.roles[0].controls.len(),"generation":"0","sequence":"0","fault":"Saved assignment is offline; reconnect and explicitly Start."}));
            }
        }
        Ok(response)
    }
    fn blueprint(&self, id: &str) -> Result<Configuration, String> {
        let provider = self
            .companion_blueprints
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .ok_or("Companion publication service is unavailable.")?;
        let p = provider(id)?;
        let p = Publication::validate(PublicationUpload {
            canonical_document: p.canonical_document,
            publication_hash: p.publication_hash,
            capability_fingerprint: Some(p.capability_fingerprint),
            assets: p.assets,
        })?;
        let definition = self.companion_provider()?.definition(id)?;
        if definition.publication_hash != p.publication_hash {
            return Err(
                "Companion assignment and published descriptor differ; reconcile them explicitly."
                    .into(),
            );
        }
        let authored = author(id, &p)?;
        for source in &p.controls {
            let expected = match source.kind {
                SourceKind::Button => crate::companions::ControlDefinition {
                    id: source.id.clone(),
                    kind: osg_mapping::profile::Kind::Button,
                    fallback: osg_mapping::profile::Value::Button(false),
                },
                SourceKind::Hat => crate::companions::ControlDefinition {
                    id: source.id.clone(),
                    kind: osg_mapping::profile::Kind::Hat,
                    fallback: osg_mapping::profile::Value::Hat(None),
                },
                SourceKind::Axis {
                    axis_mode,
                    fallback,
                } => crate::companions::ControlDefinition {
                    id: source.id.clone(),
                    kind: osg_mapping::profile::Kind::Axis {
                        range: match axis_mode {
                            osg_companion_service::AxisMode::Bipolar => {
                                osg_mapping::profile::AxisRange::Bipolar
                            }
                            osg_companion_service::AxisMode::Unipolar => {
                                osg_mapping::profile::AxisRange::Unipolar
                            }
                        },
                    },
                    fallback: osg_mapping::profile::Value::Axis(quantize_axis(
                        fallback, axis_mode, fallback,
                    )?),
                },
            };
            if !definition.controls.contains(&expected) {
                return Err(
                    "Assigned source controls/fallbacks do not match the immutable publication."
                        .into(),
                );
            }
        }
        if definition.controls.len() != p.controls.len() {
            return Err("Assigned source contains unexpected controls.".into());
        }
        Ok(authored)
    }
    fn save_route(&self, mut c: Configuration, request: &Value) -> Result<(), String> {
        if request["publicationHash"].as_str() != Some(c.publication_hash.as_str()) {
            return Err(
                "Confirm the current immutable publication hash when saving/reconciling.".into(),
            );
        }
        let _persist = self.persistence.lock().unwrap_or_else(|p| p.into_inner());
        let (mut all, root) = {
            let m = self
                .companion_runs
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if let Some(e) = &m.error {
                return Err(format!("Preserve and repair saved companion routes: {e}"));
            }
            if m.routes
                .get(&c.instance_id)
                .is_some_and(|r| r.reserved.load(Ordering::Acquire))
            {
                return Err(
                    "Stop this instance and resolve recovery before changing its configuration."
                        .into(),
                );
            }
            if let Some(bindings) = request.get("bindings") {
                c.bindings = serde_json::from_value(bindings.clone())
                    .map_err(|_| "Invalid local target bindings.")?;
            } else {
                c.bindings = m
                    .configurations
                    .get(&c.instance_id)
                    .map(|c| c.bindings.clone())
                    .unwrap_or_default();
            }
            (m.configurations.clone(), m.root.clone())
        };
        validate_configuration(&c)?;
        all.insert(c.instance_id.clone(), c);
        if all.len() > 32 {
            return Err("Companion configuration budget exhausted.".into());
        }
        let bytes = serde_json::to_vec(
            &json!({"schema":1,"configurations":all.values().collect::<Vec<_>>()}),
        )
        .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > CONFIG_LIMIT {
            return Err("Companion route configuration exceeds its byte budget.".into());
        }
        if let Some(root) = root {
            std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
            let mut file = tempfile::NamedTempFile::new_in(&root).map_err(|e| e.to_string())?;
            file.write_all(&bytes).map_err(|e| e.to_string())?;
            file.as_file().sync_all().map_err(|e| e.to_string())?;
            file.persist(root.join("companion-routes.json"))
                .map_err(|e| e.to_string())?;
        }
        self.companion_runs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .configurations = all;
        Ok(())
    }
    pub(super) fn confirm_companion_pins(&self, assignments: &[Assignment]) -> Result<(), String> {
        for a in assignments
            .iter()
            .filter(|a| a.source.origin == Origin::Companion)
        {
            let c = self.blueprint(&a.source.id)?;
            let m = self
                .companion_runs
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if m.configurations.get(&a.source.id).is_none_or(|saved| {
                saved.publication_hash != c.publication_hash
                    || saved.capability_fingerprint != c.capability_fingerprint
            }) {
                return Err("Confirm the companion's immutable revision in Companion controller routes before live Start.".into());
            }
        }
        Ok(())
    }
    pub(super) fn companion_request(&self, request: &Value) -> Result<Value, String> {
        let action = request["action"]
            .as_str()
            .ok_or("Select a companion route action.")?;
        let id = request["instanceId"]
            .as_str()
            .filter(|id| osg_mapping::profile::identifier(id))
            .ok_or("Select a companion instance.")?;
        match action {
            "save" | "reconcile" => self.save_route(self.blueprint(id)?, request)?,
            "start" => self.start_route(
                id,
                request["mode"]
                    .as_str()
                    .ok_or("Choose recording or live explicitly.")?,
            )?,
            "stop" => {
                let route = {
                    let mut m = self
                        .companion_runs
                        .lock()
                        .unwrap_or_else(|p| p.into_inner());
                    if m.configurations.contains_key(id) {
                        let epoch = m.epochs.entry(id.into()).or_default();
                        *epoch = epoch
                            .checked_add(1)
                            .ok_or("Companion lifecycle counter exhausted.")?;
                    }
                    m.routes.get(id).cloned()
                };
                if let Some(route) = route {
                    route.cancelled.store(true, Ordering::Release);
                    if let Ok(mut router) = route.router.try_lock() {
                        let result = router.stop_result();
                        let recovery =
                            result.is_err() || router.status()["recoveryRequired"] == true;
                        route.recovery.store(recovery, Ordering::Release);
                        if !route.starting.load(Ordering::Acquire) {
                            route.reserved.store(recovery, Ordering::Release);
                        }
                        result?;
                    }
                }
            }
            "recover" => self.recover_route(id)?,
            "inspect" | "recording" => {}
            _ => return Err("Unknown companion route action.".into()),
        }
        let mut response = self.route_view(id)?;
        if action == "recording" {
            let after: Counter =
                serde_json::from_value(request.get("after").cloned().unwrap_or(json!("0")))
                    .map_err(|_| "Invalid recording cursor.")?;
            let route = self
                .companion_runs
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .routes
                .get(id)
                .cloned();
            if let Some(route) = route {
                let router = route.router.try_lock().map_err(|_|"This companion's feeder is busy; retry its recording page without waiting for native I/O.")?;
                let page = router.runtime.recording.page(after, 32)?;
                // Profile/target descriptors are retained in this envelope.
                // Page transitions by byte budget as well as count; never drop a
                // transition or truncate a complete control/target description.
                let mut used = serde_json::to_vec(&response)
                    .map_err(|e| e.to_string())?
                    .len()
                    + 128;
                let mut records = Vec::new();
                let mut next = after;
                for record in page.records {
                    let bytes = serde_json::to_vec(&record)
                        .map_err(|e| e.to_string())?
                        .len()
                        + 1;
                    if used + bytes > 65_536 {
                        break;
                    }
                    used += bytes;
                    next = record.cursor;
                    records.push(record);
                }
                if records.is_empty() && page.next != after {
                    return Err("The complete companion descriptor and one recording transition exceed the local response budget.".into());
                }
                response["records"] = serde_json::to_value(records).unwrap();
                response["next"] = json!(next);
                response["gap"] = json!(page.gap);
            } else {
                response["records"] = json!([]);
                response["next"] = json!(after);
                response["gap"] = json!(false);
            }
        }
        let response = json!({"companionRoute":response});
        if serde_json::to_vec(&response)
            .map_err(|e| e.to_string())?
            .len()
            > 65_536
        {
            return Err("Complete companion route exceeds the local response budget.".into());
        }
        Ok(response)
    }
}
fn author(id: &str, p: &Publication) -> Result<Configuration, String> {
    let d: Value = serde_json::from_str(&p.canonical_document)
        .map_err(|_| "Invalid companion publication.")?;
    let controls = p
        .controls
        .iter()
        .map(|c| {
            let kind = match c.kind {
                SourceKind::Button => json!({"kind":"button"}),
                SourceKind::Hat => json!({"kind":"hat"}),
                SourceKind::Axis { axis_mode, .. } => json!({"kind":"axis","range":axis_mode}),
            };
            json!({"id":c.id,"kind":kind})
        })
        .collect::<Vec<_>>();
    let mut target_ids = BTreeMap::new();
    let mut targets = Vec::new();
    for t in d["targets"].as_array().ok_or("Missing authored targets.")? {
        let original = t["id"].as_str().ok_or("Missing logical target identity.")?;
        let target = id_for("target", &format!("{id}/{original}"));
        target_ids.insert(original.to_string(), target.clone());
        let axes = t["axes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|range| json!({"range":range,"fallback":0}))
            .collect::<Vec<_>>();
        targets.push(json!({"id":target,"name":t["label"].as_str().unwrap().chars().take(80).collect::<String>(),"layout":{"buttons":t["buttons"],"axes":axes,"hats":t["hats"]}}));
    }
    let mut nodes = Vec::new();
    for (index, b) in d["bindings"].as_array().unwrap().iter().enumerate() {
        let control = p
            .controls
            .iter()
            .find(|c| Some(c.id.as_str()) == b["controlId"].as_str())
            .ok_or("Unknown authored control.")?;
        let target = &target_ids[b["targetId"].as_str().unwrap()];
        if let SourceKind::Axis {
            axis_mode,
            fallback,
        } = control.kind
        {
            let t = targets.iter_mut().find(|t| t["id"] == *target).unwrap();
            t["layout"]["axes"][b["index"].as_u64().unwrap() as usize]["fallback"] =
                json!(quantize_axis(fallback, axis_mode, fallback)?);
        }
        nodes.push(json!({"id":format!("route-{index}"),"inputs":[{"kind":"source","role":"panel","control":control.id}],"output":{"target":target,"control":{"kind":b["kind"],"index":b["index"]}},"operation":{"kind":"direct","activation":"level"}}));
    }
    let document = json!({"schema":2,"id":profile_id(id),"name":p.title.chars().take(80).collect::<String>(),"roles":[{"id":"panel","name":"Companion instance","origin":"companion","controls":controls}],"targets":targets,"nodes":nodes,"arbitration":[],"requirements":{"virtualDevices":false,"deviceHiding":false,"visibility":[]}});
    let profile = Profile::parse(&serde_json::to_vec(&document).unwrap())?;
    let assignments = vec![Assignment {
        role: "panel".into(),
        source: Source {
            origin: Origin::Companion,
            id: id.into(),
        },
        controls: p
            .controls
            .iter()
            .map(|c| osg_mapping::engine::ControlAssignment {
                control: c.id.clone(),
                source_control: c.id.clone(),
            })
            .collect(),
    }];
    let c = Configuration {
        instance_id: id.into(),
        publication_hash: p.publication_hash.clone(),
        capability_fingerprint: p.capability_fingerprint.clone(),
        profile,
        assignments,
        bindings: vec![],
    };
    validate_configuration(&c)?;
    Ok(c)
}

impl Service {
    fn manual_recovery_resources(&self) -> Result<Option<Resources>, String> {
        let Some(root) = &self.root else {
            return Ok(None);
        };
        let path = root.join("output-recovery.json");
        let record = osg_virtual_controller::output_journal::inspect(&path)?;
        Ok(record.filter(|r| r.pending()).map(|r| Resources {
            roles: 0,
            targets: r.bindings.len(),
            nodes: 0,
            slots: r.bindings.iter().map(|b| b.slot).collect(),
            live: true,
        }))
    }
    fn route_view(&self, id: &str) -> Result<Value, String> {
        let current = self.blueprint(id);
        let (saved, route, error, journal) = {
            let m = self
                .companion_runs
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            (
                m.configurations.get(id).cloned(),
                m.routes.get(id).cloned(),
                m.error.clone(),
                m.journal(id),
            )
        };
        let desired = current.as_ref().ok().or(saved.as_ref()).ok_or_else(|| {
            current
                .as_ref()
                .err()
                .cloned()
                .unwrap_or_else(|| "Unknown companion configuration.".into())
        })?;
        let bindings = saved
            .as_ref()
            .map(|c| c.bindings.clone())
            .unwrap_or_default();
        let mut checks = Vec::new();
        let mut check = |id: &str, result: Result<(), String>| {
            let ok = result.is_ok();
            checks.push(
                json!({"id":id,"passed":ok,"reason":result.err().unwrap_or_else(||"Ready".into())}),
            );
            ok
        };
        let authority = check(
            "authorization",
            current.as_ref().map(|_| ()).map_err(Clone::clone),
        );
        let persistence = check("configurationStore", error.map_or(Ok(()), Err));
        let pinned = check(
            "configuration",
            match (&saved, &current) {
                (Some(saved), Ok(now))
                    if saved.publication_hash == now.publication_hash
                        && saved.capability_fingerprint == now.capability_fingerprint
                        && saved.profile.export()? == now.profile.export()? =>
                {
                    Ok(())
                }
                _ => Err(
                    "Save or explicitly reconcile the current immutable revision before Start."
                        .into(),
                ),
            },
        );
        let baseline = check(
            "baseline",
            self.companion_provider().and_then(|p| {
                p.require_ready(id, &desired.publication_hash, true)
                    .map(|_| ())
            }),
        );
        let binding = check(
            "targets",
            crate::settings::validate_bindings(&desired.profile, &bindings),
        );
        let enabled = {
            let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
            check(
                "enablement",
                if settings.error.is_none() && settings.value.virtual_devices {
                    Ok(())
                } else {
                    Err("Enable optional virtual output explicitly and repair any settings error before live Start.".into())
                },
            )
        };
        let qualified = check(
            "backend",
            if self.qualified {
                Ok(())
            } else {
                Err(QUALIFICATION.into())
            },
        );
        let platform = check(
            "backendCapabilities",
            if cfg!(any(target_os = "linux", target_os = "macos")) {
                bindings.iter().try_for_each(|b| {
                    osg_virtual_controller::uinput::Template::new(b.clone()).map(|_| ())
                })
            } else {
                Ok(())
            },
        );
        let inventory = check(
            "inventory",
            if cfg!(windows) && !bindings.is_empty() {
                (self.inspector)().and_then(|slots| {
                    bindings.iter().try_for_each(|b| {
                        let slot = slots
                            .iter()
                            .find(|s| s.id == b.slot)
                            .ok_or("Assigned virtual target is missing.")?;
                        b.match_slot(slot)?;
                        if slot.status != osg_virtual_controller::vjoy::Status::Free {
                            Err("Assigned virtual target is occupied by another feeder.".into())
                        } else {
                            Ok(())
                        }
                    })
                })
            } else {
                Ok(())
            },
        );
        let journal_state = journal.and_then(|path| {
            osg_virtual_controller::output_journal::inspect(&path)
                .map(|r| r.is_some_and(|r| r.pending()))
        });
        let unowned_recovery =
            self.root.is_some() && journal_state.as_ref().map_or(true, |pending| *pending);
        let recovery = check(
            "recovery",
            journal_state.and_then(|pending| {
                if pending {
                    Err("Recover interrupted output for this instance before live Start.".into())
                } else {
                    Ok(())
                }
            }),
        );
        let recovery_manual = self.manual_recovery_resources();
        let resources = check(
            "resources",
            (self.router.try_lock().map_err(|_| {
                "The manual mapping is preparing or busy; retry readiness.".to_string()
            }))
            .and_then(|r| {
                let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
                let manual = manual(&r, &settings).or(recovery_manual.clone()?);
                self.companion_runs
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .check(
                        &desired.profile,
                        &bindings,
                        false,
                        manual.as_ref(),
                        Some(id),
                    )
            }),
        );
        let live_resources = check(
            "liveResources",
            (self
                .router
                .try_lock()
                .map_err(|_| "The manual mapping is busy; retry readiness.".to_string()))
            .and_then(|r| {
                let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
                let manual = manual(&r, &settings).or(recovery_manual.clone()?);
                self.companion_runs
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .check(&desired.profile, &bindings, true, manual.as_ref(), Some(id))
            }),
        );
        let (mut session, available) = if let Some(route) = &route {
            let starting = route.starting.load(Ordering::Acquire);
            let cancelled = route.cancelled.load(Ordering::Acquire);
            let reserved = route.reserved.load(Ordering::Acquire);
            let recovery = route.recovery.load(Ordering::Acquire);
            let (mut status, busy) = match route.router.try_lock() {
                Ok(r) => (r.status(), false),
                Err(_) => (
                    json!({"running":false,"ready":false,"mode":if route.live{BACKEND}else{"recording"}}),
                    true,
                ),
            };
            let complete = !starting && !reserved && !recovery && !busy;
            status["starting"] = json!(starting);
            status["stopRequested"] = json!(cancelled);
            status["busy"] = json!(busy || starting);
            status["cleanupComplete"] = json!(complete);
            status["cleanup"] = json!(if recovery {
                "recovery-required"
            } else if complete {
                "complete"
            } else {
                "pending"
            });
            status["recoveryRequired"] = json!(recovery || status["recoveryRequired"] == true);
            (status, !reserved && !starting && !recovery)
        } else {
            (
                json!({"running":false,"ready":false,"mode":"recording","starting":false,"stopRequested":false,"busy":false,"cleanupComplete":!unowned_recovery,"cleanup":if unowned_recovery{"recovery-required"}else{"complete"},"recoveryRequired":unowned_recovery}),
                !unowned_recovery,
            )
        };
        session["mode"] = json!(if route.as_ref().is_some_and(|r| r.live) {
            "live"
        } else {
            "recording"
        });
        if !available {
            session["canStart"] = json!(false);
        } else {
            session["canStart"] = json!(true);
        }
        let recording_ready = authority && persistence && pinned && resources && available;
        let live_ready = recording_ready
            && baseline
            && binding
            && enabled
            && qualified
            && platform
            && inventory
            && recovery
            && live_resources;
        Ok(
            json!({"instanceId":id,"publicationHash":desired.publication_hash,"expectedPublicationHash":saved.as_ref().map(|c|&c.publication_hash),"capabilityFingerprint":desired.capability_fingerprint,"profile":desired.profile,"bindings":bindings,"preflight":{"recordingReady":recording_ready,"liveReady":live_ready,"checks":checks},"session":session}),
        )
    }
    fn start_route(&self, id: &str, mode: &str) -> Result<(), String> {
        let live = match mode {
            "recording" => false,
            "live" => true,
            _ => return Err("Choose recording or live explicitly.".into()),
        };
        let epoch = self
            .companion_runs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .epochs
            .get(id)
            .copied()
            .unwrap_or(0);
        let view = self.route_view(id)?;
        let ready = if live { "liveReady" } else { "recordingReady" };
        if view["preflight"][ready] != true {
            return Err(format!(
                "Companion route is not ready: {}",
                view["preflight"]["checks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|c| c["passed"] != true)
                    .filter_map(|c| c["reason"].as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        let current = self.blueprint(id)?;
        let recovery_manual = self.manual_recovery_resources()?;
        let route = {
            let _configuration = self.persistence.lock().unwrap_or_else(|p| p.into_inner());
            // Router -> manager is the fixed admission lock order. Release both
            // before compilation, persistence, backend acquisition or driver I/O.
            let global = self
                .router
                .try_lock()
                .map_err(|_| "The manual mapping is preparing; retry explicitly.")?;
            let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
            let manual = manual(&global, &settings).or(recovery_manual);
            drop(settings);
            let mut m = self
                .companion_runs
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if m.epochs.get(id).copied().unwrap_or(0) != epoch {
                return Err("Companion Start was superseded by Stop before admission.".into());
            }
            let saved = m
                .configurations
                .get(id)
                .cloned()
                .ok_or("Save this companion route first.")?;
            if saved.publication_hash != current.publication_hash
                || saved.profile.export()? != current.profile.export()?
            {
                return Err("Publication changed before Start; reconcile explicitly.".into());
            }
            if m.routes
                .get(id)
                .is_some_and(|r| r.reserved.load(Ordering::Acquire))
            {
                return Err("This instance is active or finishing cleanup.".into());
            }
            if m.routes
                .values()
                .filter(|r| r.reserved.load(Ordering::Acquire))
                .count()
                >= 8
            {
                return Err("Eight companion routes are already reserved.".into());
            }
            m.check(
                &saved.profile,
                &saved.bindings,
                live,
                manual.as_ref(),
                Some(id),
            )?;
            let route = Arc::new(Route {
                profile: saved.profile.clone(),
                bindings: saved.bindings.clone(),
                router: Mutex::new(Router::default()),
                cancelled: AtomicBool::new(false),
                starting: AtomicBool::new(true),
                reserved: AtomicBool::new(true),
                recovery: AtomicBool::new(false),
                live,
                worker: Mutex::new(None),
            });
            m.routes.insert(id.into(), route.clone());
            route
        };
        let outcome = (|| {
            let cancelled = || {
                if route.cancelled.load(Ordering::Acquire) || self.stop.load(Ordering::Acquire) {
                    Err("Companion Start cancelled by its Stop request.".to_string())
                } else {
                    Ok(())
                }
            };
            let config = self
                .companion_runs
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .configurations[id]
                .clone();
            if live {
                self.companion_provider()?
                    .require_ready(id, &config.publication_hash, true)?;
            }
            let prepared = self
                .companion_provider()?
                .prepare(&config.profile, &config.assignments)?;
            prepared.feed.require_explicit_rearm();
            let page = self.capture.stream_page("", 0);
            let snapshot = stream::Snapshot {
                session: page.session,
                cursor: page.cursor,
                captured_us: page.captured_us,
                sources: vec![],
            };
            let mut candidate = Router::default();
            candidate.start_with_sources(
                config.profile,
                config.assignments,
                snapshot,
                BTreeMap::new(),
                None,
                Some(prepared),
            )?;
            cancelled()?;
            let output = if live {
                candidate.require_live_sources()?;
                let journal = self
                    .companion_runs
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .journal(id)?;
                let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
                if settings.error.is_some() || !settings.value.virtual_devices {
                    return Err("Virtual output was disabled before activation.".into());
                }
                drop(settings);
                Some(
                    osg_virtual_controller::session::Session::start_with_journal(
                        self.output_factory.clone(),
                        candidate.runtime.token()?,
                        config.bindings,
                        Counter(0),
                        Some(journal),
                    )?,
                )
            } else {
                None
            };
            cancelled()?;
            // Authority may be revoked/reassigned while a backend is acquiring.
            if self.blueprint(id)?.publication_hash != config.publication_hash {
                return Err("Companion authority/revision changed during Start.".into());
            }
            route
                .router
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .adopt_checked(candidate, output, cancelled)?;
            Ok(())
        })();
        route.starting.store(false, Ordering::Release);
        if let Err(error) = outcome {
            route.cancelled.store(true, Ordering::Release);
            let journal = {
                self.companion_runs
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .journal(id)
                    .ok()
            };
            let pending = journal.is_some_and(|p| {
                osg_virtual_controller::output_journal::inspect(&p)
                    .map_or(true, |r| r.is_some_and(|r| r.pending()))
            });
            route.recovery.store(pending, Ordering::Release);
            route.reserved.store(pending, Ordering::Release);
            return Err(error);
        }
        let input = self.capture.clone();
        let stopped = self.stop.clone();
        let own = route.clone();
        let worker = std::thread::Builder::new()
            .name(format!(
                "companion-map-{}",
                &osg_logic::worker::hash(id)[..8]
            ))
            .spawn(move || {
                let mut was_ready = own
                    .router
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .runtime
                    .engine()
                    .is_some_and(|e| e.ready());
                while !stopped.load(Ordering::Acquire) && !own.cancelled.load(Ordering::Acquire) {
                    {
                        let mut r = own.router.lock().unwrap_or_else(|p| p.into_inner());
                        let (session, cursor) = r.cursor();
                        let page = input.stream_page(session, cursor);
                        let _ = r.pump(page);
                        let _ = r.flush_output();
                        let ready = r.runtime.engine().is_some_and(|e| e.ready());
                        if was_ready && !ready {
                            break;
                        }
                        was_ready |= ready;
                        if r.runtime.token().is_err() {
                            break;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                let mut r = own.router.lock().unwrap_or_else(|p| p.into_inner());
                let failed = r.stop_result().is_err() || r.status()["recoveryRequired"] == true;
                own.recovery.store(failed, Ordering::Release);
                own.reserved.store(failed, Ordering::Release);
            })
            .map_err(|e| e.to_string());
        match worker {
            Ok(worker) => *route.worker.lock().unwrap_or_else(|p| p.into_inner()) = Some(worker),
            Err(error) => {
                route.cancelled.store(true, Ordering::Release);
                let failed = route
                    .router
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .stop_result()
                    .is_err();
                route.recovery.store(failed, Ordering::Release);
                route.reserved.store(failed, Ordering::Release);
                return Err(error);
            }
        }
        Ok(())
    }
    /// This is not a wire/operator input command. It observes successful native
    /// submissions for the dedicated authored route and never acquires output.
    pub fn companion_assignment_available(&self, id: &str) -> Result<(), String> {
        let m = self
            .companion_runs
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if m.routes.get(id).is_some_and(|r| {
            r.reserved.load(Ordering::Acquire)
                || r.starting.load(Ordering::Acquire)
                || r.recovery.load(Ordering::Acquire)
        }) {
            return Err("Stop this companion mapping and complete recovery before a captured assignment change.".into());
        }
        if let Ok(path) = m.journal(id) {
            if osg_virtual_controller::output_journal::inspect(&path)?.is_some_and(|r| r.pending())
            {
                return Err(
                    "Resolve this companion's interrupted output before changing its assignment."
                        .into(),
                );
            }
        }
        Ok(())
    }
    pub fn companion_submission(&self, id: &str) -> Result<Value, String> {
        let (config, route) = {
            let m = self
                .companion_runs
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            (
                m.configurations
                    .get(id)
                    .cloned()
                    .ok_or("No saved companion route.")?,
                m.routes
                    .get(id)
                    .cloned()
                    .ok_or("Companion mapping is stopped.")?,
            )
        };
        let router = route
            .router
            .try_lock()
            .map_err(|_| "Companion submission proof is temporarily busy.")?;
        let output = router.submission_snapshot();
        if output.is_null() || route.cancelled.load(Ordering::Acquire) {
            return Ok(json!({"confirmed":false}));
        }
        let profile = serde_json::to_value(&config.profile).map_err(|e| e.to_string())?;
        let mut values = serde_json::Map::new();
        for node in profile["nodes"].as_array().unwrap() {
            let Some(control) = node["inputs"][0]["control"].as_str() else {
                continue;
            };
            let Some(target) = node["output"]["target"].as_str() else {
                continue;
            };
            let Some(index) = node["output"]["control"]["index"]
                .as_u64()
                .map(|i| i as usize)
            else {
                continue;
            };
            let state = &output[target];
            let value = match node["output"]["control"]["kind"].as_str() {
                Some("button") => state["buttons"][index].clone(),
                Some("hat") => state["hats"][index]
                    .as_i64()
                    .map_or(json!(-1), |v| json!(v)),
                Some("axis") => {
                    let Some(n) = state["axes"][index].as_f64() else {
                        continue;
                    };
                    json!(if n < 0. { n / 32768. } else { n / 32767. })
                }
                _ => continue,
            };
            if !value.is_null() {
                values.insert(control.into(), value);
            }
        }
        Ok(json!({"confirmed":true,"publicationHash":config.publication_hash,"values":values}))
    }
    fn recover_route(&self, id: &str) -> Result<(), String> {
        if !self.qualified {
            return Err(QUALIFICATION.into());
        }
        let (route, path) = {
            let m = self
                .companion_runs
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            (m.routes.get(id).cloned(), m.journal(id)?)
        };
        if let Some(route) = route {
            if route.starting.load(Ordering::Acquire) {
                return Err(
                    "Wait for this instance's pending Start cleanup before recovery.".into(),
                );
            }
            let mut r = route
                .router
                .try_lock()
                .map_err(|_| "This instance's native cleanup is still pending.")?;
            r.recover_durable(self.output_factory.clone(), path)?;
            route.recovery.store(false, Ordering::Release);
            route.reserved.store(false, Ordering::Release);
        } else {
            osg_virtual_controller::session::Session::recover_stored(
                self.output_factory.clone(),
                path,
            )?;
        }
        self.companion_runs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .orphaned
            .remove(&format!("{}.json", id_for("instance", id)));
        Ok(())
    }
}
// The parent worker never performs per-instance I/O; each route owns its bounded
// worker so a stalled feeder cannot block another companion or the manual run.
pub(super) fn stop_all(manager: &Arc<Mutex<Manager>>, live_only: bool) {
    let routes = manager
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .routes
        .values()
        .filter(|r| !live_only || r.live)
        .cloned()
        .collect::<Vec<_>>();
    for route in &routes {
        route.cancelled.store(true, Ordering::Release);
    }
    for route in routes {
        if let Some(worker) = route
            .worker
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            let _ = worker.join();
        } else if !route.starting.load(Ordering::Acquire) {
            let failed = route
                .router
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .stop_result()
                .is_err();
            route.recovery.store(failed, Ordering::Release);
            route.reserved.store(failed, Ordering::Release);
        }
    }
}

#[cfg(test)]
#[path = "companion_run_tests.rs"]
mod tests;
