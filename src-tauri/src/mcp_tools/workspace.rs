//! Bounded authenticated transfer of designer workspaces, with final native validation.
use super::*;
use base64::{engine::general_purpose::STANDARD,Engine};
const LIMIT:usize=64*1024*1024;
const CHUNK:usize=48*1024;
pub(super) struct Upload {id:String,client:String,revision:u64,library_revision:u64,expected:usize,hash:String,bytes:Vec<u8>,created:Instant}
impl Tools {
    pub(super) fn workspace_tool(&self,name:&str,args:&Value,client:&str)->Result<Value,Value> {
        let fail=|message:&str|failure("invalid_workspace_transfer",message,false);
        if name=="designer_workspace_read" {
            let state=self.native("designer_workspace_read",json!({}),client,Some("designerWorkspace"))?;
            let bytes=serde_json::to_vec(&state["value"]).map_err(|_|fail("Cannot encode designer workspace."))?;
            let hash=format!("sha256:{:x}",Sha256::digest(&bytes));
            if args.get("expectedHash").is_some_and(|expected|expected.as_str()!=Some(hash.as_str())) {return Err(failure("revision_conflict","Designer contents changed during transfer. Restart the read.",false));}
            let offset=args["offset"].as_u64().unwrap_or(0) as usize;
            if offset>bytes.len(){return Err(fail("Workspace offset is past the end."));}
            let end=(offset+CHUNK).min(bytes.len());
            return Ok(json!({"revision":state["revision"],"libraryRevision":state["libraryRevision"],"workspaceHash":hash,"totalBytes":bytes.len(),"offset":offset,"base64":STANDARD.encode(&bytes[offset..end]),"nextOffset":if end<bytes.len(){Some(end)}else{None}}));
        }
        let mut uploads=self.workspace_uploads.lock().unwrap_or_else(|p|p.into_inner());
        uploads.retain(|u|u.created.elapsed()<Duration::from_secs(600));
        if name=="designer_workspace_begin" {
            let expected=args["totalBytes"].as_u64().ok_or_else(||fail("Provide totalBytes."))? as usize;
            if uploads.iter().any(|u|u.client==client) || uploads.len()>=4 || uploads.iter().map(|u|u.expected).sum::<usize>()+expected>LIMIT {return Err(fail("Workspace transfer capacity is occupied. Abort an earlier upload or retry after it expires."));}
            let id=uuid::Uuid::new_v4().to_string();
            uploads.push(Upload{id:id.clone(),client:client.into(),revision:args["expectedRevision"].as_u64().ok_or_else(||fail("Provide expectedRevision."))?,library_revision:args["expectedLibraryRevision"].as_u64().ok_or_else(||fail("Provide expectedLibraryRevision."))?,expected,hash:args["workspaceHash"].as_str().unwrap().into(),bytes:vec![],created:Instant::now()});
            return Ok(json!({"uploadId":id,"chunkBytes":CHUNK,"expiresInSeconds":600}));
        }
        let index=uploads.iter().position(|u|u.client==client && Some(u.id.as_str())==args["uploadId"].as_str()).ok_or_else(||fail("Workspace upload expired or belongs to another client."))?;
        if name=="designer_workspace_abort" {uploads.remove(index);return Ok(json!({"aborted":true}));}
        let upload=&mut uploads[index];
        if name=="designer_workspace_chunk" {
            let bytes=STANDARD.decode(args["base64"].as_str().unwrap()).map_err(|_|fail("Invalid workspace base64."))?;
            if bytes.is_empty() || bytes.len()>CHUNK || args["offset"].as_u64()!=Some(upload.bytes.len() as u64) || upload.bytes.len()+bytes.len()>upload.expected {return Err(fail("Workspace chunk offset or length mismatch."));}
            upload.bytes.extend_from_slice(&bytes);
            return Ok(json!({"received":upload.bytes.len()}));
        }
        if upload.bytes.len()!=upload.expected || format!("sha256:{:x}",Sha256::digest(&upload.bytes))!=upload.hash {return Err(fail("Workspace upload is incomplete or its content hash differs."));}
        let value:Value=serde_json::from_slice(&upload.bytes).map_err(|_|fail("Invalid workspace JSON."))?;
        let saved=self.native("designer_workspace_write",json!({"expectedRevision":upload.revision,"expectedLibraryRevision":upload.library_revision,"value":value}),client,Some("designerWorkspace"))?;
        // Failed native commits keep the upload for an explicit retry. Successful
        // responses contain only the acknowledgement, never a 64 MiB MCP reply.
        uploads.remove(index);
        Ok(json!({"saved":true,"revision":saved["revision"],"libraryRevision":saved["libraryRevision"]}))
    }
}
