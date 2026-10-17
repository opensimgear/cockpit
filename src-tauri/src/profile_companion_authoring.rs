//! Passive panel authoring metadata. Retained publications describe possible
//! ports; they never grant tokens, install a panel or start a transport.
use super::Runtime;
use osg_profiles::resources::ObservedPort;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_PANELS: usize = 32;
const MAX_PORTS: usize = 1024;
const MAX_REPLY: usize = 1_900_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PanelSummary {
    pub publication_hash: String,
    pub capability_fingerprint: String,
    pub title: String,
    pub control_count: usize,
    pub asset_count: usize,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Catalogue {
    pub schema_version: u32,
    pub revision: String,
    pub panels: Vec<PanelSummary>,
    pub diagnostics: Vec<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PanelPage {
    pub schema_version: u32,
    pub resource_id: String,
    pub publication_hash: String,
    pub capability_fingerprint: String,
    pub identity_fingerprint: String,
    pub offset: usize,
    pub total: usize,
    pub ports: Vec<ObservedPort>,
    pub diagnostics: Vec<String>,
}
fn digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|value| value.len()==64
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
}
impl Runtime {
    pub(crate) fn profile_authoring_catalogue(&self) -> Result<Catalogue, String> {
        let hub = self.profile_metadata()?;
        let raw = hub.publication_summaries();
        if raw.len() > MAX_PANELS { return Err("The retained panel library exceeds its 32-publication budget.".into()); }
        let mut panels = raw.into_iter().map(|value| serde_json::from_value::<PanelSummary>(value)
            .map_err(|_| "The retained native panel summary is malformed.".to_owned()))
            .collect::<Result<Vec<_>,_>>()?;
        for panel in &panels {
            if !digest(&panel.publication_hash) || !digest(&panel.capability_fingerprint)
                || panel.title.is_empty() || panel.title.len()>1024 || panel.title.chars().any(char::is_control)
                || !(1..=128).contains(&panel.control_count) || panel.asset_count>64
            { return Err("A retained panel has an invalid immutable identity or advertised control count.".into()); }
        }
        panels.sort_by(|a,b| a.publication_hash.cmp(&b.publication_hash));
        let bytes = serde_json::to_vec(&panels).map_err(|error|error.to_string())?;
        Ok(Catalogue {schema_version:1,revision:format!("sha256:{:x}",Sha256::digest(bytes)),panels,diagnostics:vec![]})
    }
    /// `instance_id` is resolved by Manager from cached native CompanionInstance
    /// evidence; it is never supplied as browser authority or resolved by name.
    pub(crate) fn profile_authoring_inspect(
        &self, resource_id:&str, instance_id:&str, publication_hash:&str, offset:usize, limit:usize,
    ) -> Result<PanelPage, String> {
        if resource_id.is_empty() || resource_id.len()>256 || resource_id.chars().any(char::is_control)
            || !digest(publication_hash) || !(1..=64).contains(&limit)
        { return Err("Choose an exact companion resource, retained publication and a page of 1–64 ports.".into()); }
        let hub = self.profile_metadata()?;
        let proof = hub.prepare_profile_assignment(instance_id,publication_hash)?;
        let definition = osg_mapping_host::companions::native_definition(proof.definition())?;
        if !(1..=128).contains(&definition.controls.len()) {
            return Err("A real companion panel must advertise 1–128 interactive controls.".into());
        }
        let mut descriptors = crate::profile_resource_ports::companion(&definition,proof.feedback())?;
        if descriptors.len()>MAX_PORTS || offset>descriptors.len() {
            return Err("The requested companion catalogue page exceeds its native bounds.".into());
        }
        descriptors.sort_by(|a,b| a.id.cmp(&b.id));
        let total=descriptors.len();
        let ports=descriptors.into_iter().skip(offset).take(limit).map(|descriptor| {
            let contract_hash=descriptor.contract_hash()?;
            Ok(ObservedPort {descriptor,contract_hash,route_ids:vec![],current:false})
        }).collect::<Result<Vec<_>,String>>()?;
        // A read can race explicit pairing/publication edits. Do not return a
        // mixed page or retain the proof as activation authority.
        hub.validate_profile_preparation(&proof)?;
        let mut page=PanelPage {schema_version:1,resource_id:resource_id.into(),publication_hash:proof.publication_hash().into(),
            capability_fingerprint:proof.capability_fingerprint().into(),identity_fingerprint:proof.identity_fingerprint().into(),
            offset,total,ports,diagnostics:vec![]};
        while serde_json::to_vec(&page).map_err(|error|error.to_string())?.len()>MAX_REPLY {
            if page.ports.len()<=1 {return Err("One companion port exceeds the bounded IPC reply budget.".into());}
            page.ports.pop();
        }
        Ok(page)
    }
}

#[cfg(test)]
#[path = "profile_companion_authoring_tests.rs"]
mod tests;
