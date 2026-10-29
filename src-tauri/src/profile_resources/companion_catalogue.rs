//! An absent paired screen may retain its verified desired contracts for
//! compilation without gaining a route, token, input sample or assignment.
use super::{CompanionCatalogue, Runtime};
use osg_profiles::{
    compiler_models::CompileSnapshot,
    resources::{Availability, ObservedPort, ResourceType},
};

impl Runtime {
    pub(crate) fn snapshot_for_compile_with_companions(
        &self,
        assignments: Vec<osg_profiles::store::LocalAssignment>,
        catalogues: &[CompanionCatalogue],
    ) -> Result<CompileSnapshot, String> {
        let mut snapshot = self.snapshot_for_compile(assignments)?;
        capture_offline(&mut snapshot, catalogues)?;
        Ok(snapshot)
    }
}

pub(super) fn capture_offline(
    snapshot: &mut CompileSnapshot,
    catalogues: &[CompanionCatalogue],
) -> Result<(), String> {
    for catalogue in catalogues {
        let Some(resource) = snapshot.resources.iter_mut().find(|resource| {
            resource.summary.id == catalogue.inventory_resource_id
                && resource.summary.resource_type == ResourceType::Companion
                && resource.summary.availability == Availability::Offline
        }) else {
            continue;
        };
        if resource
            .summary
            .attributes
            .get("companionId")
            .is_some_and(|identity| identity != &catalogue.companion_id)
            || catalogue
                .paired_identity_fingerprint
                .as_ref()
                .is_some_and(|expected| {
                    resource
                        .summary
                        .attributes
                        .get("componentKind")
                        .is_some_and(|kind| kind == "paired-companion")
                        && resource.summary.attributes.get("pairedIdentityFingerprint")
                            != Some(expected)
                })
        {
            return Err("The retained companion pairing changed; prepare a fresh profile before using its desired contracts.".into());
        }
        for descriptor in &catalogue.ports {
            let contract_hash = descriptor.contract_hash()?;
            if resource.ports.iter().any(|port| {
                port.descriptor.id == descriptor.id && port.contract_hash == contract_hash
            }) {
                continue;
            }
            resource.ports.push(ObservedPort {
                descriptor: descriptor.clone(),
                contract_hash,
                route_ids: vec![],
                current: false,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "companion_catalogue_tests.rs"]
mod tests;
