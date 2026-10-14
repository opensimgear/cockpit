use super::*;
use crate::registry::*;
use serde_json::json;
use std::collections::BTreeMap;
pub(crate) fn fixture(path: &str, serial: Option<&str>) -> Definition {
    let mut axis = Control::new(1, 0x30, 1, 0, 1);
    axis.bits = 16;
    axis.min = -32768;
    axis.max = 32767;
    let mut hat = Control::new(1, 0x39, 2, 1, 2);
    hat.bits = 4;
    hat.min = 1;
    hat.max = 8;
    hat.has_null = true;
    Definition {
        identity: Identity {
            backend: "test".into(),
            vendor_id: 0x231d,
            product_id: 0x200,
            serial: serial.map(str::to_owned),
            collection: "interface-0/collection-1".into(),
            fingerprint: String::new(),
            path: path.into(),
        },
        name: "Controller".into(),
        controls: vec![Control::new(9, 1, 1, 0, 0), axis, hat],
        diagnostics: vec![],
    }
}
#[test]
fn signed_unsigned_axes_and_null_hats() {
    let device = fixture("a", None);
    let axis = &device.controls[1];
    let hat = &device.controls[2];
    assert_eq!(
        axis.decode(0x8000).unwrap(),
        Sample {
            native: -32768,
            normalized: Some(-32768)
        }
    );
    assert_eq!(axis.decode(0x7fff).unwrap().normalized, Some(32767));
    assert_eq!(axis.decode(0xffff).unwrap().native, -1);
    assert_eq!(hat.decode(0).unwrap().normalized, None);
    assert_eq!(hat.decode(1).unwrap().normalized, Some(0));
    assert_eq!(hat.decode(8).unwrap().normalized, Some(7));
    let mut unipolar = axis.clone();
    unipolar.min = 0;
    unipolar.max = 1023;
    unipolar.bits = 10;
    assert_eq!(unipolar.decode(0).unwrap().normalized, Some(0));
    assert_eq!(unipolar.decode(1023).unwrap().normalized, Some(32767));
    let mut four = hat.clone();
    four.min = 0;
    four.max = 3;
    assert_eq!(four.decode(3).unwrap().normalized, Some(6));
    four.has_null = false;
    assert!(four.decode(15).is_err());
    four.max = 5;
    assert!(four.decode(2).is_err());
    unipolar.bits = 0;
    assert!(unipolar.decode(0).is_err());
    unipolar.bits = 33;
    assert!(unipolar.decode(0).is_err());
    unipolar.bits = 32;
    unipolar.max = i64::from(u32::MAX);
    assert_eq!(unipolar.decode(u32::MAX).unwrap().normalized, Some(32767));
    unipolar.min = 10;
    unipolar.max = 0;
    assert!(unipolar.decode(0).is_err());
    let unknown = Control::new(0xff01, 1, 0, 0, 0);
    assert_eq!(unknown.decode(1).unwrap().normalized, None);
    for usage in [0xba, 0xbb, 0xc4, 0xc5, 0xc6, 0xc8] {
        assert_eq!(Control::new(2, usage, 0, 0, 0).kind, Kind::Axis);
    }
}
#[test]
fn report_ids_are_atomic_and_stationary_state_is_retained() {
    let mut r = Registry::default();
    let id = r.connect(fixture("a", None)).unwrap();
    assert_eq!(r.inspect(&id, 0).unwrap()["source"]["baselineReady"], false);
    r.report(&id, 1, &BTreeMap::from([(0, 1), (1, 0xffff)]))
        .unwrap();
    assert_eq!(r.inspect(&id, 0).unwrap()["source"]["baselineReady"], false);
    r.report(&id, 2, &BTreeMap::from([(2, 8)])).unwrap();
    let detail = r.inspect(&id, 0).unwrap();
    assert_eq!(detail["source"]["baselineReady"], true);
    for _ in 0..100 {
        assert_eq!(r.inspect(&id, 0).unwrap(), detail);
    }
    r.report(&id, 1, &BTreeMap::from([(1, 0)])).unwrap();
    let after = r.inspect(&id, 0).unwrap();
    assert_eq!(after["controls"][0]["value"]["normalized"], 0);
    assert_eq!(after["controls"][2]["value"]["normalized"], 7);
    r.fault(&id, "gap");
    assert_eq!(r.inspect(&id, 0).unwrap()["source"]["generation"], "2");
    assert!(r.inspect(&id, 0).unwrap()["controls"][0]["value"].is_null());
    r.disconnect(&id);
    assert!(r.report(&id, 1, &BTreeMap::new()).is_err());
    assert_eq!(r.inspect(&id, 0).unwrap()["source"]["online"], false);
    assert_eq!(r.connect(fixture("a", None)).unwrap(), id);
    assert_eq!(r.inspect(&id, 0).unwrap()["source"]["generation"], "3");
    assert!(r.report("missing", 0, &BTreeMap::new()).is_err());
    r.fault("missing", "gap");
    r.disconnect("missing");
    r.disconnect_all();
    assert!(r.id_for_path("a").is_none());
}
#[test]
fn identical_devices_ports_collections_and_explicit_reassignment() {
    let mut r = Registry::default();
    let first = r.connect(fixture("port-a", None)).unwrap();
    let second = r.connect(fixture("port-b", None)).unwrap();
    assert_ne!(first, second);
    r.update(&first, "My pedals", "pedals").unwrap();
    r.disconnect(&first);
    let moved = r.connect(fixture("port-c", None)).unwrap();
    assert_ne!(first, moved);
    r.reassign(&first, &moved).unwrap();
    assert!(r.inspect(&moved, 0).is_err());
    assert_eq!(r.id_for_path("port-c"), Some(first.clone()));
    assert_eq!(r.inspect(&first, 0).unwrap()["source"]["name"], "My pedals");
    assert!(r.reassign(&first, &second).is_err());
    let mut collection = fixture("port-d", None);
    collection.identity.collection = "collection-2".into();
    let different = r.connect(collection).unwrap();
    r.disconnect(&first);
    assert!(r.reassign(&first, &different).is_err());
    assert!(r.reassign("missing", &second).is_err());
    assert!(r.reassign(&first, "missing").is_err());
    let with_serial = r.connect(fixture("s-a", Some("unique"))).unwrap();
    r.disconnect(&with_serial);
    assert_eq!(
        r.connect(fixture("s-b", Some("unique"))).unwrap(),
        with_serial
    );
    let duplicate = r.connect(fixture("s-c", Some("unique"))).unwrap();
    assert_ne!(duplicate, with_serial);
    r.disconnect(&with_serial);
    r.disconnect(&duplicate);
    let ambiguous = r.connect(fixture("s-d", Some("unique"))).unwrap();
    assert_ne!(ambiguous, duplicate);
    assert_ne!(ambiguous, with_serial);
    r.disconnect(&ambiguous);
    let changed = r.connect(fixture("s-d", Some("different"))).unwrap();
    assert_ne!(changed, ambiguous);
}
#[test]
fn inventory_bounds_and_settings_validation() {
    let mut r = Registry::default();
    let id = r.connect(fixture("a", None)).unwrap();
    assert!(r.forget(&id).is_err());
    assert!(r.forget("missing").is_err());
    assert!(r.update(&id, "", "pedals").is_err());
    assert!(r.update(&id, "x", "invalid").is_err());
    assert!(r.update(&id, "x\ny", "panel").is_err());
    assert!(r.update("missing", "x", "panel").is_err());
    let saved = r.saved();
    let mut restored = Registry::default();
    restored.restore(saved.clone()).unwrap();
    assert_eq!(restored.inspect(&id, 0).unwrap()["source"]["online"], false);
    let mut duplicate = saved.clone();
    duplicate.push(saved[0].clone());
    assert!(restored.restore(duplicate).is_err());
    let mut invalid = saved.clone();
    invalid[0].id = "bad".into();
    assert!(restored.restore(invalid).is_err());
    let mut invalid = saved.clone();
    invalid[0].identity.fingerprint = "bad".into();
    assert!(restored.restore(invalid).is_err());
    assert!(restored
        .restore(vec![saved[0].clone(); MAX_SOURCES + 1])
        .is_err());
    restored.forget(&id).unwrap();
    assert_eq!(restored.list(0)["total"], 0);
    let mut vjoy = fixture("vjoy", None);
    vjoy.identity.vendor_id = 0x1234;
    vjoy.identity.product_id = 0xbead;
    assert!(r.connect(vjoy).is_err());
    let mut bad = fixture("", None);
    assert!(r.connect(bad.clone()).is_err());
    bad.identity.path = "b".into();
    bad.controls.push(bad.controls[0].clone());
    assert!(r.connect(bad).is_err());
    let mut generic = fixture("generic", None);
    generic.name = "".into();
    r.connect(generic).unwrap();
    for n in 2..MAX_SOURCES {
        r.connect(fixture(&format!("path-{n}"), None)).unwrap();
    }
    assert_eq!(r.list(0)["sources"].as_array().unwrap().len(), PAGE_SIZE);
    assert_eq!(r.list(MAX_SOURCES)["sources"], json!([]));
    assert!(r.connect(fixture("overflow", None)).is_err());
    assert!(r.list(0).to_string().len() < 65536);
}
#[test]
fn settings_persist_and_import_cannot_start_capture() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("controllers.json");
    let service = ControllerService::new(Some(path.clone()), false);
    let id = service
        .registry
        .lock()
        .unwrap()
        .connect(fixture("a", None))
        .unwrap();
    assert!(service
        .request(&json!({"command":"controller_list"}))
        .is_ok());
    assert!(path.exists());
    service
        .request(
            &json!({"command":"controller_update","sourceId":id,"name":"Wheel","role":"wheelbase"}),
        )
        .unwrap();
    let second = ControllerService::new(Some(path.clone()), false);
    assert_eq!(second.status()["sources"][0]["name"], "Wheel");
    assert_eq!(second.status()["sources"][0]["online"], false);
    assert_eq!(second.status()["backendStatus"], "unavailable");
    assert!(second
        .request(&json!({"command":"controller_inspect","sourceId":id,"offset":0}))
        .is_ok());
    second
        .request(&json!({"command":"controller_forget","sourceId":id}))
        .unwrap();
    for request in [
        json!({}),
        json!({"command":"bad"}),
        json!({"command":"controller_list","offset":-1}),
        json!({"command":"controller_list","offset":257}),
    ] {
        assert!(second.request(&request).is_err());
    }
    second.shutdown();
    assert!(second
        .request(&json!({"command":"controller_list"}))
        .is_err());
    service.shutdown();
    std::fs::write(&path, b"corrupt").unwrap();
    let invalid = ControllerService::new(Some(path.clone()), false);
    assert!(invalid.status()["diagnostic"]
        .as_str()
        .unwrap()
        .contains("not be overwritten"));
    invalid
        .registry
        .lock()
        .unwrap()
        .connect(fixture("b", None))
        .unwrap();
    assert!(invalid
        .request(&json!({"command":"controller_list"}))
        .is_ok());
    assert_eq!(std::fs::read(&path).unwrap(), b"corrupt");
    std::fs::write(&path, vec![b' '; MAX_SETTINGS as usize + 1]).unwrap();
    let oversized = ControllerService::new(Some(path), false);
    assert!(!oversized.storage_valid);
}
#[test]
fn service_reassignment_and_write_errors_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("file");
    std::fs::write(&parent, b"x").unwrap();
    let s = ControllerService::new(Some(parent.join("controllers.json")), false);
    assert!(s.status()["sources"].as_array().unwrap().is_empty());
    let memory = ControllerService::new(None, false);
    let first = memory
        .registry
        .lock()
        .unwrap()
        .connect(fixture("a", None))
        .unwrap();
    memory.registry.lock().unwrap().disconnect(&first);
    let observed = memory
        .registry
        .lock()
        .unwrap()
        .connect(fixture("b", None))
        .unwrap();
    assert!(memory
        .request(&json!({"command":"controller_reassign","sourceId":first,"observedId":observed}))
        .is_ok());
    assert!(memory
        .request(
            &json!({"command":"controller_update","sourceId":first,"name":"Test","role":"panel"})
        )
        .is_ok());
    let path = dir.path().join("valid").join("controllers.json");
    let s = ControllerService::new(Some(path), false);
    let id = s
        .registry
        .lock()
        .unwrap()
        .connect(fixture("x", None))
        .unwrap();
    std::fs::write(dir.path().join("valid"), b"block").unwrap();
    assert!(s
        .request(
            &json!({"command":"controller_update","sourceId":id,"name":"Test","role":"controller"})
        )
        .is_err());
    assert!(s.registry.lock().unwrap().dirty);
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn native_platforms_stay_gated_for_ordinary_startup() {
    let service = ControllerService::new(None, true);
    assert_eq!(service.status()["backendStatus"], "unavailable");
    assert!(service.status()["diagnostic"]
        .as_str()
        .unwrap()
        .contains("qualification"));
    assert!(service.worker.lock().unwrap().is_none());
    service.shutdown();
}

#[test]
fn diagnostic_only_descriptors_cannot_establish_an_input_baseline() {
    let mut r = Registry::default();
    let mut device = fixture("diagnostic", None);
    for control in &mut device.controls {
        control.kind = Kind::Diagnostic;
    }
    let id = r.connect(device).unwrap();
    r.report(&id, 1, &BTreeMap::from([(0, 0), (1, 1)])).unwrap();
    assert_eq!(r.inspect(&id, 0).unwrap()["source"]["baselineReady"], false);
}
