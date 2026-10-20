//! Validated native panel cache, restored before any companion transports start.
use osg_companion_service::{Hub, Publication, PublicationUpload};
use serde_json::{json,Value};
use std::{fs,io::Read,path::Path};
use std::collections::BTreeMap;
#[derive(Clone,Debug,serde::Serialize,serde::Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct SavedPanel {
    pub panel_id:String,
    pub publication_hash:String,
    pub previous_hashes:Vec<String>,
}
fn panel_id(value:&str)->bool {
    !value.is_empty()&&value.len()<=64&&value.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-')
        &&value.as_bytes()[0].is_ascii_alphanumeric()
}
fn digest(value:&str)->bool {value.strip_prefix("sha256:").is_some_and(|s|s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)))}
fn read_heads(directory:&Path)->Result<BTreeMap<String,String>,String>{
    let path=directory.join("companion-panel-heads.json");
    let file=match fs::File::open(&path){
        Ok(file)=>file,
        Err(error) if error.kind()==std::io::ErrorKind::NotFound=>return Ok(BTreeMap::new()),
        Err(error)=>return Err(format!("Cannot read saved panel identities: {error}")),
    };
    let mut bytes=Vec::new();
    file.take(65537).read_to_end(&mut bytes).map_err(|error|format!("Cannot read saved panel identities: {error}"))?;
    if bytes.len()>65536{return Err("Saved panel identities exceed their storage budget.".into());}
    let heads:BTreeMap<String,String>=serde_json::from_slice(&bytes).map_err(|_|"Saved panel identities are invalid.".to_owned())?;
    if heads.len()>32||heads.iter().any(|(id,hash)|!panel_id(id)||!digest(hash)){return Err("Saved panel identities are invalid.".into());}
    Ok(heads)
}
/// Only explicit successful Save operations establish a current view. Retained
/// immutable archives must never silently become restoration targets.
pub(crate) fn saved_heads(hub:&Hub,directory:&Path)->Result<BTreeMap<String,String>,String>{
    let retained=hub.publication_summaries();
    Ok(read_heads(directory)?.into_iter().filter(|(id,hash)|retained.iter().any(|panel|
        panel["uiId"]==*id&&panel["publicationHash"]==*hash)).collect())
}
pub(crate) fn latest(hub:&Hub,directory:&Path)->Result<Vec<SavedPanel>,String>{
    let mut heads=saved_heads(hub,directory)?;
    let mut grouped=BTreeMap::<String,Vec<String>>::new();
    for panel in hub.publication_summaries(){
        let id=panel["uiId"].as_str().ok_or("A retained panel has no stable identity.")?;
        let hash=panel["publicationHash"].as_str().ok_or("A retained panel has no immutable content identity.")?;
        grouped.entry(id.into()).or_default().push(hash.into());
    }
    let mut result=vec![];
    for (panel_id,previous_hashes) in grouped {
        let publication_hash=heads.remove(&panel_id).filter(|hash|previous_hashes.contains(hash))
            .unwrap_or_else(||previous_hashes.last().unwrap().clone());
        result.push(SavedPanel {panel_id,publication_hash,previous_hashes});
    }
    Ok(result)
}
pub(crate) fn mark_saved(hub:&Hub,directory:&Path,hash:&str)->Result<SavedPanel,String>{
    let summary=hub.publication_summaries().into_iter().find(|panel|panel["publicationHash"]==hash).ok_or("The saved panel was not published.")?;
    let panel_id=summary["uiId"].as_str().ok_or("The saved panel has no stable identity.")?.to_owned();
    let mut updates=latest(hub,directory)?;
    let updated=updates.iter_mut().find(|panel|panel.panel_id==panel_id).ok_or("The saved panel disappeared.")?;
    updated.publication_hash=hash.into();let saved=updated.clone();
    let mut heads=saved_heads(hub,directory)?;
    heads.insert(saved.panel_id.clone(),saved.publication_hash.clone());
    fs::create_dir_all(directory).map_err(|e|e.to_string())?;
    let temporary=directory.join("companion-panel-heads.json.tmp");
    fs::write(&temporary,serde_json::to_vec(&heads).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    fs::rename(temporary,directory.join("companion-panel-heads.json")).map_err(|e|e.to_string())?;
    Ok(saved)
}
const LIMIT:usize=80*1024*1024;
pub(super) fn ready(hub:&Hub)->Result<bool,String>{
    let state=hub.command(&json!({"command":"companion_status"}))?;
    Ok(state_ready(&state))
}
fn state_ready(state:&Value)->bool{
    state["instances"].as_array().is_none_or(|rows|rows.iter().all(|a|
        state["publications"].as_array().is_some_and(|panels|panels.iter().any(|p|p["publicationHash"]==a["publicationHash"]))))
}
pub(super) fn save(hub:&Hub,directory:&Path)->Result<(),String>{
    let bytes=serde_json::to_vec(&hub.publication_uploads()).map_err(|e|e.to_string())?;
    if bytes.len()>LIMIT{return Err("Saved panels exceed the native cache budget.".into());}
    fs::create_dir_all(directory).map_err(|e|e.to_string())?;
    let temporary=directory.join("companion-panels.json.tmp");
    fs::write(&temporary,bytes).map_err(|e|e.to_string())?;
    fs::rename(temporary,directory.join("companion-panels.json")).map_err(|e|e.to_string())
}
pub(super) fn restore(hub:&Hub,directory:&Path)->Result<(),String>{
    let path=directory.join("companion-panels.json");
    let file=match fs::File::open(path){Ok(file)=>file,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(()),Err(e)=>return Err(e.to_string())};
    let mut bytes=Vec::new();file.take((LIMIT+1) as u64).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
    if bytes.len()>LIMIT{return Err("Saved panels exceed the native cache budget.".into());}
    let uploads:Vec<PublicationUpload>=serde_json::from_slice(&bytes).map_err(|_|"Saved companion panels are invalid.")?;
    if uploads.len()>32{return Err("Too many saved companion panels.".into());}
    // Validate every record before making any of them available.
    let validated=uploads.into_iter().map(Publication::validate).collect::<Result<Vec<_>,_>>()?;
    for panel in validated {
        let mut value:Value=serde_json::to_value(&panel).map_err(|e|e.to_string())?;
        value["command"]=json!("companion_publish");
        hub.command(&value)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[test]
    fn saved_panel_heads_reuse_stable_identity_and_can_return_to_an_existing_snapshot(){
        let directory=crate::test_directory();
        let hub=Hub::new("saved-panel-heads".into(),Arc::new(osg_companion_service::NullSourceSink));
        let mut a:PublicationUpload=serde_json::from_str(include_str!("../../crates/companion-service/tests/fixtures/c1-flight-publication.json")).unwrap();
        let publish=|upload:&PublicationUpload|{let mut command=serde_json::to_value(upload).unwrap();command["command"]=json!("companion_publish");hub.command(&command).unwrap();};
        publish(&a);mark_saved(&hub,directory.path(),&a.publication_hash).unwrap();
        let first=a.publication_hash.clone();
        a.canonical_document=a.canonical_document.replace("Flight instruments","Latest flight instruments");
        use sha2::{Digest,Sha256};
        a.publication_hash=format!("sha256:{:x}",Sha256::digest(a.canonical_document.as_bytes()));a.capability_fingerprint=None;
        publish(&a);mark_saved(&hub,directory.path(),&a.publication_hash).unwrap();
        let current=latest(&hub,directory.path()).unwrap();
        assert_eq!(current.len(),1);assert_eq!(current[0].panel_id,"flight-panel");
        assert_eq!(current[0].publication_hash,a.publication_hash);assert_eq!(current[0].previous_hashes.len(),2);
        assert_eq!(hub.publication_summaries().len(),2,"Transport snapshots remain immutable and retained");
        mark_saved(&hub,directory.path(),&first).unwrap();
        assert_eq!(latest(&hub,directory.path()).unwrap()[0].publication_hash,first,"Saving a prior identical view makes it current again");
        assert!(mark_saved(&hub,directory.path(),&format!("sha256:{}","0".repeat(64))).is_err());
    }
    #[test]
    fn archived_snapshots_do_not_establish_saved_restoration_heads(){
        use sha2::{Digest,Sha256};
        let directory=crate::test_directory();
        let hub=Hub::new("explicit-panel-heads".into(),Arc::new(osg_companion_service::NullSourceSink));
        let original:PublicationUpload=serde_json::from_str(include_str!("../../crates/companion-service/tests/fixtures/c1-flight-publication.json")).unwrap();
        let mut other=original.clone();
        other.canonical_document=other.canonical_document.replace("flight-panel","other-panel");
        other.publication_hash=format!("sha256:{:x}",Sha256::digest(other.canonical_document.as_bytes()));other.capability_fingerprint=None;
        for upload in [&original,&other] {
            let mut command=serde_json::to_value(upload).unwrap();command["command"]=json!("companion_publish");hub.command(&command).unwrap();
        }
        assert_eq!(latest(&hub,directory.path()).unwrap().len(),2,"Legacy retained panels remain available to the catalogue");
        assert!(saved_heads(&hub,directory.path()).unwrap().is_empty());
        mark_saved(&hub,directory.path(),&original.publication_hash).unwrap();
        assert_eq!(saved_heads(&hub,directory.path()).unwrap(),BTreeMap::from([("flight-panel".into(),original.publication_hash)]));
    }
    #[test]
    fn saved_panel_heads_reject_oversized_storage_before_decoding(){
        let directory=crate::test_directory();
        fs::write(directory.path().join("companion-panel-heads.json"),vec![b' ';65537]).unwrap();
        assert_eq!(read_heads(directory.path()).unwrap_err(),"Saved panel identities exceed their storage budget.");
    }
    #[test]
    fn reconnect_waits_for_every_assigned_panel(){
        assert!(state_ready(&json!({"instances":[]})));
        let mut state=json!({"instances":[{"publicationHash":"a"},{"publicationHash":"b"}],"publications":[{"publicationHash":"a"}]});
        assert!(!state_ready(&state));
        state["publications"].as_array_mut().unwrap().push(json!({"publicationHash":"b"}));
        assert!(state_ready(&state));
    }
    #[test]
    fn a_saved_assignment_is_ready_before_transport_start_after_restore(){
        #[derive(Default)]
        struct Memory(std::sync::Mutex<Option<Vec<u8>>>);
        impl osg_companion_service::persistence::StateStorage for Memory{
            fn read(&self)->Result<Option<Vec<u8>>,String>{Ok(self.0.lock().unwrap().clone())}
            fn write(&self,bytes:&[u8])->Result<(),String>{*self.0.lock().unwrap()=Some(bytes.to_vec());Ok(())}
        }
        let directory=std::env::temp_dir().join(format!("osg-assigned-cache-{}",uuid::Uuid::new_v4()));
        let memory=Arc::new(Memory::default());
        let first=Hub::with_storage(memory.clone(),Arc::new(osg_companion_service::NullSourceSink)).unwrap();
        let offer=first.command(&json!({"command":"companion_pair_offer"})).unwrap();
        let pending=first.handle_wire(json!({"type":"pair","version":1,"secret":offer["secret"],"companionId":"screen","name":"Screen"}),"peer",first.now_ms());
        first.command(&json!({"command":"companion_approve","requestId":pending["requestId"],"companionId":"screen"})).unwrap();
        let mut upload:Value=serde_json::from_str(include_str!("../../crates/companion-service/tests/fixtures/c1-flight-publication.json")).unwrap();
        upload["command"]=json!("companion_publish");first.command(&upload).unwrap();
        first.command(&json!({"command":"companion_assign","companionId":"screen","instanceId":"assigned","publicationHash":upload["publicationHash"]})).unwrap();
        save(&first,&directory).unwrap();
        let restored=Hub::with_storage(memory,Arc::new(osg_companion_service::NullSourceSink)).unwrap();
        assert!(!ready(&restored).unwrap());
        restore(&restored,&directory).unwrap();
        assert!(ready(&restored).unwrap());
        assert_eq!(restored.assigned_publication("assigned").unwrap().publication_hash,upload["publicationHash"]);
        fs::remove_file(directory.join("companion-panels.json")).unwrap();
        fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn panels_restore_without_a_frontend_and_invalid_cache_is_rejected(){
        let directory=std::env::temp_dir().join(format!("osg-panel-cache-{}",uuid::Uuid::new_v4()));
        let first=Hub::new("cache-test".into(),Arc::new(osg_companion_service::NullSourceSink));
        assert!(ready(&first).unwrap());
        let mut upload:Value=serde_json::from_str(include_str!("../../crates/companion-service/tests/fixtures/c1-flight-publication.json")).unwrap();
        upload["command"]=json!("companion_publish");first.command(&upload).unwrap();
        save(&first,&directory).unwrap();
        save(&first,&directory).unwrap(); // Replacing an existing Windows cache is atomic.
        let second=Hub::new("cache-test".into(),Arc::new(osg_companion_service::NullSourceSink));
        restore(&second,&directory).unwrap();
        assert_eq!(second.publication_uploads()[0].publication_hash,upload["publicationHash"]);
        let mut corrupt=first.publication_uploads();
        corrupt[0].publication_hash=format!("sha256:{}","0".repeat(64));
        fs::write(directory.join("companion-panels.json"),serde_json::to_vec(&corrupt).unwrap()).unwrap();
        let blank=Hub::new("cache-test".into(),Arc::new(osg_companion_service::NullSourceSink));
        assert!(restore(&blank,&directory).is_err());
        assert!(blank.publication_uploads().is_empty());
        fs::write(directory.join("companion-panels.json"),b"invalid").unwrap();
        assert!(restore(&second,&directory).is_err());
        fs::remove_file(directory.join("companion-panels.json")).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
