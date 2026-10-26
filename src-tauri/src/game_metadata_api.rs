//! Atomic edits to the discovered game registry. Discovery proposes; Apply publishes.
use super::*;
use std::fs;
pub(crate) struct PreparedMetadata {
    runtime:Arc<Inner>,captured:crate::game_installations::Settings,next:crate::game_installations::Settings,
    catalogue:Vec<Value>,added_game_id:Option<String>,scan_id:Option<String>,staging:crate::game_artwork::Staging,
}
fn fields(request:&Value,allowed:&[&str])->Result<(),String> {
    if !known(request,&[&["command","clientId"][..],allowed].concat()) {return Err("Unknown game library request field.".into());}Ok(())
}
fn position(request:&Value,key:&str,existing:f64)->Result<f64,String> {
    match request.get(key) {None=>Ok(existing),Some(value)=>value.as_f64().filter(|number|number.is_finite()&&(0.0..=100.0).contains(number)).ok_or_else(||"Background crop positions must be between 0 and 100.".into())}
}
pub(super) fn unique_executable(catalogue:&[Value],id:&str,executable:Option<&str>)->Result<(),String> {
    if executable.is_some_and(|path|catalogue.iter().any(|game|game["id"]!=id&&game["installations"].as_array().is_some_and(|installations|installations.iter().any(|installation|installation["executable"].as_str().is_some_and(|other|crate::game_discovery::normalized_path(path)==crate::game_discovery::normalized_path(other)))))) {
        return Err("This executable already belongs to another discovered game.".into());
    }Ok(())
}
impl GamesRuntime {
    pub(super) fn metadata_request(&self,request:&Value)->Result<(),String> {
        let prepared=self.prepare_metadata(request)?;self.commit_metadata(prepared)?;self.finish_metadata();Ok(())
    }
    pub(crate) fn prepare_metadata(&self,request:&Value)->Result<PreparedMetadata,String> {
        if self.inner.closed.load(Ordering::Acquire) {return Err("The game metadata runtime has closed.".into());}
        if !request.is_object() || request.to_string().len()>65536 {return Err("Invalid game metadata request.".into());}
        self.inner.guard_installation_metadata()?;
        let staging=crate::game_artwork::staging(self.inner.installations.root())?;
        let (captured,catalogue)={let settings=self.inner.settings.lock().unwrap_or_else(|p|p.into_inner());(settings.saved.clone(),settings.catalogue.clone())};
        let mut next=captured.clone();let mut added=None;let mut scan_id=None;
        match request["command"].as_str().unwrap_or("") {
            "game_discovery_apply"=>{
                fields(request,&["scanId","actions"])?;
                let review=self.inner.discovery.lock().unwrap_or_else(|p|p.into_inner()).clone();
                next=discovery_review::apply(&captured,&review,request)?;
                for entry in &review.entries {
                    if entry.action=="remove" || !request["actions"].as_array().and_then(|actions|actions.iter().find(|action|action["id"]==entry.id)).map_or(entry.selected,|action|action["selected"]==true) {continue;}
                    let Some(path)=entry.artwork.as_ref() else {continue;};
                    let Some(game)=next.games.values_mut().find(|game|game.installations.values().any(|installation|discovery_review::same_path(&installation.root,&entry.install_path))) else {continue;};
                    if game.default_background.is_none() {
                        game.default_background=path.to_str().and_then(|path|crate::game_artwork::import(self.inner.installations.root(),path,50.0,50.0).ok());
                    }
                }
                scan_id=Some(review.scan_id);
            }
            "game_categories_save"=>{
                fields(request,&["categories","expectedCategories"])?;
                if request["expectedCategories"]!=json!(captured.categories) {return Err("Categories changed elsewhere. Close this dialog and try again.".into());}
                next.categories=serde_json::from_value(request["categories"].clone()).map_err(|_|"Choose a valid category list.")?;
                next.game_categories.retain(|_,category|next.categories.contains_key(category));
            }
            "game_category_save"=>{
                fields(request,&["gameId","categoryId"])?;
                let id=request["gameId"].as_str().filter(|id|next.games.contains_key(*id)).ok_or("Choose a discovered game.")?;
                match request.get("categoryId") {Some(Value::Null)=>{next.game_categories.remove(id);},Some(Value::String(category)) if next.categories.contains_key(category)=>{next.game_categories.insert(id.into(),category.clone());},_=>return Err("Choose an existing category or Uncategorized.".into())}
            }
            "game_save"=>{
                fields(request,&["gameId","name","installationId","executable"])?;
                let id=request["gameId"].as_str().ok_or("Choose a discovered game to edit.")?;
                let game=next.games.get_mut(id).ok_or("Choose a discovered game to edit.")?;
                if let Some(name)=request.get("name") {game.name=name.as_str().ok_or("Choose a game name.")?.trim().to_owned();}
                if let Some(value)=request.get("executable") {
                    let selected=request["installationId"].as_str().or(game.selected_installation.as_deref()).ok_or("Choose an installation to edit.")?;
                    let installation=game.installations.get_mut(selected).ok_or("Choose an installation of this game.")?;
                    installation.executable=match value {
                        Value::Null=>None,Value::String(path)=>{
                            let path=crate::game_discovery::validate_executable(path)?;
                            let root=fs::canonicalize(&installation.root).map_err(|_|"The installation directory is unavailable.")?;
                            if !path.starts_with(root) {return Err("Choose a runtime inside this installation.".into());}
                            let text=path.to_str().ok_or("Invalid executable path.")?.to_owned();
                            unique_executable(&catalogue,id,Some(&text))?;Some(text)
                        },_=>return Err("Choose an executable path or explicit null.".into()),
                    };
                }
                added=Some(id.to_owned());
            }
            "game_remove"=>{
                fields(request,&["gameId"])?;
                let id=request["gameId"].as_str().ok_or("Choose a discovered game.")?;
                if next.games.remove(id).is_none() {return Err("Choose a discovered game.").map_err(String::from);}
                next.backgrounds.remove(id);next.default_backgrounds.remove(id);next.game_categories.remove(id);
            }
            "game_installation_select"=>{
                fields(request,&["gameId","installationId"])?;
                let game=next.games.get_mut(request["gameId"].as_str().ok_or("Choose a discovered game.")?).ok_or("Choose a discovered game.")?;
                let id=request["installationId"].as_str().filter(|id|game.installations.contains_key(*id)).ok_or("Choose an installation of this game.")?;
                game.selected_installation=Some(id.into());
            }
            "game_background_save" => {
                fields(request, &["gameId", "path", "positionX", "positionY", "imageData", "crop"])?;
                let id = request["gameId"]
                    .as_str()
                    .filter(|id| catalogue.iter().any(|game| game["id"] == *id))
                    .ok_or("Choose an existing game background.")?;
                if let Some(data) = request.get("imageData") {
                    if request.get("path").is_some() || request.get("positionX").is_some() || request.get("positionY").is_some() {
                        return Err("Choose a preview crop or a legacy image position, not both.".into());
                    }
                    let crop = serde_json::from_value(request["crop"].clone()).map_err(|_| "Choose a valid tile crop.")?;
                    let background = crate::game_artwork::import_preview(self.inner.installations.root(), data.as_str().ok_or("Load a valid image preview.")?, crop)?;
                    next.backgrounds.insert(id.into(), background);
                } else if request.get("crop").is_some() {
                    return Err("Load the image preview before saving its crop.".into());
                } else if request.get("path") == Some(&Value::Null) {
                    next.backgrounds.remove(id);
                } else {
                    let existing = next
                        .backgrounds
                        .get(id)
                        .or_else(|| next.default_backgrounds.get(id))
                        .or_else(|| {
                            next.games
                                .get(id)
                                .and_then(|game| game.default_background.as_ref())
                        });
                    let x = position(
                        request,
                        "positionX",
                        existing.map_or(50.0, |background| background.position_x),
                    )?;
                    let y = position(
                        request,
                        "positionY",
                        existing.map_or(50.0, |background| background.position_y),
                    )?;
                    let background=match request.get("path") {
                        Some(Value::String(path))=>crate::game_artwork::import(self.inner.installations.root(),path,x,y)?,
                        None=>{let mut background=existing.cloned().ok_or("Choose a background image before setting its crop.")?;background.position_x=x;background.position_y=y;background.crop=None;background},
                        _=>return Err("Choose a local image path, omit it to adjust crop, or reset with null.".into()),
                    };
                    next.backgrounds.insert(id.into(), background);
                }
            }
            _=>return Err("Unknown game library operation.".into()),
        }
        next.revision=captured.revision.checked_add(1).ok_or("Game library revision exhausted.")?;
        next.validate()?;
        let catalogue=crate::game_discovery::merged_catalogue(self.inner.platform.as_ref(),&next);
        Ok(PreparedMetadata{runtime:self.inner.clone(),captured,next,catalogue,added_game_id:added,scan_id,staging})
    }
    pub(crate) fn commit_metadata(&self,prepared:PreparedMetadata)->Result<(),String> {
        if !Arc::ptr_eq(&prepared.runtime,&self.inner) {return Err("The prepared game metadata belongs to another runtime.".into());}
        let _admission=self.admission.try_lock().map_err(|_|"Game metadata is busy. Retry after the current operation finishes.")?;
        if self.inner.closed.load(Ordering::Acquire) {return Err("The game metadata runtime has closed.".into());}
        self.inner.guard_installation_metadata()?;
        let mut settings=self.inner.settings.lock().unwrap_or_else(|p|p.into_inner());
        if settings.saved!=prepared.captured {return Err("Game registrations changed during preparation. Refresh and retry.".into());}
        let mut scan=prepared.scan_id.as_ref().map(|_|self.inner.discovery.lock().unwrap_or_else(|p|p.into_inner()));
        if let Some(review)=scan.as_ref() {
            if Some(&review.scan_id)!=prepared.scan_id.as_ref() || review.status!="ready" || review.revision!=prepared.captured.revision {
                return Err("Discovery changed during preparation. Review the current scan before applying.".into());
            }
        }
        self.inner.installations.save(&prepared.next)?;
        settings.saved=prepared.next;settings.catalogue=prepared.catalogue;settings.added_game_id=prepared.added_game_id;settings.error=None;
        if let Some(review)=scan.as_mut() {review.status="applied".into();}
        drop(settings);drop(prepared.staging);Ok(())
    }
    pub(crate) fn finish_metadata(&self) {
        let Ok(_admission)=self.admission.try_lock() else{return;};
        let references={let settings=self.inner.settings.lock().unwrap_or_else(|p|p.into_inner());
            settings.saved.backgrounds.values().chain(settings.saved.default_backgrounds.values()).chain(settings.saved.games.values().filter_map(|game|game.default_background.as_ref())).cloned().collect::<Vec<_>>()};
        crate::game_artwork::retain(self.inner.installations.root(),references.iter());
    }
}
#[cfg(test)]
#[path="game_metadata_api_tests.rs"] mod tests;
