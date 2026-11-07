use super::*;

#[test]
fn unrelated_entries_do_not_hide_a_late_saved_games_profile() {
    let scratch = crate::test_directory();
    let saved_games = scratch.path();
    for index in 0..160 {
        fs::create_dir(saved_games.join(format!("AAA-other-{index:03}"))).unwrap();
    }
    fs::write(saved_games.join("DCS.not-a-folder"), "unrelated file").unwrap();
    let profile = saved_games.join("dCs.custom");
    fs::create_dir(&profile).unwrap();
    assert_eq!(candidates(saved_games, None).unwrap(), [profile.clone()]);
    fs::remove_dir(&profile).unwrap();
    assert!(candidates(saved_games, None).unwrap().is_empty());
}

#[test]
fn capped_variant_list_keeps_standard_folders_and_the_installation_hint() {
    let scratch = crate::test_directory();
    let saved_games = scratch.path().join("saved");
    let install = scratch.path().join("install");
    fs::create_dir_all(&saved_games).unwrap();
    fs::create_dir_all(&install).unwrap();
    let names = (0..MAX_DISCOVERED_VARIANTS + 2)
        .map(|index| saved_games.join(format!("DCS.variant-{index:03}")))
        .collect::<Vec<_>>();
    for path in names.iter().rev() {
        fs::create_dir(path).unwrap();
    }
    let standard = saved_games.join("DCS");
    let openbeta = saved_games.join("dcs.OpenBeta");
    fs::create_dir(&standard).unwrap();
    fs::create_dir(&openbeta).unwrap();
    fs::write(install.join("dcs_variant.txt"), "variant-129\n").unwrap();

    let mut expected = names[..MAX_DISCOVERED_VARIANTS].to_vec();
    expected.extend([standard, openbeta, names.last().unwrap().clone()]);
    expected.sort();
    assert_eq!(candidates(&saved_games, Some(&install)).unwrap(), expected);
}

#[cfg(windows)]
#[test]
fn installation_hint_does_not_duplicate_a_case_insensitive_variant() {
    let scratch = crate::test_directory();
    let saved_games = scratch.path().join("saved");
    let install = scratch.path().join("install");
    let profile = saved_games.join("dcs.Custom");
    fs::create_dir_all(&profile).unwrap();
    fs::create_dir_all(&install).unwrap();
    fs::write(install.join("dcs_variant.txt"), "custom\n").unwrap();
    assert_eq!(candidates(&saved_games, Some(&install)).unwrap(), [profile]);
}
