fn save_panel_upload(manager:&GatewayManager,title:&str,latest:bool)->Value{
    use sha2::{Digest,Sha256};
    let mut upload:osg_companion_service::PublicationUpload=serde_json::from_str(include_str!("../../crates/companion-service/tests/fixtures/c1-flight-publication.json")).unwrap();
    upload.canonical_document=upload.canonical_document.replace("Flight instruments",title);
    upload.publication_hash=format!("sha256:{:x}",Sha256::digest(upload.canonical_document.as_bytes()));upload.capability_fingerprint=None;
    let bytes=serde_json::to_vec(&json!({"canonicalDocument":upload.canonical_document,"publicationHash":upload.publication_hash,"assets":upload.assets})).unwrap();
    let started=request(manager,"companion_publish_begin",None,json!({"companion":{"size":bytes.len()}}));
    assert_eq!(started["ok"],true,"{started}");let id=started["state"]["companions"]["uploadId"].clone();
    for (chunk,bytes) in bytes.chunks(8192).enumerate(){
        let reply=request(manager,"companion_publish_chunk",None,json!({"companion":{"uploadId":id,"offset":chunk*8192,"bytes":bytes}}));
        assert_eq!(reply["ok"],true,"{reply}");
    }
    let committed=request(manager,"companion_publish_commit",None,json!({"companion":{"uploadId":id,"saveAsLatest":latest}}));
    assert_eq!(committed["ok"],true,"{committed}");committed["state"]["companions"].clone()
}
fn panel_profile_document(hash:&str)->osg_profiles::ProfileDocument{
    let mut document=osg_profiles::new_document("Follows saved panel").unwrap();
    document.resources.push(osg_profiles::Resource{id:"screen".into(),name:"Screen".into(),kind:osg_profiles::ResourceKind::Companion,required:false,ports:vec![]});
    document.companion_assignments.push(osg_profiles::CompanionAssignment{resource_id:"screen".into(),panel_id:None,publication_hash:hash.into()});document
}
#[test]
fn saved_panel_commit_updates_inactive_profiles_and_archive_restore_does_not_roll_them_back(){
    let (manager,_)=setup();
    let first=save_panel_upload(&manager,"First saved view",true);
    let old=first["panelSaved"]["publicationHash"].as_str().unwrap();
    let document=panel_profile_document(old);let id=document.id.clone();
    let saved=request(&manager,"profile_editor_save",None,json!({"document":document,"expectedRevision":null,"assignments":[]}));
    assert_eq!(saved["ok"],true,"{saved}");assert_eq!(saved["state"]["profiles"]["document"]["companionAssignments"][0]["panelId"],"flight-panel");
    let active=request(&manager,"profile_apply",None,json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1}));assert_eq!(active["ok"],true,"{active}");
    let owner=active["state"]["profiles"]["profileSession"]["owner"].clone();
    let latest=save_panel_upload(&manager,"Latest saved view",true);
    assert_eq!(latest["panelSaveApplication"]["state"],"inactive");
    let new=latest["panelSaved"]["publicationHash"].clone();assert_ne!(new,old);
    let source=request(&manager,"profile_assignments_get",None,json!({"id":id}));
    assert_eq!(source["state"]["profiles"]["document"]["companionAssignments"][0]["publicationHash"],new);
    let archive=save_panel_upload(&manager,"First saved view",false);assert!(archive["panelSaved"].is_null());
    let source=request(&manager,"profile_get",None,json!({"id":id}));
    assert_eq!(source["state"]["profiles"]["document"]["companionAssignments"][0]["publicationHash"],new);
    assert_eq!(request(&manager,"profile_status",None,json!({}))["state"]["profiles"]["profileSession"]["owner"],owner);
}
#[test]
fn saved_panel_rebind_failure_is_actionable_and_keeps_the_previous_active_owner(){
    let (manager,_)=setup();let first=save_panel_upload(&manager,"Initial view",true);
    let active=request(&manager,"profile_apply",None,json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1}));assert_eq!(active["ok"],true,"{active}");
    let owner=active["state"]["profiles"]["profileSession"]["owner"].clone();
    // Retained active execution remains valid while the saved graph requests a
    // companion whose native assignment cannot currently be rebound.
    let mut edited=panel_profile_document(first["panelSaved"]["publicationHash"].as_str().unwrap());
    edited.id=osg_profiles::DEFAULT_PROFILE_ID.into();edited.name="Default".into();
    manager.profiles.lock().unwrap().request(&json!({"command":"profile_save","document":edited,"expectedRevision":1})).unwrap();
    let latest=save_panel_upload(&manager,"New view requiring rebind",true);
    assert_eq!(latest["panelSaveApplication"]["state"],"failed","{latest}");
    assert!(latest["panelSaveApplication"]["error"].as_str().unwrap().contains("Panel saved, but"));
    assert_eq!(manager.profile_session.lock().unwrap().status().owner.as_ref().map(|owner|serde_json::to_value(owner).unwrap()),Some(owner));
    let saved=request(&manager,"profile_get",None,json!({"id":osg_profiles::DEFAULT_PROFILE_ID}));
    assert_eq!(saved["state"]["profiles"]["document"]["companionAssignments"][0]["publicationHash"],latest["panelSaved"]["publicationHash"]);
    // Retrying an identical Save still attempts the lagging active owner, even
    // though a passive read and the first attempt already updated storage.
    let retry=save_panel_upload(&manager,"New view requiring rebind",true);
    assert_eq!(retry["panelSaveApplication"]["state"],"failed","{retry}");
    assert!(retry["panelSaveApplication"]["error"].as_str().unwrap().contains("Panel saved, but"));
}
#[test]
fn background_panel_library_restoration_does_not_cancel_pending_profile_activation(){
    let (mut manager,_)=setup();
    save_panel_upload(&manager,"Saved library view",true);
    let id=osg_profiles::DEFAULT_PROFILE_ID;
    assert_eq!(request(&manager,"profile_apply",None,json!({"id":id,"expectedRevision":1}))["ok"],true);
    let previous=request(&manager,"profile_status",None,json!({}))["state"]["profiles"]["profileSession"]["owner"].clone();
    let prepare=manager.profile_prepare.clone();
    let (entered_tx,entered_rx)=mpsc::channel();let (release_tx,release_rx)=mpsc::channel();
    let release=Mutex::new(release_rx);
    Arc::get_mut(&mut manager).unwrap().profile_prepare=Arc::new(move|candidate|{
        entered_tx.send(()).unwrap();release.lock().unwrap().recv_timeout(Duration::from_secs(15)).unwrap();prepare(candidate)
    });
    let activating=manager.clone();
    let worker=std::thread::spawn(move||request(&activating,"profile_apply",None,json!({"id":id,"expectedRevision":1})));
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let epoch=manager.profile_admission_epoch.load(Ordering::Acquire);
    assert_eq!(manager.profile_pending.load(Ordering::Acquire),1);
    let same=save_panel_upload(&manager,"Saved library view",true);
    assert_eq!(same["panelSaveApplication"]["state"],"inactive");
    let unrelated=save_panel_upload(&manager,"Updated unrelated library view",true);
    assert_eq!(unrelated["panelSaveApplication"]["state"],"inactive");
    save_panel_upload(&manager,"Saved library view",false);
    let begun=request(&manager,"companion_publish_begin",None,json!({"companion":{"size":1}}));
    let aborted=request(&manager,"companion_publish_abort",None,json!({"companion":{"uploadId":begun["state"]["companions"]["uploadId"]}}));
    assert_eq!(aborted["ok"],true,"{aborted}");
    let pruned=request(&manager,"companion_prune",None,json!({"companion":{"hashes":[],"panelIds":[]}}));
    assert_eq!(pruned["ok"],true,"{pruned}");
    let status=request(&manager,"profile_status",None,json!({}));
    assert_eq!(status["ok"],true);assert_eq!(status["state"]["profiles"]["profileSession"]["owner"],previous);
    assert_eq!(manager.profile_admission_epoch.load(Ordering::Acquire),epoch,"Transport-only library work must not invalidate native preparation");
    assert_eq!(manager.profile_pending.load(Ordering::Acquire),1,"Library uploads must not enter the native control queue");
    release_tx.send(()).unwrap();let activated=worker.join().unwrap();
    assert_eq!(activated["ok"],true,"{activated}");
    assert_ne!(activated["state"]["profiles"]["profileSession"]["owner"],previous);
}
#[test]
fn delayed_saved_panel_application_cannot_allocate_an_epoch_after_deactivation(){
    let (manager,_)=setup();let mut saved=save_panel_upload(&manager,"Saved view",true);
    assert_eq!(request(&manager,"profile_apply",None,json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1}))["ok"],true);
    let arrival=manager.profile_admission_epoch.load(Ordering::Acquire);
    assert_eq!(request(&manager,"profile_stop",None,json!({}))["ok"],true);
    let stopped=manager.profile_admission_epoch.load(Ordering::Acquire);
    manager.apply_saved_panel(&mut saved,Some(arrival));
    assert_eq!(saved["panelSaveApplication"]["state"],"failed");
    assert!(saved["panelSaveApplication"]["error"].as_str().unwrap().contains("cancelled"));
    assert_eq!(manager.profile_admission_epoch.load(Ordering::Acquire),stopped);
    assert!(manager.profile_session.lock().unwrap().status().owner.is_none());
}

#[test]
fn mcp_profile_reads_are_passive_and_changed_panel_drafts_require_another_preview() {
    let (manager, _) = setup();
    let first = save_panel_upload(&manager, "MCP original panel", true);
    let original_hash = first["panelSaved"]["publicationHash"].as_str().unwrap();
    let latest = save_panel_upload(&manager, "MCP updated panel", true);
    let latest_hash = latest["panelSaved"]["publicationHash"].clone();
    assert_ne!(latest_hash, original_hash);
    // Model a retained library restored with its historical immutable panel.
    // It has not yet been opened by a UI reader that follows the saved panel.
    let mut document = panel_profile_document(original_hash);
    document.companion_assignments[0].panel_id = Some("flight-panel".into());
    let id = document.id.clone();
    let before = manager.profiles.lock().unwrap().request(&json!({
        "command":"profile_draft_save", "document":document,
        "expectedRevision":null, "assignments":[]
    })).unwrap()["document"].clone();
    let tools = crate::mcp_tools::Tools::new(manager.clone());
    for (tool, args) in [
        ("profile_get", json!({"id":id})),
        ("profiles_list", json!({"limit":1})),
        ("profile_export", json!({"id":id})),
    ] {
        let result = tools.call(tool, args, "mcp-passive-panel-test");
        assert_eq!(result["ok"], true, "{result}");
    }
    let draft = tools.call("profile_prepare_changes", json!({
        "profileId":id, "expectedRevision":1,
        "edits":[{"kind":"set_name","name":"Reviewed historical panel"}]
    }), "mcp-passive-panel-test");
    assert_eq!(draft["ok"], true, "{draft}");
    assert_eq!(draft["data"]["document"]["companionAssignments"][0]["publicationHash"], original_hash);
    let after_reads = manager.profiles.lock().unwrap().request(&json!({"command":"profile_get","id":id})).unwrap();
    assert_eq!(after_reads["document"], before, "MCP inspection/prepare must not persist refreshed panel references");
    let rejected = tools.call("profile_save_prepared", json!({
        "draftId":draft["data"]["draftId"], "operationId":uuid::Uuid::new_v4().to_string()
    }), "mcp-passive-panel-test");
    assert_eq!(rejected["ok"], false, "{rejected}");
    assert_eq!(rejected["error"]["code"], "resource_changed", "{rejected}");
    let after_rejected = manager.profiles.lock().unwrap().request(&json!({"command":"profile_get","id":id})).unwrap();
    assert_eq!(after_rejected["document"], before);
    assert!(manager.profile_session.lock().unwrap().status().owner.is_none());
    // Ordinary UI reads retain the established follow-latest behavior.
    let ui = request(&manager, "profile_get", None, json!({"id":id}));
    assert_eq!(ui["state"]["profiles"]["document"]["companionAssignments"][0]["publicationHash"], latest_hash);
    let revision = ui["state"]["profiles"]["document"]["revision"].as_u64().unwrap();
    assert!(revision > 1);
    let reviewed = tools.call("profile_prepare_changes", json!({
        "profileId":id, "expectedRevision":revision,
        "edits":[{"kind":"set_name","name":"Reviewed current panel"}]
    }), "mcp-passive-panel-test");
    assert_eq!(reviewed["ok"], true, "{reviewed}");
    let saved = tools.call("profile_save_prepared", json!({
        "draftId":reviewed["data"]["draftId"], "operationId":uuid::Uuid::new_v4().to_string()
    }), "mcp-passive-panel-test");
    assert_eq!(saved["ok"], true, "{saved}");
    assert_eq!(saved["data"]["document"]["companionAssignments"], reviewed["data"]["document"]["companionAssignments"]);
    assert_eq!(saved["data"]["document"]["name"], "Reviewed current panel");
    assert!(manager.profile_session.lock().unwrap().status().owner.is_none());
}
