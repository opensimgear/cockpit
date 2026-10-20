//! Prospective companion contracts are native preparation data. They never
//! replace observed inventory or grant delivery before an actual assignment.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct CompanionPortPin {
    pub instance_id: String,
    pub publication_hash: String,
    pub capability_fingerprint: String,
}
fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    })
}
impl Registry {
    /// The caller must first verify this descriptor against the retained Hub
    /// publication and paired-instance proof. Browser input cannot create a pin.
    pub fn prepare_companion_port(
        &self,
        id: &str,
        instance_id: &str,
        publication_hash: &str,
        capability_fingerprint: &str,
        descriptor: PortDescriptor,
        access: Access,
    ) -> Result<PortPreparation, String> {
        text(instance_id, 256)?;
        if !valid_digest(publication_hash) || !valid_digest(capability_fingerprint) {
            return Err("Companion preparation requires exact immutable publication and capability digests.".into());
        }
        descriptor.validate()?;
        let allowed = match access {
            Access::Read => {
                descriptor.direction == crate::Direction::Output
                    && !matches!(
                        descriptor.contract,
                        PortContract::Action { .. } | PortContract::Invocation { .. }
                    )
            }
            Access::Write => {
                descriptor.direction == crate::Direction::Input
                    && matches!(descriptor.contract, PortContract::State { .. })
            }
            Access::Invoke => false,
        };
        if !allowed {
            return Err(
                "The publication does not advertise this companion input or feedback contract."
                    .into(),
            );
        }
        let entry = self.entry(id)?;
        if entry.saved.resource_type != ResourceType::Companion {
            return Err("Only a proven companion instance can prepare a publication.".into());
        }
        let mut routes = entry.saved.sightings.iter().filter_map(|sighting| {
            if !sighting
                .evidence
                .iter()
                .any(|e| e.kind == EvidenceKind::CompanionInstance && e.value == instance_id)
            {
                return None;
            }
            let live = entry
                .live
                .get(&(sighting.scope.clone(), sighting.key.clone()))?;
            (live.route.id == format!("companion:{instance_id}")
                && (matches!(live.route.transport, Transport::Usb | Transport::Network)
                    || (live.route.transport==Transport::Integration && sighting.attributes.get("componentKind").is_some_and(|kind|kind=="paired-companion")
                        && live.route.attributes.get("preparationOnly").is_some_and(|kind|kind=="paired-companion"))))
            .then_some((sighting, live))
        });
        let (sighting, live) = routes
            .next()
            .ok_or("The paired companion has no current verified native route.")?;
        if routes.next().is_some() {
            return Err("The paired companion has ambiguous simultaneous native routes.".into());
        }
        let contract_hash = descriptor.contract_hash()?;
        let mut route = live.route.clone();
        // Even an already active outgoing publication cannot supply the incoming
        // run's baseline or feedback owner.
        route.baseline_ready = false;
        route.writable = false;
        Ok(PortPreparation {
            pin: PreparationPin {
                inventory_session: self.session.clone(),
                resource_id: entry.saved.id.clone(),
                generation: entry.generation.to_string(),
                capability_revision: capability_revision(entry)?,
                provider_session: live.provider_session.clone(),
                scope: sighting.scope.clone(),
                key: sighting.key.clone(),
                route: live.route.clone(),
                port_id: descriptor.id.clone(),
                contract_hash,
                access,
                selection_scope: None,
                selection_catalogue_revision: live.catalogue_revision.clone(),
                companion: Some(CompanionPortPin {
                    instance_id: instance_id.into(),
                    publication_hash: publication_hash.into(),
                    capability_fingerprint: capability_fingerprint.into(),
                }),
            },
            resolved: ResolvedRoute {
                resource_id: entry.saved.id.clone(),
                scope: sighting.scope.clone(),
                key: sighting.key.clone(),
                route,
                port: descriptor,
            },
        })
    }
}
impl PortPreparation {
    /// Desired immutable publication identity; this is not current delivery.
    pub fn companion_publication(&self) -> Option<(&str, &str, &str)> {
        self.pin.companion.as_ref().map(|p| {
            (
                p.instance_id.as_str(),
                p.publication_hash.as_str(),
                p.capability_fingerprint.as_str(),
            )
        })
    }
    pub(super) fn companion_matches_capture(
        &self,
        session: &str,
        resource: &ResourceSummary,
    ) -> bool {
        self.pin.companion.is_some()
            && self.pin.inventory_session == session
            && self.pin.resource_id == resource.id
            && resource.resource_type == ResourceType::Companion
            && self.pin.generation == resource.generation
            && self.pin.capability_revision == resource.capability_revision
            && resource.connections.iter().any(|c| {
                c.scope == self.pin.scope && c.key == self.pin.key && c.route == self.pin.route
            })
    }
    pub(crate) fn prospective_companion_port(
        &self,
        session: &str,
        resource: &ResourceSummary,
        publication: Option<&str>,
    ) -> Option<ObservedPort> {
        let (_, desired, _) = self.companion_publication()?;
        (Some(desired) == publication && self.companion_matches_capture(session, resource)).then(
            || ObservedPort {
                descriptor: self.resolved.port.clone(),
                contract_hash: self.pin.contract_hash.clone(),
                route_ids: vec![self.resolved.route.id.clone()],
                current: false,
            },
        )
    }
}

#[cfg(test)]
#[path = "resources_companion_tests.rs"]
mod tests;
