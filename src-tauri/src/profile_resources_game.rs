//! A native acquired game reader publishes delivery separately from catalogue
//! metadata. No browser request can publish samples or acquire this authority.
use super::*;
use osg_profiles::{
    compiler_models::{ExecutionPlan, PlanReadiness},
    resources::{EvidenceKind, ResourceType, Transport},
};

fn catalogue_game(key: &str) -> Option<&str> {
    if matches!(key, "dcs" | "assetto-corsa" | "assetto-corsa-competizione" | "assetto-corsa-evo" | "assetto-corsa-rally" | "x-plane-11" | "x-plane-12") { return Some(key); }
    key.strip_prefix("dcs:").filter(|module| !module.is_empty() && module.trim()==*module && module.len() <= 63
        && module.bytes().all(|byte|byte.is_ascii_alphanumeric() || b"_- ".contains(&byte))).map(|_|"dcs")
}
pub(super) fn game_metadata_valid(item: &Observation, key: &str) -> bool {
    let Some(game)=catalogue_game(key) else{return false;};
    item.key == key
        && item.resource_type == ResourceType::GameIntegration
        && item.attributes.get("gameId").map(String::as_str) == Some(game)
        && (key == game || item.attributes.get("aircraftModule").map(String::as_str) == key.strip_prefix("dcs:"))
        && (item.attributes.get("aircraftModule").map(String::as_str) != Some("CommonData")
            || item.ports.as_ref().is_some_and(|ports|ports.ports.iter().all(|port|port.direction==osg_profiles::Direction::Output)))
        && item.attributes.get("processVerified").map(String::as_str) == Some("true")
        && matches!(
            item.attributes.get("gameContextStatus").map(String::as_str),
            Some("ready" | "waiting-profile")
        )
        && ["providerSetupRevision", "catalogueRevision"]
            .iter()
            .all(|field| item.attributes.get(*field).is_some_and(|s| !s.is_empty()))
        && item
            .evidence
            .iter()
            .any(|e| e.kind == EvidenceKind::GameScope && e.value == key)
        && item.route.as_ref().is_some_and(|r| {
            r.id == format!("game:{key}")
                && r.transport == Transport::Integration
                && !r.baseline_ready
                && !r.writable
        })
        && item
            .ports
            .as_ref()
            .is_some_and(|p| p.complete && p.validate().is_ok())
}

/// Created only when a changed native metadata batch is admitted. Keeping the
/// observation private and immutable ties its fully validated port index to the
/// exact provider fingerprint, setup and catalogue for every later read.
pub(super) struct GameMetadata {
    observation: Observation,
    fingerprint: String,
    contracts: BTreeMap<String, String>,
}
impl GameMetadata {
    pub(super) fn capture(item: Observation, fingerprint: String) -> Option<Arc<Self>> {
        if !game_metadata_valid(&item, &item.key) { return None; }
        let contracts = item.ports.as_ref()?.ports.iter()
            .map(|port| Ok((port.id.clone(), port.contract_hash()?)))
            .collect::<Result<_, String>>().ok()?;
        Some(Arc::new(Self { observation: item, fingerprint, contracts }))
    }
}

#[cfg(test)]
fn validate_game_delivery(
    batch: &ProviderBatch,
    metadata: &Observation,
) -> Result<(), String> {
    batch.validate()?;
    if !batch.complete || batch.observations.len()!=1 {
        return Err("Fresh game delivery requires one complete native game observation and current verified metadata.".into());
    }
    let metadata = GameMetadata::capture(metadata.clone(), "test-native-capture".into())
        .ok_or("Game delivery requires valid current native metadata.")?;
    validate_game_item(batch,&batch.observations[0],&metadata)
}
fn validate_game_item(batch:&ProviderBatch,item:&Observation,admitted:&GameMetadata)->Result<(),String>{
    let metadata = &admitted.observation;
    let key=metadata.key.as_str();
    let game=catalogue_game(key).ok_or("Unknown game catalogue scope.")?;
    if admitted.fingerprint.is_empty() || batch.scope!=format!("profile-game:{game}") {
        return Err("Game delivery requires current verified metadata for its exact native module.".into());
    }
    if item.key != key
        || item.resource_type != ResourceType::GameIntegration
        || item.evidence != metadata.evidence
        || item.attributes.get("gameId") != metadata.attributes.get("gameId")
        || item.attributes.get("gameReaderSession") != Some(&batch.session)
        || !item
            .attributes
            .get("gamePreparationSession")
            .is_some_and(|proof| osg_profiles::resources_ports::text(proof, 256).is_ok())
        || ["providerSetupRevision", "catalogueRevision", "aircraftModule"]
            .iter()
            .any(|field| item.attributes.get(*field) != metadata.attributes.get(*field))
    {
        return Err(
            "Game delivery identity, preparation owner, setup or catalogue changed.".into(),
        );
    }
    let route = item
        .route
        .as_ref()
        .ok_or("Game delivery needs its actual native route.")?;
    if route.id != format!("profile-game:{key}")
        || route.transport != Transport::Integration
        || route.priority != 100
        || (route.writable && !route.baseline_ready)
        || [
            "gameId",
            "providerSetupRevision",
            "catalogueRevision",
            "gamePreparationSession",
            "gameReaderSession",
            "aircraftModule",
        ]
        .iter()
        .any(|field| route.attributes.get(*field) != item.attributes.get(*field))
    {
        return Err("Game delivery route does not match the acquired native reader.".into());
    }
    let ports = item
        .ports
        .as_ref()
        .filter(|p| p.complete)
        .ok_or("Game delivery needs its exact complete selected-port catalogue.")?;
    for port in &ports.ports {
        let native = admitted.contracts.get(&port.id)
            .ok_or("Game delivery contains a port absent from the current native catalogue.")?;
        if *native != port.contract_hash()? {
            return Err(
                "Game delivery port contract changed; reconcile its local assignment.".into(),
            );
        }
    }
    Ok(())
}
#[cfg(test)]
pub(super) fn validate_game_catalogues(batch:&ProviderBatch,metadata:&BTreeMap<String,Observation>)->Result<(),String>{
    let metadata = metadata.iter().map(|(key,item)| {
        GameMetadata::capture(item.clone(), "test-native-capture".into()).map(|item|(key.clone(),item))
            .ok_or("Game delivery requires valid current native metadata.")
    }).collect::<Result<BTreeMap<_,_>,_>>()?;
    validate_admitted_game_catalogues(batch,&metadata)
}
pub(super) fn validate_admitted_game_catalogues(batch:&ProviderBatch,metadata:&BTreeMap<String,Arc<GameMetadata>>)->Result<(),String>{
    batch.validate()?;
    let game=batch.scope.strip_prefix("profile-game:").filter(|game|matches!(*game,"dcs"|"assetto-corsa"|"assetto-corsa-competizione"|"assetto-corsa-evo"|"assetto-corsa-rally"|"x-plane-11"|"x-plane-12")).ok_or("Unknown native game delivery scope.")?;
    if !batch.complete || batch.observations.is_empty() || batch.observations.len()>if game=="dcs"{2}else{1} {
        return Err("Game delivery must contain one native catalogue, or DCS CommonData plus its single active aircraft catalogue.".into());
    }
    let mut keys=BTreeSet::new();
    let mut module_count=0;
    let first=&batch.observations[0];
    for item in &batch.observations {
        if !keys.insert(&item.key) || catalogue_game(&item.key)!=Some(game) {
            return Err("Game delivery contains duplicate or foreign catalogue identities.".into());
        }
        if item.key.starts_with("dcs:"){module_count+=1;}
        if module_count>1 || ["providerSetupRevision","gamePreparationSession","gameReaderSession"].iter().any(|field|item.attributes.get(*field)!=first.attributes.get(*field)) {
            return Err("Game delivery catalogues must belong to one acquired aircraft reader.".into());
        }
        validate_game_item(batch,item,metadata.get(&item.key).ok_or("The game has no current verified native module metadata.")?)?;
    }
    Ok(())
}
impl Runtime {
    pub(super) fn command_withdrawn(state: &State, token: &ResourceToken) -> bool {
        token.access == Access::Invoke && token.route_id.strip_prefix("profile-game:")
            .and_then(catalogue_game)
            .and_then(|game| state.game_command_withdrawals.get(&(game.into(), token.provider_session.clone())))
            .is_some_and(|ports| ports.contains(&token.port_id))
    }
    /// Revoke only exact command ports of the current reader. Read tokens and
    /// the independently advancing telemetry catalogue remain unchanged.
    pub(crate) fn withdraw_profile_game_commands(&self, game: &str, reader: &str, ports: &[String]) -> Result<(), String> {
        if self.stop.load(Ordering::Acquire) { return Err("Resource inventory has stopped.".into()); }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let batch = state.game_deliveries.get(game).filter(|batch| batch.session == reader)
            .ok_or("The exact command reader is no longer current; its replacement was preserved.")?;
        let known = batch.observations.iter().flat_map(|item| item.ports.iter().flat_map(|p| &p.ports))
            .filter(|p| p.direction == osg_profiles::Direction::Input && matches!(p.contract, osg_profiles::PortContract::Action { .. }))
            .map(|p| p.id.as_str()).collect::<BTreeSet<_>>();
        if ports.len() > 128 || ports.iter().any(|port| !known.contains(port.as_str())) {
            return Err("Command withdrawal needs exact current native command ports.".into());
        }
        state.game_command_withdrawals.entry((game.into(),reader.into())).or_default().extend(ports.iter().cloned());
        Ok(())
    }
    /// Publication is deduplicated by native reader state. This cheap owner check
    /// still observes metadata revocation on every health tick without cloning
    /// or rescanning the provider catalogue.
    pub(crate) fn validate_profile_game_delivery(&self, game: &str, reader: &str) -> Result<(), String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Resource inventory has stopped.".into());
        }
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.read_only {
            return Err("Repair retained inventory before accepting game delivery.".into());
        }
        if !state.game_deliveries.get(game).is_some_and(|batch| batch.session == reader) {
            return Err("The native game reader lost its current inventory scope or verified metadata.".into());
        }
        Ok(())
    }
    pub(crate) fn publish_profile_game_delivery(&self, batch: ProviderBatch) -> Result<(), String> {
        let _capture = self.capture_gate.lock().unwrap_or_else(|p| p.into_inner());
        if self.stop.load(Ordering::Acquire) {
            return Err("Resource inventory has stopped.".into());
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.read_only {
            return Err("Repair retained inventory before acquiring game delivery.".into());
        }
        let game = batch
            .scope
            .strip_prefix("profile-game:")
            .ok_or("Game delivery needs its dedicated native scope.")?
            .to_owned();
        let validation = validate_admitted_game_catalogues(&batch,&state.game_metadata);
        if let Err(error) = validation {
            if state
                .game_deliveries
                .get(&game)
                .is_some_and(|old| old.session == batch.session)
            {
                state.game_deliveries.remove(&game);
                state.registry.provider_failed(
                    &batch.scope,
                    "The acquired game reader lost validated sample metadata.",
                );
                state.pending_save = true;
            }
            return Err(error);
        }
        state.registry.reconcile(batch.clone())?;
        state.game_command_withdrawals.retain(|(id, reader), _| id != &game || reader == &batch.session);
        state.game_deliveries.insert(game, batch);
        state.pending_save = true;
        Ok(())
    }
    /// An outgoing run cannot revoke the incoming run's independently acquired reader.
    pub(crate) fn withdraw_profile_game_delivery(
        &self,
        game: &str,
        provider_session: &str,
    ) -> Result<(), String> {
        let _capture = self.capture_gate.lock().unwrap_or_else(|p| p.into_inner());
        if self.stop.load(Ordering::Acquire) {
            return Err("Resource inventory has stopped.".into());
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(old) = state
            .game_deliveries
            .get(game)
            .filter(|batch| batch.session == provider_session)
            .cloned()
        {
            state.game_deliveries.remove(game);
            state.game_command_withdrawals.remove(&(game.into(), provider_session.into()));
            state
                .registry
                .provider_failed(&old.scope, "The native profile game reader stopped.");
            state.pending_save = true;
        }
        Ok(())
    }
    /// Acquire only current read tokens. Command admission remains a later Start
    /// phase; pending command sinks are deliberately untouched.
    pub(crate) fn bind_delivery_reads(
        &self,
        prepared: &ExecutionPlan,
    ) -> Result<ExecutionPlan, String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Resource inventory has stopped.".into());
        }
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.read_only {
            return Err("Repair retained inventory before acquiring native delivery.".into());
        }
        let mut plan = prepared.clone();
        let mut validated_games = BTreeSet::new();
        for source in &mut plan.sources {
            if source.readiness != PlanReadiness::Ready {
                continue;
            }
            let pin = source.preparation.as_ref().ok_or(
                "A native source preparation pin is required before baseline acquisition.",
            )?;
            let expected = pin.resolved();
            if pin.access() != Access::Read
                || pin.inventory_session() != state.registry.session()
                || source.inventory_resource_id.as_deref() != Some(expected.resource_id.as_str())
                || source.observed_port_id.as_deref() != Some(expected.port.id.as_str())
            {
                return Err(
                    "The prepared source identity no longer matches this inventory capture.".into(),
                );
            }
            let token =
                state
                    .registry
                    .token(&expected.resource_id, &expected.port.id, Access::Read)?;
            let current = state
                .registry
                .validate_token(&token, &expected.port.id, Access::Read)?;
            if token.contract_hash != expected.port.contract_hash()? {
                return Err("The source contract changed before its baseline.".into());
            }
            if let Some(game) = expected.scope.strip_prefix("game:") {
                let metadata = state
                    .game_metadata
                    .get(game)
                    .ok_or("Game metadata was revoked before baseline acquisition.")?;
                let delivery = state
                    .game_deliveries
                    .get(catalogue_game(game).ok_or("Unknown prepared native game catalogue.")?)
                    .ok_or("Acquire an advancing native game sample before its baseline.")?;
                if validated_games.insert(delivery.scope.as_str()) {
                    validate_admitted_game_catalogues(delivery,&state.game_metadata)?;
                }
                if !delivery.observations.iter().any(|item|item.key==game) {return Err("The acquired reader does not publish the prepared aircraft catalogue.".into());}
                if current.scope != delivery.scope
                    || token.provider_session != delivery.session
                    || metadata.observation.route.as_ref() != Some(&expected.route)
                    || expected.key != game
                {
                    return Err("The native game reader does not match the prepared game setup and catalogue.".into());
                }
            } else if let Some((instance, publication, capability))=pin.companion_publication() {
                let summary=state.registry.inspect(&expected.resource_id,0,1)?.resource;
                // A paired installation keeps its authoring key when its first
                // assignment gains a real native transport. Only the marked
                // metadata route may make that transition; existing native
                // routes remain pinned to their prepared transport.
                let metadata=expected.route.transport==Transport::Integration
                    && expected.route.attributes.get("preparationOnly").map(String::as_str)==Some("paired-companion")
                    && summary.attributes.get("componentKind").map(String::as_str)==Some("paired-companion");
                if current.scope!=expected.scope || current.key!=expected.key
                    || current.route.id!=format!("companion:{instance}") || expected.route.id!=current.route.id
                    || !matches!(current.route.transport,Transport::Usb|Transport::Network)
                    || (!metadata && current.route.transport!=expected.route.transport)
                    || token.provider_session!=pin.provider_session()
                    || summary.attributes.get("publicationHash").map(String::as_str)!=Some(publication)
                    || summary.attributes.get("capabilityFingerprint").map(String::as_str)!=Some(capability) {
                    return Err("The actual companion baseline differs from the prepared paired instance and immutable publication.".into());
                }
            } else {
                let mut route = current.route.clone();
                route.baseline_ready = expected.route.baseline_ready;
                if current.scope != expected.scope
                    || current.key != expected.key
                    || route != expected.route
                    || token.capability_revision != pin.capability_revision()
                    || token.provider_session != pin.provider_session()
                {
                    return Err(
                        "The hardware route or catalogue changed during source acquisition.".into(),
                    );
                }
            }
            source.token = Some(token);
            source.route = Some(current.route);
        }
        Ok(plan)
    }

    /// A sealed native publication actor may advance only the CAN delivery lease
    /// and event stream of its exact settled targets. Generic hardware binding
    /// above remains strict and cannot use this permission.
    pub(crate) fn validate_publication_rebind(
        &self,
        before: &ExecutionPlan,
        after: &ExecutionPlan,
        proof: &crate::profile_session_overrides::VerifiedPublication,
    ) -> Result<(), String> {
        use osg_profiles::resources::{PortPreparation, Route, Transport};
        proof.validate()?;
        let pins = |physical: Option<&str>,
                    old: Option<&PortPreparation>,
                    new: Option<&PortPreparation>,
                    a: Option<&Route>,
                    b: Option<&Route>|
         -> Result<(), String> {
            // Outgoing and incoming profiles may name the same physical module
            // differently. Their own endpoints remain equal across recompiles;
            // only this exact sealed inventory/UID/route can inherit restoration.
            let target = proof
                .targets()
                .iter()
                .find(|t| Some(t.inventory.as_str()) == physical);
            if let (Some(old), Some(new)) = (old, new) {
                let x = old.resolved();
                let y = new.resolved();
                if old.inventory_session() != new.inventory_session()
                    || old.provider_session() != new.provider_session()
                    || old.access() != new.access()
                    || x.resource_id != y.resource_id
                    || Some(x.resource_id.as_str()) != physical
                    || x.scope != y.scope
                    || x.key != y.key
                    || x.port != y.port
                    || a != Some(&x.route)
                    || b != Some(&y.route)
                {
                    return Err(
                        "The native publication changed a prepared port identity or contract."
                            .into(),
                    );
                }
                if target.is_none() && old.capability_revision() != new.capability_revision() {
                    return Err(
                        "An unrelated resource catalogue changed during publication.".into(),
                    );
                }
            } else if old.is_some() != new.is_some() {
                return Err("A native preparation disappeared during publication.".into());
            }
            if a == b {
                return Ok(());
            }
            let (Some(target), Some(a), Some(b)) = (target, a, b) else {
                return Err("An unrelated native route changed during publication.".into());
            };
            if old.is_none()
                || new.is_none()
                || a.transport != Transport::Can
                || b.transport != Transport::Can
                || a.id != b.id
                || a.attributes != b.attributes
                || a.priority != b.priority
                || a.writable != b.writable
                || a.id != format!("can:{}:{}", target.gateway, target.module)
            {
                return Err("Publication cannot transfer a physical route or adopt another action catalogue.".into());
            }
            let x: Vec<Value> = serde_json::from_str(&a.provider_generation)
                .map_err(|_| "Invalid original CAN generation.")?;
            let y: Vec<Value> = serde_json::from_str(&b.provider_generation)
                .map_err(|_| "Invalid current CAN generation.")?;
            if x.len() != 5
                || y.len() != 5
                || x[0] != y[0]
                || x[1] != y[1]
                || x[3] != y[3]
                || y[0] != target.epoch
                || y[1] != target.boot
                || y[3] != target.configuration
                || x[2].as_u64().is_none_or(|old| {
                    old == 0
                        || y[2]
                            .as_u64()
                            .is_none_or(|new| new < old || new == old && x[4] != y[4])
                })
                || y[2].as_u64() != proof.delivery_lease(target)
            {
                return Err("Publication cannot adopt another gateway boot, module boot, configuration or unproved delivery lease.".into());
            }
            Ok(())
        };
        if before.sources.len() != after.sources.len() || before.sinks.len() != after.sinks.len() {
            return Err("The publication delivery graph changed.".into());
        }
        for (a, b) in before.sources.iter().zip(&after.sources) {
            if a.endpoint != b.endpoint
                || a.inventory_resource_id != b.inventory_resource_id
                || a.observed_port_id != b.observed_port_id
                || a.contract != b.contract
            {
                return Err("The publication input graph changed.".into());
            }
            pins(
                a.inventory_resource_id.as_deref(),
                a.preparation.as_ref(),
                b.preparation.as_ref(),
                a.route.as_ref(),
                b.route.as_ref(),
            )?;
        }
        for (a, b) in before.sinks.iter().zip(&after.sinks) {
            if a.endpoint != b.endpoint
                || a.adapter != b.adapter
                || a.inventory_resource_id != b.inventory_resource_id
                || a.observed_port_id != b.observed_port_id
                || a.contract != b.contract
            {
                return Err("The publication output graph changed.".into());
            }
            pins(
                a.inventory_resource_id.as_deref(),
                a.preparation.as_ref(),
                b.preparation.as_ref(),
                a.route.as_ref(),
                b.route.as_ref(),
            )?;
        }
        proof.validate()
    }

}

#[cfg(test)]
#[path = "profile_resources_game_tests.rs"]
mod tests;

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use osg_profiles::{resources::{EvidenceKind, IdentityEvidence}, resources_ports::{PortContract, PortDescriptor, PortUpdate, ScalarSchema}, Direction};
    use std::sync::mpsc;

    fn metadata(name: &str) -> Observation {
        let game = "assetto-corsa";
        Observation {
            key: game.into(), name: name.into(), resource_type: ResourceType::GameIntegration,
            evidence: vec![IdentityEvidence { kind: EvidenceKind::GameScope, value: game.into() }],
            attributes: BTreeMap::from([
                ("gameId".into(), game.into()), ("processVerified".into(), "true".into()),
                ("gameContextStatus".into(), "ready".into()), ("providerSetupRevision".into(), "setup".into()),
                ("catalogueRevision".into(), "catalogue".into()),
            ]), diagnostics: vec![],
            route: Some(Route { id: format!("game:{game}"), transport: Transport::Integration,
                provider_generation: "metadata".into(), attributes: BTreeMap::new(), baseline_ready: false, writable: false, priority: 0 }),
            ports: Some(PortUpdate::complete(vec![PortDescriptor {
                id: "channel:rpm".into(), name: "RPM".into(), direction: Direction::Output,
                contract: PortContract::State { schema: ScalarSchema::Integer { min: 0, max: 30000, unit: Some("rpm".into()) }, freshness_ms: 500, fallback: None },
            }]).unwrap()),
        }
    }
    fn adopt_metadata(runtime: &Runtime, revision: u64, name: &str) {
        Runtime::adopt(&runtime.state, vec![("game:assetto-corsa".into(), Ok(batch("game:assetto-corsa", "metadata", revision, vec![metadata(name)])))]);
    }
    fn delivery() -> ProviderBatch {
        let mut item = metadata("Current native reader");
        item.attributes.insert("gameReaderSession".into(), "reader".into());
        item.attributes.insert("gamePreparationSession".into(), "preparation".into());
        item.route = Some(Route { id: "profile-game:assetto-corsa".into(), transport: Transport::Integration,
            provider_generation: "reader:1".into(), attributes: item.attributes.clone(), baseline_ready: true, writable: false, priority: 100 });
        batch("profile-game:assetto-corsa", "reader", 1, vec![item])
    }

    #[test]
    fn blocked_persistence_never_blocks_native_delivery_or_cached_capture_and_retries_raced_changes() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(Runtime::new(Some(directory.path().into()), Arc::new(|_| panic!("No discovery during cached native work")), false));
        adopt_metadata(&runtime, 1, "Installed catalogue");
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let writer = runtime.clone();
        let writer = thread::spawn(move || Runtime::persist_with(&writer.state, &writer.persistence_gate, |store, saved| {
            entered_tx.send(()).unwrap(); release_rx.recv_timeout(Duration::from_secs(5)).unwrap(); store.save(saved)
        }));
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let (finished_tx, finished_rx) = mpsc::channel();
        let native = runtime.clone();
        let publisher = thread::spawn(move || {
            let result = (|| {
                native.publish_profile_game_delivery(delivery())?;
                native.validate_profile_game_delivery("assetto-corsa", "reader")?;
                native.snapshot_for_compile(vec![])?;
                native.withdraw_profile_game_delivery("assetto-corsa", "reader")?;
                Ok::<_, String>(())
            })();
            finished_tx.send(result).unwrap();
        });
        let completed = finished_rx.recv_timeout(Duration::from_secs(2));
        release_tx.send(()).unwrap();
        writer.join().unwrap(); publisher.join().unwrap();
        completed.expect("Native delivery and cached reads waited for metadata persistence").unwrap();
        assert!(runtime.state.lock().unwrap().pending_save, "A mutation after the writer snapshot must remain dirty");
        Runtime::persist(&runtime.state, &runtime.persistence_gate);
        let state = runtime.state.lock().unwrap();
        assert!(!state.pending_save);
        let restored = Registry::restore(Store::new(directory.path().into()).load().unwrap().unwrap()).unwrap();
        assert_eq!(restored.saved_fingerprint().unwrap(), state.registry.saved_fingerprint().unwrap());
    }

    #[test]
    fn queued_metadata_writers_snapshot_after_the_writer_gate_and_never_restore_older_catalogues() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(Runtime::new(Some(directory.path().into()), Arc::new(|_| vec![]), false));
        adopt_metadata(&runtime, 1, "Before repair");
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let first = runtime.clone();
        let first = thread::spawn(move || Runtime::persist_with(&first.state, &first.persistence_gate, |store, saved| {
            entered_tx.send(()).unwrap(); release_rx.recv_timeout(Duration::from_secs(5)).unwrap(); store.save(saved)
        }));
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        adopt_metadata(&runtime, 2, "After repair");
        let second = runtime.clone();
        let second = thread::spawn(move || Runtime::persist(&second.state, &second.persistence_gate));
        release_tx.send(()).unwrap();
        first.join().unwrap(); second.join().unwrap();
        assert!(!runtime.state.lock().unwrap().pending_save);
        let restored = Registry::restore(Store::new(directory.path().into()).load().unwrap().unwrap()).unwrap();
        assert_eq!(restored.list(0, 1).unwrap().resources[0].name, "After repair");
    }

    #[test]
    fn unchanged_catalogue_fingerprint_never_bypasses_an_invalid_provider_revision() {
        let runtime = Runtime::new(None, Arc::new(|_| vec![]), false);
        adopt_metadata(&runtime, 1, "Installed catalogue");
        runtime.publish_profile_game_delivery(delivery()).unwrap();
        adopt_metadata(&runtime, 0, "Installed catalogue");
        assert!(runtime.validate_profile_game_delivery("assetto-corsa", "reader").is_err());
    }

    #[test]
    fn large_catalogue_validation_is_admitted_once_and_repeated_binding_checks_only_selected_contracts() {
        use osg_profiles::{compiler_models::SourcePlan, resources::PortExpectation, store::LocalAssignment, Endpoint};
        let runtime = Runtime::new(None, Arc::new(|_| panic!("Binding cannot rediscover")), false);
        let mut item = metadata("Large native SDK");
        let rpm = item.ports.as_ref().unwrap().ports[0].clone();
        let mut ports = item.ports.take().unwrap().ports;
        for index in 0..32768 {
            let mut port = rpm.clone(); port.id = format!("channel:axis-{index}"); ports.push(port);
        }
        item.ports = Some(PortUpdate::complete(ports).unwrap());
        Runtime::adopt(&runtime.state, vec![("game:assetto-corsa".into(), Ok(batch("game:assetto-corsa", "metadata", 1, vec![item.clone()])))]);
        let (resource, admitted) = {
            let state = runtime.state.lock().unwrap();
            (state.registry.list(0, 1).unwrap().resources[0].id.clone(), state.game_metadata["assetto-corsa"].clone())
        };
        assert_eq!(admitted.contracts.len(), 32769);
        let (_, pins) = runtime.snapshot_for_preparation(vec![LocalAssignment {
            resource_id: "game".into(), source_id: resource.clone(), ports: vec![PortExpectation {
                profile_port_id: "rpm".into(), observed_port_id: rpm.id.clone(), expected_contract_hash: rpm.contract_hash().unwrap(),
            }],
        }]).unwrap();
        let pin = pins[0].clone();
        let prepared = ExecutionPlan {
            requirements: Default::default(), program: osg_mapping::program::Program { sources: vec![], nodes: vec![], sinks: vec![], controllers: vec![] },
            topological_order: vec![], trace: vec![], sources: (0..8).map(|index| SourcePlan {
                endpoint: Endpoint { node_id: format!("source-{index}"), port_id: "rpm".into() }, inventory_resource_id: Some(resource.clone()),
                observed_port_id: Some(rpm.id.clone()), contract: rpm.contract.clone(), token: None, route: Some(pin.resolved().route.clone()), preparation: Some(pin.clone()), readiness: PlanReadiness::Ready,
            }).collect(), sinks: vec![], branches: vec![], controllers: vec![], visibility: vec![], runtime_overrides: vec![], companion_assignments: vec![], budgets: Default::default(),
        };
        runtime.publish_profile_game_delivery(delivery()).unwrap();
        let started = std::time::Instant::now();
        for _ in 0..64 {
            let bound = runtime.bind_delivery_reads(&prepared).unwrap();
            assert!(bound.sources.iter().all(|source| source.token.as_ref().is_some_and(|token| token.provider_session == "reader")));
        }
        assert!(started.elapsed() < Duration::from_secs(3), "Repeated selected-port binding rescanned the complete SDK catalogue: {:?}", started.elapsed());
        assert!(Arc::ptr_eq(&admitted, &runtime.state.lock().unwrap().game_metadata["assetto-corsa"]));
        // A changed exact fingerprint must replace the proof even if a provider
        // incorrectly reuses its old human-facing setup/catalogue revision.
        if let PortContract::State { schema: ScalarSchema::Integer { max, .. }, .. } = &mut item.ports.as_mut().unwrap().ports[0].contract { *max = 20000; }
        item.ports = Some(PortUpdate::complete(item.ports.take().unwrap().ports).unwrap());
        Runtime::adopt(&runtime.state, vec![("game:assetto-corsa".into(), Ok(batch("game:assetto-corsa", "metadata", 2, vec![item])))]);
        assert!(!Arc::ptr_eq(&admitted, &runtime.state.lock().unwrap().game_metadata["assetto-corsa"]));
        assert!(runtime.bind_delivery_reads(&prepared).is_err());
        assert!(runtime.validate_profile_game_delivery("assetto-corsa", "reader").is_err());
    }
}
