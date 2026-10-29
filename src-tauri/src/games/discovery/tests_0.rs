
    use super::*;
    #[cfg(windows)]
    #[test]
    #[ignore = "manual benchmark against the currently running DCS process"]
    fn benchmark_current_dcs_process_inventory() {
        let executable = std::env::var("OSG_BENCH_DCS_EXECUTABLE")
            .expect("set OSG_BENCH_DCS_EXECUTABLE to the configured DCS executable");
        let root = Path::new(&executable).parent().and_then(Path::parent).expect("DCS installation root");
        let process_executables = ["bin/DCS.exe", "bin-mt/DCS.exe"].iter()
            .map(|relative| root.join(relative).to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let game = json!({
            "id":"dcs", "installed":true, "integrationCompatible":true,
            "integrationKey":"dcs", "provider":"dcs-bios", "executable":executable,
            "processExecutables":process_executables
        });
        let paths = process_paths(&game).expect("configured DCS process paths");
        let mut samples = Vec::new();
        let mut outcomes = BTreeMap::<String, usize>::new();
        let mut last_processes = Vec::new();
        let mut last_errors = BTreeMap::new();
        for _ in 0..25 {
            let started = Instant::now();
            let inventory = native::inventory(&paths).expect("native process inventory");
            let elapsed = started.elapsed();
            samples.push(elapsed.as_micros());
            let matching = inventory.processes.iter().filter(|(process, _)| {
                paths.iter().any(|path| paths_match(path, &process.path))
            }).collect::<Vec<_>>();
            let outcome = match inventory.inspect(&paths, false) {
                Ok(Some(_)) => "one-process".to_owned(),
                Ok(None) => "no-process".to_owned(),
                Err(error) => format!("error: {error}"),
            };
            *outcomes.entry(outcome).or_default() += 1;
            last_processes = matching.iter().map(|(identity, _)| {
                format!("pid={} epoch={} path={}", identity.pid, identity.epoch, identity.path)
            }).collect::<Vec<_>>();
            last_errors = inventory.errors;
        }
        samples.sort_unstable();
        let percentile = |fraction: f64| samples[((samples.len() - 1) as f64 * fraction).round() as usize];
        eprintln!("DCS_PROCESS_INVENTORY_BENCH samples={} paths={paths:?} p50_us={} p95_us={} max_us={} outcomes={outcomes:?} processes={last_processes:?} errors={last_errors:?}",
            samples.len(), percentile(0.50), percentile(0.95), samples[samples.len() - 1]);
    }
    #[test]
    fn one_protected_basename_does_not_poison_unrelated_batched_processes() {
        let process = ProcessIdentity {
            pid: 42,
            path: "C:\\Games\\Other.exe".into(),
            epoch: 123,
        };
        let inventory = ProcessInventory {
            processes: vec![(process.clone(), 0)],
            errors: BTreeMap::from([(
                "protected.exe".into(),
                "Access denied for protected process".into(),
            )]),
        };
        assert!(inventory
            .inspect(&["C:\\Games\\Protected.exe".into()], false)
            .unwrap_err()
            .contains("Access denied"));
        assert_eq!(
            inventory
                .inspect(&["C:\\Games\\Other.exe".into()], false)
                .unwrap(),
            Some(process)
        );
        assert_eq!(
            inventory
                .inspect(&["C:\\Games\\Absent.exe".into()], false)
                .unwrap(),
            None
        );
    }
    #[test]
    fn library_launch_target_is_distinct_from_its_exact_monitored_runtime() {
        let directory = crate::test_directory();
        let runtime = directory.path().join("Game.exe");
        let steam = directory.path().join("steam.exe");
        fs::write(&runtime, b"game").unwrap();
        fs::write(&steam, b"client").unwrap();
        let mut game = json!({"id":"discovered-title","installed":true,"executable":runtime});
        let direct = launch_request(&game).unwrap();
        // Windows runner TEMP may use an 8.3 alias; launch validation resolves it.
        assert_eq!(direct.executable, fs::canonicalize(&runtime).unwrap());
        assert!(direct.arguments.is_empty());
        game["steamAppId"] = json!(42);
        game["steamExecutable"] = json!(steam);
        let through_client = launch_request(&game).unwrap();
        assert_eq!(through_client.arguments, vec!["-applaunch", "42"]);
        assert_eq!(through_client.executable, steam);
        assert_eq!(
            process_paths(&game).unwrap(),
            vec![fs::canonicalize(runtime)
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned()]
        );
    }
    #[test]
    fn executable_picker_treats_all_valid_game_ids_equally() {
        for game in [
            "dcs", "assetto-corsa-rally", "assetto-corsa",
            "assetto-corsa-competizione", "assetto-corsa-evo",
            "x-plane-11", "x-plane-12", "discovered-title", "custom-title",
        ] {
            assert_eq!(
                executable_picker(game).unwrap(),
                ("Choose the game's runtime executable".into(), "Game.exe"),
            );
        }
        for invalid in ["", "Invalid ID", "../game", &"a".repeat(97)] {
            assert!(executable_picker(invalid).is_err());
        }
    }
    #[test]
    fn xplane_steam_versions_manual_copies_and_launch_use_the_selected_installation() {
        let scratch = Scratch::new();
        let steam = scratch
            .write("Steam/steam.exe", "")
            .parent()
            .unwrap()
            .to_path_buf();
        for (v, appid) in [(11, 269950), (12, 2014780)] {
            scratch.write(
                &format!("Steam/steamapps/appmanifest_{appid}.acf"),
                &format!("AppState {{ appid {appid} installdir \"X-Plane {v}\" }}"),
            );
            scratch.write(
                &format!("Steam/steamapps/common/X-Plane {v}/X-Plane.exe"),
                "",
            );
            scratch.write(
                &format!("Steam/steamapps/common/X-Plane {v}/Resources/plugins/DataRefs.txt"),
                &format!("2 {v}10 Wed Jun 24 15:46:43 2026\n\n"),
            );
        }
        let roots = Roots {
            steam: vec![steam.clone(), steam],
            launch_supported: true,
            ..Roots::default()
        };
        let games = discover(&roots, &BTreeMap::new());
        for (game, v) in games.iter().zip([11, 12]) {
            assert_eq!(game["id"], format!("x-plane-{v}"));
            assert_eq!(game["installed"], true);
            assert_eq!(game["source"], "steam");
            assert_eq!(game["installations"].as_array().unwrap().len(), 1);
            let request = launch_request(game).unwrap();
            assert_eq!(request.executable.to_str(), game["executable"].as_str());
            assert!(request.arguments.is_empty());
            assert_eq!(
                process_paths(game).unwrap(),
                vec![game["executable"].as_str().unwrap()]
            );
        }
        let copy = scratch.write("Standalone copy/X-Plane.exe", "");
        scratch.write(
            "Standalone copy/Resources/plugins/DataRefs.txt",
            "2 1210 Wed Jun 24 15:46:43 2026\n\n",
        );
        assert!(NativePlatform
            .validate("invalid ID", copy.to_str().unwrap())
            .is_err());
        assert!(NativePlatform
            .validate("x-plane-12", copy.to_str().unwrap())
            .is_ok());
        let manual = BTreeMap::from([("x-plane-12".into(), copy.to_str().unwrap().into())]);
        let game = discover(&roots, &manual).remove(1);
        assert_eq!(game["source"], "manual");
        assert_eq!(game["executable"], copy.to_str().unwrap());
        assert_eq!(game["installations"].as_array().unwrap().len(), 2);
        assert_eq!(launch_request(&game).unwrap().executable, copy);
    }
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../.research/game-discovery-tests")
                .join(uuid::Uuid::new_v4().to_string());
            fs::create_dir_all(&root).unwrap();
            Self(fs::canonicalize(root).unwrap())
        }
        fn write(&self, path: &str, text: &str) -> PathBuf {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, text).unwrap();
            path
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn process() -> ProcessIdentity {
        ProcessIdentity {
            pid: 10,
            path: "C:\\Games\\DCS\\bin\\DCS.exe".into(),
            epoch: 50,
        }
    }
    #[test]
    fn process_identity_requires_nonzero_pid_epoch_and_text_path() {
        let identity = process();
        assert_eq!(ProcessIdentity::parse(&identity.value()).unwrap(), identity);
        for value in [
            json!({}),
            json!({"pid":0,"path":"a","epoch":1}),
            json!({"pid":u64::MAX,"path":"a","epoch":1}),
            json!({"pid":1,"path":"","epoch":1}),
            json!({"pid":1,"path":"a\u{0000}","epoch":1}),
            json!({"pid":1,"path":"a","epoch":0}),
        ] {
            assert!(ProcessIdentity::parse(&value).is_err());
        }
        assert!(paths_match("\\\\?\\C:\\Games\\DCS.exe", "c:/games/dcs.exe"));
        assert!(!paths_match("C:\\Games\\DCS.exe", "D:\\Games\\DCS.exe"));
    }
    #[test]
    fn vdf_preserves_quoted_values_escapes_unicode_comments_and_legacy_fields() {
        let parsed=parse_vdf("\u{feff}// heading\n LibraryFolders { 0 \"C:\\\\Steam\" \"1\" { path \"D:\\\\Spiele\\\\Rally ü\" label \"\\\"{\\\"\" } } ").unwrap();
        let folders = parsed.get("libraryfolders").unwrap();
        assert_eq!(folders.get("0").unwrap().text(), Some("C:\\Steam"));
        assert_eq!(
            folders.get("1").unwrap().get("path").unwrap().text(),
            Some("D:\\Spiele\\Rally ü")
        );
        assert_eq!(
            folders.get("1").unwrap().get("label").unwrap().text(),
            Some("\"{\"")
        );
        assert_eq!(
            parse_vdf("key \"C:\\Games\\DCS\"")
                .unwrap()
                .get("key")
                .unwrap()
                .text(),
            Some("C:\\Games\\DCS")
        );
        assert!(parsed.text().is_none());
        assert!(folders.get("none").is_none());
        assert!(Vdf::Text("scalar".into()).get("key").is_none());
    }
    #[test]
    fn incomplete_excessive_and_malformed_manifests_are_rejected() {
        for input in [
            "{",
            "}",
            "key",
            "key }",
            "key { value x",
            "key \"unterminated",
        ] {
            assert!(parse_vdf(input).is_err(), "{input}");
        }
        assert!(parse_vdf(&"x".repeat(1_048_577)).is_err());
        assert!(parse_vdf(&format!("{}{}", "key { ".repeat(18), "}".repeat(18))).is_err());
        let scratch = Scratch::new();
        assert!(read_vdf(&scratch.0.join("missing")).is_none());
        assert!(read_vdf(&scratch.write("large.vdf", &"x".repeat(1_048_577))).is_none());
    }
    fn vdf_path(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "\\\\")
    }
    #[test]
    fn discovers_steam_from_root_and_secondary_library_manifests() {
        let scratch = Scratch::new();
        let steam = scratch.0.join("Steam");
        let secondary = scratch.0.join("Rally library");
        scratch.write("Steam/steam.exe", "");
        scratch.write("Steam/steamapps/libraryfolders.vdf",&format!("\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} \"1\" {{ \"path\" \"{}\" }} \"metadata\" \"ignore\" \"2\" {{\"path\" \"relative\"}} }}",vdf_path(&steam),vdf_path(&secondary)));
        scratch.write(
            "Steam/steamapps/appmanifest_223750.acf",
            "\"AppState\" {\"appid\" \"223750\" \"installdir\" \"DCSWorld\"}",
        );
        scratch.write("Steam/steamapps/common/DCSWorld/bin/DCS.exe", "");
        scratch.write("Steam/steamapps/common/DCSWorld/bin-mt/DCS.exe", "");
        scratch.write(
            "Rally library/steamapps/appmanifest_3917090.acf",
            "AppState { appid 3917090 installdir \"Assetto Corsa Rally\" }",
        );
        scratch.write(
            "Rally library/steamapps/common/Assetto Corsa Rally/acr/Binaries/Win64/acr.exe",
            "",
        );
        scratch.write(
            "Rally library/steamapps/common/Assetto Corsa Rally/acr.exe",
            "",
        );
        let roots = Roots {
            steam: vec![steam.clone()],
            launch_supported: true,
            ..Roots::default()
        };
        let games = discover(&roots, &BTreeMap::new());
        assert_eq!(games.len(), 2);
        assert_eq!(games[0]["source"], "steam");
        assert_eq!(games[1]["source"], "steam");
        assert_eq!(games[1]["provider"], "assetto-corsa-rally");
        assert_eq!(games[0]["processExecutables"].as_array().unwrap().len(), 2);
        for game in &games[..2] {
            assert_eq!(game["installed"], true);
            assert_eq!(game["launchSupported"], true);
        }
        let request = launch_request(&games[1]).unwrap();
        assert_eq!(request.executable, steam.join("steam.exe"));
        assert_eq!(request.arguments, ["-applaunch", "3917090"]);
        assert_eq!(process_paths(&games[0]).unwrap().len(), 2);
        let rally_paths = process_paths(&games[1]).unwrap();
        assert_eq!(rally_paths.len(), 1);
        assert!(Path::new(&rally_paths[0]).ends_with("acr/Binaries/Win64/acr.exe"));
        let manual = BTreeMap::from([(
            "dcs".into(),
            games[0]["executable"].as_str().unwrap().into(),
        )]);
        let manual_game = discover(&roots, &manual)[0].clone();
        assert_eq!(manual_game["source"], "manual");
        assert_eq!(
            launch_request(&manual_game).unwrap().arguments,
            ["-applaunch", "223750"]
        );
    }
