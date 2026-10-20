use super::*;
fn game(id: &str) -> GameIdentity {
    GameIdentity {
        game_id: id.into(),
        executable: format!("C:\\Games\\{id}\\game.exe"),
    }
}
fn profile(id: &str) -> osg_profiles::ProfileDocument {
    let mut document = osg_profiles::default_document();
    document.id = id.into();
    document.name = "Launch profile".into();
    document
}
const A: &str = "11111111-1111-4111-8111-111111111111";
const B: &str = "22222222-2222-4222-8222-222222222222";
fn rule(id: &str, profile: &str, priority: i16) -> Rule {
    Rule {
        identity: game(id),
        profile_id: profile.into(),
        priority,
        enabled: true,
    }
}
#[test]
fn manual_launch_uses_active_profile_or_default_and_ignores_the_retired_selection() {
    let documents = vec![osg_profiles::default_document(), profile(A)];
    let mut settings = Settings::default();
    settings.manual_profile_id = B.into();
    settings.rules = vec![rule("x-plane-12", osg_profiles::DEFAULT_PROFILE_ID, 100)];
    let running = vec![VerifiedGame {
        identity: game("x-plane-12"),
        process_session: "native-existing".into(),
    }];
    assert_eq!(
        launch_intent(&settings, &game("dcs"), &running, &documents, Some(A)).unwrap(),
        LaunchIntent {
            profile_id: A.into(),
            game_id: Some("dcs".into())
        }
    );
    assert_eq!(
        launch_intent(&settings, &game("dcs"), &running, &documents, None).unwrap(),
        LaunchIntent { profile_id: osg_profiles::DEFAULT_PROFILE_ID.into(), game_id: Some("dcs".into()) }
    );
    settings.mode = SelectionMode::Automatic;
    settings.rules.clear();
    assert_eq!(
        launch_intent(&settings, &game("dcs"), &[], &documents, Some(A)).unwrap(),
        LaunchIntent {
            profile_id: osg_profiles::DEFAULT_PROFILE_ID.into(),
            game_id: None
        }
    );
}
#[test]
fn proposed_native_identity_competes_by_priority_without_inventing_a_process_session() {
    let documents = vec![profile(A), profile(B)];
    let mut settings = Settings::default();
    settings.mode = SelectionMode::Automatic;
    settings.rules = vec![rule("dcs", A, 10), rule("x-plane-12", B, 5)];
    let running = vec![VerifiedGame {
        identity: game("x-plane-12"),
        process_session: "native-existing".into(),
    }];
    assert_eq!(
        launch_intent(&settings, &game("dcs"), &running, &documents, None).unwrap(),
        LaunchIntent {
            profile_id: A.into(),
            game_id: Some("dcs".into())
        }
    );
    settings.rules[1].priority = 15;
    assert_eq!(
        launch_intent(&settings, &game("dcs"), &running, &documents, None).unwrap(),
        LaunchIntent {
            profile_id: B.into(),
            game_id: Some("x-plane-12".into())
        }
    );
    settings.rules[1].priority = 10;
    assert!(launch_intent(&settings, &game("dcs"), &running, &documents, None)
        .unwrap_err()
        .contains("conflict"));
}
#[test]
fn disabled_changed_removed_or_ineligible_launch_rules_never_select_silently() {
    let mut documents = vec![osg_profiles::default_document(), profile(A)];
    let mut settings = Settings::default();
    settings.mode = SelectionMode::Automatic;
    settings.rules = vec![rule("dcs", A, 1)];
    settings.rules[0].enabled = false;
    assert_eq!(
        launch_intent(&settings, &game("dcs"), &[], &documents, None)
            .unwrap()
            .game_id,
        None
    );
    settings.rules[0].enabled = true;
    settings.rules[0].identity.executable.push_str(".old");
    assert_eq!(
        launch_intent(&settings, &game("dcs"), &[], &documents, None)
            .unwrap()
            .game_id,
        None
    );
    settings.rules[0].identity = game("dcs");
    documents[1].eligibility.game_ids = vec!["x-plane-12".into()];
    assert!(launch_intent(&settings, &game("dcs"), &[], &documents, None).is_err());
    documents.pop();
    assert!(launch_intent(&settings, &game("dcs"), &[], &documents, None).is_err());
}
