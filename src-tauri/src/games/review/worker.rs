use super::*;
#[path="completion.rs"] mod completion;
pub(crate) fn start(runtime:&GamesRuntime,request:&Value)->Result<(),String> {
    start_with_metadata(runtime,request,crate::game_library::installed_clients(),|client,roots| {
        crate::game_library::scan_client(client,roots)
    },|saved,candidates,root,cancelled|super::tagging::load(saved,candidates,root,cancelled))
}
#[cfg(test)]
fn start_with(
    runtime:&GamesRuntime,
    request:&Value,
    clients:Vec<(&'static str,&'static str)>,
    scan_client:impl Fn(&str,&[PathBuf])->crate::game_library::Scan+Send+'static,
)->Result<(),String> {
    start_with_metadata(runtime,request,clients,scan_client,|_,_,_,_|BTreeMap::new())
}
fn start_with_metadata(
    runtime:&GamesRuntime,
    request:&Value,
    clients:Vec<(&'static str,&'static str)>,
    scan_client:impl Fn(&str,&[PathBuf])->crate::game_library::Scan+Send+'static,
    metadata:impl Fn(&crate::game_installations::Settings,&[Candidate],&Path,&(dyn Fn()->bool+Sync))->super::tagging::Metadata+Send+'static,
)->Result<(),String> {
    let _admission=runtime.admission.try_lock().map_err(|_|"Game metadata is busy. Retry discovery after the current update.")?;
    if !known(request,&["command","clientId","paths"]) {return Err("Unknown discovery start field.".into());}
    runtime.inner.guard_installation_metadata()?;
    let paths=match request.get("paths") {
        None=>vec![],Some(value)=>serde_json::from_value::<Vec<String>>(value.clone()).map_err(|_|"Choose discovery locations.".to_owned())?,
    };
    if paths.len()>64 || paths.iter().any(|path|!crate::game_installations::executable(path)||!Path::new(path).exists()) {return Err("Choose up to 64 existing absolute discovery locations.".into());}
    let saved=runtime.inner.settings.lock().unwrap_or_else(|p|p.into_inner()).saved.clone();
    let mut review=runtime.inner.discovery.lock().unwrap_or_else(|p|p.into_inner());
    if review.status=="scanning" {return Err("Game discovery is already running.".into());}
    let scan_id=uuid::Uuid::new_v4().to_string();
    *review=Review{scan_id:scan_id.clone(),status:"scanning".into(),libraries:clients.iter().map(|(id,name)|Library{id:(*id).into(),name:(*name).into(),status:"queued".into(),found:0,message:None}).collect(),entries:vec![],diagnostics:vec![],first_start:!saved.discovery_initialized,revision:saved.revision,paths:paths.clone()};
    drop(review);
    let inner=runtime.inner.clone();
    let mut roots=saved.scan_roots.iter().chain(paths.iter()).map(PathBuf::from).collect::<Vec<_>>();
    roots.extend(saved.games.values().flat_map(|game|game.installations.values()).map(|installation|PathBuf::from(&installation.root)).filter(|path|path.exists()));
    let worker=thread::Builder::new().name("game-discovery".into()).spawn(move|| {
        let outcome=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut candidates=vec![];let mut successful=BTreeSet::new();
            for (client,_) in &clients {
                if inner.closed.load(Ordering::Acquire) {return;}
                {
                    let mut review=inner.discovery.lock().unwrap_or_else(|p|p.into_inner());
                    if review.scan_id!=scan_id {return;}
                    if let Some(library)=review.libraries.iter_mut().find(|library|library.id==*client) {library.status="scanning".into();}
                }
                let scan=scan_client(client,&roots);
                let failed=scan.diagnostics.iter().any(|message|message.contains("could not be read")||message.contains("inaccessible")||message.contains("malformed")||message.contains("not installed"));
                let mut review=inner.discovery.lock().unwrap_or_else(|p|p.into_inner());
                if let Some(library)=review.libraries.iter_mut().find(|library|library.id==*client) {
                    library.status=if failed{"failed"}else{"preparing"}.into();library.found=scan.candidates.len();
                    library.message=failed.then(||"Some installation records could not be read; removals are withheld.".into());
                }
                if !failed {successful.insert((*client).to_owned());}
                for diagnostic in scan.diagnostics {if review.diagnostics.len()<64 {review.diagnostics.push(diagnostic);}}
                candidates.extend(scan.candidates);
            }
            if successful.contains("disk") {
                for (client,_) in crate::game_library::CLIENTS {if !clients.iter().any(|(id,_)|id==client) {successful.insert((*client).into());}}
            }
            let mut artwork=std::collections::BTreeMap::new();
            for candidate in &candidates {
                if let Some(path)=&candidate.artwork {
                    artwork.entry(path_key(&candidate.install_path)).or_insert_with(||(candidate.install_path.clone(),path.clone()));
                }
            }
            let extra_sources=artwork.values().map(|(_,path)|path.clone()).collect::<Vec<_>>();
            let mut entries=super::reconcile::reconcile(&saved,candidates.clone(),&successful);
            for entry in &mut entries {
                if !clients.iter().any(|(id,_)|*id==entry.library) {entry.library="disk".into();}
            }
            if !completion::publish_entries(&inner.discovery,&scan_id,entries) {return;}
            let cancelled=||inner.closed.load(Ordering::Acquire);
            let tags=metadata(&saved,&candidates,inner.installations.root(),&cancelled);
            if cancelled() {return;}
            let mut entries=super::reconcile::with_metadata(&saved,candidates,&successful,&tags);
            for entry in &mut entries {
                if !clients.iter().any(|(id,_)|*id==entry.library) {entry.library="disk".into();}
            }
            if !completion::publish_entries(&inner.discovery,&scan_id,entries) {return;}
            completion::prepare_artwork_with_sources(&inner.discovery,&scan_id,&extra_sources,||inner.closed.load(Ordering::Acquire),|path| {
                let image=crate::game_artwork::discovery_preview(path).ok()?;
                for (root,_) in artwork.values().filter(|(_,source)|source==path) {
                    let _=crate::game_artwork::cache_installation_preview(inner.installations.root(),root,&image);
                }
                Some(image)
            });
        }));
        if outcome.is_err() {
            let mut review=inner.discovery.lock().unwrap_or_else(|p|p.into_inner());review.status="failed".into();review.diagnostics.push("Game discovery failed. Rescan to try again.".into());
        }
    }).map_err(|error|format!("Cannot start game discovery: {error}"))?;
    if let Some(previous)=runtime.discovery_worker.lock().unwrap_or_else(|p|p.into_inner()).replace(worker) {let _=previous.join();}
    Ok(())
}

#[cfg(test)]
#[path="worker_tests.rs"]
mod tests;
