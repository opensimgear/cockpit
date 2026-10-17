//! Native-only prospective panel metadata. A retained publication is a setup
//! capability, never evidence of a connected reader or an applied assignment.
use super::*;
use std::sync::atomic::Ordering;

#[derive(Clone)]
pub struct Preparation {
    instance_id: String,
    companion_id: String,
    publication_hash: String,
    capability_fingerprint: String,
    identity_fingerprint: String,
    definition: SourceDefinition,
    feedback: Vec<FeedbackDescriptor>,
    runtime_identity: String,
    revision: u64,
    current_identity: String,
}
impl std::fmt::Debug for Preparation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompanionPreparation")
            .field("instance_id", &self.instance_id)
            .field("publication_hash", &self.publication_hash)
            .field("identity_fingerprint", &self.identity_fingerprint)
            .finish_non_exhaustive()
    }
}
impl Preparation {
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }
    pub fn companion_id(&self) -> &str {
        &self.companion_id
    }
    pub fn publication_hash(&self) -> &str {
        &self.publication_hash
    }
    pub fn capability_fingerprint(&self) -> &str {
        &self.capability_fingerprint
    }
    pub fn identity_fingerprint(&self) -> &str {
        &self.identity_fingerprint
    }
    pub fn definition(&self) -> SourceDefinition {
        self.definition.clone()
    }
    pub fn feedback(&self) -> &[FeedbackDescriptor] {
        &self.feedback
    }
}

fn current_identity(inner: &Inner, instance: &str) -> Result<(String, String), String> {
    let assignment = inner
        .record
        .instances
        .iter()
        .find(|a| a.instance_id == instance)
        .ok_or("Assign this local instance to a paired companion before preparing a profile.")?;
    let credential = inner
        .record
        .credentials
        .iter()
        .find(|c| c.companion_id == assignment.companion_id)
        .ok_or("This companion installation is no longer authorized.")?;
    let bytes = serde_json::to_vec(&(assignment, &credential.token_hash))
        .map_err(|error| error.to_string())?;
    Ok((
        assignment.companion_id.clone(),
        format!("sha256:{}", hash(&bytes)),
    ))
}
impl Hub {
    pub fn prepare_profile_assignment(
        &self,
        instance: &str,
        desired: &str,
    ) -> Result<Preparation, String> {
        ensure(
            id(instance) && digest_id(desired),
            "Choose a valid native companion instance and immutable panel revision.",
        )?;
        let inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let (companion_id, current_identity) = current_identity(&inner, instance)?;
        let publication = inner.publications.get(desired)
            .ok_or("The desired immutable companion panel is not retained; publish it before applying this profile.")?;
        let revision = self.record_revision.load(Ordering::Acquire);
        let identity_fingerprint = format!(
            "sha256:{}",
            hash(
                &serde_json::to_vec(&(
                    self.runtime_identity.as_str(),
                    revision,
                    instance,
                    &companion_id,
                    &current_identity,
                    desired,
                    &publication.capability_fingerprint,
                ))
                .map_err(|error| error.to_string())?
            )
        );
        Ok(Preparation {
            instance_id: instance.into(),
            companion_id,
            publication_hash: desired.into(),
            capability_fingerprint: publication.capability_fingerprint.clone(),
            identity_fingerprint,
            definition: publication.definition(instance),
            feedback: publication.feedback.clone(),
            runtime_identity: self.runtime_identity.as_ref().clone(),
            revision,
            current_identity,
        })
    }
    pub fn validate_profile_preparation(&self, proof: &Preparation) -> Result<(), String> {
        let inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let (_, identity) = current_identity(&inner, &proof.instance_id)?;
        ensure(
            self.runtime_identity.as_str() == proof.runtime_identity
                && self.record_revision.load(Ordering::Acquire) == proof.revision
                && identity == proof.current_identity,
            "The paired companion or current assignment changed after profile preparation.",
        )?;
        let publication = inner
            .publications
            .get(&proof.publication_hash)
            .ok_or("The prepared immutable panel is no longer retained.")?;
        ensure(
            publication.capability_fingerprint == proof.capability_fingerprint
                && publication.definition(&proof.instance_id) == proof.definition,
            "The prepared companion capability or input definition changed.",
        )
    }
}
