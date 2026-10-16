//! A real authenticated, pinned WSS path through the same Hub render contract
//! exercised by the bounded serial adapter. No display or input driver is used.
use super::{Recorder,wait};
use crate::{Hub,Publication,PublicationUpload,FeedbackBinding,FeedbackSample,FeedbackValue,FeedbackStatus,FeedbackProvenance,FeedbackSource,FeedbackRange,FeedbackMode};
use crate::render_packages::{RenderPackageUpload,OBJECT_KINDS,CHUNK_BYTES};
use crate::server::{receive_json,send_json,ServerConfig,ServerRuntime};
use base64::{engine::general_purpose::STANDARD,Engine};
use serde_json::{json,Value};
use std::{collections::BTreeMap,fs,io::{Read,Write},path::Path,sync::Arc,thread,time::Duration};

fn call<S:Read+Write>(socket:&mut tungstenite::WebSocket<S>,frame:Value)->Value{
    send_json(socket,&frame).unwrap();
    receive_json(socket,Duration::from_secs(3)).unwrap()
}
fn source_package()->(Publication,RenderPackageUpload){
    let dir=Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/companion-renderer/test/poc/sunton/A");
    let source:Value=serde_json::from_slice(&fs::read(dir.join("source-publication.json")).unwrap()).unwrap();
    let publication=Publication::validate(PublicationUpload{
        canonical_document:String::from_utf8(serde_json_canonicalizer::to_vec(&source["document"]).unwrap()).unwrap(),
        publication_hash:source["publicationHash"].as_str().unwrap().into(),
        capability_fingerprint:Some(source["capabilityFingerprint"].as_str().unwrap().into()),assets:vec![]
    }).unwrap();
    let manifest=fs::read_to_string(dir.join("manifest.json")).unwrap();
    let package=RenderPackageUpload{package_hash:crate::tls::sha256(manifest.as_bytes()),manifest,objects:OBJECT_KINDS.into_iter().map(|kind|(kind.into(),STANDARD.encode(fs::read(dir.join(format!("{kind}.bin"))).unwrap()))).collect()};
    (publication,package)
}
fn prepare(source:&Publication,manifest:&Value)->Value{
    json!({"type":"render-prepare","version":2,"instanceId":"wss-panel","publicationHash":source.publication_hash,"profileId":manifest["targetProfileId"],"profileHash":manifest["targetProfileHash"]})
}
fn read(prepared:&Value,kind:&str,offset:usize)->Value{
    json!({"type":"render-read","version":2,"preparedId":prepared["preparedId"],"packageHash":prepared["packageHash"],"kind":kind,"offset":offset,"bytes":CHUNK_BYTES})
}
fn open(source:&Publication,prepared:&Value)->Value{
    json!({"type":"render-open","version":2,"instanceId":"wss-panel","publicationHash":source.publication_hash,"preparedId":prepared["preparedId"],"packageHash":prepared["packageHash"],"profileHash":prepared["profileHash"],"semanticDescriptorHash":prepared["semanticDescriptorHash"]})
}
#[test]
fn real_wss_render_management_content_baseline_compact_feedback_and_revocation(){
    let directory=tempfile::tempdir().unwrap();let sink=Arc::new(Recorder::default());
    let hub=Hub::open(directory.path().join("record"),sink.clone()).unwrap();
    let server=ServerRuntime::new(hub.clone(),directory.path().join("tls")).unwrap();
    server.start(ServerConfig{bind:"127.0.0.1:0".parse().unwrap(),advertised_host:"127.0.0.1".into(),discovery:false}).unwrap();
    let(source,package)=source_package();
    hub.command(&json!({"command":"companion_publish","canonicalDocument":source.canonical_document,"publicationHash":source.publication_hash,"capabilityFingerprint":source.capability_fingerprint,"assets":[]})).unwrap();
    assert_eq!(hub.install_render_package(package.clone()).unwrap(),package.package_hash);
    let manifest:Value=serde_json::from_str(&package.manifest).unwrap();
    let offer=hub.command(&json!({"command":"companion_pair_offer","kind":"esp32"})).unwrap();
    let payload=server.pairing_payload(offer["secret"].as_str().unwrap(),offer["expiresAtMs"].as_u64().unwrap()).unwrap();
    let mut socket=crate::client::connect(&payload.endpoint,&payload.ca_certificate,&payload.ca_spki_sha256).unwrap();
    assert_eq!(call(&mut socket,prepare(&source,&manifest))["ok"],false);
    let pending=call(&mut socket,json!({"type":"pair","secret":payload.secret,"companionId":"wss-board","name":"Render WSS board"}));
    assert_eq!(pending["type"],"pair-pending");
    hub.command(&json!({"command":"companion_pair_approve","requestId":pending["requestId"],"companionId":"wss-board"})).unwrap();
    let paired=call(&mut socket,json!({"type":"pair-status","secret":payload.secret,"requestId":pending["requestId"]}));
    assert_eq!(paired["type"],"paired");
    let token=zeroize::Zeroizing::new(paired["token"].as_str().unwrap().to_owned());
    hub.command(&json!({"command":"companion_assign","instanceId":"wss-panel","companionId":"wss-board","publicationHash":source.publication_hash})).unwrap();
    assert_eq!(call(&mut socket,json!({"type":"auth","token":token.as_str()}))["ok"],true);
    assert_eq!(call(&mut socket,json!({"type":"render-capabilities","version":2}))["ok"],true);
    let obsolete=call(&mut socket,prepare(&source,&manifest));assert_eq!(obsolete["ok"],true);
    let prepared=call(&mut socket,prepare(&source,&manifest));assert_eq!(prepared["ok"],true);
    assert_eq!(prepared["manifest"],package.manifest);assert_eq!(prepared["cached"],false);
    assert!(serde_json::to_vec(&prepared).unwrap().len()<=3072);
    assert_eq!(call(&mut socket,read(&obsolete,"scene",0))["ok"],false);
    let mut bad=open(&source,&prepared);bad["profileHash"]=json!(crate::tls::sha256(b"other profile"));
    assert_eq!(call(&mut socket,bad)["ok"],false);
    let mut foreign=crate::client::connect(&payload.endpoint,&payload.ca_certificate,&payload.ca_spki_sha256).unwrap();
    assert_eq!(call(&mut foreign,json!({"type":"auth","token":token.as_str()}))["ok"],true);
    assert_eq!(call(&mut foreign,read(&prepared,"scene",0))["ok"],false);
    assert_eq!(call(&mut foreign,open(&source,&prepared))["ok"],false);
    foreign.close(None).unwrap();drop(foreign);
    // Fetch every immutable object, in serial-sized chunks, across actual TLS.
    // Pace below the listener's 128-frames/second admission budget.
    for kind in OBJECT_KINDS{
        let expected=STANDARD.decode(&package.objects[kind]).unwrap();let mut received=Vec::new();
        while received.len()<expected.len(){
            let offset=received.len();let response=call(&mut socket,read(&prepared,kind,offset));assert_eq!(response["ok"],true);
            assert!(serde_json::to_vec(&response).unwrap().len()<=3072);
            let chunk=&response["chunk"];assert_eq!(chunk["offset"],offset);assert_eq!(chunk["totalBytes"],expected.len());
            let bytes=STANDARD.decode(chunk["base64"].as_str().unwrap()).unwrap();
            assert!(!bytes.is_empty()&&bytes.len()<=CHUNK_BYTES);assert_eq!(chunk["bytes"],bytes.len());received.extend(bytes);
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(received,expected);let descriptor=manifest["objects"].as_array().unwrap().iter().find(|v|v["kind"]==kind).unwrap();
        assert_eq!(crate::tls::sha256(&received),descriptor["sha256"]);
    }
    assert!(sink.events.lock().unwrap().is_empty(),"Content transport cannot create input or a neutral baseline.");
    let opened=call(&mut socket,open(&source,&prepared));assert_eq!(opened["ok"],true);assert!(opened.get("publication").is_none());
    assert_eq!(opened["packageHash"],package.package_hash);assert_eq!(call(&mut socket,read(&prepared,"scene",0))["ok"],false);
    hub.configure_feedback("wss-panel",&source.publication_hash,&[
        FeedbackBinding{channel_id:"reactor-power".into(),output_id:"power-readout".into()},
        FeedbackBinding{channel_id:"laser-confirmed".into(),output_id:"laser-state".into()}
    ]).unwrap();
    let route=hub.feedback_token("wss-panel").unwrap();
    let power=FeedbackSample{channel_id:"reactor-power".into(),output_id:"power-readout".into(),value:Some(FeedbackValue::Number(37.12345678)),status:FeedbackStatus::Fresh,provenance:FeedbackProvenance::GameConfirmed,source_kind:FeedbackSource::Simulator,source_age_ms:8,unit:"%".into(),range:Some(FeedbackRange{min:0.,max:100.})};
    let indicator=FeedbackSample{channel_id:"laser-confirmed".into(),output_id:"laser-state".into(),value:Some(FeedbackValue::Boolean(true)),status:FeedbackStatus::Fresh,provenance:FeedbackProvenance::HostAccepted,source_kind:FeedbackSource::Host,source_age_ms:3,unit:"state".into(),range:None};
    hub.publish_feedback(&route,&[power.clone(),indicator],FeedbackMode::Snapshot,hub.now_ms()).unwrap();
    let frame=|kind:&str,sequence:&str|json!({"type":kind,"version":1,"clientId":opened["clientId"],"sessionId":opened["sessionId"],"generation":opened["generation"],"publicationHash":source.publication_hash,"sequence":sequence});
    let mut baseline=frame("baseline","0");baseline["values"]=json!(source.controls.iter().map(|c|(c.id.clone(),false)).collect::<BTreeMap<_,_>>());
    let snapshot=call(&mut socket,baseline);assert_eq!(snapshot["ok"],true);assert_eq!(snapshot["ackStage"],"source-accepted");assert_eq!(snapshot["submitted"],false);
    assert!(snapshot.get("feedback").is_none());assert!(serde_json::to_vec(&snapshot).unwrap().len()<=3072);
    assert_eq!(snapshot["renderFeedback"]["v"],2);assert_eq!(snapshot["renderFeedback"]["m"],0);assert_eq!(snapshot["renderFeedback"]["b"],json!([0,2]));
    let values=snapshot["renderFeedback"]["x"].as_array().unwrap();assert_eq!(values.len(),2);
    let number=values.iter().find(|v|v[0]==0).unwrap();assert_eq!(number[1],0);assert_eq!(number[2],4);assert_eq!(number[3],1);assert_eq!(number[5],37.123457);
    let light=values.iter().find(|v|v[0]==2).unwrap();assert_eq!(light[2],2);assert_eq!(light[3],0);assert_eq!(light[5],true);
    thread::sleep(Duration::from_millis(35)); // honor native feedback's 30 Hz budget
    let mut update=power;update.value=Some(FeedbackValue::Number(73.25));
    hub.publish_feedback(&route,&[update],FeedbackMode::Delta,hub.now_ms()).unwrap();
    let delta=call(&mut socket,frame("heartbeat","1"));assert_eq!(delta["ok"],true);assert!(serde_json::to_vec(&delta).unwrap().len()<=512);
    assert_eq!(delta["renderFeedback"]["m"],1);assert!(delta["renderFeedback"].get("b").is_none());assert_eq!(delta["renderFeedback"]["x"][0][0],0);assert_eq!(delta["renderFeedback"]["x"][0][5],73.25);
    let unchanged=call(&mut socket,frame("heartbeat","2"));assert_eq!(unchanged["renderFeedback"]["s"],delta["renderFeedback"]["s"]);assert_eq!(unchanged["renderFeedback"]["x"],json!([]));
    assert_eq!(*sink.events.lock().unwrap(),vec!["baseline"]);
    hub.command(&json!({"command":"companion_revoke","companionId":"wss-board"})).unwrap();
    assert_eq!(call(&mut socket,frame("heartbeat","3"))["ok"],false);
    assert_eq!(call(&mut socket,prepare(&source,&manifest))["ok"],false);
    socket.close(None).unwrap();drop(socket);
    let mut revoked=crate::client::connect(&payload.endpoint,&payload.ca_certificate,&payload.ca_spki_sha256).unwrap();
    assert_eq!(call(&mut revoked,json!({"type":"auth","token":token.as_str()}))["ok"],false);
    drop(revoked);server.stop();wait(||server.status().active_connections==0);
}