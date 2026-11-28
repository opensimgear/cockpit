use super::*;
fn scan() -> Result<Inventory, inventory::Failure> {
    Ok(inventory::tests::fixture_inventory())
}

#[test]
fn list_inspect_and_selection_are_scoped_to_discovery_evidence() {
    // Freeze the fixture scan so discovery and inspection see the same epoch.
    thread_local! {static FIXTURE:Inventory=scan().unwrap();}
    let backend = Linux {
        scan: || Ok(FIXTURE.with(Clone::clone)),
        ..Default::default()
    };
    assert!(backend.inventory(Some("/dev/hidraw0")).is_err());
    let listed = backend.inventory(None).unwrap();
    let id = listed["interfaces"][0]["interfaceId"].as_str().unwrap();
    assert!(listed["interfaces"][0].get("descriptor").is_none());
    let detail = backend.inventory(Some(id)).unwrap();
    assert_eq!(detail["descriptorHex"], detail["descriptor"]["raw"]);
    assert!(detail["descriptorHex"]
        .as_str()
        .is_some_and(|s| !s.is_empty()));
    let mut removed = backend.selected(id).unwrap();
    assert!(backend.open(id).is_err());
    removed.restricted = true;
    assert!(receiver::open(removed)
        .err()
        .unwrap()
        .contains("Metadata only"));
    let mut changed = backend.selected(id).unwrap();
    changed.hid_parent.push("replacement");
    assert_ne!(key(&changed), id);
}
