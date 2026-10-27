//! Platform metadata seeds unedited assignments. It never classifies by title.
use super::*;
type Metadata = BTreeMap<u32,Vec<String>>;

pub(super) fn load(
    saved:&crate::game_installations::Settings,
    candidates:&[Candidate],
    root:&Path,
    cancelled:impl Fn()->bool+Sync,
)->Metadata {
    let ids=candidates.iter().filter(|candidate|candidate.client=="steam"&&candidate.kind!="tool")
        .filter(|candidate|!saved.games.iter().any(|(id,game)|saved.game_tags.contains_key(id)&&game.installations.values().any(|installation|
            same_path(&installation.root,&candidate.install_path)||installation.sources.iter().any(|source|source.client=="steam"&&source.app_id==candidate.app_id))))
        .filter_map(|candidate|candidate.app_id.parse::<u32>().ok()).collect::<Vec<_>>();
    crate::game_installations::tags::platform::load(root,&ids,cancelled)
}

pub(super) fn proposed(
    saved:&crate::game_installations::Settings,
    game_id:&str,
    sources:&[LibrarySource],
    metadata:&Metadata,
)->Vec<String> {
    if saved.game_tags.contains_key(game_id) {return vec![];}
    let labels=sources.iter().filter(|source|source.client=="steam")
        .filter_map(|source|source.app_id.parse::<u32>().ok()).filter_map(|id|metadata.get(&id))
        .flatten().cloned().collect::<Vec<_>>();
    crate::game_installations::tags::from_metadata(&saved.tags,&labels)
}

#[cfg(test)]
#[path="tagging_tests.rs"]
mod tests;
