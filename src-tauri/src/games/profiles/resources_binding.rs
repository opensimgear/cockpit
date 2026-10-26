impl Runtime {
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
                    validate_admitted_game_catalogues(delivery, &state.game_metadata)?;
                }
                if !delivery.observations.iter().any(|item| item.key == game) {
                    return Err(
                        "The acquired reader does not publish the prepared aircraft catalogue."
                            .into(),
                    );
                }
                if current.scope != delivery.scope
                    || token.provider_session != delivery.session
                    || metadata.observation.route.as_ref() != Some(&expected.route)
                    || expected.key != game
                {
                    return Err("The native game reader does not match the prepared game setup and catalogue.".into());
                }
            } else if let Some((instance, publication, capability)) = pin.companion_publication() {
                let summary = state
                    .registry
                    .inspect(&expected.resource_id, 0, 1)?
                    .resource;
                // A paired installation keeps its authoring key when its first
                // assignment gains a real native transport. Only the marked
                // metadata route may make that transition; existing native
                // routes remain pinned to their prepared transport.
                let metadata = expected.route.transport == Transport::Integration
                    && expected
                        .route
                        .attributes
                        .get("preparationOnly")
                        .map(String::as_str)
                        == Some("paired-companion")
                    && summary.attributes.get("componentKind").map(String::as_str)
                        == Some("paired-companion");
                if current.scope != expected.scope
                    || current.key != expected.key
                    || current.route.id != format!("companion:{instance}")
                    || expected.route.id != current.route.id
                    || !matches!(current.route.transport, Transport::Usb | Transport::Network)
                    || (!metadata && current.route.transport != expected.route.transport)
                    || token.provider_session != pin.provider_session()
                    || summary
                        .attributes
                        .get("publicationHash")
                        .map(String::as_str)
                        != Some(publication)
                    || summary
                        .attributes
                        .get("capabilityFingerprint")
                        .map(String::as_str)
                        != Some(capability)
                {
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
