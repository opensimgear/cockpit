//! Native companion public-content storage; credentials stay in sealed records.
use osg_persistence::{Change,Database};
use serde::{Deserialize,Serialize};
use serde_json::Value;
use std::{collections::BTreeSet,path::Path};

pub const CACHE_BYTES:usize=24*1024*1024;
const ENTRY_BYTES:usize=13*1024*1024;
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Entry {
    host_id:String,
    instance_id:String,
    name:String,
    key:String,
    publication:Value,
    saved_at_ms:u64,
    page_id:String,
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Content {entries:Vec<Entry>,selected_key:String}
#[derive(Deserialize)]
#[serde(tag="op",rename_all="snake_case",deny_unknown_fields)]
pub enum CacheRequest {
    Read {},
    Write {#[serde(rename="expectedRevision")]expected_revision:u64,payload:String},
}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct Snapshot {
    pub revision:u64,
    pub payload:Option<String>,
    pub error:Option<String>,
}
pub struct ClientCache {database:Database}
fn label(value:&str,limit:usize)->bool{
    !value.is_empty()&&value.len()<=limit&&!value.chars().any(char::is_control)
}
fn validate(raw:&[u8])->Result<(),String>{
    if raw.len()>CACHE_BYTES{return Err("Offline cache exceeds 24 MiB.".into());}
    let content:Content=serde_json::from_slice(raw).map_err(|_|"Invalid offline cache document.")?;
    if content.entries.len()>4{return Err("Offline cache exceeds four panels.".into());}
    let mut seen=BTreeSet::new();
    for entry in &content.entries {
        if !label(&entry.host_id,64)||!label(&entry.instance_id,64)||!label(&entry.name,80)
            ||entry.saved_at_ms>9_007_199_254_740_991
            ||entry.key!=serde_json::to_string(&[&entry.host_id,&entry.instance_id]).unwrap()
            ||!seen.insert(&entry.key) {
            return Err("Invalid offline panel identity.".into());
        }
        let size=serde_json::to_vec(&serde_json::json!({"hostId":entry.host_id,"instanceId":entry.instance_id,"name":entry.name,"key":entry.key,"publication":entry.publication,"savedAtMs":entry.saved_at_ms,"pageId":entry.page_id})).map_err(|_|"Invalid offline panel.")?.len();
        if size>ENTRY_BYTES{return Err("Offline panel exceeds 13 MiB.".into());}
        crate::client::cache_validation::validate(&entry.publication)?;
        let pages=entry.publication["document"]["ui"]["pages"].as_array().filter(|pages|!pages.is_empty()&&pages.len()<=32).ok_or("Invalid offline pages.")?;
        let mut ids=BTreeSet::new();
        for page in pages {
            let id=page["id"].as_str().filter(|id|label(id,64)).ok_or("Invalid offline page identity.")?;
            if !ids.insert(id){return Err("Duplicate offline page.".into());}
        }
        if !ids.contains(entry.page_id.as_str()){return Err("Saved offline page is unavailable.".into());}
    }
    if !content.selected_key.is_empty()&&!seen.contains(&content.selected_key){return Err("Selected offline panel is unavailable.".into());}
    Ok(())
}
impl ClientCache {
    pub fn open(directory:&Path)->Result<Self,String>{
        Ok(Self{database:Database::for_namespace(&directory.join("companion-publications"))?})
    }
    pub fn read(&self)->Result<Snapshot,String>{
        let(revision,record)=self.database.read_with_revision("companion-client","publications")?;
        match record {
            None=>Ok(Snapshot{revision,payload:None,error:None}),
            Some(record)=>match validate(&record.payload) {
                Ok(())=>Ok(Snapshot{revision,payload:Some(String::from_utf8(record.payload).map_err(|_|"Invalid offline encoding.")?),error:None}),
                Err(error)=>Ok(Snapshot{revision,payload:None,error:Some(error)}),
            },
        }
    }
    pub fn write(&self,expected_revision:u64,payload:&str)->Result<u64,String>{
        validate(payload.as_bytes())?;
        Ok(self.database.commit(&[Change{domain:"companion-client",key:"publications",expected_revision,payload:Some(payload.as_bytes())}])?[0])
    }
    pub fn execute(&self,request:CacheRequest)->Result<Snapshot,String>{
        match request {
            CacheRequest::Read{}=>self.read(),
            CacheRequest::Write{expected_revision,payload}=>Ok(Snapshot{revision:self.write(expected_revision,&payload)?,payload:None,error:None}),
        }
    }
}
#[cfg(test)]
mod tests;
