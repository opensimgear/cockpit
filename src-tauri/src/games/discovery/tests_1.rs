    #[test]
    fn legacy_library_file_and_manifest_validation_prevent_false_discovery() {
        let scratch = Scratch::new();
        let steam = scratch.0.join("Steam");
        let library = scratch.0.join("Library");
        scratch.write(
            "Steam/config/libraryfolders.vdf",
            &format!(
                "LibraryFolders {{ 0 \"{}\" 1 \"{}\" 3 relative }}",
                vdf_path(&library),
                vdf_path(&library)
            ),
        );
        assert_eq!(library_paths(&steam), vec![steam.clone(), library.clone()]);
        for manifest in [
            "AppState { appid 223750 installdir wrong }",
            "AppState { appid nope installdir Rally }",
            "AppState { appid 3917090 installdir \"../Rally\" }",
            "AppState { appid 3917090 installdir .. }",
            "AppState { appid 3917090 installdir \"\" }",
            "AppState { appid 3917090 installdir \"C:\\Rally\" }",
        ] {
            scratch.write("Library/steamapps/appmanifest_3917090.acf", manifest);
            assert!(steam_installation(&GAMES[1], &library).is_none());
        }
        scratch.write(
            "Library/steamapps/appmanifest_3917090.acf",
            "AppState { appid 3917090 installdir Rally }",
        );
        let games = discover(
            &Roots {
                steam: vec![steam],
                ..Roots::default()
            },
            &BTreeMap::new(),
        );
        assert!(games.is_empty());
    }
    #[test]
    fn standalone_manual_roots_executables_and_missing_paths() {
        let scratch = Scratch::new();
        let executable = scratch.write("DCS/bin-mt/DCS.exe", "");
        let root = scratch.0.join("DCS");
        let roots = Roots {
            dcs: vec![root.clone()],
            launch_supported: true,
            ..Roots::default()
        };
        let standalone = discover(&roots, &BTreeMap::new());
        assert_eq!(standalone[0]["source"], "standalone");
        assert_eq!(standalone[0]["launchSupported"], true);
        let request = launch_request(&standalone[0]).unwrap();
        assert_eq!(request.executable, executable);
        assert_eq!(request.directory, root);
        assert!(request.arguments.is_empty());
        for selected in [&root, &executable] {
            let manual = BTreeMap::from([("dcs".into(), selected.display().to_string())]);
            let games = discover(&roots, &manual);
            assert_eq!(games[0]["source"], "manual");
            assert_eq!(games[0]["installPath"], root.display().to_string());
        }
        let manual = BTreeMap::from([(
            "dcs".into(),
            scratch.0.join("missing").display().to_string(),
        )]);
        assert!(discover(&roots, &manual).is_empty());
        let manual = BTreeMap::from([("dcs".into(), " ".into())]);
        assert_eq!(discover(&roots, &manual)[0]["source"], "standalone");
        assert!(installation(&GAMES[0], Path::new("relative")).is_none());
        assert!(validated_executable(&GAMES[0], &scratch.write("wrong.exe", "")).is_err());
        let custom = scratch.write("custom/DCS.exe", "");
        assert_eq!(
            installation(&GAMES[0], &custom).unwrap().2,
            scratch.0.join("custom")
        );
    }
    #[test]
    fn rally_manual_detection_requires_steam_and_launch_data_is_revalidated() {
        let scratch = Scratch::new();
        let executable = scratch.write("Rally/acr/Binaries/Win64/acr.exe", "");
        let manual = BTreeMap::from([(
            "assetto-corsa-rally".into(),
            executable.display().to_string(),
        )]);
        let roots = Roots {
            launch_supported: true,
            ..Roots::default()
        };
        let mut game = discover(&roots, &manual)[0].clone();
        assert_eq!(game["installed"], true);
        assert_eq!(game["launchSupported"], false);
        assert_eq!(
            game["installPath"],
            scratch.0.join("Rally").display().to_string()
        );
        assert!(launch_request(&game).is_err());
        game["steamExecutable"] = json!(scratch.write("Steam/steam.exe", "").display().to_string());
        assert!(launch_request(&game).is_ok());
        game["steamAppId"] = json!(223750);
        assert!(launch_request(&game).is_err());
        game["steamAppId"] = Value::Null;
        assert!(launch_request(&game).is_err());
        game["steamAppId"] = json!(3917090);
        game["steamExecutable"] = json!(scratch.write("Steam/wrong.exe", "").display().to_string());
        assert!(launch_request(&game).is_err());
        game["installed"] = json!(false);
        assert!(launch_request(&game).is_err());
        assert!(launch_request(&json!({"id":"unknown"})).is_err());
        assert!(launch_request(&json!({"id":"dcs"})).is_err());
        assert!(process_paths(&json!({"id":"dcs"})).is_err());
        assert!(process_paths(&json!({"id":"dcs","executable":"missing"})).is_err());
        let root_executable = scratch.write("RallyRoot/acr.exe", "");
        assert_eq!(
            installation(&GAMES[1], &root_executable).unwrap().2,
            scratch.0.join("RallyRoot")
        );
        assert_eq!(
            process_paths(&json!({"id":"assetto-corsa-rally","executable":root_executable}))
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn startup_wait_handles_delayed_handoff_timeout_and_inspection_error() {
        let mut attempts = 0;
        let found = wait_for_process(
            Duration::from_secs(1),
            || {
                attempts += 1;
                Ok(if attempts == 3 { Some(process()) } else { None })
            },
            |_| {},
        )
        .unwrap();
        assert_eq!(found, process());
        assert_eq!(attempts, 3);
        assert!(wait_for_process(Duration::ZERO, || Ok(None), |_| {})
            .unwrap_err()
            .contains("timeout"));
        assert_eq!(
            wait_for_process(Duration::ZERO, || Err("inspection failed".into()), |_| {})
                .unwrap_err(),
            "inspection failed"
        );
    }
    #[test]
    #[cfg(not(windows))]
    fn non_windows_catalogue_supports_manual_detection_but_refuses_live_launch() {
        let platform = NativePlatform;
        let scratch = Scratch::new();
        let executable = scratch.write("DCS/bin/DCS.exe", "");
        platform
            .validate("dcs", executable.to_str().unwrap())
            .unwrap();
        assert!(platform
            .validate("Invalid ID", executable.to_str().unwrap())
            .is_err());
        let manual = BTreeMap::from([("dcs".into(), executable.display().to_string())]);
        let catalogue = platform.catalogue(&manual);
        assert_eq!(catalogue[0]["installed"], true);
        assert_eq!(catalogue[0]["launchSupported"], false);
        assert!(platform
            .launch(&catalogue[0])
            .unwrap_err()
            .contains("Windows"));
        assert!(platform
            .find(&catalogue[0])
            .unwrap_err()
            .contains("Windows"));
        assert!(platform.alive(&process()).unwrap_err().contains("Windows"));
        assert!(native::spawn(&launch_request(&catalogue[0]).unwrap()).is_err());
    }
    #[test]
    fn assetto_titles_discover_launch_and_monitor_their_own_native_runtime() {
        let scratch = Scratch::new();
        let steam = scratch
            .write("Steam/steam.exe", "")
            .parent()
            .unwrap()
            .to_path_buf();
        let titles = [
            ("assetto-corsa", 244210, "assettocorsa", "acs.exe"),
            (
                "assetto-corsa-competizione",
                805550,
                "Assetto Corsa Competizione",
                "AC2/Binaries/Win64/AC2-Win64-Shipping.exe",
            ),
            (
                "assetto-corsa-evo",
                3058630,
                "Assetto Corsa EVO",
                "AssettoCorsaEVO.exe",
            ),
        ];
        for (_, app, folder, runtime) in titles {
            scratch.write(
                &format!("Steam/steamapps/appmanifest_{app}.acf"),
                &format!("AppState {{ appid {app} installdir \"{folder}\" }}"),
            );
            scratch.write(&format!("Steam/steamapps/common/{folder}/{runtime}"), "");
        }
        // ACC's bootstrap can remain alongside the long-lived simulator process.
        scratch.write(
            "Steam/steamapps/common/Assetto Corsa Competizione/AC2.exe",
            "",
        );
        scratch.write("Steam/steamapps/common/assettocorsa/AssettoCorsa.exe", "");
        let roots = Roots {
            steam: vec![steam.clone()],
            launch_supported: true,
            ..Roots::default()
        };
        let games = discover(&roots, &BTreeMap::new());
        for (id, app, folder, runtime) in titles {
            let game = games.iter().find(|game| game["id"] == id).unwrap();
            assert_eq!(game["provider"], id);
            assert_eq!(game["installed"], true);
            let launch = launch_request(game).unwrap();
            assert_eq!(launch.executable, steam.join("steam.exe"));
            assert_eq!(
                launch.arguments,
                vec!["-applaunch".to_owned(), app.to_string()]
            );
            let process = process_paths(game).unwrap();
            assert_eq!(process.len(), 1);
            assert!(Path::new(&process[0]).ends_with(runtime));
            let manual_path = scratch
                .0
                .join(format!("Steam/steamapps/common/{folder}/{runtime}"));
            let manual = discover(
                &roots,
                &BTreeMap::from([(id.into(), manual_path.display().to_string())]),
            );
            let selected = manual.iter().find(|game| game["id"] == id).unwrap();
            assert_eq!(selected["source"], "manual");
            assert_eq!(
                selected["installPath"],
                steam
                    .join("steamapps/common")
                    .join(folder)
                    .display()
                    .to_string()
            );
            assert!(NativePlatform
                .validate(id, manual_path.to_str().unwrap())
                .is_ok());
            for (other, _, _, _) in titles {
                if other != id {
                    assert!(NativePlatform
                        .validate("Invalid ID", manual_path.to_str().unwrap())
                        .is_err());
                }
            }
        }
    }

    #[test]
    fn assetto_corsa_menu_completes_launch_without_becoming_a_telemetry_producer() {
        let scratch = Scratch::new();
        let runtime = scratch.write("AC/acs.exe", "");
        let launcher = scratch.write("AC/AssettoCorsa.exe", "");
        let game = json!({"id":"assetto-corsa","integrationCompatible":true,"integrationKey":"assetto-corsa","provider":"assetto-corsa","executable":runtime});
        let menu = ProcessIdentity {
            pid: 42,
            epoch: 1,
            path: launcher.display().to_string(),
        };
        let mut scans = 0;
        let found = find_game_process(&game, false, |paths| {
            scans += 1;
            Ok((paths == [launcher.display().to_string()]).then(|| menu.clone()))
        })
        .unwrap();
        assert_eq!(found, Some(menu));
        assert_eq!(scans, 2);
        let mut scans = 0;
        assert!(find_game_process(&game, true, |_| {
            scans += 1;
            Ok(None)
        })
        .unwrap()
        .is_none());
        assert_eq!(scans, 1);
        let driving = ProcessIdentity {
            pid: 43,
            epoch: 2,
            path: runtime.display().to_string(),
        };
        let mut scans = 0;
        assert_eq!(
            find_game_process(&game, false, |_| {
                scans += 1;
                Ok(Some(driving.clone()))
            })
            .unwrap(),
            Some(driving)
        );
        assert_eq!(scans, 1);
    }

    fn launcher_process(pid: u32, parent: u32, epoch: u64) -> (ProcessIdentity, u32) {
        (
            ProcessIdentity {
                pid,
                path: "C:/Steam/assettocorsa/AssettoCorsa.exe".into(),
                epoch,
            },
            parent,
        )
    }
