//! Passive cached metadata access for native clients and MCP consumers.
use super::*;
use crate::game_metadata::{MetadataQuery,ExternalId,LookupStatus};

impl GamesRuntime {
    pub(crate) fn metadata_details(&self,request:&Value)->Result<Value,String> {
        if self.inner.closed.load(Ordering::Acquire) {return Err("The game metadata runtime has closed.".into());}
        if !known(request,&["command","clientId","gameId"]) {return Err("Unknown game metadata lookup field.".into());}
        let id=request["gameId"].as_str().ok_or("Choose a discovered game for metadata.")?;
        let game=self.inner.settings.lock().unwrap_or_else(|p|p.into_inner()).saved.games.get(id).cloned()
            .ok_or("Choose a discovered game for metadata.")?;
        let mut seen=std::collections::BTreeSet::new();
        let mut results=vec![];
        for source in game.installations.values().flat_map(|installation|installation.sources.iter()) {
            let key=format!("{}:{}",source.client,source.app_id);
            if !seen.insert(key.clone())||results.len()>=16 {continue;}
            let query=MetadataQuery{key,title:game.name.clone(),external_ids:if source.client=="disk" {vec![]}else{
                vec![ExternalId{platform:source.client.clone(),id:source.app_id.clone()}]}};
            if let Some(lookup)=crate::game_metadata::cached(self.inner.installations.root(),&query) {
                results.push(json!({"platform":source.client,"appId":source.app_id,"lookup":lookup}));
            }
        }
        let matched=results.iter().any(|result|result["lookup"]["status"]==json!(LookupStatus::Matched));
        Ok(json!({"gameId":id,"source":"wikidata","sourceName":"Wikidata","license":"CC0",
            "status":if matched{"matched"}else if results.is_empty(){"not-cached"}else{"unmatched"},"results":results}))
    }
}

#[cfg(test)]
#[path="metadata_read_tests.rs"]
mod tests;
