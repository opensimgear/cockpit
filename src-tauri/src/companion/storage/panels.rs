//! Host content is already restored atomically before transports can start.
use osg_companion_service::Hub;
use std::collections::BTreeMap;
pub(crate) use osg_companion_service::SavedPanel;
pub(crate) fn saved_heads(hub:&Hub)->Result<BTreeMap<String,String>,String>{Ok(hub.saved_heads())}
pub(crate) fn latest(hub:&Hub)->Result<Vec<SavedPanel>,String>{Ok(hub.saved_panels())}
pub(super) fn ready(hub:&Hub)->Result<bool,String>{
    let state=hub.command(&serde_json::json!({"command":"companion_status"}))?;
    Ok(state["instances"].as_array().is_none_or(|instances|instances.iter().all(|instance|
        state["publications"].as_array().is_some_and(|panels|panels.iter().any(|panel|panel["publicationHash"]==instance["publicationHash"])))))
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    #[test]
    fn retained_archives_establish_a_restoration_head_only_after_explicit_save(){
        let hub=Hub::new("head-test".into(),std::sync::Arc::new(osg_companion_service::NullSourceSink));
        let mut upload:Value=serde_json::from_str(include_str!("../../../../crates/companion-service/tests/fixtures/compiled-toggle-publication.json")).unwrap();
        upload["command"]=serde_json::json!("companion_publish");hub.command(&upload).unwrap();assert!(ready(&hub).unwrap());
        assert_eq!(latest(&hub).unwrap().len(),1);assert!(saved_heads(&hub).unwrap().is_empty());
        let saved=hub.mark_saved(upload["publicationHash"].as_str().unwrap()).unwrap();assert_eq!(saved_heads(&hub).unwrap()[&saved.panel_id],saved.publication_hash);
        assert!(hub.mark_saved("missing").is_err());
    }
}
