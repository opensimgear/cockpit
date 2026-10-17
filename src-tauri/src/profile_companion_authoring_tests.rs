use super::*;
use osg_companion_service::{Hub, Publication, PublicationUpload};
use osg_mapping_host::companions::CompanionProvider;
use serde_json::{json,Value};
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
    let directory=tempfile::tempdir().unwrap();
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
    assert_eq!(first.schema_version,1);assert_eq!(first.panels.len(),2);assert!(digest(&first.revision));
    assert!(first.panels.iter().all(|panel|(1..=128).contains(&panel.control_count)));
    assert_eq!(first.revision,runtime.profile_authoring_catalogue().unwrap().revision);
    assert!(runtime.services.get().is_none());assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(),0);
    let serialized=serde_json::to_value(&first).unwrap();
    assert!(serialized["panels"][0].get("assets").is_none());
    assert!(serialized["panels"][0].get("canonicalDocument").is_none());
    let mut summary=hub.publication_summaries();summary.pop();assert_eq!(hub.publication_summaries().len(),2);
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
    let directory=tempfile::tempdir().unwrap();let missing=directory.path().join("missing");
    let runtime=Runtime::new(Some(missing.clone()),Arc::new(CompanionProvider::new(Arc::new(||1))));
    assert!(runtime.profile_authoring_catalogue().is_err());
    assert!(runtime.profile_authoring_inspect("r","instance",&format!("sha256:{}","0".repeat(64)),0,1).is_err());
    assert!(!missing.exists());assert!(runtime.services.get().is_none());assert!(runtime.metadata.get().is_none());runtime.shutdown();
}
