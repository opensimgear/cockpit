#[test]
fn incomplete_alias_binding_exposes_only_passive_authoring_catalogue() {
    let (mut sources,_temporary)=sources();
    sources.games.seed_test_catalogue(vec![json!({"id":"disk-found-flight","name":"Disk flight","integrationKey":"x-plane-11","provider":"x-plane","integrationCompatible":false,"integrationAvailable":true,"installed":false,"executable":null,"processMonitoringSupported":false})]);
    let batches=sources.collect(1);
    let item=&batches.iter().find(|(scope,_)|scope=="game:disk-found-flight").unwrap().1.as_ref().unwrap().observations[0];
    assert_eq!(item.attributes["integrationKey"],"x-plane-11");
    assert_eq!(item.attributes["processVerified"],"false");
    assert!(item.ports.is_some());
    assert!(!item.route.as_ref().unwrap().writable);
    assert!(!item.route.as_ref().unwrap().baseline_ready);
}
