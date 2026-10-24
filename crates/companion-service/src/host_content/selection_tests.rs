use super::*;
use super::tests::{open,database,upload,publish,other};
fn pair(hub:&Hub,id:&str,kind:&str){
    let offer=hub.command(&json!({"command":"companion_pair_offer","kind":kind})).unwrap();
    let pending=hub.handle_wire(json!({"type":"pair","version":1,"secret":offer["secret"],"companionId":id,"name":id}),id,hub.now_ms());
    hub.command(&json!({"command":"companion_approve","requestId":pending["requestId"],"companionId":id})).unwrap();
}
fn token(hub:&Hub)->Value{hub.command(&json!({"command":"companion_status"})).unwrap()["snapshotToken"].clone()}
fn save(hub:&Hub,mut publication:Value,selection:Value)->Result<Value,String>{
    publication["command"]=json!("companion_publish");publication["saveAsLatest"]=json!(true);publication["selection"]=selection;hub.command(&publication)
}
fn assign(hub:&Hub,instance:&str,companion:&str,publication:&Value){
    hub.command(&json!({"command":"companion_assign","instanceId":instance,"companionId":companion,"publicationHash":publication["publicationHash"]})).unwrap();
}
#[test]
fn bulk_selection_updates_adds_removes_and_reuses_instances_in_one_guarded_snapshot(){
    let dir=tempfile::tempdir().unwrap();let hub=open(dir.path()).unwrap();for id in ["a","b","c"]{pair(&hub,id,"browser");}
    let first=upload();let next=other(first.clone());publish(&hub,first.clone(),true);assign(&hub,"one","a",&first);assign(&hub,"two","b",&first);
    let db=database(dir.path());let before=db.read_many(&[("secrets","companion-host"),("companion","publications")]).unwrap();
    let result=save(&hub,next.clone(),json!({"expectedHostToken":token(&hub),"companionIds":["b","c","c"]})).unwrap();assert_eq!(result["selectionCommitted"],true);
    let status=hub.command(&json!({"command":"companion_status"})).unwrap();let instances=status["instances"].as_array().unwrap();
    assert_eq!(instances.len(),2);assert!(hub.assigned_publication("one").is_err());assert_eq!(hub.assigned_publication("two").unwrap().publication_hash,next["publicationHash"]);
    assert!(instances.iter().any(|a|a["companionId"]=="c"&&a["publicationHash"]==next["publicationHash"]));
    let after=db.read_many(&[("secrets","companion-host"),("companion","publications")]).unwrap();assert_eq!(after[0].0,before[0].0+1);assert_eq!(after[1].0,before[1].0+1);
    let reopened=open(dir.path()).unwrap();assert_eq!(reopened.saved_heads(),hub.saved_heads());assert_eq!(reopened.assigned_publication("two").unwrap().publication_hash,next["publicationHash"]);
    save(&hub,first.clone(),json!({"expectedHostToken":token(&hub)})).unwrap();assert_eq!(hub.assigned_publication("two").unwrap().publication_hash,first["publicationHash"]);
    save(&hub,next,json!({"expectedHostToken":token(&hub),"companionIds":[]})).unwrap();assert_eq!(hub.command(&json!({"command":"companion_status"})).unwrap()["instances"],json!([]));
}
#[test]
fn malformed_stale_unpaired_ambiguous_and_incompatible_selections_preserve_every_record(){
    let dir=tempfile::tempdir().unwrap();let hub=open(dir.path()).unwrap();pair(&hub,"a","browser");pair(&hub,"esp","esp32");let first=upload();publish(&hub,first.clone(),true);assign(&hub,"one","a",&first);
    let stale=token(&hub);hub.command(&json!({"command":"companion_update","companionId":"a","name":"renamed"})).unwrap();
    let db=database(dir.path());let before=db.read_many(&[("secrets","companion-host"),("companion","publications")]).unwrap();let next=other(first.clone());
    for selection in [json!({}),json!({"expectedHostToken":token(&hub),"unexpected":true}),json!({"expectedHostToken":stale}),
        json!({"expectedHostToken":token(&hub),"companionIds":["missing"]}),json!({"expectedHostToken":token(&hub),"companionIds":["esp"]}),
        json!({"expectedHostToken":token(&hub),"companionIds":vec!["a";MAX_INSTANCES+1]})]{assert!(save(&hub,next.clone(),selection).is_err());}
    assert_eq!(db.read_many(&[("secrets","companion-host"),("companion","publications")]).unwrap(),before);assert_eq!(hub.publication_uploads().len(),1);
    assign(&hub,"extra","a",&first);let mut unrelated=next.clone();unrelated["canonicalDocument"]=json!(next["canonicalDocument"].as_str().unwrap().replace("toggle-native-fixture","different-panel"));unrelated["publicationHash"]=json!(format!("sha256:{}",hash(unrelated["canonicalDocument"].as_str().unwrap().as_bytes())));
    assert!(save(&hub,unrelated,json!({"expectedHostToken":token(&hub),"companionIds":["a"]})).unwrap_err().contains("multiple controller"));
    let mut request=next;request["command"]=json!("companion_publish");request["selection"]=json!({"expectedHostToken":token(&hub)});assert!(hub.command(&request).unwrap_err().contains("require saving"));
}
#[test]
fn writer_busy_or_cross_process_edit_never_acknowledges_partial_publication_or_selection(){
    let dir=tempfile::tempdir().unwrap();let hub=open(dir.path()).unwrap();pair(&hub,"a","browser");let first=upload();publish(&hub,first.clone(),true);assign(&hub,"one","a",&first);
    let db=database(dir.path());let before=db.read_many(&[("secrets","companion-host"),("companion","publications")]).unwrap();let next=other(first.clone());let old_token=token(&hub);
    let lock=osg_persistence::test_support::hold_writer(&db).unwrap();assert!(save(&hub,next.clone(),json!({"expectedHostToken":old_token,"companionIds":[]})).is_err());drop(lock);
    assert_ne!(token(&hub),old_token);assert_eq!(hub.assigned_publication("one").unwrap().publication_hash,first["publicationHash"]);assert_eq!(hub.publication_uploads().len(),1);
    assert_eq!(db.read_many(&[("secrets","companion-host"),("companion","publications")]).unwrap(),before);
    let external=open(dir.path()).unwrap();external.mark_saved(first["publicationHash"].as_str().unwrap()).unwrap();
    assert!(save(&hub,next,json!({"expectedHostToken":token(&hub),"companionIds":["a"]})).unwrap_err().contains("revision conflict"));
    assert_eq!(hub.assigned_publication("one").unwrap().publication_hash,first["publicationHash"]);assert_eq!(open(dir.path()).unwrap().publication_uploads().len(),1);
}
#[test]
fn selection_keeps_profile_ownership_and_reuses_unrelated_direct_slot(){
    let dir=tempfile::tempdir().unwrap();let hub=open(dir.path()).unwrap();pair(&hub,"a","browser");pair(&hub,"b","browser");let first=upload();let next=other(first.clone());publish(&hub,first.clone(),true);assign(&hub,"one","a",&first);
    let proof=hub.prepare_profile_assignment("one",first["publicationHash"].as_str().unwrap()).unwrap();
    let receipt=hub.capture_profile_assignment(&proof,&uuid::Uuid::new_v4().to_string(),None).unwrap();hub.acquire_profile_assignment(&receipt).unwrap();
    let mut unrelated=first.clone();unrelated["canonicalDocument"]=json!(first["canonicalDocument"].as_str().unwrap().replace("toggle-native-fixture","different-panel"));unrelated["publicationHash"]=json!(format!("sha256:{}",hash(unrelated["canonicalDocument"].as_str().unwrap().as_bytes())));unrelated["capabilityFingerprint"]=Value::Null;
    publish(&hub,unrelated.clone(),false);assign(&hub,"reuse","b",&unrelated);
    save(&hub,next.clone(),json!({"expectedHostToken":token(&hub),"companionIds":["b"]})).unwrap();hub.validate_profile_assignment_owner(&receipt).unwrap();
    assert_eq!(hub.assigned_publication("one").unwrap().publication_hash,first["publicationHash"]);assert_eq!(hub.assigned_publication("reuse").unwrap().publication_hash,next["publicationHash"]);
    save(&hub,next,json!({"expectedHostToken":token(&hub)})).unwrap();hub.validate_profile_assignment_owner(&receipt).unwrap();
}
#[derive(Default)]
struct FaultSink {definitions:Mutex<BTreeMap<String,SourceDefinition>>,fail_register:std::sync::atomic::AtomicBool,fail_unregister:std::sync::atomic::AtomicBool}
impl SourceSink for FaultSink {
    fn register(&self,definition:SourceDefinition)->Result<(),String>{
        if self.fail_register.load(std::sync::atomic::Ordering::Acquire){return Err("descriptor failure".into());}
        self.definitions.lock().unwrap().insert(definition.source_id.clone(),definition);Ok(())
    }
    fn unregister(&self,id:&str)->Result<(),String>{
        if self.fail_unregister.load(std::sync::atomic::Ordering::Acquire){return Err("removal failure".into());}
        self.definitions.lock().unwrap().remove(id);Ok(())
    }
    fn baseline(&self,_:&SourceToken,_:u64,_:&[SourceChange])->Result<(),String>{Ok(())}
    fn changes(&self,_:&SourceToken,_:u64,_:&[SourceChange])->Result<(),String>{Ok(())}
    fn loss(&self,_:&SourceToken)->Result<(),String>{Ok(())}
}
#[test]
fn descriptor_failure_and_failed_compensation_preserve_the_old_snapshot_and_surface_diagnostics(){
    let dir=tempfile::tempdir().unwrap();let sink=Arc::new(FaultSink::default());
    let hub=Hub::with_storage(Arc::new(crate::persistence::test_support::store(dir.path().join("companion-host-state.dpapi"))),sink.clone()).unwrap();
    pair(&hub,"a","browser");let first=upload();let next=other(first.clone());publish(&hub,first.clone(),true);assign(&hub,"one","a",&first);
    let db=database(dir.path());let before=db.read_many(&[("secrets","companion-host"),("companion","publications")]).unwrap();
    sink.fail_register.store(true,std::sync::atomic::Ordering::Release);
    assert!(save(&hub,next.clone(),json!({"expectedHostToken":token(&hub)})).unwrap_err().contains("descriptor failure"));
    assert!(hub.command(&json!({"command":"companion_status"})).unwrap()["diagnostic"].as_str().unwrap().contains("restoration"));
    sink.fail_register.store(false,std::sync::atomic::Ordering::Release);sink.fail_unregister.store(true,std::sync::atomic::Ordering::Release);
    assert!(save(&hub,next.clone(),json!({"expectedHostToken":token(&hub),"companionIds":[]})).unwrap_err().contains("removal failure"));
    assert_eq!(db.read_many(&[("secrets","companion-host"),("companion","publications")]).unwrap(),before);assert_eq!(hub.saved_heads().values().next().unwrap(),first["publicationHash"].as_str().unwrap());
    sink.fail_unregister.store(false,std::sync::atomic::Ordering::Release);save(&hub,next,json!({"expectedHostToken":token(&hub),"companionIds":[]})).unwrap();assert!(sink.definitions.lock().unwrap().is_empty());
}
