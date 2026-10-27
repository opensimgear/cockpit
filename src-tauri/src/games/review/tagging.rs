//! Metadata enriches discovery without owning local names or tag assignments.
use super::*;
use crate::game_metadata::{GameMetadata,MetadataQuery,ExternalId,MatchMethod};

#[derive(Clone,Default)]
pub(super) struct SourceMetadata {
    pub platform_genres:Vec<String>,
    pub wikidata:Option<GameMetadata>,
}
pub(super) type Metadata=BTreeMap<String,SourceMetadata>;
pub(super) struct TagProposal {pub ids:Vec<String>,pub source:Option<String>}

pub(super) fn query(client:&str,app_id:&str,title:&str)->MetadataQuery {
    MetadataQuery{key:format!("{client}:{app_id}"),title:title.to_owned(),external_ids:
        if client=="disk" {vec![]}else{vec![ExternalId{platform:client.into(),id:app_id.into()}]}}
}

pub(super) fn load(
    _saved:&crate::game_installations::Settings,
    candidates:&[Candidate],
    root:&Path,
    cancelled:impl Fn()->bool+Sync,
)->Metadata {
    let candidates=candidates.iter().filter(|candidate|candidate.kind!="tool").collect::<Vec<_>>();
    let steam=candidates.iter().filter(|candidate|candidate.client=="steam")
        .filter_map(|candidate|candidate.app_id.parse::<u32>().ok()).collect::<Vec<_>>();
    let platform=crate::game_installations::tags::platform::load(root,&steam,&cancelled);
    if cancelled() {return BTreeMap::new();}
    let queries=candidates.iter().map(|candidate|query(&candidate.client,&candidate.app_id,&candidate.name)).collect::<Vec<_>>();
    let wikidata=crate::game_metadata::load(root,&queries,&cancelled);
    candidates.into_iter().map(|candidate| {
        let key=format!("{}:{}",candidate.client,candidate.app_id);
        let platform_genres=if candidate.client=="steam" {candidate.app_id.parse::<u32>().ok().and_then(|id|platform.get(&id)).cloned().unwrap_or_default()}else{vec![]};
        let metadata=wikidata.get(&key).and_then(|result|result.metadata.clone());
        (key,SourceMetadata{platform_genres,wikidata:metadata})
    }).collect()
}

pub(super) fn proposed(
    saved:&crate::game_installations::Settings,
    game_id:&str,
    sources:&[LibrarySource],
    metadata:&Metadata,
)->TagProposal {
    let none=||TagProposal{ids:vec![],source:None};
    if saved.game_tags.contains_key(game_id) {return none();}
    let records=sources.iter().filter_map(|source|metadata.get(&format!("{}:{}",source.client,source.app_id))).collect::<Vec<_>>();
    let labels=records.iter().flat_map(|record|record.platform_genres.iter().cloned()).collect::<Vec<_>>();
    let ids=crate::game_installations::tags::from_metadata(&saved.tags,&labels);
    if !ids.is_empty() {return TagProposal{ids,source:Some("platform".into())};}
    let matches=sources.iter().filter_map(|source|metadata.get(&format!("{}:{}",source.client,source.app_id))
        .and_then(|record|record.wikidata.as_ref()).map(|record| {
            let rank=if record.matched_by==MatchMethod::ExternalId {0}else if source.client!="disk" {1}else{2};(rank,record)
        })).collect::<Vec<_>>();
    let Some(best)=matches.iter().map(|(rank,_)|*rank).min() else{return none();};
    let candidates=matches.iter().filter(|(rank,_)|*rank==best).map(|(_,record)|*record).collect::<Vec<_>>();
    if candidates.iter().map(|record|&record.entity_id).collect::<BTreeSet<_>>().len()!=1 {return none();}
    let Some(record)=candidates.into_iter().max_by_key(|record|record.fetched_at) else{return none();};
    let labels=record.genres.iter().chain(record.genre_ancestors.iter()).map(|genre|genre.label.clone()).collect::<Vec<_>>();
    let ids=crate::game_installations::tags::from_metadata(&saved.tags,&labels);
    let source=(!ids.is_empty()).then(||"wikidata".into());
    TagProposal{ids,source}
}

#[cfg(test)]
#[path="tagging_tests.rs"]
mod tests;
