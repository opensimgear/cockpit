//! Explicit refresh of one retained title. Never enumerates client libraries.
use crate::game_installations::{Settings,GameRecord};
use std::{collections::{BTreeMap,BTreeSet},path::{Path,PathBuf}};

type Labels=(Vec<String>,Vec<String>);
type Found=(Labels,Vec<crate::game_library::ArtworkChoice>);

pub(super) fn refresh(settings:&mut Settings,id:&str,root:&Path,cancelled:impl Fn()->bool+Sync)->Result<(),String> {
    refresh_with(settings,id,root,|game| {
        let ids=game.installations.values().flat_map(|installation|&installation.sources).filter(|source|source.client=="steam")
            .filter_map(|source|source.app_id.parse::<u32>().ok()).collect::<BTreeSet<_>>();
        let mut artwork=vec![];
        for app_id in ids.into_iter().take(4) {
            if cancelled(){break;}
            if let Some(path)=crate::game_artwork::steam_online::refresh(root,app_id) {artwork.push(crate::game_library::ArtworkChoice{label:"Steam Store".into(),path});}
        }
        (metadata(root,game,&cancelled),artwork)
    })
}

fn metadata(root:&Path,game:&GameRecord,cancelled:impl Fn()->bool+Sync)->Labels {
    use crate::game_metadata::{MetadataQuery,ExternalId};
    let mut sources=BTreeMap::new();
    for source in game.installations.values().flat_map(|installation|&installation.sources) {
        sources.entry(format!("{}:{}",source.client,source.app_id)).or_insert(source);
    }
    let steam=sources.values().filter(|source|source.client=="steam").filter_map(|source|source.app_id.parse::<u32>().ok()).collect::<Vec<_>>();
    let platform=crate::game_installations::tags::platform::refresh(root,&steam,&cancelled);
    let queries=sources.into_iter().take(16).map(|(key,source)|MetadataQuery{key,title:game.name.clone(),
        external_ids:if source.client=="disk" {vec![]}else{vec![ExternalId{platform:source.client.clone(),id:source.app_id.clone()}]}}).collect::<Vec<_>>();
    let details=crate::game_metadata::refresh(root,&queries,&cancelled);
    let fallback=details.into_values().filter_map(|lookup|lookup.metadata).flat_map(|metadata|metadata.genres.into_iter().chain(metadata.genre_ancestors)).map(|genre|genre.label).collect();
    (platform.into_values().flatten().collect(),fallback)
}

fn refresh_with(settings:&mut Settings,id:&str,root:&Path,metadata:impl FnOnce(&GameRecord)->Found)->Result<(),String> {
    let previous=settings.games.get(id).cloned().ok_or("Choose a discovered game to rescan.")?;
    let ((primary,fallback),mut found)=metadata(&previous);
    let sources=previous.installations.values().flat_map(|installation|installation.sources.clone()).collect::<Vec<_>>();
    found.extend(crate::game_library::artwork_choices(&sources));
    let artwork=best_artwork(found);
    let game=settings.games.get_mut(id).expect("captured game remains present");
    for installation in game.installations.values_mut() {
        let candidates=crate::game_library::installation_executables(Path::new(&installation.root));
        installation.executables=crate::game_library::executable_ranking::rank(&game.name,&candidates,installation.executable.as_deref());
        // Refresh suggestions without changing a selected or deliberately cleared runtime.
    }
    if let Some(background)=artwork.and_then(|path|path.to_str().and_then(|path|crate::game_artwork::import(root,path,50.0,50.0).ok())) {
        game.default_background=Some(background.clone());settings.default_backgrounds.insert(id.into(),background);
    }
    if !settings.game_tags.contains_key(id) {
        let tags=crate::game_installations::tags::from_metadata(&settings.tags,&primary);
        let tags=if tags.is_empty(){crate::game_installations::tags::from_metadata(&settings.tags,&fallback)}else{tags};
        if !tags.is_empty(){settings.game_tags.insert(id.into(),tags);}
    }
    Ok(())
}

fn best_artwork(choices:Vec<crate::game_library::ArtworkChoice>)->Option<PathBuf> {
    let scored=choices.into_iter().filter_map(|choice| {
        let (width,height)=image::ImageReader::open(&choice.path).ok()?.with_guessed_format().ok()?.into_dimensions().ok()?;
        let name=choice.path.file_name()?.to_string_lossy().to_lowercase();
        let picture=!name.contains("logo")&&!name.contains("icon");
        let edge=u64::from(width.max(height).max(960));
        let crop_width=(u64::from(width)*960/edge).min(u64::from(height)*960*16/edge/9);
        Some(((picture,crop_width),choice.path))
    });
    scored.max_by(|left,right|left.0.cmp(&right.0).then(left.1.cmp(&right.1))).map(|(_,path)|path)
}

#[cfg(test)]
#[path="refresh_tests.rs"]
mod tests;
