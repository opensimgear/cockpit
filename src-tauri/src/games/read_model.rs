//! Read-model capture uses only prepared owner caches. It neither calls the
//! legacy status endpoint nor starts source, filesystem or inventory work.
use super::*;
use crate::game_read_models::{Parts,signals::Signal};
use std::collections::BTreeMap;

impl GamesRuntime {
    pub(crate) fn read_model_signal(&self)->Arc<Signal> {self.inner.changes.clone()}
    pub(crate) fn read_model_deadline(&self)->Option<Instant> {
        self.inner.activation.view_deadline().into_iter().chain(self.inner.vehicles.lock().unwrap_or_else(|p|p.into_inner()).view_deadline()).min()
    }
    pub(crate) fn capture_read_model_parts(&self,discovery:bool,detail_ids:&[String],
        asset:&mut impl FnMut(&Arc<str>)->Result<Value,String>)->Result<Parts,String> {
        let (mut catalogue,mut meta,revision)={
            let settings=self.inner.settings.lock().unwrap_or_else(|p|p.into_inner());
            (settings.catalogue.clone(),json!({"tags":settings.saved.tags,"addedGameId":settings.added_game_id,
                "error":settings.error}),settings.saved.revision)
        };
        let mut details=BTreeMap::new();
        for id in detail_ids {
            if let Some(game)=catalogue.iter().find(|game|game["id"]==*id) {
                let mut single=vec![game.clone()];self.decorate_resource_fields(&mut single,false);
                self.inner.prepared_sources.snapshot().decorate_with(&mut single,asset)?;
                let mut state=json!({"catalogue":single});
                if self.inner.online_artwork {crate::game_artwork::steam_online::decorate_assets(self.inner.installations.root(),&mut state,asset)?;}
                details.insert(id.clone(),state["catalogue"][0].take());
            }
        }
        self.decorate_resource_fields(&mut catalogue,true);
        self.inner.prepared_sources.snapshot().decorate_with(&mut catalogue,asset)?;
        let mut projected=json!({"catalogue":catalogue});
        if self.inner.online_artwork {crate::game_artwork::steam_online::decorate_assets(self.inner.installations.root(),&mut projected,asset)?;}
        let catalogue=projected["catalogue"].take();
        meta["artworkPending"]=json!(projected["artworkPending"].as_bool().unwrap_or(false));
        let rows=catalogue.as_array().expect("captured catalogue array");
        meta["detectedGames"]=json!(rows.iter().filter(|game|game["profileContext"]["status"]=="verified")
            .map(|game|json!({"gameId":game["id"],"name":game["name"]})).collect::<Vec<_>>());
        meta["detectionErrors"]=json!(rows.iter().filter(|game|game["profileContext"]["status"]=="unavailable")
            .filter_map(|game|game["profileContext"]["message"].as_str()).collect::<Vec<_>>());
        if let Some(error)=self.inner.artwork_error.lock().unwrap_or_else(|p|p.into_inner()).clone() {meta["artworkError"]=json!(error);}
        if let Some(error)=self.inner.prepared_sources.error(){meta["artworkError"]=json!(error);}
        let discovery=if discovery {
            let review=self.inner.discovery.lock().unwrap_or_else(|p|p.into_inner()).clone();
            let mut entries=Vec::with_capacity(review.entries.len());
            for entry in &review.entries {
                let mut metadata=entry.clone();metadata.image=None;
                let mut value=serde_json::to_value(metadata).map_err(|error|error.to_string())?;
                if let Some(image)=&entry.image {value["image"]=asset(image)?;}
                entries.push(value);
            }
            let mut value=json!({"scanId":review.scan_id,"status":review.status,"libraries":review.libraries,
                "entries":entries,"observations":review.observations,"diagnostics":review.diagnostics,"firstStart":review.first_start});
            if self.inner.online_artwork {
                let mut state=json!({"catalogue":[],"discovery":value});
                crate::game_artwork::steam_online::decorate_assets(self.inner.installations.root(),&mut state,asset)?;
                value=state["discovery"].take();
            }
            Some(value)
        }else{None};
        if self.inner.closed.load(Ordering::Acquire){return Err("The game source has closed.".into());}
        if self.inner.settings.lock().unwrap_or_else(|p|p.into_inner()).saved.revision!=revision {
            return Err("Game read-model source changed during capture.".into());
        }
        Ok(Parts{catalogue,meta,discovery,details,library_revision:revision})
    }
}
