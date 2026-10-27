//! Provider-neutral game metadata; remote facts never own local game settings.
use serde::{Deserialize,Serialize};

#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct ExternalId {pub platform:String,pub id:String}

#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct MetadataQuery {
    pub key:String,pub title:String,pub external_ids:Vec<ExternalId>,
}

#[derive(Clone,Debug,PartialEq,Eq,PartialOrd,Ord,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct Entity {pub id:String,pub label:String}

#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="kebab-case")]
pub(crate) enum MatchMethod {ExternalId,ExactTitle}

#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct GameMetadata {
    pub source:String,pub entity_id:String,pub url:String,pub title:String,
    pub description:Option<String>,pub aliases:Vec<String>,
    pub genres:Vec<Entity>,pub genre_ancestors:Vec<Entity>,pub platforms:Vec<Entity>,
    pub developers:Vec<Entity>,pub publishers:Vec<Entity>,pub release_dates:Vec<String>,
    pub external_ids:Vec<ExternalId>,pub fetched_at:u64,pub matched_by:MatchMethod,
}

#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="kebab-case")]
pub(crate) enum LookupStatus {Matched,NotFound,Ambiguous,Unavailable}

#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct MetadataLookup {
    pub status:LookupStatus,pub metadata:Option<GameMetadata>,pub checked_at:u64,pub stale:bool,
}
impl MetadataLookup {
    pub(crate) fn empty(status:LookupStatus,checked_at:u64)->Self {Self{status,metadata:None,checked_at,stale:false}}
}

pub(crate) fn normalized_title(value:&str)->String {
    value.chars().filter(|character|!matches!(character,'™'|'®'|'©')).flat_map(char::to_lowercase)
        .map(|character|if character.is_alphanumeric(){character}else{' '})
        .collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}
pub(crate) fn valid_text(value:&str,max:usize)->bool {
    !value.trim().is_empty()&&value.trim()==value&&value.chars().count()<=max&&!value.chars().any(char::is_control)
}
pub(crate) fn valid_qid(value:&str)->bool {
    value.strip_prefix('Q').is_some_and(|id|!id.is_empty()&&id.len()<=16&&id.bytes().all(|byte|byte.is_ascii_digit())&& !id.starts_with('0'))
}
impl MetadataQuery {
    pub(crate) fn valid(&self)->bool {
        valid_text(&self.key,512)&&valid_text(&self.title,256)&&self.external_ids.len()<=32
            &&self.external_ids.iter().all(|id|valid_text(&id.platform,32)&&valid_text(&id.id,256))
    }
}
impl GameMetadata {
    pub(crate) fn valid(&self)->bool {
        let entities=|values:&[Entity],max|values.len()<=max&&values.iter().all(|entity|valid_qid(&entity.id)&&valid_text(&entity.label,256));
        self.source=="wikidata"&&valid_qid(&self.entity_id)&&self.url==format!("https://www.wikidata.org/wiki/{}",self.entity_id)
            &&valid_text(&self.title,256)&&self.description.as_ref().is_none_or(|text|valid_text(text,2048))
            &&self.aliases.len()<=64&&self.aliases.iter().all(|text|valid_text(text,256))
            &&entities(&self.genres,64)&&entities(&self.genre_ancestors,128)&&entities(&self.platforms,64)
            &&entities(&self.developers,64)&&entities(&self.publishers,64)
            &&self.release_dates.len()<=32&&self.release_dates.iter().all(|text|valid_text(text,64))
            &&self.external_ids.len()<=64&&self.external_ids.iter().all(|id|valid_text(&id.platform,32)&&valid_text(&id.id,256))
            &&serde_json::to_vec(self).is_ok_and(|bytes|bytes.len()<=60*1024)
    }
}
impl MetadataLookup {
    pub(crate) fn valid(&self)->bool {
        match (&self.status,&self.metadata) {
            (LookupStatus::Matched,Some(metadata))=>metadata.valid()&&metadata.fetched_at==self.checked_at,
            (LookupStatus::NotFound|LookupStatus::Ambiguous|LookupStatus::Unavailable,None)=>true,
            _=>false,
        }
    }
}
