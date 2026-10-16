//! Validated native panel cache, restored before any companion transports start.
use osg_companion_service::{Hub, Publication, PublicationUpload};
use serde_json::{json,Value};
use std::{fs,io::Read,path::Path};
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
