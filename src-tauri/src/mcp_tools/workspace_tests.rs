use super::*;
use base64::{engine::general_purpose::STANDARD,Engine};
const CLIENT:&str="workspace-mcp-client";
fn mutate(tools:&Tools,name:&str,mut args:Value)->Value {args["operationId"]=json!(uuid::Uuid::new_v4().to_string());tools.call(name,args,CLIENT)}
#[test]
fn authenticated_workspace_transfer_commits_and_rejects_stale_or_other_client_uploads() {
    let tools=Tools::new(Arc::new(GatewayManager::mcp_test_manager()));
    let first=tools.call("designer_workspace_read",json!({}),CLIENT);assert_eq!(first["ok"],true,"{first}");assert_eq!(first["data"]["revision"],0);
    assert!(serde_json::from_slice::<Value>(&STANDARD.decode(first["data"]["base64"].as_str().unwrap()).unwrap()).unwrap()["sources"].is_array());
    assert_eq!(tools.call("designer_workspace_read",json!({"expectedHash":"changed"}),CLIENT)["ok"],false);
    assert_eq!(tools.call("designer_workspace_read",json!({"offset":1_000_000}),CLIENT)["ok"],false);
    let source=tools.read_resource("cockpit://guide/widgets",CLIENT)["data"]["example"]["definitions"].clone();
    let workspace=json!({"sources":source,"drafts":[],"publications":{}});
    let bytes=serde_json::to_vec(&workspace).unwrap();let hash=format!("sha256:{:x}",Sha256::digest(&bytes));
    let begin=mutate(&tools,"designer_workspace_begin",json!({"expectedRevision":0,"expectedLibraryRevision":0,"totalBytes":bytes.len(),"workspaceHash":hash}));assert_eq!(begin["ok"],true,"{begin}");let id=begin["data"]["uploadId"].clone();
    assert_eq!(mutate(&tools,"designer_workspace_begin",json!({"expectedRevision":0,"expectedLibraryRevision":0,"totalBytes":bytes.len(),"workspaceHash":hash}))["ok"],false);
    assert_eq!(tools.call("designer_workspace_abort",json!({"operationId":uuid::Uuid::new_v4().to_string(),"uploadId":id}),"another-client")["ok"],false);
    assert_eq!(mutate(&tools,"designer_workspace_commit",json!({"uploadId":id}))["ok"],false);
    assert_eq!(mutate(&tools,"designer_workspace_chunk",json!({"uploadId":id,"offset":1,"base64":STANDARD.encode(&bytes)}))["ok"],false);
    assert_eq!(mutate(&tools,"designer_workspace_chunk",json!({"uploadId":id,"offset":0,"base64":"bad!!"}))["ok"],false);
    let chunk=json!({"operationId":uuid::Uuid::new_v4().to_string(),"uploadId":id,"offset":0,"base64":STANDARD.encode(&bytes)});
    let sent=tools.call("designer_workspace_chunk",chunk.clone(),CLIENT);assert_eq!(sent["ok"],true,"{sent}");assert_eq!(tools.call("designer_workspace_chunk",chunk,CLIENT),sent);
    let commit=json!({"operationId":uuid::Uuid::new_v4().to_string(),"uploadId":id});let saved=tools.call("designer_workspace_commit",commit.clone(),CLIENT);assert_eq!(saved["data"]["revision"],1,"{saved}");assert_eq!(tools.call("designer_workspace_commit",commit,CLIENT),saved);
    let current=tools.call("designer_workspace_read",json!({}),CLIENT);assert_eq!(current["data"]["revision"],1);
    let decoded:Value=serde_json::from_slice(&STANDARD.decode(current["data"]["base64"].as_str().unwrap()).unwrap()).unwrap();assert_eq!(decoded,workspace);
    let begin=mutate(&tools,"designer_workspace_begin",json!({"expectedRevision":0,"expectedLibraryRevision":0,"totalBytes":bytes.len(),"workspaceHash":hash}));let id=begin["data"]["uploadId"].clone();
    assert_eq!(mutate(&tools,"designer_workspace_chunk",json!({"uploadId":id,"offset":0,"base64":STANDARD.encode(&bytes)}))["ok"],true);
    assert_eq!(mutate(&tools,"designer_workspace_commit",json!({"uploadId":id}))["error"]["code"],"revision_conflict");
    assert_eq!(mutate(&tools,"designer_workspace_abort",json!({"uploadId":id}))["data"]["aborted"],true);
}

#[test]
fn publication_archives_transfer_in_multiple_bounded_mcp_chunks() {
    let tools=Tools::new(Arc::new(GatewayManager::mcp_test_manager()));
    let upload:osg_companion_service::PublicationUpload=serde_json::from_str(include_str!("../../../crates/companion-service/tests/fixtures/compiled-toggle-publication.json")).unwrap();
    let document:Value=serde_json::from_str(&upload.canonical_document).unwrap();
    let source=tools.read_resource("cockpit://guide/widgets",CLIENT)["data"]["example"]["definitions"].clone();
    let value=json!({"sources":source,"drafts":[],"publications":{"toggle-native-fixture":[{"document":document,"assets":upload.assets,"publicationHash":upload.publication_hash,"capabilityFingerprint":upload.capability_fingerprint}]}});
    let bytes=serde_json::to_vec(&value).unwrap();assert!(bytes.len()>48*1024);
    let hash=format!("sha256:{:x}",Sha256::digest(&bytes));
    let begin=mutate(&tools,"designer_workspace_begin",json!({"expectedRevision":0,"expectedLibraryRevision":0,"totalBytes":bytes.len(),"workspaceHash":hash}));assert_eq!(begin["ok"],true,"{begin}");let id=begin["data"]["uploadId"].clone();
    for (index,chunk) in bytes.chunks(48*1024).enumerate() {
        let reply=mutate(&tools,"designer_workspace_chunk",json!({"uploadId":id,"offset":index*48*1024,"base64":STANDARD.encode(chunk)}));assert_eq!(reply["ok"],true,"{reply}");
    }
    assert_eq!(mutate(&tools,"designer_workspace_commit",json!({"uploadId":id}))["ok"],true);
    let mut offset=0;let mut received=vec![];let mut expected_hash:Option<Value>=None;
    loop {
        let mut args=json!({"offset":offset});if let Some(hash)=&expected_hash {args["expectedHash"]=hash.clone();}
        let reply=tools.call("designer_workspace_read",args,CLIENT);assert_eq!(reply["ok"],true,"{reply}");
        expected_hash=Some(reply["data"]["workspaceHash"].clone());let chunk=STANDARD.decode(reply["data"]["base64"].as_str().unwrap()).unwrap();assert!(chunk.len()<=48*1024);received.extend(chunk);
        match reply["data"]["nextOffset"].as_u64(){Some(next)=>offset=next,None=>break}
    }
    assert_eq!(serde_json::from_slice::<Value>(&received).unwrap(),value);
}
