use super::*;
use crate::{game_metadata::{MetadataLookup,MatchMethod},game_discovery::{Platform,ProcessIdentity}};
use std::collections::BTreeMap;
struct EmptyPlatform;
impl Platform for EmptyPlatform {
    fn catalogue(&self,_:&BTreeMap<String,String>)->Vec<Value>{vec![]}
    fn validate(&self,_:&str,_:&str)->Result<(),String>{Ok(())}
    fn launch(&self,_:&Value)->Result<ProcessIdentity,String>{Err("No launch".into())}
    fn find(&self,_:&Value)->Result<Option<ProcessIdentity>,String>{Ok(None)}
    fn alive(&self,_:&ProcessIdentity)->Result<bool,String>{Ok(false)}
}
fn runtime()->(GamesRuntime,tempfile::TempDir) {
    let root=crate::test_directory();
    let runtime=GamesRuntime::test_runtime(Arc::new(|_|json!({})),crate::games::Paths::in_directory(root.path()),Arc::new(EmptyPlatform));
    let game=serde_json::from_value(json!({"name":"Portal 2","installations":{"copy":{"root":"C:/Fixture","executable":null,"executables":[],
        "sources":[{"client":"disk","appId":"fixture","installPath":"C:/Fixture"}]}},"selectedInstallation":"copy","integration":null,"defaultBackground":null})).unwrap();
    runtime.inner.settings.lock().unwrap().saved.games.insert("fixture".into(),game);
    (runtime,root)
}
#[test]
fn cached_metadata_is_passive_and_remains_available_after_manual_rename() {
    let (runtime,_root)=runtime();let request=json!({"command":"game_metadata_get","gameId":"fixture"});
    assert_eq!(runtime.metadata_details(&request).unwrap()["status"],"not-cached");
    let query=MetadataQuery{key:"disk:fixture".into(),title:"Portal 2".into(),external_ids:vec![]};
    let mut metadata=crate::game_metadata::test_record("Portal 2","puzzle video game");
    metadata.fetched_at=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();metadata.matched_by=MatchMethod::ExactTitle;
    let lookup=MetadataLookup{checked_at:metadata.fetched_at,status:LookupStatus::Matched,metadata:Some(metadata),stale:false};
    assert!(crate::game_metadata::test_store(runtime.inner.installations.root(),&query,&lookup));
    let details=runtime.metadata_details(&request).unwrap();assert_eq!(details["sourceName"],"Wikidata");assert_eq!(details["status"],"matched");
    assert_eq!(details["results"][0]["lookup"]["metadata"]["genres"][0]["label"],"puzzle video game");
    runtime.inner.settings.lock().unwrap().saved.games.get_mut("fixture").unwrap().name="My favourite puzzle".into();
    assert_eq!(runtime.metadata_details(&request).unwrap()["status"],"matched");
    assert!(runtime.inner.settings.lock().unwrap().saved.game_tags.is_empty());
    for invalid in [json!({"gameId":"missing"}),json!({}),json!({"gameId":"fixture","refresh":true})] {assert!(runtime.metadata_details(&invalid).is_err());}
    runtime.shutdown();assert!(runtime.metadata_details(&request).is_err());
}
