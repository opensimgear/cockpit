use super::*;
use crate::tests::fixture;

fn baseline(registry: &mut Registry, id: &str) {
    registry
        .report(id, 1, &BTreeMap::from([(0, 1), (1, 123)]))
        .unwrap();
    registry.report(id, 2, &BTreeMap::from([(2, 1)])).unwrap();
    assert_eq!(
        registry.inspect(id, 0).unwrap()["source"]["baselineReady"],
        true
    );
}

#[test]
fn same_interface_layout_change_preserves_identity_metadata_and_fences_old_samples() {
    let mut registry = Registry::default();
    let id = registry
        .connect(fixture("same-usb-interface", None))
        .unwrap();
    registry.update(&id, "My joystick", "joystick").unwrap();
    baseline(&mut registry, &id);
    let before = registry.inspect(&id, 0).unwrap();
    let snapshot = registry.stream_snapshot(&[id.clone()]).unwrap();
    let mut changed = fixture("same-usb-interface", None);
    changed.controls[1].max = 16383;
    changed.controls.push(Control::new(0xff01, 2, 0, 0, 3));
    assert_eq!(registry.connect(changed).unwrap(), id);
    let after = registry.inspect(&id, 0).unwrap();
    assert_eq!(registry.list(0)["total"], 1);
    assert_eq!(after["source"]["name"], "My joystick");
    assert_eq!(after["source"]["role"], "joystick");
    assert_eq!(after["source"]["generation"], "2");
    assert_ne!(
        after["identity"]["fingerprint"],
        before["identity"]["fingerprint"]
    );
    assert_eq!(after["source"]["baselineReady"], false);
    assert!(after["controls"]
        .as_array()
        .unwrap()
        .iter()
        .all(|control| control["value"].is_null()));
    assert!(registry.stream_snapshot(&[id.clone()]).is_err());
    let page = registry.stream_page(&snapshot.session, snapshot.cursor);
    assert_eq!(page.events.len(), 1);
    assert_eq!(page.events[0].reason, Reason::Gap);
    assert!(page.events[0].values.is_empty());
    baseline(&mut registry, &id);
    let saved = registry.saved();
    let mut restored = Registry::default();
    restored.restore(saved).unwrap();
    assert_eq!(
        restored
            .connect(fixture("same-usb-interface", None))
            .unwrap(),
        id
    );
    assert_eq!(restored.list(0)["total"], 1);
}

#[test]
fn enumeration_order_and_unchanged_layout_keep_the_current_baseline() {
    let mut registry = Registry::default();
    let original = fixture("interface", None);
    let mut reordered = original.clone();
    reordered.controls.reverse();
    assert_eq!(
        fingerprint(&original.controls),
        fingerprint(&reordered.controls)
    );
    let id = registry.connect(original).unwrap();
    baseline(&mut registry, &id);
    let before = registry.inspect(&id, 0).unwrap();
    assert_eq!(registry.connect(reordered).unwrap(), id);
    let after = registry.inspect(&id, 0).unwrap();
    assert_eq!(
        after["source"]["generation"],
        before["source"]["generation"]
    );
    assert_eq!(after["source"]["sequence"], before["source"]["sequence"]);
    assert_eq!(after["source"]["baselineReady"], true);
    assert_eq!(registry.list(0)["total"], 1);
}

#[test]
fn changed_layout_never_joins_other_paths_collections_serials_or_products() {
    let mut registry = Registry::default();
    let first = registry.connect(fixture("interface", None)).unwrap();
    for field in ["path", "collection", "serial", "product", "backend"] {
        let mut other = fixture("interface", None);
        other.controls[1].min = -16384;
        match field {
            "path" => other.identity.path = "other-interface".into(),
            "collection" => other.identity.collection = "other-collection".into(),
            "serial" => other.identity.serial = Some("other-device".into()),
            "product" => other.identity.product_id += 1,
            "backend" => other.identity.backend = "other-backend".into(),
            _ => unreachable!(),
        }
        assert_ne!(registry.connect(other).unwrap(), first);
    }
    assert_eq!(registry.list(0)["total"], 6);
}

#[test]
fn remembered_duplicate_layout_never_replaces_the_online_identity() {
    let mut registry = Registry::default();
    let id = registry.connect(fixture("interface", None)).unwrap();
    let mut duplicate = registry.saved()[0].clone();
    duplicate.id = uuid::Uuid::new_v4().to_string();
    duplicate.identity.fingerprint = "0".repeat(64);
    registry.sources.insert(
        duplicate.id.clone(),
        Source {
            saved: duplicate,
            online: false,
            generation: 0,
            sequence: 0,
            controls: vec![],
            values: BTreeMap::new(),
            diagnostics: vec![],
        },
    );
    let mut changed = fixture("interface", None);
    changed.controls[1].max = 16383;
    assert_eq!(registry.connect(changed).unwrap(), id);
    assert!(registry.sources[&id].online);
    assert_eq!(
        registry
            .sources
            .values()
            .filter(|source| source.online)
            .count(),
        1
    );
}
