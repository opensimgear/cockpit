use super::*;

#[test]
fn reordered_and_case_changed_runtime_observations_do_not_propose_an_update() {
    let directory = crate::test_directory();
    let root = directory.path().join("game");
    let mut imported = candidate(&root, "steam", "42", Some("dcs"));
    let alpha = root.join("Alpha.exe");
    let beta = root.join("Beta.exe");
    fs::write(&alpha, b"fixture").unwrap();
    fs::write(&beta, b"fixture").unwrap();
    imported.executable_candidates.extend([
        alpha.to_string_lossy().into_owned(),
        beta.to_string_lossy().into_owned(),
    ]);

    let empty = crate::game_installations::Settings::default();
    let saved = accept(&empty, &review(&empty, vec![imported.clone()]));

    imported.install_path = format!("{}{}", imported.install_path.to_uppercase(), std::path::MAIN_SEPARATOR);
    imported.executable_candidates.reverse();
    imported.executable_candidates = imported
        .executable_candidates
        .iter()
        .map(|path| path.to_uppercase())
        .collect();

    assert!(review(&saved, vec![imported]).entries.is_empty());
}

#[test]
fn missing_selected_runtime_still_proposes_a_refresh() {
    let directory = crate::test_directory();
    let root = directory.path().join("game");
    let imported = candidate(&root, "disk", "one", None);
    let empty = crate::game_installations::Settings::default();
    let saved = accept(&empty, &review(&empty, vec![imported.clone()]));
    let selected = saved.games.values().next().unwrap().installations
        .values().next().unwrap().executable.clone().unwrap();
    fs::remove_file(selected).unwrap();

    let pending = review(&saved, vec![imported]);

    assert_eq!(pending.entries.len(), 1);
    assert_eq!(pending.entries[0].update_kind.as_deref(), Some("refresh"));
}

#[test]
fn trailing_separator_observations_form_one_installation_with_both_sources() {
    let directory = crate::test_directory();
    let root = directory.path().join("game");
    let local = candidate(&root, "disk", "local", None);
    let mut platform = local.clone();
    platform.client = "steam".into();
    platform.app_id = "42".into();
    platform.install_path.push(std::path::MAIN_SEPARATOR);
    let empty = crate::game_installations::Settings::default();
    let pending = review(&empty, vec![local, platform]);
    assert_eq!(pending.entries.len(), 1);
    assert_eq!(pending.entries[0].sources.len(), 2);
    let saved = accept(&empty, &pending);
    assert_eq!(saved.games.values().next().unwrap().installations.len(), 1);
}
