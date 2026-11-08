    #[test]
    fn assetto_launcher_handoff_selects_the_verified_leaf_of_one_parent_chain() {
        let (root, child, leaf) = (
            launcher_process(10, 1, 100),
            launcher_process(11, 10, 110),
            launcher_process(12, 11, 120),
        );
        for candidates in [
            vec![root.clone(), child.clone()],
            vec![child.clone(), root.clone()],
            vec![leaf.clone(), root.clone(), child.clone()],
        ] {
            let expected = if candidates.len() == 2 {
                child.0.clone()
            } else {
                leaf.0.clone()
            };
            assert_eq!(
                selected_process(&candidates, false).unwrap(),
                Some(expected)
            );
        }
    }
    #[test]
    fn independent_or_unverified_launcher_processes_and_runtime_duplicates_remain_ambiguous() {
        let (root, child) = (launcher_process(10, 1, 100), launcher_process(11, 10, 110));
        let mut foreign = child.clone();
        foreign.0.path = "D:/Steam/assettocorsa/AssettoCorsa.exe".into();
        let mut runtime_root = root.clone();
        runtime_root.0.path = "C:/Steam/assettocorsa/acs.exe".into();
        let mut runtime_child = child.clone();
        runtime_child.0.path = runtime_root.0.path.clone();
        for candidates in [
            vec![root.clone(), launcher_process(20, 1, 120)],
            vec![root.clone(), child.clone(), launcher_process(12, 10, 120)],
            vec![launcher_process(10, 11, 100), launcher_process(11, 10, 110)],
            vec![root.clone(), launcher_process(11, 10, 90)],
            vec![root.clone(), foreign],
            vec![runtime_root, runtime_child],
        ] {
            assert!(matches!(selected_process(&candidates, false), Err(ProcessLookupError::Ambiguous(_))));
        }
        assert!(matches!(selected_process(&[root, child], true), Err(ProcessLookupError::Ambiguous(_))));
    }

#[test]
fn persisted_discovery_is_the_only_title_universe_and_binding_is_optional() {
    use crate::game_installations::{Settings,GameRecord,Installation};
    let scratch = Scratch::new();
    let runtime = scratch.write("DCS/bin/DCS.exe", "");
    let root = scratch.0.join("DCS").display().to_string();
    let mut settings = Settings::default();
    assert!(merged_catalogue(&NativePlatform,&settings).is_empty());
    settings.games.insert("discovered-title".into(),GameRecord {
        name:"My installed simulator".into(),
        installations:BTreeMap::from([("copy-a".into(),Installation {root,executable:Some(runtime.display().to_string()),executables:vec![runtime.display().to_string()],sources:vec![]})]),
        selected_installation:Some("copy-a".into()),integration:None,default_background:None,
    });
    let plain = merged_catalogue(&NativePlatform,&settings);
    assert_eq!(plain.len(),1);
    assert_eq!(plain[0]["id"],"discovered-title");
    assert!(plain[0]["provider"].is_null());
    assert_eq!(plain[0]["integrationAvailable"], false);
    assert!(plain[0].get("integrationSupported").is_none());
    settings.games.get_mut("discovered-title").unwrap().integration = Some("dcs".into());
    let bound = merged_catalogue(&NativePlatform,&settings);
    assert_eq!(bound[0]["integrationKey"],"dcs");
    assert_eq!(bound[0]["provider"],"dcs-bios");
    assert_eq!(recognize_installation(&scratch.0.join("DCS"),&[]),Some(("dcs".into(),"DCS World".into(),"dcs".into())));
    settings.games.get_mut("discovered-title").unwrap().installations.get_mut("copy-a").unwrap().executable=None;
    let unresolved = merged_catalogue(&NativePlatform,&settings);
    assert_eq!(unresolved[0]["provider"],"dcs-bios");
    assert_eq!(unresolved[0]["integrationAvailable"],true);
    assert_eq!(unresolved[0]["integrationCompatible"],false);
    assert_eq!(integration_binding(&unresolved[0]),Some("dcs"));
    assert!(integration_key(&unresolved[0]).is_none());
    settings.games.get_mut("discovered-title").unwrap().installations.get_mut("copy-a").unwrap().executable=Some(runtime.display().to_string());
    settings.games.get_mut("discovered-title").unwrap().integration = Some("assetto-corsa".into());
    assert_eq!(merged_catalogue(&NativePlatform,&settings)[0]["integrationKey"],"assetto-corsa");
    assert_eq!(merged_catalogue(&NativePlatform,&settings)[0]["provider"],"assetto-corsa");
    assert_eq!(merged_catalogue(&NativePlatform,&settings)[0]["integrationCompatible"],false);
    assert!(integration_key(&merged_catalogue(&NativePlatform,&settings)[0]).is_none());
}
#[test]
fn removed_and_unavailable_selected_installations_never_change_the_launch_target() {
    use crate::game_installations::{Settings,GameRecord,Installation};
    let scratch = Scratch::new();
    let available = scratch.write("Available/Game.exe", "").display().to_string();
    let missing = scratch.0.join("Missing/Game.exe").display().to_string();
    let mut settings = Settings::default();
    settings.games.insert("title".into(),GameRecord {
        name:"Title".into(),installations:BTreeMap::from([
            ("available".into(),Installation {root:scratch.0.display().to_string(),executable:Some(available.clone()),executables:vec![available],sources:vec![]}),
            ("missing".into(),Installation {root:scratch.0.display().to_string(),executable:Some(missing.clone()),executables:vec![missing],sources:vec![]})]),
        selected_installation:Some("missing".into()),integration:None,default_background:None,
    });
    let games = merged_catalogue(&NativePlatform,&settings);
    assert_eq!(games[0]["installed"],false);
    assert!(games[0]["executable"].is_null());
    assert_eq!(games[0]["installations"][0]["available"],true);
    assert_eq!(games[0]["installations"][1]["available"],false);
}
    #[cfg(windows)]
    #[test]
    fn integration_inventory_keeps_results_ordered_and_invalid_paths_local() {
        let path = std::env::current_exe().unwrap();
        let first = json!({"id":"first","executable":path});
        let invalid = json!({"id":"invalid"});
        let second = json!({"id":"second","executable":path});
        let results = NativePlatform.integration_processes(&[&first, &invalid, &second]);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].as_ref().unwrap().as_ref().unwrap().pid, std::process::id());
        assert!(results[1].is_err());
        assert_eq!(results[0], results[2]);
    }

    #[cfg(windows)]
    #[test]
    fn activation_and_integration_share_one_background_capture() {
        let path = std::env::current_exe().unwrap();
        let game = json!({"id":"first","installed":true,"processMonitoringSupported":true,"executable":path});
        let invalid = json!({"id":"invalid","installed":true,"processMonitoringSupported":true});
        let mut calls = 0;
        let rows = capture_background_processes(&[game.clone(), invalid.clone()], |paths| {
            calls += 1;
            native::inventory(paths)
        });
        let activation = rows.activation(std::slice::from_ref(&game));
        let integration = rows.integration(&[&game, &invalid]);
        assert_eq!(calls, 1);
        assert_eq!(activation[0].as_ref().unwrap().as_ref().unwrap().pid, std::process::id());
        assert_eq!(activation[0].as_ref().unwrap(), integration[0].as_ref().unwrap());
        assert!(matches!(integration[1], Err(ProcessLookupError::Unavailable(_))));
        let mut replacement = game;
        replacement["executable"] = json!("replacement.exe");
        assert!(matches!(rows.integration(&[&replacement])[0], Err(ProcessLookupError::Unavailable(_))));
    }

    #[cfg(windows)]
    #[test]
    fn background_integration_capture_survives_the_activation_catalogue_limit() {
        let game = json!({"id":"dcs","installed":true,"processMonitoringSupported":true,
            "integrationKey":"dcs","provider":"dcs-bios","integrationCompatible":true,
            "executable":std::env::current_exe().unwrap()});
        let mut games = vec![Value::Null; osg_profiles::activation::MAX_GAMES];
        games.push(game.clone());
        let rows = capture_background_processes(&games, native::inventory);
        assert!(rows.activation.is_empty());
        assert_eq!(rows.integration(&[&game])[0].as_ref().unwrap().as_ref().unwrap().pid, std::process::id());
    }
