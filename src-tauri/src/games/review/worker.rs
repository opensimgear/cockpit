use super::*;
#[path="completion.rs"] mod completion;
pub(crate) fn start(runtime:&GamesRuntime,request:&Value)->Result<(),String> {
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
    let clients=crate::game_library::installed_clients();
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
                let scan=crate::game_library::scan_client(client,&roots);
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
            let mut entries=super::reconcile::reconcile(&saved,candidates,&successful);
            for entry in &mut entries {
                if !clients.iter().any(|(id,_)|*id==entry.library) {entry.library="disk".into();}
            }
            if !completion::publish_entries(&inner.discovery,&scan_id,entries) {return;}
            completion::prepare_artwork(&inner.discovery,&scan_id,||inner.closed.load(Ordering::Acquire),|path|crate::game_artwork::discovery_preview(path).ok());
        }));
        if outcome.is_err() {
            let mut review=inner.discovery.lock().unwrap_or_else(|p|p.into_inner());review.status="failed".into();review.diagnostics.push("Game discovery failed. Rescan to try again.".into());
        }
    }).map_err(|error|format!("Cannot start game discovery: {error}"))?;
    if let Some(previous)=runtime.discovery_worker.lock().unwrap_or_else(|p|p.into_inner()).replace(worker) {let _=previous.join();}
    Ok(())
}
