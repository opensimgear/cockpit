//! Public archives and explicit saved heads share the protected host commit.
use super::*;
mod selection;
pub(super) const CONTENT_BYTES:usize=80*1024*1024;
#[derive(Default,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Content {publications:Vec<PublicationUpload>,saved_heads:BTreeMap<String,String>}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct SavedPanel {pub panel_id:String,pub publication_hash:String,pub previous_hashes:Vec<String>}
pub(super) fn load(storage:&dyn StateStorage)->Result<crate::persistence::HostSnapshot,String>{
    match storage.host_snapshot_store(){
        Some(store)=>store.read_host_snapshot(),
        None=>Ok(crate::persistence::HostSnapshot{state:storage.read()?.map(Zeroizing::new),content:None}),
    }
}
pub(super) fn decode(bytes:Option<&[u8]>)->Result<(BTreeMap<String,Publication>,BTreeMap<String,String>),String>{
    let Some(bytes)=bytes else{return Ok(Default::default())};
    ensure(bytes.len()<=CONTENT_BYTES,"Saved panels exceed 80 MiB.")?;
    let content:Content=serde_json::from_slice(bytes).map_err(|_|"Saved companion content is invalid. Preserve its SQLite record.")?;
    ensure(content.publications.len()<=32&&content.saved_heads.len()<=32,"Saved panel budget exceeded.")?;
    let mut publications=BTreeMap::new();let mut total=0;
    for upload in content.publications {
        let publication=Publication::validate(upload)?;total+=publication.bytes();
        ensure(total<=64*1024*1024,"Retained publication bytes exceed 64 MiB.")?;
        ensure(publications.insert(publication.publication_hash.clone(),publication).is_none(),"Duplicate archived publication.")?;
    }
    for (id,hash) in &content.saved_heads {
        ensure(crate::publication::id(id)&&publications.get(hash).is_some_and(|publication|publication.ui_id==*id),"Saved panel head has no matching immutable publication.")?;
    }
    Ok((publications,content.saved_heads))
}
fn uploads(inner:&Inner)->Vec<PublicationUpload>{
    inner.publications.values().map(|p|PublicationUpload{canonical_document:p.canonical_document.clone(),publication_hash:p.publication_hash.clone(),capability_fingerprint:Some(p.capability_fingerprint.clone()),assets:p.assets.clone()}).collect()
}
impl Hub {
    /// Validate retained public content and issue an empty identity for an
    /// explicitly confirmed restore. The caller seals these bytes before SQL.
    /// No credential or assignment from the historical host is accepted.
    pub fn fresh_restore_record(content:&[u8])->Result<Zeroizing<Vec<u8>>,String>{
        decode(Some(content))?;
        let record=Record{host_id:uuid::Uuid::new_v4().to_string(),credentials:vec![],instances:vec![]};
        serde_json::to_vec(&record).map(Zeroizing::new).map_err(|_|"Cannot encode the fresh restored host identity.".into())
    }

    pub(super) fn content_bytes(inner:&Inner)->Result<Vec<u8>,String>{
        let bytes=serde_json::to_vec(&Content{publications:uploads(inner),saved_heads:inner.saved_heads.clone()}).map_err(|_|"Cannot encode companion content.")?;
        ensure(bytes.len()<=CONTENT_BYTES,"Saved panels exceed 80 MiB.")?;Ok(bytes)
    }
    fn saved_panels_inner(inner:&Inner)->Vec<SavedPanel>{
        let mut grouped=BTreeMap::<String,Vec<String>>::new();
        for publication in inner.publications.values(){grouped.entry(publication.ui_id.clone()).or_default().push(publication.publication_hash.clone());}
        grouped.into_iter().map(|(panel_id,previous_hashes)|{
            let publication_hash=inner.saved_heads.get(&panel_id).cloned().unwrap_or_else(||previous_hashes.last().unwrap().clone());
            SavedPanel{panel_id,publication_hash,previous_hashes}
        }).collect()
    }
    pub fn saved_panels(&self)->Vec<SavedPanel>{Self::saved_panels_inner(&self.inner.lock().unwrap_or_else(|p|p.into_inner()))}
    pub fn saved_heads(&self)->BTreeMap<String,String>{self.inner.lock().unwrap_or_else(|p|p.into_inner()).saved_heads.clone()}
    pub fn saved_panel_references(&self)->Vec<SavedPanel>{
        let inner=self.inner.lock().unwrap_or_else(|p|p.into_inner());
        Self::saved_panels_inner(&inner).into_iter().filter(|panel|inner.saved_heads.get(&panel.panel_id)==Some(&panel.publication_hash)).collect()
    }
    pub fn mark_saved(&self,hash:&str)->Result<SavedPanel,String>{
        let mut inner=self.inner.lock().unwrap_or_else(|p|p.into_inner());
        let publication=inner.publications.get(hash).ok_or("The saved panel was not published.")?.clone();
        self.publish_content(&mut inner,publication,true)?;
        Self::saved_panels_inner(&inner).into_iter().find(|panel|panel.publication_hash==hash).ok_or_else(||"The saved panel disappeared.".into())
    }
    pub(super) fn publish_content(&self,inner:&mut Inner,publication:Publication,save:bool)->Result<Value,String>{
        let hash=publication.publication_hash.clone();let id=publication.ui_id.clone();
        let old_publication=inner.publications.get(&hash).cloned();let old_heads=inner.saved_heads.clone();
        let mut result=Self::retain_publication(inner,publication)?;
        if save {inner.saved_heads.insert(id,hash.clone());}
        if let Err(error)=self.persist_content(&inner.record,inner) {
            match old_publication {Some(old)=>{inner.publications.insert(hash,old);},None=>{inner.publications.remove(&hash);}}
            inner.saved_heads=old_heads;return Err(error);
        }
        if save {result["panelSaved"]=serde_json::to_value(Self::saved_panels_inner(inner).into_iter().find(|panel|panel.publication_hash==hash).ok_or("Saved panel disappeared.")?).map_err(|_|"Cannot encode saved panel.")?;}
        Ok(result)
    }
    pub(super) fn prune_content(&self,inner:&mut Inner,request:&Value)->Result<Value,String>{
        let hashes=request["hashes"].as_array().filter(|a|a.len()<=100).ok_or("Invalid panel retention list.")?;
        let ids=request["panelIds"].as_array().filter(|a|a.len()<=128).ok_or("Invalid panel identity list.")?;
        let mut managed=BTreeSet::new();for value in ids {managed.insert(value.as_str().filter(|s|id(s)).ok_or("Invalid panel identity.")?.to_owned());}
        let mut retained=BTreeSet::new();for value in hashes {retained.insert(value.as_str().filter(|s|digest_id(s)).ok_or("Invalid panel retention hash.")?.to_owned());}
        retained.extend(inner.record.instances.iter().map(|a|a.publication_hash.clone()));
        retained.extend(inner.record.instances.iter().filter_map(|a|a.ownership.as_ref().map(|owner|owner.baseline_publication_hash.clone())));
        retained.extend(inner.sessions.values().map(|s|s.token.publication_hash.clone()));
        if let Some(store)=self.storage.as_ref().and_then(|storage|storage.host_snapshot_store()){retained.extend(store.retained_publications()?);}
        let remove=inner.publications.iter().filter(|(hash,p)|managed.contains(&p.ui_id)&&!retained.contains(*hash)).map(|(hash,_)|hash.clone()).collect::<Vec<_>>();
        if remove.is_empty(){return Ok(json!({"removed":0}));}
        let old_heads=inner.saved_heads.clone();let mut removed=BTreeMap::new();
        for hash in remove {removed.insert(hash.clone(),inner.publications.remove(&hash).unwrap());}
        inner.saved_heads.retain(|_,hash|!removed.contains_key(hash));
        if let Err(error)=self.persist_content(&inner.record,inner){inner.publications.append(&mut removed);inner.saved_heads=old_heads;return Err(error);}
        Ok(json!({"removed":removed.len()}))
    }
}
#[cfg(test)]
mod tests;
#[cfg(test)]
mod selection_tests;
