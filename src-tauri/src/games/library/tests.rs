use super::*;
#[test]
fn long_executable_lists_and_many_diagnostics_share_one_serialized_budget() {
    let mut scan = Scan::default();
    for index in 0..96 {
        scan.candidates.push(Candidate {
            id: format!("steam:{index}"),
            client: "steam".into(),
            name: format!("Game {index}"),
            install_path: "C:\\Games".into(),
            executable_candidates: (0..16)
                .map(|file| format!("C:\\{}\\Game{file}.exe", "x".repeat(3000)))
                .collect(),
            preferred_executable: None,
            game_id: None,
            kind: "unknown".into(),
            app_id: index.to_string(),
            launcher: None,
            artwork: None,
        });
    }
    for _ in 0..1000 {
        scan.diagnostic("Malformed manifest ".repeat(1000));
    }
    bound_metadata(&mut scan);
    assert!(!scan.candidates.is_empty());
    assert!(scan.candidates.len() < 96);
    assert_eq!(scan.diagnostics.len(), MAX_DIAGNOSTICS);
    assert!(scan
        .diagnostics
        .iter()
        .any(|message| message.contains("omitted")));
    assert!(scan
        .diagnostics
        .iter()
        .any(|message| message.contains("native response limit")));
    let response = serde_json::json!({"libraryCandidates":scan.candidates,"libraryDiagnostics":scan.diagnostics});
    assert!(serde_json::to_vec(&response).unwrap().len() <= MAX_SCAN_BYTES);
}
#[test]
fn steam_multiple_libraries_reuse_builtin_ids_and_discover_nested_art_without_importing() {
    let directory = crate::test_directory();
    let root = directory.path().join("Steam");
    let library = directory.path().join("Second");
    fs::create_dir_all(root.join("steamapps")).unwrap();
    fs::create_dir_all(library.join("steamapps/common/Racer/bin")).unwrap();
    fs::create_dir_all(root.join("appcache/librarycache/42")).unwrap();
    fs::write(root.join("steam.exe"), b"fixture").unwrap();
    fs::write(
        root.join("steamapps/libraryfolders.vdf"),
        format!(
            "\"libraryfolders\" {{ \"1\" {{ \"path\" \"{}\" }} }}",
            library.display()
        ),
    )
    .unwrap();
    fs::write(
        library.join("steamapps/appmanifest_42.acf"),
        b"\"AppState\" {\"appid\" \"42\" \"name\" \"Racer\" \"installdir\" \"Racer\"}",
    )
    .unwrap();
    fs::write(
        library.join("steamapps/common/Racer/bin/Racer.exe"),
        b"fixture",
    )
    .unwrap();
    image::RgbImage::from_pixel(640, 360, image::Rgb([40, 80, 120]))
        .save(root.join("appcache/librarycache/42/hashed_header.jpg"))
        .unwrap();
    fs::create_dir_all(root.join("steamapps/common/DCS")).unwrap();
    fs::write(
        root.join("steamapps/appmanifest_223750.acf"),
        b"\"AppState\" {\"appid\" \"223750\" \"name\" \"DCS\" \"installdir\" \"DCS\"}",
    )
    .unwrap();
    let scan = scan_roots(&[root], &[]);
    assert_eq!(scan.candidates.len(), 2);
    let racer = scan
        .candidates
        .iter()
        .find(|game| game.app_id == "42")
        .unwrap();
    assert_eq!(racer.executable_candidates.len(), 1);
    assert!(racer.artwork.is_some());
    assert_eq!(
        scan.candidates
            .iter()
            .find(|game| game.app_id == "223750")
            .unwrap()
            .game_id
            .as_deref(),
        Some("dcs")
    );
}
#[test]
fn epic_uses_manifest_runtime_and_skips_incomplete_and_traversal_installs() {
    let directory = crate::test_directory();
    let manifests = directory.path().join("manifests");
    let game = directory.path().join("game");
    fs::create_dir_all(&manifests).unwrap();
    fs::create_dir_all(game.join("Binaries")).unwrap();
    fs::write(game.join("Binaries/Game.exe"), b"exe").unwrap();
    let mut value = serde_json::json!({"AppName":"EpicGame","DisplayName":"Epic game","InstallLocation":game,"LaunchExecutable":"Binaries/Game.exe"});
    fs::write(
        manifests.join("game.item"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    value["AppName"] = serde_json::json!("Incomplete");
    value["bIsIncompleteInstall"] = serde_json::json!(true);
    fs::write(
        manifests.join("incomplete.item"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    let scan = scan_roots(&[], &[manifests]);
    assert_eq!(scan.candidates.len(), 1);
    assert_eq!(scan.candidates[0].client, "epic");
    assert!(scan.candidates[0].executable_candidates[0].ends_with("Game.exe"));
}
#[test]
fn epic_engine_plugins_and_dlc_are_not_game_candidates() {
    for value in [
        serde_json::json!({"bIsApplication":false,"AppCategories":["engines/ue5","engines"],"CatalogNamespace":"ue"}),
        serde_json::json!({"bIsApplication":false,"AppCategories":["plugins/engine","plugins"]}),
        serde_json::json!({"AppCategories":["dlc"],"MainGameAppName":"BaseGame"}),
        serde_json::json!({"MainGameAppName":"BaseGame"}),
    ] {
        assert_eq!(epic::epic_kind(&value, "App"), None);
    }
    assert_eq!(
        epic::epic_kind(
            &serde_json::json!({"bIsApplication":true,"AppCategories":["games"],"MainGameAppName":"App"}),
            "App"
        ),
        Some("game")
    );
}
#[test]
fn steam_rejects_ambiguous_manifest_identities_and_path_traversal() {
    let directory = crate::test_directory();
    let root = directory.path();
    fs::create_dir_all(root.join("steamapps/common/Game")).unwrap();
    for (filename, manifest) in [
        (
            "appmanifest_1.acf",
            r#""AppState" {"appid" "2" "installdir" "Game"}"#,
        ),
        (
            "appmanifest_3.acf",
            r#""AppState" {"appid" "3" "appid" "4" "installdir" "Game"}"#,
        ),
        (
            "appmanifest_5.acf",
            r#""AppState" {"appid" "5" "installdir" "../Game"}"#,
        ),
    ] {
        fs::write(root.join("steamapps").join(filename), manifest).unwrap();
    }
    let scan = scan_roots(&[root.to_owned()], &[]);
    assert!(scan.candidates.is_empty());
    assert!(scan
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.contains("identity does not match")));
}

#[test]
fn runtime_suggestions_reject_installers_and_preferred_paths_outside_the_game() {
    let temp = crate::test_directory();
    let game = temp.path().join("game");
    fs::create_dir_all(&game).unwrap();
    for path in [
        game.join("Game.exe"),
        game.join("Setup.exe"),
        temp.path().join("Outside.exe"),
    ] {
        fs::write(path, b"fixture").unwrap();
    }
    let outside = temp.path().join("Outside.exe");
    for preferred in [outside, game.join("Setup.exe")] {
        let candidates = executable_candidates(&game, Some(&preferred));
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].ends_with("Game.exe"));
    }
}

#[test]
fn local_scan_returns_only_bounded_distinct_existing_installations() {
    let scan = super::scan();
    assert!(scan.candidates.len() <= MAX_CANDIDATES);
    assert!(scan.diagnostics.len() <= MAX_DIAGNOSTICS);
    let mut ids = BTreeSet::new();
    for candidate in &scan.candidates {
        assert!(ids.insert(&candidate.id));
        candidate.source().validate().unwrap();
        assert!(Path::new(&candidate.install_path).is_dir());
        assert!(clean_name(&candidate.name).is_some());
        assert!(candidate.executable_candidates.len() <= 32);
    }
}

fn scan_roots(steam: &[PathBuf], epic: &[PathBuf]) -> Scan {
    let mut scan = Scan::default();
    steam::steam_candidates(steam, &mut scan);
    epic::epic_candidates(epic, &mut scan);
    finish(scan)
}
