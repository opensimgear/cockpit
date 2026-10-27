use super::*;
use crate::game_installations::{Installation,LibrarySource};
use std::fs;

fn record(directory:&Path,name:&str)->GameRecord {
    let root=directory.join(name);fs::create_dir_all(&root).unwrap();let executable=root.join("Game.exe");fs::write(&executable,b"fixture").unwrap();
    let root=fs::canonicalize(root).unwrap().to_string_lossy().into_owned();
    let executable=fs::canonicalize(executable).unwrap().to_string_lossy().into_owned();
    GameRecord{name:name.into(),installations:BTreeMap::from([("installation".into(),Installation{
        root:root.clone(),executable:Some(executable.clone()),executables:vec![executable],
        sources:vec![LibrarySource{client:"disk".into(),app_id:name.into(),install_path:root,launcher:None}],
    })]),selected_installation:Some("installation".into()),integration:None,default_background:None}
}
fn labels(primary:&[&str],fallback:&[&str])->Found {
    ((primary.iter().map(|label|(*label).into()).collect(),fallback.iter().map(|label|(*label).into()).collect()),vec![])
}

#[test]
fn single_game_refresh_updates_its_runtime_and_artwork_without_touching_other_games() {
    let directory=crate::test_directory();let mut settings=Settings::default();
    settings.games.insert("first".into(),record(directory.path(),"First"));settings.games.insert("other".into(),record(directory.path(),"Other"));
    let other=settings.games["other"].clone();let first=settings.games["first"].clone();
    for name in ["first","other"] {
        let bin=Path::new(&settings.games[name].installations["installation"].root).join("bin");fs::create_dir_all(&bin).unwrap();fs::write(bin.join("NewRuntime.exe"),b"fixture").unwrap();
    }
    let image=Path::new(&first.installations["installation"].root).join("cover.jpg");image::RgbImage::from_pixel(920,518,image::Rgb([40,80,120])).save(&image).unwrap();
    refresh_with(&mut settings,"first",directory.path(),|game|{assert_eq!(game.name,"First");labels(&["Racing"],&[])}).unwrap();
    assert_eq!(settings.games["other"],other);
    let refreshed=&settings.games["first"];
    assert_eq!(refreshed.name,first.name);assert_eq!(refreshed.selected_installation,first.selected_installation);
    assert_eq!(refreshed.installations["installation"].executable,first.installations["installation"].executable);
    assert_eq!(refreshed.installations["installation"].executables.len(),2);
    assert!(refreshed.default_background.is_some());assert_eq!(settings.game_tags["first"],["racing"]);
    assert!(!settings.game_tags.contains_key("other"));settings.validate().unwrap();
}

#[test]
fn edits_empty_tag_choices_and_custom_backgrounds_are_preserved() {
    let directory=crate::test_directory();let mut base=Settings::default();base.games.insert("first".into(),record(directory.path(),"First"));
    let image=directory.path().join("manual.jpg");image::RgbImage::from_pixel(320,180,image::Rgb([190,20,70])).save(&image).unwrap();
    let custom=crate::game_artwork::import(directory.path(),image.to_str().unwrap(),20.0,30.0).unwrap();base.backgrounds.insert("first".into(),custom.clone());
    for ids in [vec!["flight".to_owned()],vec![]] {
        let mut settings=base.clone();settings.game_tags.insert("first".into(),ids.clone());
        settings.games.get_mut("first").unwrap().installations.get_mut("installation").unwrap().executable=None;
        refresh_with(&mut settings,"first",directory.path(),|_|labels(&["Racing"],&["Simulation"])).unwrap();
        assert_eq!(settings.game_tags["first"],ids);assert_eq!(settings.backgrounds["first"],custom);
        assert!(settings.games["first"].installations["installation"].executable.is_none());
    }
}

#[test]
fn unassigned_tags_can_use_fallback_metadata_and_deleted_tags_are_not_recreated() {
    let directory=crate::test_directory();let mut settings=Settings::default();settings.games.insert("first".into(),record(directory.path(),"First"));
    settings.tags.remove("racing");
    refresh_with(&mut settings,"first",directory.path(),|_|labels(&["Racing"],&["Simulation"])).unwrap();
    assert_eq!(settings.game_tags["first"],["simulation"]);assert!(!settings.tags.contains_key("racing"));
    let before=settings.clone();assert!(refresh_with(&mut settings,"absent",directory.path(),|_|panic!("Unknown game must not fetch")).is_err());assert_eq!(settings,before);
}

#[test]
fn unavailable_installations_or_metadata_do_not_remove_the_game_or_its_saved_runtime() {
    let directory=crate::test_directory();let mut settings=Settings::default();let mut game=record(directory.path(),"First");
    let original=game.installations["installation"].executable.clone();game.installations.get_mut("installation").unwrap().root=directory.path().join("missing").to_string_lossy().into_owned();settings.games.insert("first".into(),game);
    refresh_with(&mut settings,"first",directory.path(),|_|labels(&[],&[])).unwrap();
    assert_eq!(settings.games["first"].installations["installation"].executable,original);
    assert!(settings.games["first"].installations["installation"].executables.is_empty());assert!(settings.game_tags.is_empty());
}

#[test]
fn cancelled_remote_refresh_keeps_local_metadata_without_network_work() {
    let directory=crate::test_directory();let mut settings=Settings::default();settings.games.insert("first".into(),record(directory.path(),"First"));
    let before=settings.clone();
    refresh(&mut settings,"first",directory.path(),||true).unwrap();
    assert_eq!(settings,before);
}

#[test]
fn default_artwork_uses_resolution_remaining_after_preview_resize_and_tile_crop() {
    let directory=crate::test_directory();let header=directory.path().join("header.jpg");let hero=directory.path().join("hero.jpg");
    image::RgbImage::new(920,430).save(&header).unwrap();image::RgbImage::new(1920,620).save(&hero).unwrap();
    let choices=[header.clone(),hero].into_iter().map(|path|crate::game_library::ArtworkChoice{label:"Fixture".into(),path}).collect();
    assert_eq!(best_artwork(choices),Some(header));
}
