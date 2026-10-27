//! Bounded on-disk provider cache. Local query keys never become file names.
use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, io::{Read, Write}, path::{Component, Path, PathBuf}, sync::{Mutex, OnceLock}};

const MAX_BYTES:u64=64*1024;
const MAX_FILES:usize=1024;
const POSITIVE_TTL:u64=30*24*60*60;
const NEGATIVE_TTL:u64=24*60*60;

#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Cached {fingerprint:String,lookup:MetadataLookup}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Cooldown {until:u64}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceIndex {key_hash:String,external_ids:Vec<ExternalId>,fingerprint:String,lookup:MetadataLookup}

fn key_hash(query:&MetadataQuery)->String {format!("{:x}",Sha256::digest(query.key.as_bytes()))}
fn external_ids(query:&MetadataQuery)->Vec<ExternalId> {
    let mut ids=query.external_ids.clone();
    ids.sort_by(|left,right|(&left.platform,&left.id).cmp(&(&right.platform,&right.id)));ids.dedup();ids
}

/// Read-only consumers can retain a source's last positive facts after a local
/// display-name edit. Discovery must use `read`, which verifies the full query.
pub(super) fn read_source(root:&Path,query:&MetadataQuery,now:u64)->Option<MetadataLookup> {
    if !query.valid() {return None;}
    let directory=directory(root,false)?;let key_hash=key_hash(query);
    let bytes=read_file(&directory.join(format!("source-{key_hash}.json")),MAX_BYTES)?;
    let mut index:SourceIndex=serde_json::from_slice(&bytes).ok()?;
    if index.key_hash!=key_hash||index.external_ids!=external_ids(query)||index.fingerprint.len()!=64
        ||!index.fingerprint.bytes().all(|byte|byte.is_ascii_hexdigit()) {return None;}
    if !index.lookup.valid()||index.lookup.stale||index.lookup.status!=LookupStatus::Matched
        ||index.lookup.checked_at>now {return None;}
    index.lookup.stale=now.saturating_sub(index.lookup.checked_at)>=POSITIVE_TTL;
    Some(index.lookup)
}

pub(super) fn fingerprint(query:&MetadataQuery)->String {
    let mut ids=query.external_ids.iter().map(|id|(&id.platform,&id.id)).collect::<Vec<_>>();
    ids.sort();ids.dedup();
    let value=serde_json::to_vec(&(normalized_title(&query.title),ids)).unwrap_or_default();
    format!("{:x}",Sha256::digest(value))
}

pub(super) fn read(root:&Path,query:&MetadataQuery,now:u64,allow_stale:bool)->Option<MetadataLookup> {
    if !query.valid() {return None;}
    let directory=directory(root,false)?;
    let fingerprint=fingerprint(query);
    let bytes=read_file(&directory.join(format!("{fingerprint}.json")),MAX_BYTES)?;
    let mut cached:Cached=serde_json::from_slice(&bytes).ok()?;
    if cached.fingerprint!=fingerprint||!cached.lookup.valid()||cached.lookup.stale||cached.lookup.checked_at>now {return None;}
    let ttl=match cached.lookup.status {
        LookupStatus::Matched=>POSITIVE_TTL,
        LookupStatus::NotFound|LookupStatus::Ambiguous=>NEGATIVE_TTL,
        LookupStatus::Unavailable=>return None,
    };
    if now.saturating_sub(cached.lookup.checked_at)>=ttl {
        if !allow_stale||cached.lookup.status!=LookupStatus::Matched {return None;}
        cached.lookup.stale=true;
    }
    Some(cached.lookup)
}

pub(super) fn write(root:&Path,query:&MetadataQuery,lookup:&MetadataLookup)->bool {
    if !query.valid()||!lookup.valid()||lookup.stale||lookup.status==LookupStatus::Unavailable {return false;}
    let Some(directory)=directory(root,true) else{return false;};
    let fingerprint=fingerprint(query);
    let Ok(bytes)=serde_json::to_vec(&Cached{fingerprint:fingerprint.clone(),lookup:lookup.clone()}) else{return false;};
    if !atomic(&directory,&format!("{fingerprint}.json"),&bytes,MAX_BYTES) {return false;}
    if lookup.status==LookupStatus::Matched {
        let key_hash=key_hash(query);
        if let Ok(bytes)=serde_json::to_vec(&SourceIndex{key_hash:key_hash.clone(),external_ids:external_ids(query),fingerprint,lookup:lookup.clone()}) {
            let _=atomic(&directory,&format!("source-{key_hash}.json"),&bytes,MAX_BYTES);
        }
    }
    true
}

pub(super) fn cooldown(root:&Path,now:u64)->u64 {
    let Some(directory)=directory(root,false) else{return 0;};
    let Some(bytes)=read_file(&directory.join("cooldown.json"),1024) else{return 0;};
    serde_json::from_slice::<Cooldown>(&bytes).ok().map(|value|value.until)
        .filter(|until|*until>now).unwrap_or(0)
}

pub(super) fn save_cooldown(root:&Path,until:u64) {
    let Some(directory)=directory(root,true) else{return;};
    let Ok(bytes)=serde_json::to_vec(&Cooldown{until}) else{return;};
    let _=atomic(&directory,"cooldown.json",&bytes,1024);
}

fn directory(root:&Path,create:bool)->Option<PathBuf> {
    if !root.is_absolute()||root.components().any(|part|matches!(part,Component::ParentDir))||!safe_directory(root) {return None;}
    let path=root.join("game-metadata");
    if create&&!path.exists() {match fs::create_dir(&path) {Ok(())=>{},Err(error) if error.kind()==std::io::ErrorKind::AlreadyExists=>{},Err(_)=>return None}}
    safe_directory(&path).then_some(path)
}

fn safe_directory(path:&Path)->bool {
    let mut current=PathBuf::new();
    for part in path.components() {
        current.push(part.as_os_str());
        if matches!(part,Component::Prefix(_)) {continue;}
        match fs::symlink_metadata(&current) {Ok(metadata) if metadata.is_dir()&&!redirected(&metadata)=>{},_=>return false}
    }
    true
}

fn redirected(metadata:&fs::Metadata)->bool {
    if metadata.file_type().is_symlink() {return true;}
    #[cfg(windows)] {use std::os::windows::fs::MetadataExt;if metadata.file_attributes()&0x400!=0 {return true;}}
    false
}

fn read_file(path:&Path,limit:u64)->Option<Vec<u8>> {
    let metadata=fs::symlink_metadata(path).ok()?;
    if redirected(&metadata)||!metadata.is_file()||metadata.len()>limit {return None;}
    let mut bytes=Vec::new();
    fs::File::open(path).ok()?.take(limit+1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64<=limit).then_some(bytes)
}

fn atomic(directory:&Path,name:&str,bytes:&[u8],limit:u64)->bool {
    if bytes.len() as u64>limit||!safe_directory(directory) {return false;}
    static WRITE:OnceLock<Mutex<()>>=OnceLock::new();
    let _guard=WRITE.get_or_init(||Mutex::new(())).lock().unwrap_or_else(|p|p.into_inner());
    let path=directory.join(name);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file()&&!redirected(&metadata)=>{},
        Err(error) if error.kind()==std::io::ErrorKind::NotFound=> {
            let Ok(entries)=fs::read_dir(directory) else{return false;};
            let mut count=0;
            for entry in entries.take(MAX_FILES+1) {if entry.is_err(){return false;}count+=1;}
            // Keep one slot for provider cooldown even when records fill up.
            let capacity=if name=="cooldown.json" {MAX_FILES}else{MAX_FILES-1};
            if count>=capacity {return false;}
        },_=>return false,
    }
    let Ok(mut temporary)=tempfile::NamedTempFile::new_in(directory) else{return false;};
    if temporary.write_all(bytes).is_err() {return false;}
    temporary.persist(path).is_ok()
}

#[cfg(test)]
#[path="cache_tests.rs"]
mod tests;
