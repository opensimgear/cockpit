//! Shared, cached game metadata providers for discovery and future consumers.
mod model;
mod cache;
mod network;
mod wikidata;
mod provider;
pub(crate) use model::*;
pub(crate) use provider::{load,cached,refresh};

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn test_store(root:&std::path::Path,query:&MetadataQuery,lookup:&MetadataLookup)->bool {cache::write(root,query,lookup)}

#[cfg(test)]
pub(crate) fn test_record(title:&str,genre:&str)->GameMetadata {
    GameMetadata{source:"wikidata".into(),entity_id:"Q123".into(),url:"https://www.wikidata.org/wiki/Q123".into(),
        title:title.into(),description:Some("A video game".into()),aliases:vec![],
        genres:vec![Entity{id:"Q8607".into(),label:genre.into()}],genre_ancestors:vec![],platforms:vec![],developers:vec![],publishers:vec![],release_dates:vec![],
        external_ids:vec![],fetched_at:1,matched_by:MatchMethod::ExactTitle}
}
