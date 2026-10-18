//! Native inventory cache. Provider discovery runs independently of UI requests;
//! reads below cannot start a profile, claim a writer or submit an action.
use osg_profiles::resources::{
    Access, Observation, PortPreparation, ProviderBatch, Registry, ResourceToken, Route,
};
use osg_profiles::resources_store::Store;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub(crate) type Collect =
    Arc<dyn Fn(u64) -> Vec<(String, Result<ProviderBatch, String>)> + Send + Sync>;
/// Native Hub-verified desired metadata. Never accepted from an IPC request.
#[derive(Clone)]
pub(crate) struct CompanionCatalogue {
    pub logical_resource_id: String,
    pub inventory_resource_id: String,
    pub instance_id: String,
    pub publication_hash: String,
    pub capability_fingerprint: String,
    pub ports: Vec<osg_profiles::resources_ports::PortDescriptor>,
}
struct State {
    registry: Registry,
    store: Option<Store>,
    saved: String,
    diagnostics: Vec<String>,
    read_only: bool,
    fingerprints: BTreeMap<String, String>,
    pending_save: bool,
    game_metadata: BTreeMap<String, Observation>,
    game_deliveries: BTreeMap<String, ProviderBatch>,
    game_command_withdrawals: BTreeMap<(String, String), BTreeSet<String>>,
}
pub(crate) struct Runtime {
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
    collect: Collect,
    local_revision: Arc<AtomicU64>,
    /// Serializes discovery revisions without excluding live delivery adoption.
    collector_gate: Arc<Mutex<()>>,
    capture_gate: Arc<Mutex<()>>,
}
impl Runtime {
    /// Resolve only retained native identity. Authoring never triggers discovery,
    /// grants a port token or selects a transport for a future Apply.
    pub(crate) fn companion_authoring_identity(&self, resource: &str) -> Result<(String, String), String> {
        if self.stop.load(Ordering::Acquire) { return Err("Resource inventory has stopped.".into()); }
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let details = state.registry.inspect_selected(resource, &[])?;
        if details.resource.resource_type != osg_profiles::resources::ResourceType::Companion {
            return Err("Choose a retained native companion resource.".into());
        }
        let instances = details.evidence.iter().filter(|e| e.kind == osg_profiles::resources::EvidenceKind::CompanionInstance).collect::<Vec<_>>();
        if instances.len() != 1 || instances[0].value.is_empty() {
            return Err("The companion has no single exact native paired identity.".into());
        }
        Ok((details.resource.id, instances[0].value.clone()))
    }
    /// Capture only cached descriptors and exact local port assignments. This
    /// method performs no provider I/O and never changes inventory authority.
    pub(crate) fn snapshot_for_compile(
        &self,
        assignments: Vec<osg_profiles::store::LocalAssignment>,
    ) -> Result<osg_profiles::compiler_models::CompileSnapshot, String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Resource inventory has stopped.".into());
        }
        if assignments.len() > 64
            || assignments
                .iter()
                .map(|assignment| assignment.ports.len())
                .sum::<usize>()
                > 1024
        {
            return Err("The compiler capture exceeds the profile resource/port limit.".into());
        }
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        Self::snapshot_locked(&state, assignments)
    }
    fn snapshot_locked(
        state: &State,
        mut assignments: Vec<osg_profiles::store::LocalAssignment>,
    ) -> Result<osg_profiles::compiler_models::CompileSnapshot, String> {
        use osg_profiles::compiler_models::{
            BackendCapabilities, CapturedResource, CompileSnapshot,
        };
        use osg_profiles::resources_ports::PortContract;
        use osg_profiles::Direction;
        let mut resources: Vec<CapturedResource> = Vec::new();
        let mut diagnostics = state.diagnostics.clone();
        for assignment in &mut assignments {
            let details = match state
                .registry
                .inspect_selected(&assignment.source_id, &assignment.ports)
            {
                Ok(details) => details,
                Err(error) => {
                    diagnostics.push(format!(
                        "Resource {} could not be captured: {error}",
                        assignment.resource_id
                    ));
                    continue;
                }
            };
            // Resolve retained UUID redirects in this immutable capture. The
            // saved local assignment is unchanged; no identity is inferred.
            assignment.source_id = details.resource.id.clone();
            let mut tokens = Vec::new();
            for observed in &details.ports {
                if !observed.current {
                    continue;
                }
                let access = match (
                    &observed.descriptor.direction,
                    &observed.descriptor.contract,
                ) {
                    (Direction::Output, _) => Access::Read,
                    (Direction::Input, PortContract::Action { .. }) => Access::Invoke,
                    (Direction::Input, _) => Access::Write,
                };
                if state.read_only && access != Access::Read {
                    continue;
                }
                if let Ok(token) =
                    state
                        .registry
                        .token(&details.resource.id, &observed.descriptor.id, access)
                {
                    if token.contract_hash == observed.contract_hash && !Self::command_withdrawn(state, &token) && !tokens.contains(&token) {
                        tokens.push(token);
                    }
                }
            }
            if let Some(resource) = resources
                .iter_mut()
                .find(|resource| resource.summary.id == details.resource.id)
            {
                for port in details.ports {
                    if !resource.ports.iter().any(|old| {
                        old.descriptor.id == port.descriptor.id
                            && old.contract_hash == port.contract_hash
                    }) {
                        resource.ports.push(port);
                    }
                }
                for token in tokens {
                    if !resource.tokens.contains(&token) {
                        resource.tokens.push(token);
                    }
                }
            } else {
                resources.push(CapturedResource {
                    provider_setup_revision: details
                        .resource
                        .attributes
                        .get("providerSetupRevision")
                        .cloned(),
                    game_id: details.resource.attributes.get("gameId").cloned(),
                    summary: details.resource,
                    ports: details.ports,
                    tokens,
                });
            }
        }
        resources.sort_by(|left, right| left.summary.id.cmp(&right.summary.id));
        for resource in &mut resources {
            resource.ports.sort_by(|left, right| {
                (&left.descriptor.id, &left.contract_hash)
                    .cmp(&(&right.descriptor.id, &right.contract_hash))
            });
            resource.tokens.sort_by(|left, right| {
                (&left.port_id, &left.route_id, &left.contract_hash).cmp(&(
                    &right.port_id,
                    &right.route_id,
                    &right.contract_hash,
                ))
            });
        }
        Ok(CompileSnapshot {
            inventory_session: state.registry.session().into(),
            inventory_revision: state.registry.revision(),
            assignments,
            resources,
            game_context: None,
            backend: BackendCapabilities {
                name: if cfg!(windows) {
                    "windows-preflight-required"
                } else {
                    "platform-activation-unqualified"
                }
                .into(),
                ..BackendCapabilities::default()
            },
            inventory_diagnostics: diagnostics,
            read_only: state.read_only,
        })
    }
    /// Deliberately distinct from delivery capture; no token or sample is fabricated.
    pub(crate) fn snapshot_for_preparation(
        &self,
        assignments: Vec<osg_profiles::store::LocalAssignment>,
    ) -> Result<
        (
            osg_profiles::compiler_models::CompileSnapshot,
            Vec<PortPreparation>,
        ),
        String,
    > {
        self.snapshot_for_preparation_with_companions(assignments, &[])
    }
    /// Check only the runtime/storage status used by final native admission.
    /// This grants no port or route authority: each retained preparation pin
    /// must still pass validate_preparation against the current registry.
    pub(crate) fn validate_preparation_state(
        &self,
        assignments: &[osg_profiles::store::LocalAssignment],
    ) -> Result<(), String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Resource inventory has stopped.".into());
        }
        if assignments.len() > 64 || assignments.iter().map(|a| a.ports.len()).sum::<usize>() > 1024 {
            return Err("Preparation capture exceeds bounded assignments.".into());
        }
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.read_only || state.registry.session().is_empty() {
            return Err("The native prepared profile identity changed.".into());
        }
        Ok(())
    }
    pub(crate) fn snapshot_for_preparation_with_companions(
        &self,
        assignments: Vec<osg_profiles::store::LocalAssignment>,
        companions: &[CompanionCatalogue],
    ) -> Result<(osg_profiles::compiler_models::CompileSnapshot, Vec<PortPreparation>), String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Resource inventory has stopped.".into());
        }
        if assignments.len() > 64 || assignments.iter().map(|a| a.ports.len()).sum::<usize>() > 1024
        {
            return Err("Preparation capture exceeds bounded assignments.".into());
        }
        let mut selected = BTreeSet::new();
        let mut instances = BTreeSet::new();
        if companions.len()>8 || companions.iter().map(|c|c.ports.len()).sum::<usize>()>1024
            || companions.iter().any(|c| !selected.insert(&c.logical_resource_id) || !instances.insert(&c.instance_id)
                || !assignments.iter().any(|a|a.resource_id==c.logical_resource_id && a.source_id==c.inventory_resource_id)) {
            return Err("Companion preparation needs unique exact assigned instances within its bounded catalogue.".into());
        }
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut snapshot = Self::snapshot_locked(&state, assignments)?;
        let mut preparations = Vec::new();
        let mut fingerprints = BTreeSet::new();
        if !state.read_only {
            for assignment in &snapshot.assignments {
                let companion=companions.iter().find(|c|c.logical_resource_id==assignment.resource_id);
                for expected in &assignment.ports {
                    let descriptor = if let Some(companion)=companion {
                        companion.ports.iter().find(|p|p.id==expected.observed_port_id && p.contract_hash().ok().as_ref()==Some(&expected.expected_contract_hash))
                    } else { snapshot
                        .resources
                        .iter()
                        .find(|r| r.summary.id == assignment.source_id)
                        .and_then(|resource| {
                            resource.ports.iter().find(|p| {
                                p.descriptor.id == expected.observed_port_id
                                    && p.contract_hash == expected.expected_contract_hash
                            }).map(|p|&p.descriptor)
                        }) };
                    let Some(descriptor) = descriptor
                    else {
                        continue;
                    };
                    let access = match (
                        &descriptor.direction,
                        &descriptor.contract,
                    ) {
                        (osg_profiles::Direction::Output, _) => Access::Read,
                        (_, osg_profiles::PortContract::Action { .. }) => Access::Invoke,
                        _ => Access::Write,
                    };
                    let Some(captured) = snapshot
                        .resources
                        .iter()
                        .find(|r| r.summary.id == assignment.source_id)
                    else { continue; };
                    let preparation = if let Some(companion)=companion {
                        state.registry.prepare_companion_port(&assignment.source_id,&companion.instance_id,&companion.publication_hash,&companion.capability_fingerprint,descriptor.clone(),access)
                    } else if captured.summary.resource_type
                        == osg_profiles::resources::ResourceType::GameIntegration
                    {
                        captured
                            .game_id
                            .as_deref()
                            .ok_or_else(|| "Game metadata has no exact game identity.".to_owned())
                            .and_then(|game| {
                                state.registry.prepare_game_metadata_port(
                                    &assignment.source_id,
                                    &expected.observed_port_id,
                                    &expected.expected_contract_hash,
                                    access,
                                    game,
                                )
                            })
                    } else {
                        state.registry.prepare_port(
                            &assignment.source_id,
                            &expected.observed_port_id,
                            &expected.expected_contract_hash,
                            access,
                        )
                    };
                    match preparation {
                        Ok(pin) => {
                            if fingerprints.insert(pin.fingerprint()?) {
                                preparations.push(pin)
                            }
                        }
                        Err(error) => snapshot.inventory_diagnostics.push(format!(
                            "Resource {} port {} cannot prepare: {error}",
                            assignment.resource_id, expected.profile_port_id
                        )),
                    }
                }
            }
        }
        Ok((snapshot, preparations))
    }
    /// Cached delivery check only. Calling this never discovers/acquires a provider.
    pub(crate) fn validate_delivery(&self, token: &ResourceToken) -> Result<Route, String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Resource inventory has stopped.".into());
        }
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.read_only && token.access != Access::Read {
            return Err("Repair retained inventory before output delivery.".into());
        }
        if Self::command_withdrawn(&state, token) {
            return Err("This DCS command's publication authority was withdrawn; Apply a fresh profile.".into());
        }
        Ok(state
            .registry
            .validate_token(token, &token.port_id, token.access)?
            .route)
    }
    pub(crate) fn validate_preparation(
        &self,
        preparation: &PortPreparation,
    ) -> Result<Route, String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Resource inventory has stopped.".into());
        }
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.read_only {
            return Err("Repair retained inventory before acquisition.".into());
        }
        Ok(state.registry.validate_preparation(preparation)?.route)
    }

    pub(crate) fn new(root: Option<PathBuf>, collect: Collect, native: bool) -> Self {
        let store = root.map(Store::new);
        let loaded = store
            .as_ref()
            .map_or(Ok(None), Store::load)
            .and_then(|saved| saved.map(Registry::restore).transpose());
        let (registry, diagnostics, read_only) = match loaded {
            Ok(registry) => (registry.unwrap_or_default(), vec![], false),
            Err(error) => (
                Registry::new(),
                vec![format!(
                    "Retained inventory could not be loaded; it will not be overwritten: {error}"
                )],
                true,
            ),
        };
        let state = Arc::new(Mutex::new(State {
            saved: serde_json::to_string(&registry.saved()).unwrap(),
            registry,
            store,
            diagnostics,
            read_only,
            fingerprints: BTreeMap::new(),
            pending_save: false,
            game_metadata: BTreeMap::new(),
            game_deliveries: BTreeMap::new(),
            game_command_withdrawals: BTreeMap::new(),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let local_revision = Arc::new(AtomicU64::new(0));
        let collector_gate = Arc::new(Mutex::new(()));
        let capture_gate = Arc::new(Mutex::new(()));
        let worker = if native {
            let state = state.clone();
            let stopped = stop.clone();
            let collect = collect.clone();
            let revisions = local_revision.clone();
            let collectors = collector_gate.clone();
            let gate = capture_gate.clone();
            Some(
                thread::Builder::new()
                    .name("profile-resource-inventory".into())
                    .spawn(move || {
                        while !stopped.load(Ordering::Acquire) {
                            {
                                let _collector =
                                    collectors.lock().unwrap_or_else(|p| p.into_inner());
                                if stopped.load(Ordering::Acquire) {
                                    break;
                                }
                                let revision = revisions.fetch_add(1, Ordering::AcqRel) + 1;
                                let batches = collect(revision);
                                // Discovery may block; the live reader must keep
                                // publishing and renewing while it is in flight.
                                let _capture = gate.lock().unwrap_or_else(|p| p.into_inner());
                                if stopped.load(Ordering::Acquire) {
                                    break;
                                }
                                Self::adopt(&state, batches);
                            }
                            for _ in 0..20 {
                                if stopped.load(Ordering::Acquire) {
                                    break;
                                }
                                thread::sleep(Duration::from_millis(50));
                            }
                        }
                    })
                    .expect("profile inventory worker"),
            )
        } else {
            None
        };
        Self {
            state,
            stop,
            worker: Mutex::new(worker),
            collect,
            local_revision,
            collector_gate,
            capture_gate,
        }
    }
    /// Explicit native coordinator refresh. Cached capture/validation and native
    /// browser reads never call this; acquisitions opt into bounded provider I/O.
    pub(crate) fn refresh_native(&self) -> Result<(), String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Resource inventory has stopped.".into());
        }
        let _collector = self
            .collector_gate
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if self.stop.load(Ordering::Acquire) {
            return Err("Resource inventory has stopped.".into());
        }
        let revision = self.local_revision.fetch_add(1, Ordering::AcqRel) + 1;
        let batches = (self.collect)(revision);
        let _capture = self.capture_gate.lock().unwrap_or_else(|p| p.into_inner());
        if self.stop.load(Ordering::Acquire) {
            return Err("Resource inventory has stopped.".into());
        }
        Self::adopt(&self.state, batches);
        Ok(())
    }
    fn adopt(state: &Mutex<State>, batches: Vec<(String, Result<ProviderBatch, String>)>) {
        let mut state = state.lock().unwrap_or_else(|p| p.into_inner());
        let mut changed = false;
        for (scope, batch) in batches {
            let metadata = batch.as_ref().ok().and_then(|batch| {
                scope
                    .strip_prefix("game:")
                    .map(|id| (id.to_owned(), batch.clone()))
            });
            let result = batch.and_then(|batch| {
                if batch.scope != scope {
                    return Err("Provider scope does not match its envelope.".into());
                }
                use sha2::{Digest, Sha256};
                let fingerprint = format!(
                    "{:x}",
                    Sha256::digest(
                        serde_json::to_vec(&(&batch.session, batch.complete, &batch.observations))
                            .map_err(|e| e.to_string())?
                    )
                );
                if state.fingerprints.get(&scope) == Some(&fingerprint) {
                    return Ok(());
                }
                state.registry.reconcile(batch)?;
                state.fingerprints.insert(scope.clone(), fingerprint);
                changed = true;
                Ok(())
            });
            if scope.starts_with("game:") {
                let game = scope.trim_start_matches("game:");
                let item = metadata.filter(|_| result.is_ok()).and_then(|(_, batch)| {
                    batch
                        .observations
                        .into_iter()
                        .find(|item| item.key == game && game_metadata_valid(item, game))
                });
                if let Some(item) = item {
                    state.game_metadata.insert(game.into(), item);
                } else {
                    state.game_metadata.remove(game);
                }
                if state.game_deliveries.get(game).is_some_and(|batch| {
                    state
                        .game_metadata
                        .get(game)
                        .is_none_or(|metadata| validate_game_delivery(batch, metadata).is_err())
                }) {
                    let overlay = state.game_deliveries.remove(game).unwrap();
                    state.registry.provider_failed(&overlay.scope, "Game process, setup or catalogue changed; acquire a fresh native baseline.");
                    state.fingerprints.remove(&overlay.scope);
                    changed = true;
                }
            }
            if let Err(error) = result {
                state.fingerprints.remove(&scope);
                state.registry.provider_failed(&scope, &error);
                changed = true;
            }
        }
        if !changed && !state.pending_save {
            return;
        }
        Self::persist_locked(&mut state);
    }
    fn persist_locked(state: &mut State) {
        let saved =
            serde_json::to_string(&state.registry.saved()).expect("inventory metadata JSON");
        if saved != state.saved && !state.read_only {
            if let Some(store) = &state.store {
                if let Err(error) = store.save(&state.registry.saved()) {
                    state.diagnostics = vec![format!("Cannot retain resource metadata: {error}")];
                    state.pending_save = true;
                    return;
                }
            }
            state.saved = saved;
            state.pending_save = false;
            state.diagnostics.clear();
        }
    }
    pub(crate) fn request(&self, request: &Value) -> Result<Value, String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Resource inventory has stopped.".into());
        }
        let command = request["command"]
            .as_str()
            .ok_or("Missing inventory command.")?;
        let fields: &[&str] = match command {
            "profile_resources_list" => &["offset", "limit"],
            "profile_resources_inspect" => &["resourceId", "offset", "limit"],
            "profile_resources_token" => &["resourceId", "portId", "access"],
            "profile_resources_validate" => &["resourceToken", "portId", "access"],
            _ => return Err("Unknown inventory operation.".into()),
        };
        if request
            .as_object()
            .ok_or("Inventory requests must be objects.")?
            .keys()
            .any(|key| key != "command" && !fields.contains(&key.as_str()))
        {
            return Err("Unknown inventory request field.".into());
        }
        // Tests/development without native capture use an explicit synthetic
        // collector. Native UI reads never trigger discovery or capability I/O.
        if self
            .worker
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_none()
        {
            self.refresh_native()?;
        }
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let text = |key: &str| {
            request[key]
                .as_str()
                .ok_or_else(|| format!("{key} must be a string."))
        };
        let access = || {
            serde_json::from_value::<Access>(request["access"].clone())
                .map_err(|_| "Choose read, write or invoke access.".to_owned())
        };
        let offset = page_number(request, "offset", 0, 40_000)?;
        let limit = page_number(request, "limit", 32, 128)?;
        let mut result = match command {
            "profile_resources_list" => {
                serde_json::to_value(state.registry.list(offset, limit)?).unwrap()
            }
            "profile_resources_inspect" => {
                serde_json::to_value(state.registry.inspect(text("resourceId")?, offset, limit)?)
                    .unwrap()
            }
            "profile_resources_token" => {
                let access = access()?;
                if state.read_only && access != Access::Read {
                    return Err(
                        "Repair the retained inventory before acquiring write authority.".into(),
                    );
                }
                json!({"resourceToken":state.registry.token(text("resourceId")?, text("portId")?, access)?})
            }
            _ => {
                let token: ResourceToken = serde_json::from_value(request["resourceToken"].clone())
                    .map_err(|_| "Invalid resource token.")?;
                let access = access()?;
                if state.read_only && access != Access::Read {
                    return Err(
                        "Repair the retained inventory before acquiring write authority.".into(),
                    );
                }
                state
                    .registry
                    .validate_token(&token, text("portId")?, access)?;
                json!({"valid":true})
            }
        };
        result["inventoryDiagnostics"] = json!(state.diagnostics);
        result["readOnly"] = json!(state.read_only);
        // Bound the browser envelope without truncating descriptors. A short
        // page advances by its actual length; total and revision stay intact.
        while serde_json::to_vec(&result)
            .map_err(|e| e.to_string())?
            .len()
            > 1_900_000
        {
            let key = if result.get("ports").is_some() {
                "ports"
            } else {
                "resources"
            };
            let page = result[key]
                .as_array_mut()
                .ok_or("Inventory reply exceeds its size limit.")?;
            if page.len() <= 1 {
                return Err("A resource descriptor exceeds the inventory reply limit.".into());
            }
            page.pop();
        }
        Ok(result)
    }
    pub(crate) fn shutdown(&self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = worker.join();
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn page_number(request: &Value, key: &str, default: usize, max: usize) -> Result<usize, String> {
    request.get(key).map_or(Ok(default), |value| {
        value
            .as_u64()
            .filter(|n| *n <= max as u64)
            .map(|n| n as usize)
            .ok_or_else(|| format!("Invalid inventory {key}."))
    })
}

pub(crate) fn batch(
    scope: &str,
    session: &str,
    revision: u64,
    observations: Vec<Observation>,
) -> ProviderBatch {
    ProviderBatch {
        scope: scope.into(),
        session: session.into(),
        revision,
        complete: true,
        observations,
    }
}
pub(crate) fn attributes<const N: usize>(fields: [(&str, String); N]) -> BTreeMap<String, String> {
    fields
        .into_iter()
        .collect::<BTreeMap<_, _>>()
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, value)| (key.into(), value))
        .collect()
}

#[path = "profile_resources_game.rs"]
mod game_delivery;
use game_delivery::{game_metadata_valid, validate_game_delivery};

#[cfg(test)]
#[path = "profile_resources_tests.rs"]
mod tests;
