use super::*;
pub(super) fn open(path:&Path)->Result<Hub,String>{Hub::with_storage(Arc::new(crate::persistence::test_support::store(path.join("companion-host-state.dpapi"))),Arc::new(NullSourceSink))}
pub(super) fn database(path:&Path)->osg_persistence::Database{osg_persistence::Database::for_namespace(&path.join("companion-host-state.dpapi")).unwrap()}
pub(super) fn upload()->Value{serde_json::from_str(include_str!("../../tests/fixtures/compiled-toggle-publication.json")).unwrap()}
pub(super) fn publish(hub:&Hub,mut value:Value,save:bool)->Value{value["command"]=json!("companion_publish");value["saveAsLatest"]=json!(save);hub.command(&value).unwrap()}
pub(super) fn other(mut value:Value)->Value{
    value["canonicalDocument"]=json!(value["canonicalDocument"].as_str().unwrap().replace("\"title\":\"Toggle\"","\"title\":\"Later Toggle\""));
    value["publicationHash"]=json!(format!("sha256:{}",hash(value["canonicalDocument"].as_str().unwrap().as_bytes())));value["capabilityFingerprint"]=Value::Null;value
}
fn pair(hub:&Hub){
    let offer=hub.command(&json!({"command":"companion_pair_offer","kind":"browser"})).unwrap();
    let pending=hub.handle_wire(json!({"type":"pair","version":1,"secret":offer["secret"],"companionId":"screen","name":"Screen"}),"peer",hub.now_ms());
    hub.command(&json!({"command":"companion_approve","requestId":pending["requestId"],"companionId":"screen"})).unwrap();
}
#[test]
fn sqlite_reopen_restores_complete_content_heads_and_assignments_before_any_transport(){
    let dir=tempfile::tempdir().unwrap();for name in ["companion-panels.json","companion-panel-heads.json"]{std::fs::write(dir.path().join(name),b"retired invalid file").unwrap();}
    let hub=open(dir.path()).unwrap();assert!(hub.saved_panel_references().is_empty());pair(&hub);let value=upload();let saved=publish(&hub,value.clone(),true);assert_eq!(saved["panelSaved"]["publicationHash"],value["publicationHash"]);assert_eq!(hub.saved_panel_references().len(),1);
    hub.command(&json!({"command":"companion_assign","instanceId":"assigned","companionId":"screen","publicationHash":value["publicationHash"]})).unwrap();
    let restored=open(dir.path()).unwrap();assert_eq!(restored.host_id(),hub.host_id());assert_eq!(restored.assigned_publication("assigned").unwrap().publication_hash,value["publicationHash"]);
    assert_eq!(restored.saved_heads(),hub.saved_heads());assert_eq!(restored.saved_panels()[0].previous_hashes.len(),1);
    let db=database(dir.path());let rows=db.read_many(&[("secrets","companion-host"),("companion","publications")]).unwrap();assert_eq!(rows[0].0,4);assert_eq!(rows[1].0,2);
    assert!(db.read("secrets","companion-identity").unwrap().is_none());
    for name in ["companion-panels.json","companion-panel-heads.json"]{assert_eq!(std::fs::read(dir.path().join(name)).unwrap(),b"retired invalid file");}
}
#[test]
fn failed_save_head_assignment_and_prune_leave_the_prior_acknowledged_snapshot_in_memory_and_sqlite(){
    let dir=tempfile::tempdir().unwrap();let hub=open(dir.path()).unwrap();pair(&hub);let first=upload();publish(&hub,first.clone(),true);let second=other(first.clone());assert_ne!(first["publicationHash"],second["publicationHash"]);
    let db=database(dir.path());let before=db.read_many(&[("secrets","companion-host"),("companion","publications")]).unwrap();let lock=osg_persistence::test_support::hold_writer(&db).unwrap();
    let mut request=second.clone();request["command"]=json!("companion_publish");request["saveAsLatest"]=json!(true);assert!(hub.command(&request).is_err());
    assert!(hub.command(&json!({"command":"companion_assign","instanceId":"uncommitted","companionId":"screen","publicationHash":first["publicationHash"]})).is_err());
    assert_eq!(hub.publication_uploads().len(),1);assert_eq!(hub.saved_panels()[0].publication_hash,first["publicationHash"]);assert!(hub.assigned_publication("uncommitted").is_err());drop(lock);
    assert_eq!(db.read_many(&[("secrets","companion-host"),("companion","publications")]).unwrap(),before);
    publish(&hub,second.clone(),true);let lock=osg_persistence::test_support::hold_writer(&db).unwrap();assert!(hub.mark_saved(first["publicationHash"].as_str().unwrap()).is_err());
    let prune=json!({"command":"companion_prune","panelIds":["toggle-native-fixture"],"hashes":[second["publicationHash"]]});assert!(hub.command(&prune).is_err());drop(lock);
    assert_eq!(hub.publication_uploads().len(),2);assert_eq!(hub.saved_panels()[0].publication_hash,second["publicationHash"]);
    hub.mark_saved(first["publicationHash"].as_str().unwrap()).unwrap();assert_eq!(open(dir.path()).unwrap().saved_panels()[0].publication_hash,first["publicationHash"]);
    assert!(hub.mark_saved("missing").is_err());
}
#[test]
fn profile_and_assignment_references_retain_immutable_archives_until_their_references_are_removed(){
    let dir=tempfile::tempdir().unwrap();let hub=open(dir.path()).unwrap();let first=upload();let second=other(first.clone());publish(&hub,first.clone(),true);publish(&hub,second.clone(),true);
    let db=database(dir.path());let profile=serde_json::to_vec(&json!({"profiles":[{"id":"00000000-0000-4000-8000-000000000001","name":"Retain panel","revision":1,"companionAssignments":[{"resourceId":"panel","panelId":"toggle-native-fixture","publicationHash":first["publicationHash"]}]}],"localAssignments":{}})).unwrap();
    db.commit(&[osg_persistence::Change{domain:"profiles",key:"library",expected_revision:0,payload:Some(&profile)}]).unwrap();
    let request=json!({"command":"companion_prune","panelIds":["toggle-native-fixture"],"hashes":[second["publicationHash"]]});assert_eq!(hub.command(&request).unwrap()["removed"],0);
    db.commit(&[osg_persistence::Change{domain:"profiles",key:"library",expected_revision:1,payload:Some(b"{\"profiles\":[],\"localAssignments\":{}}")}]).unwrap();
    assert_eq!(hub.command(&request).unwrap()["removed"],1);assert_eq!(open(dir.path()).unwrap().saved_panels()[0].publication_hash,second["publicationHash"]);assert_eq!(db.revision("profiles","library").unwrap(),2);
    // A separately reopened owner cannot overwrite a later acknowledged edit.
    let stale=open(dir.path()).unwrap();hub.mark_saved(second["publicationHash"].as_str().unwrap()).unwrap();let mut request=first;request["command"]=json!("companion_publish");assert!(stale.command(&request).unwrap_err().contains("revision conflict"));assert_eq!(stale.publication_uploads().len(),1);
}
#[test]
fn invalid_sql_archive_never_partially_restores_or_overwrites_the_rejected_evidence(){
    let dir=tempfile::tempdir().unwrap();let hub=open(dir.path()).unwrap();publish(&hub,upload(),true);let db=database(dir.path());let revision=db.revision("companion","publications").unwrap();
    let mut bad=upload();bad["publicationHash"]=json!(format!("sha256:{}","0".repeat(64)));let bytes=serde_json::to_vec(&json!({"publications":[upload(),bad],"savedHeads":{}})).unwrap();
    db.commit(&[osg_persistence::Change{domain:"companion",key:"publications",expected_revision:revision,payload:Some(&bytes)}]).unwrap();assert!(open(dir.path()).is_err());assert!(hub.mark_saved(upload()["publicationHash"].as_str().unwrap()).is_err());assert_eq!(db.read("companion","publications").unwrap().unwrap().payload,bytes);
    assert!(decode(Some(b"invalid")).is_err());let missing=json!({"publications":[],"savedHeads":{"panel":"missing"}});assert!(decode(Some(&serde_json::to_vec(&missing).unwrap())).is_err());
}

#[test]
fn fresh_restore_record_validates_content_and_generates_unpaired_identity(){
 let archive=b"{\"publications\":[],\"savedHeads\":{}}";let first=Hub::fresh_restore_record(archive).unwrap();let second=Hub::fresh_restore_record(archive).unwrap();let first:Value=serde_json::from_slice(&first).unwrap();let second:Value=serde_json::from_slice(&second).unwrap();assert_ne!(first["hostId"],second["hostId"]);assert_eq!(first["credentials"],json!([]));assert_eq!(first["instances"],json!([]));assert!(Hub::fresh_restore_record(b"bad").is_err());
}
