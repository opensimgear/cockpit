use super::*;

fn request(directory: &Path) -> Value {
    let root=directory.join("Manual game");fs::create_dir_all(root.join("bin")).unwrap();
    let executable=root.join("bin/Main.exe");fs::write(&executable,b"harmless fixture").unwrap();
    json!({"command":"game_add","name":"My local game","installPath":root,"executable":executable})
}

#[test]
fn manual_add_creates_a_game_and_image_without_starting_or_replacing_discovery() {
    let (runtime,directory)=fixture();let mut command=request(directory.path());
    let image=directory.path().join("image.png");image::RgbImage::new(640,360).save(&image).unwrap();
    command["imagePath"]=json!(image);
    let discovery=runtime.status()["discovery"].clone();
    runtime.metadata_request(&command).unwrap();
    let value=runtime.status();let id=value["addedGameId"].as_str().unwrap();
    let game=value["catalogue"].as_array().unwrap().iter().find(|game|game["id"]==id).unwrap();
    assert_eq!(game["name"],"My local game");assert_eq!(game["installed"],true);
    assert_eq!(game["source"],"disk");assert!(game["background"]["image"].as_str().unwrap().starts_with("data:image/jpeg;base64,"));
    assert_eq!(value["discovery"],discovery);
    let mut candidate=crate::game_library::manual_candidate(Path::new(command["installPath"].as_str().unwrap()),Path::new(command["executable"].as_str().unwrap()),"My local game").unwrap();
    candidate.install_path=fs::canonicalize(Path::new(command["installPath"].as_str().unwrap()).join("bin")).unwrap().to_string_lossy().into_owned();
    let saved=saved(&runtime);
    assert!(!discovery_review::test_reconcile(&saved,vec![candidate]).iter().any(|entry|entry.game_id==id));
    let reopened=crate::game_installations::Store::new(directory.path().join("game-library.json")).load().unwrap().unwrap();
    assert!(reopened.games.contains_key(id));runtime.shutdown();
}

#[test]
fn manual_add_rejects_invalid_or_duplicate_folders_and_does_not_publish_partial_games() {
    let (runtime,directory)=fixture();let command=request(directory.path());let before=saved(&runtime);
    let outside=directory.path().join("Outside.exe");fs::write(&outside,b"fixture").unwrap();
    let broken=directory.path().join("broken.png");fs::write(&broken,b"invalid image").unwrap();
    for (key,value) in [
        ("installPath",json!("relative")),("executable",json!(outside)),("name",json!(" ")),
        ("imagePath",json!(broken)),("unexpected",json!(true)),
    ] {
        let mut invalid=command.clone();invalid[key]=value;
        assert!(runtime.metadata_request(&invalid).is_err(),"{invalid}");assert_eq!(saved(&runtime),before);
    }
    runtime.metadata_request(&command).unwrap();let added=saved(&runtime);
    assert!(runtime.metadata_request(&command).unwrap_err().contains("already in your library"));
    assert_eq!(saved(&runtime),added);runtime.shutdown();
}

#[test]
fn manual_add_retains_recognized_telemetry_identity() {
    let (runtime,directory)=fixture();let root=directory.path().join("DCS");fs::create_dir_all(root.join("bin")).unwrap();
    let executable=root.join("bin/DCS.exe");fs::write(&executable,b"fixture").unwrap();
    runtime.metadata_request(&json!({"command":"game_add","name":"My DCS","installPath":root,"executable":executable})).unwrap();
    let settings=saved(&runtime);assert_eq!(settings.games["dcs"].integration.as_deref(),Some("dcs"));
    assert_eq!(settings.games["dcs"].name,"My DCS");runtime.shutdown();
}

#[test]
fn manual_add_accepts_an_explicit_executable_excluded_from_automatic_suggestions() {
    let (runtime,directory)=fixture();let root=directory.path().join("Explicit game");fs::create_dir_all(&root).unwrap();
    let executable=root.join("Setup.exe");fs::write(&executable,b"fixture").unwrap();
    runtime.metadata_request(&json!({"command":"game_add","name":"Explicit game","installPath":root,"executable":executable})).unwrap();
    assert!(saved(&runtime).games.values().any(|game|game.name=="Explicit game"&&game.installations.values().any(|installation|installation.executable.as_deref().unwrap().ends_with("Setup.exe"))));
    runtime.shutdown();
}
