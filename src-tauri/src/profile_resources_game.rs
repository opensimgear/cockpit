//! A native acquired game reader publishes delivery separately from catalogue
//! metadata. No browser request can publish samples or acquire this authority.
use super::*;
use osg_profiles::{
    compiler_models::{ExecutionPlan, PlanReadiness},
    resources::{EvidenceKind, ResourceType, Transport},
};

pub(super) fn game_metadata_valid(item: &Observation, game: &str) -> bool {
    item.key == game
        && item.resource_type == ResourceType::GameIntegration
        && item.attributes.get("gameId").map(String::as_str) == Some(game)
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
            .any(|e| e.kind == EvidenceKind::GameScope && e.value == game)
        && item.route.as_ref().is_some_and(|r| {
            r.id == format!("game:{game}")
                && r.transport == Transport::Integration
                && !r.baseline_ready
                && !r.writable
        })
        && item
            .ports
            .as_ref()
            .is_some_and(|p| p.complete && p.validate().is_ok())
}

pub(super) fn validate_game_delivery(
    batch: &ProviderBatch,
    metadata: &Observation,
) -> Result<(), String> {
    batch.validate()?;
    let game = metadata.key.as_str();
    if !game_metadata_valid(metadata, game)
        || batch.scope != format!("profile-game:{game}")
        || !batch.complete
        || batch.observations.len() != 1
    {
        return Err("Fresh game delivery requires one complete native game observation and current verified metadata.".into());
    }
    let item = &batch.observations[0];
    if item.key != game
        || item.resource_type != ResourceType::GameIntegration
        || item.evidence != metadata.evidence
        || item.attributes.get("gameId") != metadata.attributes.get("gameId")
        || item.attributes.get("gameReaderSession") != Some(&batch.session)
        || !item
            .attributes
            .get("gamePreparationSession")
            .is_some_and(|proof| osg_profiles::resources_ports::text(proof, 256).is_ok())
        || ["providerSetupRevision", "catalogueRevision"]
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
    if route.id != batch.scope
        || route.transport != Transport::Integration
        || route.priority != 100
        || (route.writable && !route.baseline_ready)
        || [
            "gameId",
            "providerSetupRevision",
            "catalogueRevision",
            "gamePreparationSession",
            "gameReaderSession",
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
    let current = &metadata.ports.as_ref().unwrap().ports;
    for port in &ports.ports {
        let native = current
            .iter()
            .find(|p| p.id == port.id)
            .ok_or("Game delivery contains a port absent from the current native catalogue.")?;
        if native.contract_hash()? != port.contract_hash()? {
            return Err(
                "Game delivery port contract changed; reconcile its local assignment.".into(),
            );
        }
    }
    Ok(())
}
impl Runtime {
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
        let validation = state
            .game_metadata
            .get(&game)
            .ok_or_else(|| "The game has no current verified native metadata.".to_owned())
            .and_then(|metadata| validate_game_delivery(&batch, metadata));
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
                Self::persist_locked(&mut state);
            }
            return Err(error);
        }
        state.registry.reconcile(batch.clone())?;
        state.game_deliveries.insert(game, batch);
        Self::persist_locked(&mut state);
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
            state
                .registry
                .provider_failed(&old.scope, "The native profile game reader stopped.");
            Self::persist_locked(&mut state);
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
                    .get(game)
                    .ok_or("Acquire an advancing native game sample before its baseline.")?;
                validate_game_delivery(delivery, metadata)?;
                if current.scope != delivery.scope
                    || token.provider_session != delivery.session
                    || metadata.route.as_ref() != Some(&expected.route)
                    || expected.key != game
                {
                    return Err("The native game reader does not match the prepared game setup and catalogue.".into());
                }
            } else if let Some((instance, publication, capability))=pin.companion_publication() {
                let summary=state.registry.inspect(&expected.resource_id,0,1)?.resource;
                if current.scope!=expected.scope || current.key!=instance || expected.key!=instance
                    || current.route.id!=format!("companion:{instance}") || current.route.transport!=expected.route.transport
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
