use super::*;
use osg_companion_service::{Hub, Publication, PublicationUpload};
use osg_mapping_host::companions::CompanionProvider;
use serde_json::json;
use std::sync::Arc;

fn publish(hub:&Hub,changed:bool)->Publication {
    let mut upload:PublicationUpload=serde_json::from_str(include_str!("../../crates/companion-service/tests/fixtures/c1-flight-publication.json")).unwrap();
    if changed {
        upload.canonical_document=upload.canonical_document.replace("Flight instruments","Explicit replacement panel");
        upload.publication_hash=format!("sha256:{:x}",Sha256::digest(upload.canonical_document.as_bytes()));
        upload.capability_fingerprint=None;
    }
    let publication=Publication::validate(upload.clone()).unwrap();
    let mut command=serde_json::to_value(upload).unwrap();command["command"]=json!("companion_publish");hub.command(&command).unwrap();publication
}
fn fixture()->(Runtime,Hub,Publication,Publication,tempfile::TempDir) {
    let directory=crate::test_directory();
    let provider=Arc::new(CompanionProvider::new(Arc::new(||1)));
    let hub=Hub::new("passive-authoring".into(),provider.clone());
    let offer=hub.command(&json!({"command":"companion_pair_offer"})).unwrap();
    let pair=hub.handle_wire(json!({"type":"pair","secret":offer["secret"],"companionId":"screen","name":"Screen"}),"peer",hub.now_ms());
    hub.command(&json!({"command":"companion_approve","requestId":pair["requestId"],"companionId":"screen"})).unwrap();
    let a=publish(&hub,false);let b=publish(&hub,true);
    hub.command(&json!({"command":"companion_assign","instanceId":"instance","companionId":"screen","publicationHash":a.publication_hash})).unwrap();
    let runtime=Runtime::with_profile_metadata(hub.clone(),provider,directory.path().to_owned());
    (runtime,hub,a,b,directory)
}
#[test]
fn passive_catalogue_has_bounded_real_metadata_no_assets_and_stable_content_fingerprint() {
    let(runtime,hub,_,_,directory)=fixture();
    let first=runtime.profile_authoring_catalogue().unwrap();
    assert_eq!(first.schema_version,1);assert_eq!(first.panels.len(),1);assert!(digest(&first.revision));
    assert_eq!(first.panels[0].publication_hashes.len(),2);
    assert_eq!(first.panels[0].panel_id,"flight-panel");
    assert!(first.panels.iter().all(|panel|(1..=128).contains(&panel.control_count)));
    assert_eq!(first.revision,runtime.profile_authoring_catalogue().unwrap().revision);
    assert!(runtime.services.get().is_none());assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(),0);
    let serialized=serde_json::to_value(&first).unwrap();
    assert!(serialized["panels"][0].get("assets").is_none());
    assert!(serialized["panels"][0].get("canonicalDocument").is_none());
    let mut summary=hub.publication_summaries();summary.pop();assert_eq!(hub.publication_summaries().len(),2);
    runtime.shutdown();
}
fn publish_zero_control_panel(hub:&Hub,feedback_only:bool)->Publication {
    let mut upload:PublicationUpload=serde_json::from_str(include_str!("../../crates/companion-service/tests/fixtures/c1-flight-publication.json")).unwrap();
    let mut document:serde_json::Value=serde_json::from_str(&upload.canonical_document).unwrap();
    document["ui"]["id"]=json!(if feedback_only {"feedback-only-panel"}else{"empty-panel"});
    document["ui"]["title"]=json!(if feedback_only {"Feedback only"}else{"Empty saved panel"});
    document["ui"]["controls"]=json!([]);document["bindings"]=json!([]);document["targets"]=json!([]);
    document["presentation"]["widgets"].as_array_mut().unwrap().retain(|widget|feedback_only&&widget["controlIds"].as_array().unwrap().is_empty());
    for page in document["ui"]["pages"].as_array_mut().unwrap(){page["items"].as_array_mut().unwrap().retain(|item|feedback_only&&item["kind"]=="feedback");}
    if !feedback_only{
        document["ui"]["feedbackWidgets"]=json!([]);
        document["presentation"]["widgets"]=json!([{"id":"static-image","pageId":document["ui"]["pages"][0]["id"],"kind":"image","label":"Image placeholder","bounds":{"x":0,"y":0,"width":1,"height":1},"controlIds":[],"feedbackIds":[],"config":{}}]);
    }
    upload.canonical_document=serde_json::to_string(&document).unwrap();
    upload.publication_hash=format!("sha256:{:x}",Sha256::digest(upload.canonical_document.as_bytes()));upload.capability_fingerprint=None;
    let publication=Publication::validate(upload.clone()).unwrap();
    let mut command=serde_json::to_value(upload).unwrap();command["command"]=json!("companion_publish");hub.command(&command).unwrap();publication
}
#[test]
fn zero_control_panels_do_not_block_catalogue_and_inspect_real_feedback_or_empty_ports() {
    let(runtime,hub,current,_,_directory)=fixture();
    let empty=publish_zero_control_panel(&hub,false);
    let feedback=publish_zero_control_panel(&hub,true);
    let catalogue=runtime.profile_authoring_catalogue().unwrap();
    assert_eq!(catalogue.panels.len(),3);
    for panel in [&empty,&feedback]{assert_eq!(catalogue.panels.iter().find(|summary|summary.publication_hash==panel.publication_hash).unwrap().control_count,0);}
    let blank=runtime.profile_authoring_inspect("companion-resource","instance",&empty.publication_hash,0,32).unwrap();
    assert_eq!(blank.total,0);assert!(blank.ports.is_empty());
    assert!(runtime.profile_authoring_inspect("companion-resource","instance",&empty.publication_hash,1,32).is_err());
    let outputs=runtime.profile_authoring_inspect("companion-resource","instance",&feedback.publication_hash,0,32).unwrap();
    assert!(!outputs.ports.is_empty());
    assert!(outputs.ports.iter().all(|port|port.descriptor.direction==osg_profiles::Direction::Input));
    assert_eq!(hub.assigned_publication("instance").unwrap().publication_hash,current.publication_hash);
    assert!(runtime.services.get().is_none());runtime.shutdown();
}
#[test]
fn catalogue_compatibility_matches_paired_screen_size_and_device_support_without_transports() {
    let(runtime,hub,_,_,directory)=fixture();
    for companion in ["sunton","waveshare","unknown-screen"]{
        let offer=hub.command(&json!({"command":"companion_pair_offer","kind":"esp32"})).unwrap();
        let pending=hub.handle_wire(json!({"type":"pair","secret":offer["secret"],"companionId":companion,"name":companion}),companion,hub.now_ms());
        hub.command(&json!({"command":"companion_approve","requestId":pending["requestId"],"companionId":companion})).unwrap();
    }
    std::fs::write(directory.path().join("companion-firmware.json"),serde_json::to_vec(&json!({"sunton":{"profile":"sunton-3248s035r"},"waveshare":{"profile":"waveshare-lcd7-s3"}})).unwrap()).unwrap();
    let blank=publish_zero_control_panel(&hub,true);
    let mut document=hub.publication_document(&blank.publication_hash).unwrap();
    document["ui"]["id"]=json!("sunton-panel");document["ui"]["title"]=json!("Sunton display only");
    document["presentation"]["widgets"].as_array_mut().unwrap().retain(|widget|widget["kind"]=="numeric");
    let feedback_id=document["presentation"]["widgets"][0]["feedbackIds"][0].clone();
    document["ui"]["feedbackWidgets"].as_array_mut().unwrap().retain(|feedback|feedback["id"]==feedback_id);
    for page in document["ui"]["pages"].as_array_mut().unwrap(){page["items"].as_array_mut().unwrap().retain(|item|item["refId"]==feedback_id);}
    for viewport in document["presentation"]["viewports"].as_array_mut().unwrap(){viewport["width"]=json!(480);viewport["height"]=json!(320);}
    let canonical=serde_json::to_string(&document).unwrap();
    let hash=format!("sha256:{:x}",Sha256::digest(canonical.as_bytes()));
    hub.command(&json!({"command":"companion_publish","canonicalDocument":canonical,"publicationHash":hash,"assets":[]})).unwrap();
    let catalogue=runtime.profile_authoring_catalogue().unwrap();
    let panel=catalogue.panels.iter().find(|panel|panel.publication_hash==hash).unwrap();
    let row=|id:&str|panel.compatibility.iter().find(|row|row.companion_id==id).unwrap();
    assert!(row("screen").compatible,"Browser/desktop companions can use any valid publication");
    assert!(row("sunton").compatible);assert!(row("sunton").reason.is_none());
    assert!(!row("waveshare").compatible);assert!(row("waveshare").reason.as_ref().unwrap().contains("800 × 480"));
    assert!(!row("unknown-screen").compatible);assert!(row("unknown-screen").reason.as_ref().unwrap().contains("USB"));
    let sunton=osg_companion_service::profile::profile_instance_id("sunton");
    assert_eq!(runtime.profile_authoring_inspect("resource",&sunton,&hash,0,32).unwrap().total,1);
    let waveshare=osg_companion_service::profile::profile_instance_id("waveshare");
    assert!(runtime.profile_authoring_inspect("resource",&waveshare,&hash,0,32).unwrap_err().contains("800 × 480"));
    assert!(catalogue.panels.iter().find(|panel|panel.panel_id=="flight-panel").unwrap().compatibility.iter().find(|row|row.companion_id=="sunton").unwrap().reason.as_ref().unwrap().contains("ESP32"));
    assert!(runtime.services.get().is_none());assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(),1);
    runtime.shutdown();
}
#[test]
fn desired_panel_inspection_preserves_actual_assignment_and_returns_only_exact_prospective_contracts() {
    let(runtime,hub,a,b,_directory)=fixture();
    let page=runtime.profile_authoring_inspect("opaque-resource","instance",&b.publication_hash,0,64).unwrap();
    assert_eq!(page.resource_id,"opaque-resource");assert_eq!(page.publication_hash,b.publication_hash);
    assert_eq!(page.capability_fingerprint,b.capability_fingerprint);assert!(digest(&page.identity_fingerprint));
    assert!(page.ports.iter().all(|port|!port.current&&port.route_ids.is_empty()&&port.contract_hash==port.descriptor.contract_hash().unwrap()));
    let definition=osg_mapping_host::companions::native_definition(b.definition("instance")).unwrap();
    let expected=crate::profile_resource_ports::companion(&definition,&b.feedback).unwrap();
    assert_eq!(page.total,expected.len());
    assert!(page.ports.iter().all(|port|expected.iter().any(|descriptor|descriptor==&port.descriptor)));
    assert!(page.ports.iter().any(|port|port.descriptor.direction==osg_profiles::Direction::Input));
    assert!(page.ports.iter().any(|port|port.descriptor.direction==osg_profiles::Direction::Output));
    assert_eq!(hub.assigned_publication("instance").unwrap().publication_hash,a.publication_hash);
    let json=serde_json::to_value(page).unwrap();assert!(json.get("token").is_none());
    assert!(runtime.services.get().is_none());runtime.shutdown();
}
#[test]
fn choosing_a_panel_for_an_unassigned_paired_component_is_passive() {
    let(runtime,hub,_,panel,_directory)=fixture();
    hub.command(&json!({"command":"companion_unassign","instanceId":"instance"})).unwrap();
    let instance=osg_companion_service::profile::profile_instance_id("screen");
    let page=runtime.profile_authoring_inspect("paired-component",&instance,&panel.publication_hash,0,32).unwrap();
    assert!(!page.ports.is_empty());assert_eq!(page.publication_hash,panel.publication_hash);
    assert!(page.ports.iter().all(|port|!port.current&&port.route_ids.is_empty()));
    assert!(hub.resource_instance_ids().is_empty());
    assert!(runtime.services.get().is_none());
    hub.command(&json!({"command":"companion_revoke","companionId":"screen"})).unwrap();
    assert!(runtime.profile_authoring_inspect("paired-component",&instance,&panel.publication_hash,0,32).is_err());
}
#[test]
fn page_offsets_are_exact_and_identity_fences_change_after_explicit_pair_assignment_changes() {
    let(runtime,hub,a,b,_directory)=fixture();
    let first=runtime.profile_authoring_inspect("r","instance",&b.publication_hash,0,1).unwrap();
    let second=runtime.profile_authoring_inspect("r","instance",&b.publication_hash,1,1).unwrap();
    assert_eq!(first.total,second.total);assert_eq!(second.offset,1);assert_eq!(first.identity_fingerprint,second.identity_fingerprint);
    assert_ne!(first.ports[0].descriptor.id,second.ports[0].descriptor.id);
    let end=runtime.profile_authoring_inspect("r","instance",&b.publication_hash,first.total,1).unwrap();assert!(end.ports.is_empty());
    hub.command(&json!({"command":"companion_assign","instanceId":"instance","companionId":"screen","publicationHash":a.publication_hash})).unwrap();
    let changed=runtime.profile_authoring_inspect("r","instance",&b.publication_hash,1,1).unwrap();
    assert_ne!(first.identity_fingerprint,changed.identity_fingerprint);
    runtime.shutdown();
}
#[test]
fn malformed_unknown_unpaired_and_oversized_requests_fail_without_initializing_transports() {
    let(runtime,hub,_,b,_directory)=fixture();
    for(resource,instance,publication,offset,limit)in[
        ("","instance",b.publication_hash.as_str(),0,1),("r","instance","sha256:bad",0,1),
        ("r","unknown",b.publication_hash.as_str(),0,1),("r","instance",b.publication_hash.as_str(),0,0),
        ("r","instance",b.publication_hash.as_str(),0,65),("r","instance",b.publication_hash.as_str(),1025,1),
    ]{assert!(runtime.profile_authoring_inspect(resource,instance,publication,offset,limit).is_err());}
    assert!(runtime.profile_authoring_inspect("r","instance",&format!("sha256:{}","0".repeat(64)),0,1).is_err());
    hub.command(&json!({"command":"companion_revoke","companionId":"screen"})).unwrap();
    assert!(runtime.profile_authoring_inspect("r","instance",&b.publication_hash,0,1).is_err());
    assert!(runtime.services.get().is_none());runtime.shutdown();assert!(runtime.profile_authoring_catalogue().is_err());
}
#[test]
fn authoring_read_of_missing_setup_never_creates_state_or_listener() {
    let directory=crate::test_directory();let missing=directory.path().join("missing");
    let runtime=Runtime::new(Some(missing.clone()),Arc::new(CompanionProvider::new(Arc::new(||1))));
    assert!(runtime.profile_authoring_catalogue().is_err());
    assert!(runtime.profile_authoring_inspect("r","instance",&format!("sha256:{}","0".repeat(64)),0,1).is_err());
    assert!(!missing.exists());assert!(runtime.services.get().is_none());assert!(runtime.metadata.get().is_none());runtime.shutdown();
}
