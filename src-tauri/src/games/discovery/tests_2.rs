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
            assert!(selected_process(&candidates, false).is_err());
        }
        assert!(selected_process(&[root, child], true).is_err());
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
