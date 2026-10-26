use super::*;
use crate::{default_document, new_document};
fn identity(game: &str) -> GameIdentity {
    GameIdentity {
        game_id: game.into(),
        executable: format!("C:\\Games\\{game}\\game.exe"),
    }
}
fn observed(game: &str, session: &str) -> VerifiedGame {
    VerifiedGame {
        identity: identity(game),
        process_session: session.into(),
    }
}
fn rule(game: &str, profile: &ProfileDocument, priority: i16) -> Rule {
    Rule {
        identity: identity(game),
        profile_id: profile.id.clone(),
        priority,
        enabled: true,
    }
}
fn choose(rules: &[Rule], games: &[VerifiedGame], profiles: &[ProfileDocument]) -> Decision {
    select(
        SelectionMode::Automatic,
        Locks::default(),
        rules,
        games,
        profiles,
    )
    .unwrap()
}
#[test]
fn verified_identity_priority_exit_and_default_are_deterministic() {
    let a = new_document("A").unwrap();
    let b = new_document("B").unwrap();
    let profiles = vec![default_document(), a.clone(), b.clone()];
    let rules = vec![rule("dcs", &a, -4), rule("x-plane-12", &b, 9)];
    assert_eq!(
        choose(&rules, &[], &profiles),
        Decision::Select(Target::default())
    );
    let dcs = observed("dcs", "native-dcs-1");
    let xp = observed("x-plane-12", "native-xp-1");
    assert_eq!(
        choose(&rules, &[dcs.clone()], &profiles),
        Decision::Select(Target {
            profile_id: a.id.clone(),
            game_id: Some("dcs".into()),
            process_session: Some("native-dcs-1".into())
        })
    );
    let both = choose(&rules, &[dcs.clone(), xp.clone()], &profiles);
    assert_eq!(
        both,
        choose(
            &rules.iter().cloned().rev().collect::<Vec<_>>(),
            &[xp, dcs.clone()],
            &profiles
        )
    );
    assert!(matches!(both, Decision::Select(Target { profile_id, .. }) if profile_id == b.id));
    let mut wrong = dcs;
    wrong.identity.executable.push_str(".replacement");
    assert_eq!(
        choose(&rules, &[wrong], &profiles),
        Decision::Select(Target::default())
    );
    let mut disabled = rules;
    disabled[0].enabled = false;
    assert_eq!(
        choose(&disabled, &[observed("dcs", "new-epoch")], &profiles),
        Decision::Select(Target::default())
    );
}
#[test]
fn ties_hold_current_even_for_the_same_profile_and_ignore_lower_priority() {
    let a = new_document("A").unwrap();
    let rules = vec![
        rule("dcs", &a, 4),
        rule("x-plane-11", &a, 4),
        rule("assetto-corsa-rally", &a, 3),
    ];
    let games = vec![
        observed("x-plane-11", "xp"),
        observed("dcs", "dcs"),
        observed("assetto-corsa-rally", "acr"),
    ];
    assert_eq!(
        choose(&rules, &games, &[a]),
        Decision::Conflict(vec!["dcs".into(), "x-plane-11".into()])
    );
}
#[test]
fn manual_stop_fault_and_recovery_cannot_be_overridden() {
    let profiles = [default_document()];
    assert!(matches!(
        select(SelectionMode::Manual, Locks::default(), &[], &[], &profiles).unwrap(),
        Decision::Hold(_)
    ));
    for locks in [
        Locks {
            emergency_stopped: true,
            ..Locks::default()
        },
        Locks {
            recovery_needed: true,
            ..Locks::default()
        },
        Locks {
            faulted: true,
            ..Locks::default()
        },
    ] {
        assert!(matches!(
            select(SelectionMode::Automatic, locks, &[], &[], &profiles).unwrap(),
            Decision::Hold(_)
        ));
    }
}
#[test]
fn missing_or_ineligible_target_blocks_without_falling_to_an_unrelated_rule() {
    let mut a = new_document("A").unwrap();
    let r = rule("dcs", &a, 5);
    assert!(matches!(
        choose(&[r.clone()], &[observed("dcs", "one")], &[]),
        Decision::Hold(_)
    ));
    a.eligibility.game_ids = vec!["x-plane-11".into()];
    assert!(matches!(
        choose(&[r], &[observed("dcs", "one")], &[a]),
        Decision::Hold(_)
    ));
}
#[test]
fn strict_bounded_documents_and_observations_reject_browser_authority() {
    let a = default_document();
    let r = rule("dcs", &a, 0);
    let value = serde_json::to_value(&r).unwrap();
    assert_eq!(serde_json::from_value::<Rule>(value.clone()).unwrap(), r);
    for key in ["pid", "processSession", "sample", "resume", "token"] {
        let mut forged = value.clone();
        forged[key] = serde_json::json!(1);
        assert!(serde_json::from_value::<Rule>(forged).is_err());
    }
    assert!(validate_rules(&[r.clone(), r.clone()]).is_err());
    assert!(validate_rules(&vec![r.clone(); 5]).is_err());
    let mut bad = r.clone();
    bad.profile_id = "bad".into();
    assert!(validate_rules(&[bad]).is_err());
    for path in ["relative.exe", "C:relative.exe", "", "C:\\bad\n.exe"] {
        let mut bad = r.clone();
        bad.identity.executable = path.into();
        assert!(validate_rules(&[bad]).is_err());
    }
    let mut bad = r.clone();
    bad.identity.game_id = "Untrusted".into();
    assert!(validate_rules(&[bad]).is_err());
    // Game identity syntax is generic. A valid but unknown key can be retained
    // in rules; only the native watcher can produce a VerifiedGame observation.
    let generic = rule("flight-sim-unknown", &a, 0);
    assert!(validate_rules(&[generic]).is_ok());
    let mut duplicate = vec![observed("dcs", "one"), observed("dcs", "two")];
    assert!(select(
        SelectionMode::Automatic,
        Locks::default(),
        &[],
        &duplicate,
        &[]
    )
    .is_err());
    duplicate = vec![observed("dcs", "")];
    assert!(select(
        SelectionMode::Automatic,
        Locks::default(),
        &[],
        &duplicate,
        &[]
    )
    .is_err());
    assert!(select(
        SelectionMode::Automatic,
        Locks::default(),
        &[],
        &vec![observed("dcs", "one"); 5],
        &[]
    )
    .is_err());
    identity("dcs").validate().unwrap();
    GameIdentity {
        executable: "\\\\server\\game.exe".into(),
        ..identity("dcs")
    }
    .validate()
    .unwrap();
}
#[test]
fn repeated_observation_or_failed_apply_does_not_replay_effects() {
    let profile = default_document();
    let rules = [rule("dcs", &profile, 0)];
    let mut reconciler = Reconciler::default();
    let first = choose(&rules, &[observed("dcs", "epoch-1")], &[profile.clone()]);
    assert!(reconciler.next(&first, 0).is_some());
    assert!(reconciler.next(&first, 0).is_none()); // Includes an admission failure.
    assert!(reconciler
        .next(&Decision::Conflict(vec!["dcs".into()]), 0)
        .is_none());
    assert!(reconciler.next(&first, 0).is_none());
    assert!(reconciler.next(&first, 1).is_some()); // Explicit operator/rule change.
    let replaced = choose(&rules, &[observed("dcs", "epoch-2")], &[profile]);
    assert!(reconciler.next(&replaced, 1).is_some());
    assert!(reconciler
        .next(&Decision::Select(Target::default()), 1)
        .is_some());
    assert!(reconciler
        .next(&Decision::Select(Target::default()), 1)
        .is_none());
}

#[test]
fn automatic_selection_accepts_rules_and_observations_for_recognized_and_discovered_games() {
    let profile = new_document("All supported games").unwrap();
    let profiles = vec![default_document(), profile.clone()];
    let ids = ["dcs", "x-plane-11", "game-0123abcd"];
    let rules = ids
        .iter()
        .enumerate()
        .map(|(index, game)| rule(game, &profile, index as i16))
        .collect::<Vec<_>>();
    let games = ids
        .iter()
        .map(|game| observed(game, game))
        .collect::<Vec<_>>();
    validate_rules(&rules).unwrap();
    assert_eq!(rules.len(), ids.len());
    let Decision::Select(selected) = choose(&rules, &games, &profiles) else {
        panic!("All recognized and discovered game rules must be selectable");
    };
    assert_eq!(selected.game_id.as_deref(), Some("game-0123abcd"));
}

#[test]
fn generic_identity_is_stable_syntax_without_machine_local_membership() {
    let custom = "game-0123456789abcdef";
    assert!(valid_game_id(custom));
    identity(custom).validate().unwrap();
    for invalid in [
        "Game",
        "-game",
        "game_name",
        "game/name",
        "game-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    ] {
        assert!(!valid_game_id(invalid), "{invalid}");
        assert!(identity(invalid).validate().is_err());
    }
    let mut profile = new_document("Any game").unwrap();
    profile.eligibility.game_ids = vec![custom.into()];
    crate::validate_document(&profile).unwrap();
    let rules = [rule(custom, &profile, 0)];
    assert_eq!(
        choose(&rules, &[], &[profile.clone()]),
        Decision::Select(Target::default()),
        "an unavailable registration remains readable without matching a process"
    );
    let Decision::Select(target) = choose(&rules, &[observed(custom, "native-custom")], &[profile])
    else {
        panic!("A verified custom process must select its saved profile");
    };
    assert_eq!(target.game_id.as_deref(), Some(custom));
}

#[test]
fn game_and_rule_bounds_are_independent_of_the_integration_count() {
    let profile = new_document("Large library").unwrap();
    let rules = (0..MAX_RULES)
        .map(|index| {
            let game = format!("game-{}", uuid::Uuid::from_u128(index as u128 + 1));
            rule(&game, &profile, index as i16)
        })
        .collect::<Vec<_>>();
    validate_rules(&rules).unwrap();
    let games = rules
        .iter()
        .take(MAX_GAMES)
        .map(|rule| VerifiedGame {
            identity: rule.identity.clone(),
            process_session: "verified-native-session".into(),
        })
        .collect::<Vec<_>>();
    let Decision::Select(target) = choose(&rules, &games, &[profile.clone()]) else {
        panic!("large library");
    };
    assert_eq!(
        target.game_id,
        Some(rules[MAX_GAMES - 1].identity.game_id.clone())
    );
    let mut too_many = rules.clone();
    too_many.push(rule(&format!("game-{}", uuid::Uuid::new_v4()), &profile, 0));
    assert!(validate_rules(&too_many).is_err());
    let mut observations = games;
    observations.push(VerifiedGame {
        identity: rules[MAX_GAMES].identity.clone(),
        process_session: "one-more".into(),
    });
    assert!(select(
        SelectionMode::Automatic,
        Locks::default(),
        &rules,
        &observations,
        &[profile]
    )
    .is_err());
}
