#[test]
fn configured_game_membership_keeps_missing_installations_and_excludes_unsupported_titles() {
    let (mut sources, _temporary) = sources();
    let integration = |id: &str| json!({"id":id,"name":id,"integrationKey":"x-plane-11","provider":"x-plane","installed":false,"integrationAvailable":true,"integrationCompatible":false});
    sources.games.seed_test_catalogue(vec![integration("removed")]);
    assert!(sources.collect_games(1).iter().any(|(scope, _)| scope == "game:removed"));
    assert!(sources.catalogues.contains_key("removed"));
    sources.games.seed_test_catalogue(vec![integration("missing"),json!({"id":"diablo","name":"Diablo","installed":true,"integrationAvailable":false})]);
    let batches = sources.collect_games(2);
    let marker = batches.iter().find(|(scope, _)| scope == "game-catalogue").unwrap().1.as_ref().unwrap();
    assert!(marker.complete && marker.observations.is_empty());
    assert!(!sources.catalogues.contains_key("removed"));
    let games = batches.iter().filter(|(scope, _)| scope.starts_with("game:")).collect::<Vec<_>>();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].0, "game:missing");
    let item = &games[0].1.as_ref().unwrap().observations[0];
    assert_eq!(item.attributes["installed"], "false");
    assert_eq!(item.attributes["integrationKey"], "x-plane-11");
    sources.games.seed_test_catalogue(vec![]);
    assert_eq!(sources.collect_games(3).len(), 1);
    assert!(sources.catalogues.is_empty());
}
