use base64::{engine::general_purpose::STANDARD,Engine};
use osg_companion_service::{Hub,NullSourceSink,Publication,PublicationUpload};
use osg_companion_service::render_packages::{RenderDiskCache,RenderPackage,RenderPackageUpload,OBJECT_KINDS};
use osg_companion_service::render_object::{inspect_render_object,RenderObjectKind};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::BTreeMap,fs,path::PathBuf,sync::Arc};

fn hash(bytes:&[u8])->String {format!("sha256:{:x}",Sha256::digest(bytes))}
fn directory(board:&str,revision:&str)->PathBuf {PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packages/companion-renderer/test/poc").join(board).join(revision)}
fn fixture(board:&str,revision:&str)->(Publication,RenderPackageUpload) {
    let dir=directory(board,revision);
    let source:Value=serde_json::from_slice(&fs::read(dir.join("source-publication.json")).unwrap()).unwrap();
    let document=String::from_utf8(serde_json_canonicalizer::to_vec(&source["document"]).unwrap()).unwrap();
    let source=Publication::validate(PublicationUpload{canonical_document:document,publication_hash:source["publicationHash"].as_str().unwrap().into(),capability_fingerprint:Some(source["capabilityFingerprint"].as_str().unwrap().into()),assets:vec![]}).unwrap();
    let manifest=fs::read_to_string(dir.join("manifest.json")).unwrap();
    let upload=RenderPackageUpload{package_hash:hash(manifest.as_bytes()),manifest,objects:OBJECT_KINDS.into_iter().map(|kind|(kind.to_owned(),STANDARD.encode(fs::read(dir.join(format!("{kind}.bin"))).unwrap()))).collect()};
    (source,upload)
}
fn manifest_mutation(upload:&mut RenderPackageUpload,change:impl FnOnce(&mut Value)) {
    let mut value:Value=serde_json::from_str(&upload.manifest).unwrap();change(&mut value);
    upload.manifest=String::from_utf8(serde_json_canonicalizer::to_vec(&value).unwrap()).unwrap();upload.package_hash=hash(upload.manifest.as_bytes());
}
fn object_mutation(upload:&mut RenderPackageUpload,kind:&str,change:impl FnOnce(&mut Vec<u8>)) {
    let mut bytes=STANDARD.decode(&upload.objects[kind]).unwrap();change(&mut bytes);let digest=hash(&bytes);
    upload.objects.insert(kind.into(),STANDARD.encode(&bytes));
    manifest_mutation(upload,|manifest|{
        let desc=manifest["objects"].as_array_mut().unwrap().iter_mut().find(|d|d["kind"]==kind).unwrap();desc["sha256"]=json!(digest);
        if kind=="semantics" {manifest["semanticDescriptorHash"]=json!(digest);}
    });
}
#[test]
fn native_authority_accepts_both_compiled_board_revisions_and_bounds_reads() {
    for board in ["sunton","waveshare"] {for revision in ["A","B"] {
        let (source,upload)=fixture(board,revision);let p=RenderPackage::validate(upload,&source).unwrap();
        assert_eq!(p.manifest.source_publication_hash,source.publication_hash);
        assert_eq!(p.manifest.semantic_fingerprint,source.capability_fingerprint);
        for kind in ["manifest","semantics","scene","images","fonts"] {
            let bytes=if kind=="manifest"{&p.manifest_bytes}else{&p.objects[kind]};
            let mut read=vec![];for offset in (0..bytes.len()).step_by(2048) {
                let c=p.chunk(kind,offset,2048).unwrap();read.extend(STANDARD.decode(c["base64"].as_str().unwrap()).unwrap());
            }assert_eq!(read.as_slice(),bytes.as_ref());
            assert!(p.chunk(kind,bytes.len(),2048).is_err());assert!(p.chunk(kind,1,2048).is_err());
        }
        assert!(p.chunk("executable",0,2048).is_err());assert!(p.chunk("scene",0,0).is_err());assert!(p.chunk("scene",0,2049).is_err());
    }}
}
#[test]
fn integrity_profile_schema_and_memory_fail_before_admission() {
    let (source,good)=fixture("sunton","A");
    for field in ["sourceUiId","sourcePublicationHash","semanticFingerprint","targetProfileHash","semanticDescriptorHash"] {
        let mut bad=good.clone();manifest_mutation(&mut bad,|v|v[field]=json!("sha256:wrong"));assert!(RenderPackage::validate(bad,&source).is_err(),"{field}");
    }
    for (key,value) in [("formatVersion",json!(2)),("rendererAbi",json!(2)),("targetProfileId",json!("unknown")),("extra",json!(true))] {
        let mut bad=good.clone();manifest_mutation(&mut bad,|v|v[key]=value);assert!(RenderPackage::validate(bad,&source).is_err());
    }
    for resource in ["internalRamBytes","psramBytes","maxDirtyPixels","maxUpdateInstructions","maxLayersPerTile","maxBlendPixelLayers"] {
        let mut bad=good.clone();manifest_mutation(&mut bad,|v|v["resources"][resource]=json!(u32::MAX));assert!(RenderPackage::validate(bad,&source).is_err(),"{resource}");
    }
    for resource in ["internalRamBytes","tileBytes","transferBytes"] {
        let mut bad=good.clone();manifest_mutation(&mut bad,|v|v["resources"][resource]=json!(0));assert!(RenderPackage::validate(bad,&source).is_err());
    }
    let mut bad=good.clone();bad.manifest.push(' ');assert!(RenderPackage::validate(bad,&source).is_err());
    let mut bad=good.clone();bad.package_hash=hash(b"different");assert!(RenderPackage::validate(bad,&source).is_err());
    let mut bad=good.clone();bad.objects.remove("fonts");assert!(RenderPackage::validate(bad,&source).is_err());
    let mut bad=good.clone();bad.objects.insert("scene".into(),"!".into());assert!(RenderPackage::validate(bad,&source).is_err());
    let mut bad=good.clone();manifest_mutation(&mut bad,|v|v["requires"]=json!(["scene.script"]));assert!(RenderPackage::validate(bad,&source).is_err());
    let mut bad=good;manifest_mutation(&mut bad,|v|v["objects"][1]["bytes"]=json!(u32::MAX));assert!(RenderPackage::validate(bad,&source).is_err());
}
#[test]
fn valid_object_hashes_cannot_change_trusted_input_or_feedback_semantics() {
    let (source,good)=fixture("sunton","A");
    for (section,offset) in [(1,6),(1,14),(1,20),(1,28),(1,30),(2,6),(2,14),(2,16),(2,20),(2,24),(3,6),(3,8),(3,10),(3,14),(3,16)] {
        let mut bad=good.clone();object_mutation(&mut bad,"semantics",|bytes|{
            let sections=inspect_render_object(RenderObjectKind::Semantics,bytes,16384).unwrap();
            let start=sections.iter().find(|s|s.kind==section).unwrap().bytes.start;bytes[start+offset]^=1;
        });assert!(RenderPackage::validate(bad,&source).is_err(),"section{section}offset{offset}");
    }
}
#[test]
fn native_disk_cache_revalidates_hashes_source_and_canonical_content() {
    let dir=tempfile::tempdir().unwrap();let cache=RenderDiskCache::new(dir.path());let (source,upload)=fixture("sunton","A");
    let p=RenderPackage::validate(upload,&source).unwrap();let path=cache.save(&p).unwrap();cache.save(&p).unwrap();
    assert_eq!(cache.load(&p.package_hash,&source).unwrap().package_hash,p.package_hash);
    assert!(cache.load("../../outside",&source).is_err());
    let mut other=source.clone();other.publication_hash=hash(b"another-source");assert!(cache.load(&p.package_hash,&other).is_err());
    fs::write(&path,b"{}").unwrap();assert!(cache.load(&p.package_hash,&source).is_err());
    fs::write(&path,vec![b' ';4*1024*1024]).unwrap();assert!(cache.load(&p.package_hash,&source).is_err());
}
fn wire(h:&Hub,connection:&str,frame:Value)->Value {h.handle_wire(frame,connection,h.now_ms())}
fn setup()->(Hub,String,Publication,RenderPackageUpload) {
    let h=Hub::new("render-host".into(),Arc::new(NullSourceSink));let offer=h.command(&json!({"command":"companion_pair_offer"})).unwrap();
    let pending=wire(&h,"pairing",json!({"type":"pair","secret":offer["secret"],"companionId":"board","name":"Board"}));
    h.command(&json!({"command":"companion_pair_approve","requestId":pending["requestId"],"companionId":"board"})).unwrap();
    let paired=wire(&h,"pairing",json!({"type":"pair-status","secret":offer["secret"],"requestId":pending["requestId"]}));
    let (source,upload)=fixture("sunton","A");
    h.command(&json!({"command":"companion_publish","canonicalDocument":source.canonical_document,"publicationHash":source.publication_hash,"capabilityFingerprint":source.capability_fingerprint,"assets":[]})).unwrap();
    h.command(&json!({"command":"companion_assign","companionId":"board","instanceId":"panel","publicationHash":source.publication_hash})).unwrap();
    let hash=h.install_render_package(upload.clone()).unwrap();assert_eq!(hash,upload.package_hash);
    assert_eq!(wire(&h,"usb",json!({"type":"auth","token":paired["token"]}))["ok"],true);
    (h,paired["token"].as_str().unwrap().into(),source,upload)
}
fn prepare(h:&Hub,source:&Publication,upload:&RenderPackageUpload,cached:bool)->Value {
    let manifest:Value=serde_json::from_str(&upload.manifest).unwrap();
    let mut frame=json!({"type":"render-prepare","version":2,"instanceId":"panel","publicationHash":source.publication_hash,"profileId":manifest["targetProfileId"],"profileHash":manifest["targetProfileHash"]});
    if cached {frame["cachedPackageHash"]=json!(upload.package_hash);}
    wire(h,"usb",frame)
}
#[test]
fn authenticated_preparation_read_and_compact_open_keep_legacy_runtime_rules() {
    let (h,_,source,upload)=setup();
    assert_eq!(wire(&h,"stranger",json!({"type":"render-capabilities","version":2}))["ok"],false);
    assert_eq!(wire(&h,"usb",json!({"type":"render-capabilities","version":1}))["ok"],false);
    assert_eq!(wire(&h,"usb",json!({"type":"render-capabilities","version":2}))["ok"],true);
    let p=prepare(&h,&source,&upload,false);assert_eq!(p["ok"],true,"{p}");assert!(p["manifest"].is_string());
    assert_eq!(h.command(&json!({"command":"companion_status"})).unwrap()["instances"][0]["connected"],false);
    let read=json!({"type":"render-read","version":2,"preparedId":p["preparedId"],"packageHash":p["packageHash"],"kind":"images","offset":0,"bytes":2048});
    assert_eq!(wire(&h,"usb",read.clone())["ok"],true);assert_eq!(wire(&h,"stranger",read.clone())["ok"],false);
    let mut bad=read.clone();bad["preparedId"]=json!("stale");assert_eq!(wire(&h,"usb",bad)["ok"],false);
    let mut bad=read;bad["bytes"]=json!(2049);assert_eq!(wire(&h,"usb",bad)["ok"],false);
    let warm=prepare(&h,&source,&upload,true);assert_eq!(warm["cached"],true);assert!(warm["manifest"].is_null());
    let open_request=json!({"type":"render-open","version":2,"instanceId":"panel","publicationHash":source.publication_hash,"preparedId":warm["preparedId"],"packageHash":warm["packageHash"],"profileHash":warm["profileHash"],"semanticDescriptorHash":warm["semanticDescriptorHash"]});
    let mut bad=open_request.clone();bad["semanticDescriptorHash"]=json!(hash(b"wrong"));assert_eq!(wire(&h,"usb",bad)["ok"],false);
    let open=wire(&h,"usb",open_request);assert_eq!(open["ok"],true,"{open}");assert!(open.get("publication").is_none());
    assert!(serde_json::to_vec(&open).unwrap().len()<1024);
    assert_eq!(prepare(&h,&source,&upload,true)["ok"],false);
    let base=json!({"type":"baseline","version":1,"clientId":open["clientId"],"sessionId":open["sessionId"],"generation":open["generation"],"publicationHash":source.publication_hash,"sequence":"0","values":source.controls.iter().map(|c|(c.id.clone(),false)).collect::<BTreeMap<_,_>>()});
    assert_eq!(wire(&h,"usb",base)["ok"],true);
    // A new recipe may also replace compiled presentation for the unchanged
    // trusted semantic source; this must invalidate its previous writer.
    let (_,mut b)=fixture("sunton","B");
    manifest_mutation(&mut b,|m|m["sourcePublicationHash"]=json!(source.publication_hash));
    h.install_render_package(b).unwrap();
    let heartbeat=json!({"type":"heartbeat","version":1,"clientId":open["clientId"],"sessionId":open["sessionId"],"generation":open["generation"],"publicationHash":source.publication_hash,"sequence":"1"});
    assert_eq!(wire(&h,"usb",heartbeat)["ok"],false);
    h.disconnect_connection("usb",h.now_ms());assert_eq!(wire(&h,"usb",json!({"type":"render-capabilities","version":2}))["ok"],false);
}

fn prepare_on(h:&Hub,connection:&str,source:&Publication,upload:&RenderPackageUpload)->Value {
    let manifest:Value=serde_json::from_str(&upload.manifest).unwrap();
    wire(h,connection,json!({"type":"render-prepare","version":2,"instanceId":"panel","publicationHash":source.publication_hash,"profileId":manifest["targetProfileId"],"profileHash":manifest["targetProfileHash"]}))
}
fn render_open_frame(source:&Publication,prepared:&Value)->Value {
    json!({"type":"render-open","version":2,"instanceId":"panel","publicationHash":source.publication_hash,"preparedId":prepared["preparedId"],"packageHash":prepared["packageHash"],"profileHash":prepared["profileHash"],"semanticDescriptorHash":prepared["semanticDescriptorHash"]})
}
fn render_read_frame(prepared:&Value)->Value {
    json!({"type":"render-read","version":2,"preparedId":prepared["preparedId"],"packageHash":prepared["packageHash"],"kind":"scene","offset":0,"bytes":2048})
}

#[test]
fn assignment_change_after_prepare_invalidates_content_reads_and_open() {
    let (h,_,a,upload_a)=setup();
    let prepared=prepare(&h,&a,&upload_a,false);assert_eq!(prepared["ok"],true);
    let (b,upload_b)=fixture("sunton","B");
    h.command(&json!({"command":"companion_publish","canonicalDocument":b.canonical_document,"publicationHash":b.publication_hash,"capabilityFingerprint":b.capability_fingerprint,"assets":[]})).unwrap();
    h.install_render_package(upload_b.clone()).unwrap();
    h.command(&json!({"command":"companion_assign","companionId":"board","instanceId":"panel","publicationHash":b.publication_hash})).unwrap();
    assert_eq!(wire(&h,"usb",render_read_frame(&prepared))["ok"],false);
    assert_eq!(wire(&h,"usb",render_open_frame(&a,&prepared))["ok"],false);
    let fresh=prepare(&h,&b,&upload_b,false);assert_eq!(fresh["ok"],true,"{fresh}");
    h.command(&json!({"command":"companion_unassign","instanceId":"panel"})).unwrap();
    assert_eq!(wire(&h,"usb",render_read_frame(&fresh))["ok"],false);
    assert_eq!(wire(&h,"usb",render_open_frame(&b,&fresh))["ok"],false);
}

#[test]
fn selected_recipe_replacement_invalidates_prepared_content_before_open() {
    let (h,_,source,a)=setup();let prepared=prepare(&h,&source,&a,false);assert_eq!(prepared["ok"],true);
    // Reinstalling identical immutable bytes must not discard a valid grant.
    h.install_render_package(a.clone()).unwrap();
    assert_eq!(wire(&h,"usb",render_read_frame(&prepared))["ok"],true);
    let (_,mut b)=fixture("sunton","B");manifest_mutation(&mut b,|m|m["sourcePublicationHash"]=json!(source.publication_hash));
    h.install_render_package(b.clone()).unwrap();
    assert_eq!(wire(&h,"usb",render_read_frame(&prepared))["ok"],false);
    assert_eq!(wire(&h,"usb",render_open_frame(&source,&prepared))["ok"],false);
    let fresh=prepare(&h,&source,&b,false);assert_eq!(fresh["ok"],true,"{fresh}");
    assert_ne!(fresh["preparedId"],prepared["preparedId"]);
    let mut downgraded=render_open_frame(&source,&fresh);downgraded["version"]=json!(1);
    assert_eq!(wire(&h,"usb",downgraded)["ok"],false);
    let open=wire(&h,"usb",render_open_frame(&source,&fresh));assert_eq!(open["ok"],true,"{open}");
    assert_ne!(open["sessionId"],prepared["preparedId"]);
}

#[test]
fn prepared_content_neither_takes_over_nor_refreshes_an_existing_legacy_writer() {
    let (h,token,source,upload)=setup();
    let legacy=wire(&h,"usb",json!({"type":"open","version":1,"instanceId":"panel","publicationHash":source.publication_hash}));
    assert_eq!(legacy["ok"],true,"{legacy}");
    assert_eq!(wire(&h,"usb-second",json!({"type":"auth","token":token}))["ok"],true);
    let prepared=prepare_on(&h,"usb-second",&source,&upload);assert_eq!(prepared["ok"],true,"{prepared}");
    assert_eq!(wire(&h,"usb-second",render_read_frame(&prepared))["ok"],true);
    assert_eq!(wire(&h,"usb-second",render_open_frame(&source,&prepared))["ok"],false);
    let baseline=json!({"type":"baseline","version":1,"clientId":legacy["clientId"],"sessionId":legacy["sessionId"],"generation":legacy["generation"],"publicationHash":source.publication_hash,"sequence":"0","values":source.controls.iter().map(|c|(c.id.clone(),false)).collect::<BTreeMap<_,_>>()});
    assert_eq!(wire(&h,"usb",baseline)["ok"],true);
    // Content traffic uses a separate management grant and cannot extend the
    // legacy source lease. A new source still requires its own fresh baseline.
    h.tick(h.now_ms()+osg_companion_service::LEASE_MS+1);
    assert_eq!(wire(&h,"usb-second",render_read_frame(&prepared))["ok"],true);
    let next=wire(&h,"usb-second",render_open_frame(&source,&prepared));assert_eq!(next["ok"],true,"{next}");
    assert_ne!(next["sessionId"],legacy["sessionId"]);
    let stale=json!({"type":"heartbeat","version":1,"clientId":legacy["clientId"],"sessionId":legacy["sessionId"],"generation":legacy["generation"],"publicationHash":source.publication_hash,"sequence":"1"});
    assert_eq!(wire(&h,"usb",stale)["ok"],false);
}
#[test]
fn initial_render_adapter_rejects_inexact_numeric_source_boundaries(){
    let(source,upload)=fixture("sunton","A");
    let mut document:Value=serde_json::from_str(&source.canonical_document).unwrap();
    for widget in document["presentation"]["widgets"].as_array_mut().unwrap(){
        if widget["kind"]=="numeric"||widget["kind"]=="gauge"{widget["config"]["min"]=json!(0.1);widget["config"]["max"]=json!(0.9);}
    }
    let bytes=serde_json_canonicalizer::to_vec(&document).unwrap();
    let changed=Publication::validate(PublicationUpload{canonical_document:String::from_utf8(bytes.clone()).unwrap(),publication_hash:hash(&bytes),capability_fingerprint:None,assets:vec![]}).unwrap();
    let semantics=STANDARD.decode(&upload.objects["semantics"]).unwrap();
    let error=osg_companion_service::render_packages::verify_source_semantics(&semantics,&changed).unwrap_err();
    assert!(error.contains("exactly representable as Q16.16"),"{error}");
}
