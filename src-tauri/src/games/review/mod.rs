//! A scan proposes immutable, reviewable changes; only Apply edits the library.
use super::*;
use crate::{game_installations::{GameRecord,Installation,LibrarySource},game_library::Candidate};
use serde::Serialize;
use std::collections::{BTreeMap,BTreeSet};
mod reconcile;
mod tagging;
mod worker;
pub(super) use worker::start;

#[cfg(test)]
pub(super) fn test_reconcile(
    saved: &crate::game_installations::Settings,
    candidates: Vec<Candidate>,
) -> Vec<Entry> {
    let successful = crate::game_library::CLIENTS
        .iter()
        .map(|(id, _)| (*id).to_owned())
        .chain(std::iter::once("disk".to_owned()))
        .collect();
    reconcile::reconcile(saved, candidates, &successful)
}

#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub(super) struct Library {
    pub id:String,pub name:String,pub status:String,pub found:usize,pub message:Option<String>,
}
#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub(super) struct Entry {
    pub id:String,pub action:String,pub selected:bool,pub game_id:String,pub installation_id:String,
    pub name:String,pub library:String,pub install_path:String,pub previous_path:Option<String>,
    pub executable_candidates:Vec<String>,pub executable:Option<String>,pub update_kind:Option<String>,
    pub integration:Option<String>,pub message:Option<String>,
    pub image:Option<String>,
    #[serde(skip_serializing_if="Vec::is_empty")] pub tag_ids:Vec<String>,
    #[serde(skip)] pub sources:Vec<LibrarySource>,
    #[serde(skip)] pub artwork:Option<PathBuf>,
}
#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub(super) struct Review {
    pub scan_id:String,pub status:String,pub libraries:Vec<Library>,pub entries:Vec<Entry>,
    pub diagnostics:Vec<String>,pub first_start:bool,
    #[serde(skip)] pub revision:u64,
    #[serde(skip)] pub paths:Vec<String>,
}
impl Default for Review {
    fn default()->Self {Self{scan_id:String::new(),status:"idle".into(),libraries:vec![],entries:vec![],diagnostics:vec![],first_start:false,revision:0,paths:vec![]}}
}
pub(super) fn key(prefix:&str,value:&str)->String {
    use sha2::{Digest,Sha256};
    format!("{prefix}-{:x}",Sha256::digest(value.as_bytes()))
}
pub(super) fn absent(path:&str)->bool {
    std::fs::metadata(path).is_err_and(|error|error.kind()==std::io::ErrorKind::NotFound)
}
fn path_key(path:&str)->String {crate::game_discovery::normalized_path(path).trim_end_matches('\\').to_owned()}
pub(super) fn same_path(left:&str,right:&str)->bool {path_key(left)==path_key(right)}

pub(super) fn apply(saved:&crate::game_installations::Settings,review:&Review,request:&Value)->Result<crate::game_installations::Settings,String> {
    if review.status!="ready" || request["scanId"]!=review.scan_id || saved.revision!=review.revision {
        return Err("Discovery changed or the library was edited. Rescan before applying.".into());
    }
    let actions=request["actions"].as_array().ok_or("Review the selected discovery actions before applying.")?;
    if actions.len()>1024 {return Err("Too many discovery selections.".into());}
    let mut selections=BTreeMap::new();
    for action in actions {
        if !known(action,&["id","selected","executable","replaceInstallationId","gameId"]) || !action["selected"].is_boolean() {
            return Err("Choose valid discovery action selections.".into());
        }
        let id=action["id"].as_str().ok_or("Choose a discovery entry.")?;
        if !review.entries.iter().any(|entry|entry.id==id) || selections.insert(id,action).is_some() {
            return Err("Discovery selections must reference distinct current entries.".into());
        }
    }
    let mut aliases=BTreeMap::<String,String>::new();
    for entry in &review.entries {
        if let Some(action)=selections.get(entry.id.as_str()).filter(|action|action["selected"]==true) {
            if let Some(target)=action["gameId"].as_str().filter(|target|*target!=entry.game_id) {
                if !saved.games.contains_key(target)&&!review.entries.iter().any(|entry|entry.game_id==target&&entry.action!="remove") {return Err("Choose a saved or newly discovered title to match.".into());}
                if entry.integration.is_some() {return Err("A recognized title cannot be reassigned to another game.".into());}
                if aliases.insert(entry.game_id.clone(),target.into()).as_ref().is_some_and(|previous|previous!=target) {return Err("All copies of a title must use the same identity resolution.".into());}
            }
        }
    }
    let resolve=|id:&str|->Result<String,String> {
        let mut current=id.to_owned();let mut seen=BTreeSet::new();
        while let Some(next)=aliases.get(&current) {if !seen.insert(current.clone()) {return Err("Game identity matches contain a cycle. Choose one title.".into());}current=next.clone();}
        Ok(current)
    };
    for id in aliases.keys() {resolve(id)?;}
    let mut next=saved.clone();
    let mut claimed=BTreeSet::new();
    let mut automatic_tags=BTreeMap::<String,BTreeSet<String>>::new();
    for entry in &review.entries {
        let action=selections.get(entry.id.as_str()).copied();
        let selected=action.map_or(entry.selected,|value|value["selected"]==true);
        if !selected {continue;}
        if entry.action=="remove" {
            // A removal always requires an explicit checkbox selection.
            if action.is_none() || !absent(&entry.install_path) {return Err("The removed installation is present or could not be verified. Rescan.".into());}
            if !claimed.insert((entry.game_id.clone(),entry.installation_id.clone())) {return Err("An installation cannot be updated and removed in the same review.".into());}
            let game=next.games.get_mut(&entry.game_id).ok_or("The removed game changed. Rescan.")?;
            game.installations.remove(&entry.installation_id);
            if game.selected_installation.as_ref()==Some(&entry.installation_id) {game.selected_installation=None;}
            continue;
        }
        let root=std::fs::canonicalize(&entry.install_path).map_err(|error|format!("Installation disappeared since discovery: {error}"))?;
        if !root.is_dir() {return Err("Choose an existing installation directory.".into());}
        let game_id=resolve(&entry.game_id)?;
        if !saved.game_tags.contains_key(&game_id) {
            automatic_tags.entry(game_id.clone()).or_default().extend(entry.tag_ids.iter().cloned());
        }
        let executable=match action.and_then(|value|value.get("executable")) {
            Some(Value::Null)=>None,
            Some(Value::String(path))=>Some(path.clone()),
            Some(_)=>return Err("Choose an executable path or leave it unresolved.".into()),
            None=>entry.executable.clone(),
        };
        let executable=executable.map(|path| {
            let path=crate::game_discovery::validate_executable(&path)?;
            if !path.starts_with(&root) {return Err("The runtime executable must be inside this installation.".to_owned());}
            path.to_str().map(str::to_owned).ok_or("Invalid executable path.".into())
        }).transpose()?;
        let replace_value=action.and_then(|value|value.get("replaceInstallationId"));
        let explicit_new=replace_value==Some(&Value::Null)||game_id!=entry.game_id;
        let replace=match replace_value {
            Some(Value::Null)=>None,Some(Value::String(id))=>Some(id.as_str()),Some(_)=>return Err("Choose an installation to replace or add a new instance.".into()),
            None if !explicit_new&&entry.update_kind.as_deref()==Some("replace-path")=>Some(entry.installation_id.as_str()),None=>None,
        };
        let target=review.entries.iter().find(|item|item.game_id==game_id&&item.action!="remove").unwrap_or(entry);
        let game=next.games.entry(game_id.clone()).or_insert_with(||GameRecord{name:target.name.clone(),installations:BTreeMap::new(),selected_installation:None,integration:target.integration.clone(),default_background:None});
        if let Some(integration)=&entry.integration {game.integration=Some(integration.clone());}
        let installation_id=if let Some(replace)=replace {
            if !game.installations.contains_key(replace) {return Err("Choose an installation of this game to replace.".into());}
            replace.to_owned()
        } else if explicit_new {key("installation",&crate::game_discovery::normalized_path(&entry.install_path))}else{entry.installation_id.clone()};
        if !claimed.insert((game_id.clone(),installation_id.clone())) {return Err("Two selections replace the same installation. Review their paths.".into());}
        let mut sources=game.installations.get(&installation_id).filter(|previous|same_path(&previous.root,&entry.install_path)).map(|previous|previous.sources.clone()).unwrap_or_default();
        for source in &entry.sources {if !sources.contains(source) {sources.push(source.clone());}}
        let clear=action.and_then(|value|value.get("executable"))==Some(&Value::Null);
        let selected=if clear {None}else{executable.or_else(||game.installations.get(&installation_id).filter(|old|same_path(&old.root,&entry.install_path)).and_then(|old|old.executable.clone()).filter(|path|std::path::Path::new(path).is_file()))};
        game.installations.insert(installation_id.clone(),Installation{root:root.to_string_lossy().into_owned(),executable:selected,executables:entry.executable_candidates.clone(),sources});
        if game.selected_installation.is_none() {game.selected_installation=Some(installation_id);}
    }
    next.games.retain(|_,game|!game.installations.is_empty());
    for (id,ids) in automatic_tags {
        let ids=ids.into_iter().filter(|id|next.tags.contains_key(id)).take(32).collect::<Vec<_>>();
        if next.games.contains_key(&id)&&!ids.is_empty() {next.game_tags.insert(id,ids);}
    }
    next.game_tags.retain(|id,_|next.games.contains_key(id));
    next.backgrounds.retain(|id,_|next.games.contains_key(id));
    next.default_backgrounds.retain(|id,_|next.games.contains_key(id));
    for path in &review.paths {if !next.scan_roots.contains(path) {next.scan_roots.push(path.clone());}}
    next.discovery_initialized=true;
    next.validate()?;
    Ok(next)
}

#[cfg(test)] mod tests;
