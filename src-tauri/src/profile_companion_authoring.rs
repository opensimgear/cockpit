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
    pub panel_id: String,
    pub publication_hash: String,
    pub publication_hashes: Vec<String>,
    pub capability_fingerprint: String,
    pub title: String,
    pub control_count: usize,
    pub asset_count: usize,
    #[serde(default)]
    pub compatibility: Vec<PanelCompatibility>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PanelCompatibility {
    pub companion_id: String,
    pub compatible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
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
    fn profile_panel_compatibility(&self,hub:&osg_companion_service::Hub,publication:&str)->Result<Vec<PanelCompatibility>,String>{
        use std::io::Read;
        let metadata=self.metadata(false)?;
        let mut bytes=vec![];
        if let Ok(file)=std::fs::File::open(metadata.directory.join("companion-firmware.json")){
            let _=file.take(65537).read_to_end(&mut bytes);
        }
        let firmware=if bytes.len()<=65536 {serde_json::from_slice::<serde_json::Value>(&bytes).ok().filter(serde_json::Value::is_object)}else{None}.unwrap_or(serde_json::json!({}));
        let devices=self.services.get().and_then(|services|services.as_ref().ok()).map(|services|services.usb.statuses()).unwrap_or_default();
        let document=hub.publication_document(publication)?;
        Ok(hub.resource_companions().into_iter().map(|companion|{
            let reason=if companion.kind=="esp32"{
                let current=devices.iter().find(|usb|usb["connected"]==true&&usb["device"]["companionId"]==companion.companion_id);
                let profile=current.and_then(|usb|usb["device"]["boardProfile"].as_str()).or_else(||firmware[&companion.companion_id]["profile"].as_str());
                match profile.and_then(super::screen_dimensions){
                    Some(size)=>super::validate_screen_document(&document,size).err(),
                    None=>Some("Connect this ESP32 screen by USB to identify its screen size before assigning a panel.".into()),
                }
            }else{None};
            PanelCompatibility {companion_id:companion.companion_id,compatible:reason.is_none(),reason}
        }).collect())
    }
    pub(crate) fn profile_authoring_catalogue(&self) -> Result<Catalogue, String> {
        let hub = self.profile_metadata()?;
        let updates=self.saved_panel_updates()?;
        let raw = hub.publication_summaries().into_iter().filter_map(|mut value|{
            let saved=updates.iter().find(|saved|value["publicationHash"]==saved.publication_hash)?;
            value.as_object_mut()?.remove("uiId");
            value["panelId"]=serde_json::json!(saved.panel_id);
            value["publicationHashes"]=serde_json::json!(saved.previous_hashes);
            Some(value)
        }).collect::<Vec<_>>();
        if raw.len() > MAX_PANELS { return Err("The retained panel library exceeds its 32-publication budget.".into()); }
        let mut panels = raw.into_iter().map(|value| serde_json::from_value::<PanelSummary>(value)
            .map_err(|_| "The retained native panel summary is malformed.".to_owned()))
            .collect::<Result<Vec<_>,_>>()?;
        for panel in &panels {
            if !digest(&panel.publication_hash) || !digest(&panel.capability_fingerprint)
                || panel.title.is_empty() || panel.title.len()>1024 || panel.title.chars().any(char::is_control)
                || panel.control_count>128 || panel.asset_count>64
            { return Err("A retained panel has an invalid immutable identity or advertised control count.".into()); }
        }
        for panel in &mut panels {panel.compatibility=self.profile_panel_compatibility(&hub,&panel.publication_hash)?;}
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
        if let Some(reason)=self.profile_panel_compatibility(&hub,publication_hash)?.into_iter()
            .find(|row|row.companion_id==proof.companion_id()).and_then(|row|row.reason){return Err(reason);}
        let definition = osg_mapping_host::companions::native_definition(proof.definition())?;
        if definition.controls.len()>128 {
            return Err("A companion panel may advertise at most 128 interactive controls.".into());
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
