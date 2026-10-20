use super::*;
fn game(id: &str) -> GameIdentity {
    GameIdentity {
        game_id: id.into(),
        executable: format!("C:\\Games\\{id}\\game.exe"),
    }
}
fn profile(id: &str, context: Option<&str>) -> osg_profiles::ProfileDocument {
    let mut document = osg_profiles::default_document();
    document.id = id.into();
    document.name = "Launch profile".into();
    document.eligibility.game_ids = context.map(|id| vec![id.into()]).unwrap_or_default();
    document
}
const A: &str = "11111111-1111-4111-8111-111111111111";
const B: &str = "22222222-2222-4222-8222-222222222222";
#[test]
fn saved_context_candidates_choose_one_in_both_modes_and_none_uses_hardware_fallback() {
    let documents = vec![
        osg_profiles::default_document(),
        profile(A, Some("dcs")),
        profile(B, None),
    ];
    let mut settings = Settings::default();
    for mode in [SelectionMode::Manual, SelectionMode::Automatic] {
        settings.mode = mode;
        assert_eq!(
            launch_intent(&settings, &game("dcs"), &documents, None).unwrap(),
            LaunchIntent {
                profile_id: A.into(),
                game_id: Some("dcs".into())
            }
        );
        assert_eq!(
            launch_intent(&settings, &game("x-plane-12"), &documents, None).unwrap(),
            LaunchIntent {
                profile_id: osg_profiles::DEFAULT_PROFILE_ID.into(),
                game_id: None
            }
        );
    }
}
#[test]
fn ambiguous_saved_context_needs_explicit_or_remembered_matching_choice() {
    let documents = vec![
        osg_profiles::default_document(),
        profile(A, Some("dcs")),
        profile(B, Some("dcs")),
    ];
    let mut settings = Settings::default();
    assert!(launch_intent(&settings, &game("dcs"), &documents, None)
        .unwrap_err()
        .contains("Several profiles"));
    assert_eq!(
        launch_intent(&settings, &game("dcs"), &documents, Some(B))
            .unwrap()
            .profile_id,
        B
    );
    settings.preferred_profiles.insert("dcs".into(), A.into());
    assert_eq!(
        launch_intent(&settings, &game("dcs"), &documents, None)
            .unwrap()
            .profile_id,
        A
    );
    assert_eq!(
        launch_intent(&settings, &game("dcs"), &documents, Some(B))
            .unwrap()
            .profile_id,
        B
    );
    assert!(launch_intent(&settings, &game("x-plane-12"), &documents, Some(A)).is_err());
}
#[test]
fn removed_or_changed_context_invalidates_remembered_choice_without_promoting_unscoped_profiles() {
    let mut documents = vec![
        osg_profiles::default_document(),
        profile(A, Some("dcs")),
        profile(B, Some("dcs")),
    ];
    let mut settings = Settings::default();
    settings.preferred_profiles.insert("dcs".into(), A.into());
    documents[1].eligibility.game_ids.clear();
    assert_eq!(
        launch_intent(&settings, &game("dcs"), &documents, None)
            .unwrap()
            .profile_id,
        B
    );
    documents.pop();
    assert_eq!(
        launch_intent(&settings, &game("dcs"), &documents, None)
            .unwrap()
            .game_id,
        None
    );
    documents.push(profile(B, Some("dcs")));
    documents.remove(1);
    assert_eq!(
        launch_intent(&settings, &game("dcs"), &documents, None)
            .unwrap()
            .profile_id,
        B
    );
}
