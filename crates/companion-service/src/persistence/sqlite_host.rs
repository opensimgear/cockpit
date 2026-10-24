//! One commit owns encrypted trust/assignments and immutable public references.
use super::*;
use crate::persistence::{HostSnapshot,HostSnapshotStore};
use osg_persistence::{Record,RevisionCheck};
use serde_json::Value;
use std::collections::BTreeSet;
use zeroize::Zeroizing;
pub(super) const CONTENT_BYTES:usize=80*1024*1024;
fn hashes(bytes:Option<&[u8]>)->Result<BTreeSet<String>,String>{
    let Some(bytes)=bytes else{return Ok(BTreeSet::new())};
    if bytes.len()>CONTENT_BYTES{return Err("Companion archive exceeds 80 MiB.".into());}
    let value:Value=serde_json::from_slice(bytes).map_err(|_|"Invalid companion archive.")?;
    let publications=value["publications"].as_array().filter(|items|items.len()<=32).ok_or("Invalid companion archive budget.")?;
    publications.iter().map(|item|item["publicationHash"].as_str().filter(|hash|crate::publication::digest_id(hash)).map(str::to_owned).ok_or_else(||"Invalid archived publication identity.".into())).collect()
}
fn collect_references(value:&Value,references:&mut BTreeSet<String>){
    match value {
        Value::String(value) if crate::publication::digest_id(value)=>{references.insert(value.clone());},
        Value::Array(values)=>for value in values{collect_references(value,references)},
        Value::Object(values)=>for value in values.values(){collect_references(value,references)},
        _=>{},
    }
}
fn profile_references(database:&Database)->Result<(u64,BTreeSet<String>),String>{
    let(revision,record)=database.read_with_revision("profiles","library")?;let mut references=BTreeSet::new();
    if let Some(record)=record {
        let value:Value=serde_json::from_slice(&record.payload).map_err(|_|"Cannot prune while saved profile references are invalid.")?;
        if !value["profiles"].is_array()||!value["localAssignments"].is_object(){return Err("Cannot prune while saved profile references are invalid.".into());}
        collect_references(&value,&mut references);
    }
    Ok((revision,references))
}
fn pair(database:&Database,key:&str)->Result<[(u64,Option<Record>);2],String>{
    database.read_many(&[("secrets",key),("companion","publications")])?.try_into().map_err(|_|"Invalid host snapshot size.".into())
}
impl HostSnapshotStore for SealedSqliteStore {
    fn retained_publications(&self)->Result<BTreeSet<String>,String>{
        match self.database(false)?{Some(database)=>Ok(profile_references(database)?.1),None=>Ok(BTreeSet::new())}
    }
    fn write_host_record(&self,state:&[u8])->Result<(),String>{
        if state.is_empty()||state.len()>STATE_BYTES{return Err("Protected state exceeds budget.".into());}
        let mut expected=self.0.revision.lock().map_err(|_|"Protected-store access failed.")?;
        let expected_content=self.0.content_revision.lock().map_err(|_|"Protected-store access failed.")?;
        let revision=expected.ok_or("Load the host snapshot before saving.")?;
        let content_revision=expected_content.ok_or("Load the host snapshot before saving.")?;
        let database=self.database(false)?.ok_or("Protected namespace is unavailable.")?;
        let(current,record)=database.read_with_revision("secrets",self.key()?)?;
        if current!=revision{return Err("Companion snapshot revision conflict. Reload before saving.".into());}
        let record=record.ok_or("Protected host identity is unavailable.")?;
        if record.payload.len()>STATE_BYTES{return Err("Protected SQLite state exceeds budget.".into());}
        let _plain=Zeroizing::new(self.0.sealer.unseal(&record.payload)?);
        let cipher=self.0.sealer.seal(state)?;
        if cipher.len()>STATE_BYTES{return Err("Protected SQLite state exceeds budget.".into());}
        // Unchanged media bytes are not fetched, encoded or rewritten. The
        // archive version still participates in this same writer transaction.
        let revisions=database.commit_checked(&[Change{domain:"secrets",key:self.key()?,expected_revision:revision,payload:Some(&cipher)}],
            &[RevisionCheck{domain:"companion",key:"publications",expected_revision:content_revision}])?;
        *expected=Some(revisions[0]);Ok(())
    }
    fn read_host_snapshot(&self)->Result<HostSnapshot,String>{
        let mut expected=self.0.revision.lock().map_err(|_|"Protected-store access failed.")?;
        let mut expected_content=self.0.content_revision.lock().map_err(|_|"Protected-store access failed.")?;
        let Some(database)=self.database(false)? else{*expected=Some(0);*expected_content=Some(0);return Ok(HostSnapshot{state:None,content:None})};
        let[(revision,secret),(content_revision,content)]=pair(database,self.key()?)?;
        if secret.is_none()&&content.is_some(){return Err("Companion archive has no protected host identity.".into());}
        let state=secret.map(|record|{
            if record.payload.len()>STATE_BYTES{return Err("Protected SQLite state exceeds budget.".into());}
            self.0.sealer.unseal(&record.payload).map(Zeroizing::new)
        }).transpose()?;
        if content.as_ref().is_some_and(|record|record.payload.len()>CONTENT_BYTES){return Err("Companion archive exceeds 80 MiB.".into());}
        *expected=Some(revision);*expected_content=Some(content_revision);
        Ok(HostSnapshot{state,content:content.map(|record|record.payload)})
    }
    fn write_host_snapshot(&self,state:&[u8],content:&[u8])->Result<(),String>{
        if state.is_empty()||state.len()>STATE_BYTES{return Err("Protected state exceeds budget.".into());}
        let next_hashes=hashes(Some(content))?;
        let mut expected=self.0.revision.lock().map_err(|_|"Protected-store access failed.")?;
        let mut expected_content=self.0.content_revision.lock().map_err(|_|"Protected-store access failed.")?;
        let database=self.database(true)?.ok_or("Protected namespace is unavailable.")?;
        let[(revision,secret),(content_revision,previous)]=pair(database,self.key()?)?;
        if secret.is_none()&&previous.is_some(){return Err("Companion archive has no protected host identity.".into());}
        if expected.is_some_and(|old|old!=revision)||expected_content.is_some_and(|old|old!=content_revision){return Err("Companion snapshot revision conflict. Reload before saving.".into());}
        if let Some(record)=secret {
            if record.payload.len()>STATE_BYTES{return Err("Protected SQLite state exceeds budget.".into());}
            let _plain=Zeroizing::new(self.0.sealer.unseal(&record.payload)?);
        }
        let removed=hashes(previous.as_ref().map(|record|record.payload.as_slice()))?.difference(&next_hashes).cloned().collect::<BTreeSet<_>>();
        // Read current SQL references, never a UI/manager cache. The read
        // dependency is rechecked atomically with archive removal below.
        let profile_revision=if removed.is_empty(){None}else{
            let(revision,references)=profile_references(database)?;
            if !references.is_disjoint(&removed){return Err("Saved profiles still reference this publication. Remove their references before pruning.".into());}
            Some(revision)
        };
        let cipher=self.0.sealer.seal(state)?;
        if cipher.len()>STATE_BYTES{return Err("Protected SQLite state exceeds budget.".into());}
        let checks=profile_revision.map(|expected_revision|RevisionCheck{domain:"profiles",key:"library",expected_revision}).into_iter().collect::<Vec<_>>();
        let revisions=database.commit_checked(&[
            Change{domain:"secrets",key:self.key()?,expected_revision:revision,payload:Some(&cipher)},
            Change{domain:"companion",key:"publications",expected_revision:content_revision,payload:Some(content)},
        ],&checks)?;
        *expected=Some(revisions[0]);*expected_content=Some(revisions[1]);Ok(())
    }
}
#[cfg(test)]
#[path="sqlite_host_tests.rs"]
mod tests;
