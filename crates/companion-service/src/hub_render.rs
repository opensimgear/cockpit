//! Host render selection, restart restoration and bounded cache collection.
//! All file I/O, base64 conversion and content hashing run outside the input
//! mutex. Disk identities remain hints until revalidated against native source.
use super::*;
use crate::render_packages::{RenderDiskCache,RenderPackage,RenderPackageUpload,RenderSelections,CACHE_BYTES};

struct CachePlan {
    packages:BTreeMap<String,Arc<RenderPackage>>,
    selected:RenderSelections,
    removed:Vec<String>,
}
fn plan(inner:&Inner,package:&Arc<RenderPackage>,inventory:&BTreeMap<String,usize>)->Result<CachePlan,String>{
    let mut packages=inner.render_packages.clone();packages.insert(package.package_hash.clone(),package.clone());
    let mut selected=inner.render_selected.clone();
    selected.insert((package.manifest.source_publication_hash.clone(),package.manifest.target_profile_hash.clone()),package.package_hash.clone());
    let mut files=inventory.clone();files.insert(package.package_hash.clone(),package.cache_bytes);
    let original=files.keys().chain(packages.keys()).cloned().collect::<BTreeSet<_>>();
    let mut protected=inner.sessions.values().filter_map(|s|s.render_package.as_ref().map(|(p,_)|p.clone())).collect::<BTreeSet<_>>();
    protected.extend(inner.render_prepared.values().filter(|p|p.expires>inner.last_now).map(|p|p.package.clone()));
    protected.insert(package.package_hash.clone());
    let assigned=inner.record.instances.iter().map(|a|a.publication_hash.as_str()).collect::<BTreeSet<_>>();
    let selected_hashes=selected.values().cloned().collect::<BTreeSet<_>>();
    packages.retain(|h,_|selected_hashes.contains(h)||protected.contains(h));
    files.retain(|h,_|selected_hashes.contains(h)||protected.contains(h));
    loop{
        let memory=packages.values().map(|p|p.bytes as u64).sum::<u64>();
        let disk=files.values().map(|b|*b as u64).sum::<u64>();
        if memory<=CACHE_BYTES as u64&&disk<=(CACHE_BYTES-32768) as u64&&selected.len()<=64{break;}
        let key=selected.iter().find(|((source,_),hash)|!assigned.contains(source.as_str())&&!protected.contains(*hash)).map(|(k,_)|k.clone()).ok_or("Render cache is full of assigned, active or prepared content.")?;
        let removed=selected.remove(&key).unwrap();
        if !selected.values().any(|h|h==&removed){packages.remove(&removed);files.remove(&removed);}
    }
    let retained=files.keys().chain(packages.keys()).cloned().collect::<BTreeSet<_>>();
    Ok(CachePlan{packages,selected,removed:original.difference(&retained).cloned().collect()})
}
impl Hub {
    /// Explicit opt-in for tests/embedders. The normal persistent Hub::open
    /// attaches this store automatically; new/with_storage do no filesystem I/O.
    pub fn with_render_cache(mut self,path:impl AsRef<Path>)->Result<Self,String>{
        ensure(Arc::strong_count(&self.inner)==1,"Attach render storage before cloning the hub.")?;
        let cache=Arc::new(RenderDiskCache::new(path));
        let selected=cache.selections();
        let mut inner=self.inner.lock().unwrap_or_else(|p|p.into_inner());
        ensure(inner.render_packages.is_empty()&&inner.render_prepared.is_empty()&&inner.sessions.is_empty(),"Attach render storage before rendering.")?;
        match selected{
            Ok(selected)=>inner.render_selected=selected,
            Err(error)=>{inner.render_selected.clear();inner.diagnostic=Some(format!("Render cache index ignored: {error}"));}
        }
        drop(inner);self.render_cache=Some(cache);Ok(self)
    }
    /// Native publication boundary. Recipes never supply source authority.
    pub fn install_render_package(&self,upload:RenderPackageUpload)->Result<String,String>{
        let _serial=self.render_io.lock().unwrap_or_else(|p|p.into_inner());
        ensure(upload.manifest.len()<=8192,"Render manifest byte budget.")?;
        let source_hash=serde_json::from_str::<Value>(&upload.manifest).ok().and_then(|v|v["sourcePublicationHash"].as_str().map(str::to_owned)).ok_or("Missing render source identity.")?;
        let source=self.inner.lock().unwrap_or_else(|p|p.into_inner()).publications.get(&source_hash).cloned().ok_or("Publish and validate the immutable source first.")?;
        let package=Arc::new(RenderPackage::validate(upload,&source)?);
        let inventory=match &self.render_cache{Some(cache)=>cache.inventory()?,None=>BTreeMap::new()};
        let previous=self.inner.lock().unwrap_or_else(|p|p.into_inner()).render_selected.clone();
        let mut staged=plan(&self.inner.lock().unwrap_or_else(|p|p.into_inner()),&package,&inventory)?;
        let result=(||{
            if let Some(cache)=&self.render_cache{cache.save(&package)?;}
            // Input/prepare state may change while content is written. Recheck
            // protected references before committing; never delete a new grant.
            for _ in 0..4{
                if let Some(cache)=&self.render_cache{cache.save_selections(&staged.selected)?;}
                let mut inner=self.inner.lock().unwrap_or_else(|p|p.into_inner());
                ensure(inner.publications.contains_key(&source_hash),"Render source was removed during preparation.")?;
                let checked=plan(&inner,&package,&inventory)?;
                if checked.selected!=staged.selected{drop(inner);staged=checked;continue;}
                let selection=(source_hash.clone(),package.manifest.target_profile_hash.clone());
                if inner.render_selected.get(&selection).is_some_and(|h|h!=&package.package_hash){
                    let sessions=inner.sessions.iter().filter(|(_,s)|s.token.publication_hash==source_hash&&s.render_package.as_ref().is_some_and(|(_,p)|p==&selection.1)).map(|(id,_)|id.clone()).collect::<Vec<_>>();
                    for id in sessions{self.disarm(&mut inner,&id,"render-package-replaced",true);}
                    inner.render_prepared.retain(|_,p|p.publication!=source_hash||p.profile!=selection.1);
                }
                inner.render_packages=checked.packages;inner.render_selected=checked.selected;
                return Ok(checked.removed);
            }
            Err("Render references kept changing during cache publication; retry.".into())
        })();
        let removed:Vec<String>=match result{
            Ok(v)=>v,
            Err(error)=>{
                if let Some(cache)=&self.render_cache{
                    let _=cache.save_selections(&previous);
                    if !inventory.contains_key(&package.package_hash){let _=cache.remove(&package.package_hash);}
                }
                return Err(error);
            }
        };
        if let Some(cache)=&self.render_cache{
            for hash in removed{if let Err(error)=cache.remove(&hash){self.inner.lock().unwrap_or_else(|p|p.into_inner()).diagnostic=Some(format!("Render cache eviction failed: {error}"));}}
        }
        Ok(package.package_hash.clone())
    }
    pub(super) fn load_cached_render(&self,source:&Publication)->Vec<Arc<RenderPackage>>{
        let Some(cache)=&self.render_cache else{return vec![]};
        let _serial=self.render_io.lock().unwrap_or_else(|p|p.into_inner());
        let candidates=self.inner.lock().unwrap_or_else(|p|p.into_inner()).render_selected.iter().filter(|((hash,_),_)|hash==&source.publication_hash).map(|((_,profile),package)|(profile.clone(),package.clone())).collect::<Vec<_>>();
        let mut result=Vec::new();
        for(profile,hash)in candidates{
            match cache.load(&hash,source){
                Ok(package) if package.manifest.target_profile_hash==profile=>result.push(Arc::new(package)),
                _=>{self.inner.lock().unwrap_or_else(|p|p.into_inner()).diagnostic=Some("Cached render content failed validation; republish its compiled package.".into());}
            }
        }result
    }
    pub(super) fn attach_cached_render(inner:&mut Inner,packages:Vec<Arc<RenderPackage>>){
        for package in packages{
            let key=(package.manifest.source_publication_hash.clone(),package.manifest.target_profile_hash.clone());
            if inner.render_selected.get(&key)!=Some(&package.package_hash){continue;}
            let prior=inner.render_packages.get(&package.package_hash).map_or(0,|p|p.bytes);
            if inner.render_packages.values().map(|p|p.bytes).sum::<usize>()-prior+package.bytes>CACHE_BYTES{
                inner.diagnostic=Some("Cached render content exceeds current native memory budget.".into());continue;
            }
            inner.render_packages.insert(package.package_hash.clone(),package);
        }
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    use crate::persistence::StateStorage;
    use base64::{engine::general_purpose::STANDARD,Engine};
    use std::fs;
    #[derive(Default)]struct Memory(Mutex<Option<Vec<u8>>>);
    impl StateStorage for Memory{
        fn read(&self)->Result<Option<Vec<u8>>,String>{Ok(self.0.lock().unwrap().clone())}
        fn write(&self,bytes:&[u8])->Result<(),String>{*self.0.lock().unwrap()=Some(bytes.to_vec());Ok(())}
    }
    fn fixture(board:&str)->(Publication,RenderPackageUpload){
        let dir=Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/companion-renderer/test/poc").join(board).join("A");
        let source:Value=serde_json::from_slice(&fs::read(dir.join("source-publication.json")).unwrap()).unwrap();
        let source=Publication::validate(PublicationUpload{canonical_document:String::from_utf8(serde_json_canonicalizer::to_vec(&source["document"]).unwrap()).unwrap(),publication_hash:source["publicationHash"].as_str().unwrap().into(),capability_fingerprint:Some(source["capabilityFingerprint"].as_str().unwrap().into()),assets:vec![]}).unwrap();
        let manifest=fs::read_to_string(dir.join("manifest.json")).unwrap();
        (source,RenderPackageUpload{package_hash:format!("sha256:{}",hash(manifest.as_bytes())),manifest,objects:crate::render_packages::OBJECT_KINDS.into_iter().map(|k|(k.into(),STANDARD.encode(fs::read(dir.join(format!("{k}.bin"))).unwrap()))).collect()})
    }
    fn mutate(upload:&mut RenderPackageUpload,f:impl FnOnce(&mut Value)){
        let mut manifest:Value=serde_json::from_str(&upload.manifest).unwrap();f(&mut manifest);
        upload.manifest=String::from_utf8(serde_json_canonicalizer::to_vec(&manifest).unwrap()).unwrap();upload.package_hash=format!("sha256:{}",hash(upload.manifest.as_bytes()));
    }
    fn publish(h:&Hub,source:&Publication){
        h.command(&json!({"command":"companion_publish","canonicalDocument":source.canonical_document,"publicationHash":source.publication_hash,"capabilityFingerprint":source.capability_fingerprint,"assets":[]})).unwrap();
    }
    fn wire(h:&Hub,connection:&str,v:Value)->Value{h.handle_wire(v,connection,h.now_ms())}
    fn pair(h:&Hub)->String{
        let offer=h.command(&json!({"command":"companion_pair_offer"})).unwrap();
        let pending=wire(h,"pair",json!({"type":"pair","secret":offer["secret"],"companionId":"board","name":"Board"}));
        h.command(&json!({"command":"companion_pair_approve","requestId":pending["requestId"],"companionId":"board"})).unwrap();
        let paired=wire(h,"pair",json!({"type":"pair-status","secret":offer["secret"],"requestId":pending["requestId"]}));
        paired["token"].as_str().unwrap().into()
    }
    fn prepare(h:&Hub,source:&Publication,upload:&RenderPackageUpload)->Value{
        let m:Value=serde_json::from_str(&upload.manifest).unwrap();
        wire(h,"usb",json!({"type":"render-prepare","version":2,"instanceId":"panel","publicationHash":source.publication_hash,"profileId":m["targetProfileId"],"profileHash":m["targetProfileHash"],"cachedPackageHash":upload.package_hash}))
    }
    #[test]fn restart_restores_only_after_native_source_validation_and_keeps_input_closed(){
        let dir=tempfile::tempdir().unwrap();let storage=Arc::new(Memory::default());
        let h=Hub::with_storage(storage.clone(),Arc::new(NullSourceSink)).unwrap().with_render_cache(dir.path()).unwrap();
        let token=pair(&h);let(source,upload)=fixture("sunton");publish(&h,&source);
        h.command(&json!({"command":"companion_assign","companionId":"board","instanceId":"panel","publicationHash":source.publication_hash})).unwrap();
        h.install_render_package(upload.clone()).unwrap();assert!(dir.path().join("selected.json").is_file());drop(h);
        let h=Hub::with_storage(storage,Arc::new(NullSourceSink)).unwrap().with_render_cache(dir.path()).unwrap();
        assert_eq!(wire(&h,"usb",json!({"type":"auth","token":token}))["ok"],true);
        assert_eq!(prepare(&h,&source,&upload)["ok"],false);
        publish(&h,&source);
        let p=prepare(&h,&source,&upload);assert_eq!(p["ok"],true,"{p}");assert_eq!(p["cached"],true);
        let read=wire(&h,"usb",json!({"type":"render-read","version":2,"preparedId":p["preparedId"],"packageHash":p["packageHash"],"kind":"scene","offset":0,"bytes":2048}));
        assert_eq!(read["ok"],true);assert_eq!(read["chunk"]["totalBytes"],STANDARD.decode(&upload.objects["scene"]).unwrap().len());
        let inner=h.inner.lock().unwrap();assert!(inner.sessions.is_empty());assert_eq!(inner.render_packages.len(),1);
        assert_eq!(inner.render_packages[&upload.package_hash].source_document["schemaVersion"],2);
    }
    #[test]fn corrupt_content_and_selection_index_are_rebuildable_without_authority(){
        let dir=tempfile::tempdir().unwrap();let(source,upload)=fixture("sunton");
        let h=Hub::new("host".into(),Arc::new(NullSourceSink)).with_render_cache(dir.path()).unwrap();publish(&h,&source);h.install_render_package(upload.clone()).unwrap();drop(h);
        let path=dir.path().join(format!("{}.json",&upload.package_hash[7..]));fs::write(&path,b"{}").unwrap();
        let h=Hub::new("host".into(),Arc::new(NullSourceSink)).with_render_cache(dir.path()).unwrap();publish(&h,&source);
        assert!(h.inner.lock().unwrap().render_packages.is_empty());assert!(h.inner.lock().unwrap().diagnostic.is_some());
        h.install_render_package(upload.clone()).unwrap();drop(h);
        let h=Hub::new("host".into(),Arc::new(NullSourceSink)).with_render_cache(dir.path()).unwrap();publish(&h,&source);assert_eq!(h.inner.lock().unwrap().render_packages.len(),1);drop(h);
        fs::write(dir.path().join("selected.json"),b"{\"version\":2,\"entries\":[]}").unwrap();
        let h=Hub::new("host".into(),Arc::new(NullSourceSink)).with_render_cache(dir.path()).unwrap();publish(&h,&source);
        assert!(h.inner.lock().unwrap().render_packages.is_empty());h.install_render_package(upload).unwrap();
        assert_eq!(RenderDiskCache::new(dir.path()).selections().unwrap().len(),1);
    }
    #[test]fn ordinary_recipe_revisions_collect_old_unselected_objects(){
        let dir=tempfile::tempdir().unwrap();let h=Hub::new("host".into(),Arc::new(NullSourceSink)).with_render_cache(dir.path()).unwrap();
        let(source,base)=fixture("sunton");publish(&h,&source);let mut last=String::new();
        // More than 16 MiB of distinct revisions cumulatively; only the latest
        // unreferenced immutable object set remains selected/cached.
        for n in 0..56{
            let mut upload=base.clone();mutate(&mut upload,|m|m["compilerVersion"]=json!(format!("test-{n}")));
            last=h.install_render_package(upload).unwrap();
            let inner=h.inner.lock().unwrap();assert_eq!(inner.render_packages.len(),1);assert_eq!(inner.render_selected.len(),1);
            assert!(inner.render_packages.values().map(|p|p.bytes).sum::<usize>()<CACHE_BYTES);
        }
        let inventory=RenderDiskCache::new(dir.path()).inventory().unwrap();assert_eq!(inventory.len(),1);assert!(inventory.contains_key(&last));
        assert!(inventory.values().sum::<usize>()+32768<CACHE_BYTES);
    }
    #[test]fn pressure_evicts_unassigned_selections_and_preserves_prepared_and_active_packages(){
        let dir=tempfile::tempdir().unwrap();let h=Hub::new("host".into(),Arc::new(NullSourceSink)).with_render_cache(dir.path()).unwrap();
        let token=pair(&h);let(source,upload)=fixture("sunton");publish(&h,&source);
        h.command(&json!({"command":"companion_assign","companionId":"board","instanceId":"panel","publicationHash":source.publication_hash})).unwrap();
        h.install_render_package(upload.clone()).unwrap();assert_eq!(wire(&h,"usb",json!({"type":"auth","token":token}))["ok"],true);
        let prepared=prepare(&h,&source,&upload);assert_eq!(prepared["ok"],true);
        let(wave,base)=fixture("waveshare");let mut installed=Vec::new();
        for n in 0..15{
            let mut document:Value=serde_json::from_str(&wave.canonical_document).unwrap();document["ui"]["title"]=json!(format!("Revision {n}"));
            let bytes=serde_json_canonicalizer::to_vec(&document).unwrap();
            let current=Publication::validate(PublicationUpload{canonical_document:String::from_utf8(bytes.clone()).unwrap(),publication_hash:format!("sha256:{}",hash(&bytes)),capability_fingerprint:Some(wave.capability_fingerprint.clone()),assets:vec![]}).unwrap();
            publish(&h,&current);let mut next=base.clone();mutate(&mut next,|m|m["sourcePublicationHash"]=json!(current.publication_hash));
            let hash=h.install_render_package(next).unwrap();installed.push(hash);
        }
        let cache=RenderDiskCache::new(dir.path());let inventory=cache.inventory().unwrap();assert!(inventory.values().sum::<usize>()+32768<=CACHE_BYTES);
        assert!(inventory.contains_key(&upload.package_hash));assert!(installed.iter().any(|hash|!inventory.contains_key(hash)));
        let inner=h.inner.lock().unwrap();assert!(inner.render_prepared.contains_key("usb"));assert!(inner.render_packages.contains_key(&upload.package_hash));drop(inner);
        let open=wire(&h,"usb",json!({"type":"render-open","version":2,"instanceId":"panel","publicationHash":source.publication_hash,"preparedId":prepared["preparedId"],"packageHash":prepared["packageHash"],"profileHash":prepared["profileHash"],"semanticDescriptorHash":prepared["semanticDescriptorHash"]}));
        assert_eq!(open["ok"],true,"{open}");
        // Force a synthetic pressure plan while this package is an active
        // writer; protected content cannot be a capacity victim.
        let inner=h.inner.lock().unwrap();let active=inner.render_packages[&upload.package_hash].clone();
        let mut artificial=inventory;artificial.insert(upload.package_hash.clone(),CACHE_BYTES);
        assert!(plan(&inner,&active,&artificial).is_ok()); // incoming bytes replace its prior disk size
        assert!(inner.sessions.values().any(|s|s.render_package.as_ref().is_some_and(|(p,_)|p==&upload.package_hash)));
    }
    #[test]fn failed_disk_selection_commit_keeps_live_selection_unchanged(){
        let dir=tempfile::tempdir().unwrap();fs::create_dir(dir.path().join("selected.json")).unwrap();
        let h=Hub::new("host".into(),Arc::new(NullSourceSink)).with_render_cache(dir.path()).unwrap();let(source,upload)=fixture("sunton");publish(&h,&source);
        assert!(h.install_render_package(upload).is_err());let inner=h.inner.lock().unwrap();assert!(inner.render_packages.is_empty()&&inner.render_selected.is_empty());drop(inner);
        assert!(RenderDiskCache::new(dir.path()).inventory().unwrap().is_empty());
        let clone=h.clone();assert!(h.with_render_cache(dir.path()).is_err());drop(clone);
    }
}