use super::*;
use std::fs;
#[path = "unchanged_runtime_tests.rs"]
mod unchanged_runtime_tests;
fn candidate(root: &Path, client: &str, app_id: &str, title: Option<&str>) -> Candidate {
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("Game.exe"), b"fixture").unwrap();
    Candidate {
        id: format!("{client}:{app_id}"),
        client: client.into(),
        name: "Discovered game".into(),
        install_path: root.to_string_lossy().into_owned(),
        executable_candidates: vec![root.join("Game.exe").to_string_lossy().into_owned()],
        preferred_executable: None,
        game_id: title.map(str::to_owned),
        kind: "game".into(),
        app_id: app_id.into(),
        launcher: None,
        artwork: None,
    }
}
fn review(saved: &crate::game_installations::Settings, candidates: Vec<Candidate>) -> Review {
    let clients = crate::game_library::CLIENTS
        .iter()
        .map(|(id, _)| (*id).into())
        .collect();
    Review {
        scan_id: "scan".into(),
        status: "ready".into(),
        entries: reconcile::reconcile(saved, candidates, &clients),
        revision: saved.revision,
        ..Default::default()
    }
}
fn accept(
    saved: &crate::game_installations::Settings,
    review: &Review,
) -> crate::game_installations::Settings {
    apply(
        saved,
        review,
        &json!({"scanId":review.scan_id,"actions":[]}),
    )
    .unwrap()
}
#[test]
fn sources_merge_into_one_installation_and_known_titles_share_multiple_copies() {
    let directory = crate::test_directory();
    let root = directory.path().join("first");
    let other = directory.path().join("second");
    let saved = crate::game_installations::Settings::default();
    let first = candidate(&root, "steam", "223750", Some("dcs"));
    let mut disk = first.clone();
    disk.client = "disk".into();
    disk.app_id = "disk-copy".into();
    disk.launcher = None;
    let second = candidate(&other, "disk", "other-copy", Some("dcs"));
    let review = review(&saved, vec![first, disk, second]);
    assert_eq!(review.entries.len(), 2);
    assert!(review
        .entries
        .iter()
        .all(|entry| entry.selected && entry.action == "add"));
    let next = accept(&saved, &review);
    assert_eq!(next.games.len(), 1);
    let game = &next.games["dcs"];
    assert_eq!(game.installations.len(), 2);
    assert_eq!(game.integration.as_deref(), Some("dcs"));
    assert!(game
        .installations
        .values()
        .any(|instance| instance.sources.len() == 2));
}
#[test]
fn changed_source_path_updates_same_instance_without_changing_title_or_selection() {
    let directory = crate::test_directory();
    let first = directory.path().join("first");
    let moved = directory.path().join("moved");
    let candidate = candidate(&first, "gog", "42", None);
    let saved = crate::game_installations::Settings::default();
    let saved = accept(&saved, &review(&saved, vec![candidate.clone()]));
    let game_id = saved.games.keys().next().unwrap().clone();
    let installation = saved.games[&game_id].selected_installation.clone();
    fs::rename(&first, &moved).unwrap();
    let mut changed = candidate;
    changed.install_path = moved.to_string_lossy().into_owned();
    changed.executable_candidates = vec![moved.join("Game.exe").to_string_lossy().into_owned()];
    let pending = review(&saved, vec![changed]);
    assert_eq!(pending.entries.len(), 1);
    assert_eq!(pending.entries[0].action, "update");
    assert_eq!(
        pending.entries[0].update_kind.as_deref(),
        Some("replace-path")
    );
    let next = accept(&saved, &pending);
    assert_eq!(next.games[&game_id].selected_installation, installation);
    assert_eq!(next.games[&game_id].installations.len(), 1);
    assert_eq!(
        next.games[&game_id]
            .installations
            .values()
            .next()
            .unwrap()
            .root,
        fs::canonicalize(moved).unwrap().to_string_lossy()
    );
}
#[test]
fn removals_require_explicit_selection_and_successful_library_observation() {
    let directory = crate::test_directory();
    let root = directory.path().join("gone");
    let saved = crate::game_installations::Settings::default();
    let saved = accept(
        &saved,
        &review(&saved, vec![candidate(&root, "disk", "one", None)]),
    );
    fs::remove_dir_all(&root).unwrap();
    assert!(reconcile::reconcile(&saved, vec![], &BTreeSet::new()).is_empty());
    let pending = review(&saved, vec![]);
    assert_eq!(pending.entries.len(), 1);
    assert!(!pending.entries[0].selected);
    assert_eq!(accept(&saved, &pending).games.len(), 1);
    let next = apply(
        &saved,
        &pending,
        &json!({"scanId":"scan","actions":[{"id":pending.entries[0].id,"selected":true}]}),
    )
    .unwrap();
    assert!(next.games.is_empty());
    fs::create_dir_all(&root).unwrap();
    assert!(apply(
        &saved,
        &pending,
        &json!({"scanId":"scan","actions":[{"id":pending.entries[0].id,"selected":true}]})
    )
    .is_err());
}
#[test]
fn stale_scans_and_cross_installation_executables_do_not_publish_changes() {
    let directory = crate::test_directory();
    let root = directory.path().join("game");
    let saved = crate::game_installations::Settings::default();
    let pending = review(&saved, vec![candidate(&root, "disk", "one", None)]);
    let outside = directory.path().join("Outside.exe");
    fs::write(&outside, b"fixture").unwrap();
    assert!(apply(&saved,&pending,&json!({"scanId":"scan","actions":[{"id":pending.entries[0].id,"selected":true,"executable":outside}]})).is_err());
    assert!(apply(&saved, &pending, &json!({"scanId":"wrong","actions":[]})).is_err());
    let mut newer = saved.clone();
    newer.revision = 1;
    assert!(apply(&newer, &pending, &json!({"scanId":"scan","actions":[]})).is_err());
    assert!(saved.games.is_empty());
}
#[test]
fn repeated_observations_keep_unresolved_and_selected_runtime_choices() {
    let directory = crate::test_directory();
    let root = directory.path().join("game");
    let candidate = candidate(&root, "disk", "one", None);
    let saved = crate::game_installations::Settings::default();
    let saved = accept(&saved, &review(&saved, vec![candidate.clone()]));
    let pending = review(&saved, vec![candidate]);
    assert!(pending.entries.is_empty());
    assert_eq!(accept(&saved, &pending).games, saved.games);
}

#[test]
fn explicit_new_instance_keeps_the_previous_instance_after_a_move() {
    let directory = crate::test_directory();
    let first = directory.path().join("first");
    let moved = directory.path().join("moved");
    let empty = crate::game_installations::Settings::default();
    let saved = accept(
        &empty,
        &review(&empty, vec![candidate(&first, "gog", "42", None)]),
    );
    let game_id = saved.games.keys().next().unwrap();
    let old_id = saved.games[game_id].selected_installation.as_ref().unwrap();
    let old_root = saved.games[game_id].installations[old_id].root.clone();
    fs::rename(&first, &moved).unwrap();
    let pending = review(&saved, vec![candidate(&moved, "gog", "42", None)]);
    assert_eq!(
        pending.entries[0].update_kind.as_deref(),
        Some("replace-path")
    );

    let next = apply(
        &saved,
        &pending,
        &json!({"scanId":"scan","actions":[{
            "id":pending.entries[0].id,"selected":true,"replaceInstallationId":null
        }]}),
    )
    .unwrap();

    let game = &next.games[game_id];
    assert_eq!(game.installations.len(), 2);
    assert_eq!(game.installations[old_id].root, old_root);
    assert_eq!(game.selected_installation.as_ref(), Some(old_id));
    let moved_root = fs::canonicalize(&moved)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(game
        .installations
        .iter()
        .any(|(id, instance)| id != old_id && instance.root == moved_root));
}

#[test]
fn replacing_a_different_root_does_not_inherit_its_previous_runtime() {
    let directory = crate::test_directory();
    let first = directory.path().join("first");
    let replacement = directory.path().join("replacement");
    let empty = crate::game_installations::Settings::default();
    let saved = accept(
        &empty,
        &review(&empty, vec![candidate(&first, "gog", "42", None)]),
    );
    let game_id = saved.games.keys().next().unwrap();
    let old_id = saved.games[game_id].selected_installation.as_ref().unwrap();
    let mut observed = candidate(&replacement, "gog", "42", None);
    let other = replacement.join("Other.exe");
    fs::write(&other, b"fixture").unwrap();
    observed
        .executable_candidates
        .push(other.to_string_lossy().into_owned());
    let pending = review(&saved, vec![observed]);
    let proposed = fs::canonicalize(pending.entries[0].executable.as_ref().unwrap())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let replacement_root = fs::canonicalize(&replacement)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(proposed.starts_with(&replacement_root));
    assert!(first.join("Game.exe").is_file());

    let next = apply(
        &saved,
        &pending,
        &json!({"scanId":"scan","actions":[{
            "id":pending.entries[0].id,"selected":true,"replaceInstallationId":old_id
        }]}),
    )
    .unwrap();

    let installation = &next.games[game_id].installations[old_id];
    assert_eq!(next.games[game_id].installations.len(), 1);
    assert_eq!(
        installation.root,
        fs::canonicalize(replacement).unwrap().to_string_lossy()
    );
    assert_eq!(installation.executable.as_deref(), Some(proposed.as_str()));
}

#[test]
fn split_sources_replace_a_missing_instance_once_and_add_the_second_copy() {
    let directory = crate::test_directory();
    let first = directory.path().join("first");
    let steam_root = directory.path().join("steam");
    let gog_root = directory.path().join("gog");
    let empty = crate::game_installations::Settings::default();
    let saved = accept(
        &empty,
        &review(
            &empty,
            vec![
                candidate(&first, "steam", "42", None),
                candidate(&first, "gog", "7", None),
            ],
        ),
    );
    let game_id = saved.games.keys().next().unwrap();
    let old_id = saved.games[game_id].selected_installation.as_ref().unwrap();
    assert_eq!(saved.games[game_id].installations[old_id].sources.len(), 2);
    fs::remove_dir_all(&first).unwrap();
    let pending = review(
        &saved,
        vec![
            candidate(&steam_root, "steam", "42", None),
            candidate(&gog_root, "gog", "7", None),
        ],
    );
    assert_eq!(pending.entries.len(), 2);
    assert_eq!(
        pending
            .entries
            .iter()
            .filter(|entry| entry.update_kind.as_deref() == Some("replace-path"))
            .count(),
        1
    );

    let next = accept(&saved, &pending);

    assert_eq!(next.games.len(), 1);
    let game = &next.games[game_id];
    assert_eq!(game.installations.len(), 2);
    assert!(game.installations.contains_key(old_id));
    assert_eq!(game.selected_installation.as_ref(), Some(old_id));
    for root in [&steam_root, &gog_root] {
        let root = fs::canonicalize(root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(game
            .installations
            .values()
            .any(|instance| instance.root == root));
    }
}

#[test]
fn pending_unknown_titles_can_share_a_game_independently_of_review_order() {
    let directory = crate::test_directory();
    let empty = crate::game_installations::Settings::default();
    let mut source = candidate(&directory.path().join("source"), "steam", "42", None);
    let mut target = candidate(&directory.path().join("target"), "epic", "target", None);
    source.name = "Alpha source".into();
    target.name = "Zulu target".into();
    let mut pending = review(&empty, vec![source, target]);
    let source_id = pending.entries[0].id.clone();
    let target_id = pending.entries[1].game_id.clone();
    let request = json!({"scanId":"scan","actions":[{
        "id":source_id,"selected":true,"gameId":target_id
    }]});

    for _ in 0..2 {
        let next = apply(&empty, &pending, &request).unwrap();
        assert_eq!(next.games.len(), 1);
        let game = &next.games[&target_id];
        assert_eq!(game.name, "Zulu target");
        assert_eq!(game.installations.len(), 2);
        pending.entries.reverse();
    }
}

#[test]
fn pending_game_matches_resolve_transitively_and_reject_cycles() {
    let directory = crate::test_directory();
    let empty = crate::game_installations::Settings::default();
    let candidates = [
        ("a", "steam", "42"),
        ("b", "gog", "7"),
        ("c", "epic", "target"),
    ]
    .into_iter()
    .map(|(name, client, app)| {
        let mut observed = candidate(&directory.path().join(name), client, app, None);
        observed.name = name.into();
        observed
    })
    .collect();
    let pending = review(&empty, candidates);
    let [a, b, c] = pending.entries.as_slice() else {
        panic!("three distinct pending games")
    };
    let next = apply(
        &empty,
        &pending,
        &json!({"scanId":"scan","actions":[
            {"id":a.id,"selected":true,"gameId":b.game_id},
            {"id":b.id,"selected":true,"gameId":c.game_id}
        ]}),
    )
    .unwrap();
    assert_eq!(next.games.len(), 1);
    assert_eq!(next.games[&c.game_id].installations.len(), 3);
    assert_eq!(next.games[&c.game_id].name, "c");

    assert!(apply(
        &empty,
        &pending,
        &json!({"scanId":"scan","actions":[
            {"id":a.id,"selected":true,"gameId":b.game_id},
            {"id":b.id,"selected":true,"gameId":a.game_id}
        ]})
    )
    .is_err());
    assert!(empty.games.is_empty());
}

#[test]
fn new_installation_selects_the_best_title_match_from_multiple_executables() {
    let directory = crate::test_directory();
    let root = directory.path().join("ets");
    let mut observed = candidate(&root, "disk", "ets", None);
    observed.name = "Euro Truck Simulator 20".into();
    let sequel = root.join("ETS20.exe");
    let launcher = root.join("EuroTruckSimulatorLauncher.exe");
    fs::write(&sequel, b"fixture").unwrap();
    fs::write(&launcher, b"fixture").unwrap();
    observed.executable_candidates = vec![
        root.join("Game.exe").to_string_lossy().into_owned(),
        launcher.to_string_lossy().into_owned(),
        sequel.to_string_lossy().into_owned(),
    ];

    let pending = review(
        &crate::game_installations::Settings::default(),
        vec![observed],
    );

    assert!(pending.entries[0]
        .executable
        .as_deref()
        .unwrap()
        .ends_with("ETS20.exe"));
    assert!(pending.entries[0].executable_candidates[0].ends_with("ETS20.exe"));
}

#[test]
fn refresh_preserves_an_existing_valid_user_selected_executable() {
    let directory = crate::test_directory();
    let root = directory.path().join("game");
    let saved = crate::game_installations::Settings::default();
    let mut first = candidate(&root, "disk", "one", None);
    let selected_by_ranker = root.join("DiscoveredGame.exe");
    let explicitly_selected = root.join("Other.exe");
    fs::write(&selected_by_ranker, b"fixture").unwrap();
    fs::write(&explicitly_selected, b"fixture").unwrap();
    first.executable_candidates.extend([
        selected_by_ranker.to_string_lossy().into_owned(),
        explicitly_selected.to_string_lossy().into_owned(),
    ]);
    let mut saved = accept(&saved, &review(&saved, vec![first.clone()]));
    let game_id = saved.games.keys().next().unwrap().clone();
    let installation_id = saved.games[&game_id].selected_installation.clone().unwrap();
    let selected = fs::canonicalize(explicitly_selected).unwrap();
    saved
        .games
        .get_mut(&game_id)
        .unwrap()
        .installations
        .get_mut(&installation_id)
        .unwrap()
        .executable = Some(selected.to_string_lossy().into_owned());

    first
        .executable_candidates
        .push(root.join("NewRuntime.exe").to_string_lossy().into_owned());
    fs::write(root.join("NewRuntime.exe"), b"fixture").unwrap();
    let pending = review(&saved, vec![first]);

    assert_eq!(pending.entries.len(), 1);
    assert_eq!(pending.entries[0].update_kind.as_deref(), Some("refresh"));
    assert_eq!(
        pending.entries[0].executable.as_deref(),
        Some(selected.to_str().unwrap())
    );
    assert!(pending.entries[0].executable_candidates[0].ends_with("DiscoveredGame.exe"));
}

#[test]
fn explicit_null_executable_clears_the_automatic_suggestion() {
    let directory = crate::test_directory();
    let root = directory.path().join("game");
    let empty = crate::game_installations::Settings::default();
    let pending = review(&empty, vec![candidate(&root, "disk", "one", None)]);

    assert!(pending.entries[0].executable.is_some());
    let next = apply(
        &empty,
        &pending,
        &json!({"scanId":"scan","actions":[{
            "id":pending.entries[0].id,"selected":true,"executable":null
        }]}),
    )
    .unwrap();

    let game = next.games.values().next().unwrap();
    let installation = game.installations.values().next().unwrap();
    assert!(installation.executable.is_none());
}
