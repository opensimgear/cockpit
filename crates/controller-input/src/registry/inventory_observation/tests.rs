use super::{test_support::*, *};
use crate::tests::fixture;
use osg_streams::Reader;
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) fn subscribe(registry: &mut Registry, broker: &Broker) -> Reader {
    registry.prepare_inventory_observation(broker).unwrap().with_subscription(|| {
        broker.subscribe("inventory-test", "ui", "controllers.inventory").map_err(|e| e.to_string())
    }).unwrap()
}
pub(super) fn setup() -> (Registry, Broker, String, Reader) {
    let mut registry = Registry::default();
    let id = registry.connect(fixture("inventory-controller", None)).unwrap();
    let broker = Broker::new(uuid::Uuid::new_v4().to_string()).unwrap();
    let reader = subscribe(&mut registry, &broker);
    (registry, broker, id, reader)
}
pub(super) fn baseline(registry: &mut Registry, id: &str) {
    registry.report(id, 1, &BTreeMap::from([(0, 1), (1, 123)])).unwrap();
    registry.report(id, 2, &BTreeMap::from([(2, 1)])).unwrap();
}
fn row<'a>(payload: &'a Value, id: &str) -> &'a Value {
    payload["sources"].as_array().unwrap().iter().find(|source| source["id"] == id).unwrap()
}

#[test]
fn native_resource_watch_wakes_on_metadata_and_first_readiness_only() {
    let mut registry = Registry::default();
    let wake = Arc::new(NativeWake::default());
    registry.watch_resource_inventory(&wake).unwrap();
    let initial = wake.observed();
    assert!(initial > 0);
    assert!(registry.watch_resource_inventory(&Arc::new(NativeWake::default())).is_err());
    let id = registry.connect(fixture("resource-watch", None)).unwrap();
    let connected = wake.observed();
    assert!(connected > initial);
    baseline(&mut registry, &id);
    let ready = wake.observed();
    assert!(ready > connected);
    for value in 0..32 { registry.report(&id, 1, &BTreeMap::from([(0, value % 2), (1, value)])).unwrap(); }
    assert_eq!(wake.observed(), ready);
    let before = registry.clone();
    let mut staged = before.clone();
    staged.update(&id, "Renamed", "pedals").unwrap();
    assert_eq!(wake.observed(), ready);
    registry.commit_metadata(&before, &staged);
    assert!(wake.observed() > ready);
    let committed = wake.observed();
    registry.disconnect(&id);
    assert!(wake.observed() > committed);
}

#[test]
fn first_baseline_changes_metadata_but_reports_never_rebuild_or_publish_inventory() {
    let (mut registry, _broker, id, mut reader) = setup();
    let first = reader.recv().unwrap();
    assert_eq!(first.source, "controller-input");assert_eq!(first.clock_id, format!("controllers:{}", registry.session));
    let payload = first.payload.unwrap();
    assert_eq!(payload["offset"], 0);assert_eq!(payload["limit"], 256);assert_eq!(payload["total"], 1);
    assert_eq!(row(&payload, &id)["baselineReady"], false);
    let captured = full_capture_count(&registry.session);
    registry.report(&id, 1, &BTreeMap::from([(0, 1), (1, 3)])).unwrap();
    assert!(reader.try_recv().unwrap().is_none());
    registry.report(&id, 2, &BTreeMap::from([(2, 1)])).unwrap();wait_processed(&registry);
    assert_eq!(full_capture_count(&registry.session), captured, "readiness copied unrelated inventory metadata");
    let ready = reader.recv().unwrap().payload.unwrap();
    assert_eq!(row(&ready, &id)["baselineReady"], true);assert_eq!(row(&ready, &id)["sequence"], "0");
    let epoch = registry.inventory_observation.epoch;
    let encoded = encoded_count(&registry.session);
    for value in 0..1000 { registry.report(&id, 1, &BTreeMap::from([(0, value % 2), (1, value)])).unwrap(); }
    assert_eq!(registry.inventory_observation.epoch, epoch);
    assert_eq!(encoded_count(&registry.session), encoded);
    assert!(reader.try_recv().unwrap().is_none());
    assert!(row(&ready, &id).get("controls").is_none());assert!(row(&ready, &id).get("values").is_none());
    assert_eq!(registry.inspect(&id, 0).unwrap()["source"]["sequence"], "1002");
}

#[test]
fn staged_metadata_is_silent_and_commit_backend_disconnect_and_forget_are_current() {
    let (mut registry, broker, id, mut reader) = setup();reader.recv().unwrap();
    let before = registry.clone();let mut staged = before.clone();
    staged.update(&id, "Saved pedals", "pedals").unwrap();
    assert!(reader.try_recv().unwrap().is_none());
    registry.commit_metadata(&before, &staged);wait_processed(&registry);
    assert_eq!(row(&reader.recv().unwrap().payload.unwrap(), &id)["name"], "Saved pedals");
    registry.set_backend_state("ready", "Capture diagnostic".into());wait_processed(&registry);
    let updated = reader.recv().unwrap().payload.unwrap();
    assert_eq!(updated["backendStatus"], "ready");assert_eq!(updated["diagnostic"], "Capture diagnostic");
    registry.disconnect(&id);wait_processed(&registry);
    assert_eq!(row(&reader.recv().unwrap().payload.unwrap(), &id)["online"], false);
    let before = registry.clone();let mut staged = before.clone();staged.forget(&id).unwrap();
    registry.commit_metadata(&before, &staged);wait_processed(&registry);
    assert_eq!(reader.recv().unwrap().payload.unwrap()["total"], 0);
    let mut late = subscribe(&mut registry, &broker);
    assert_eq!(late.recv().unwrap().payload.unwrap()["total"], 0);
}

#[test]
fn reassignment_commits_one_current_inventory_and_requires_a_new_baseline() {
    let (mut registry, _broker, id, mut reader) = setup();reader.recv().unwrap();
    registry.disconnect(&id);let observed = registry.connect(fixture("replacement-controller", None)).unwrap();
    baseline(&mut registry, &observed);wait_processed(&registry);
    while reader.try_recv().unwrap().is_some() {}
    let before = registry.clone();let mut staged = before.clone();staged.reassign(&id, &observed).unwrap();
    registry.commit_metadata(&before, &staged);wait_processed(&registry);
    let mut last = None;while let Some(frame) = reader.try_recv().unwrap() { if frame.payload.is_some() { last = frame.payload; } }
    let payload = last.unwrap();assert_eq!(payload["total"], 1);
    assert_eq!(row(&payload, &id)["online"], true);assert_eq!(row(&payload, &id)["baselineReady"], false);
    assert_eq!(row(&payload, &id)["sequence"], "0");
}

#[test]
fn all_256_valid_summaries_fit_snapshot_and_oversized_updates_explicitly_resynchronize() {
    let mut registry = Registry::default();
    let mut first_id = String::new();
    for index in 0..256 {
        let mut definition = fixture(&format!("inventory-{index}"), None);
        definition.name = "🛩".repeat(80);
        definition.identity.backend = "\\".repeat(32);
        let id = registry.connect(definition).unwrap();if index == 0 { first_id = id; }
    }
    let broker = Broker::new("large-inventory".into()).unwrap();let mut reader = subscribe(&mut registry, &broker);
    let snapshot = reader.recv().unwrap();
    assert_eq!(snapshot.payload.as_ref().unwrap()["sources"].as_array().unwrap().len(), 256);
    let bytes = serde_json::to_vec(&snapshot).unwrap().len();
    assert!(bytes < osg_streams::MAX_SNAPSHOT_BYTES);assert!(bytes > osg_streams::MAX_CHANGE_BYTES);
    registry.update(&first_id, "Updated", "panel").unwrap();wait_processed(&registry);
    assert_eq!(reader.recv().unwrap().kind, "loss");
    let snapshot = reader.recv().unwrap();assert_eq!(snapshot.kind, "snapshot");
    assert_eq!(row(snapshot.payload.as_ref().unwrap(), &first_id)["name"], "Updated");
}

#[test]
fn service_subscription_keeps_immutable_self_access_and_rejects_after_shutdown() {
    let service = crate::ControllerService::new(None, false);
    let broker = Broker::new("service-inventory".into()).unwrap();
    let mut reader = service.observe_inventory(&broker, || {
        broker.subscribe("service", "mcp", "controllers.inventory").map_err(|e| e.to_string())
    }).unwrap();
    assert_eq!(reader.recv().unwrap().payload.unwrap()["selfAccess"], serde_json::json!(service.visibility.self_access_status()));
    service.registry.lock().unwrap().set_diagnostic("A later diagnostic".into());
    assert_eq!(reader.recv().unwrap().payload.unwrap()["selfAccess"], serde_json::json!(service.visibility.self_access_status()));
    service.shutdown();
    assert!(service.observe_inventory(&broker, || Ok(())).is_err());
    assert_eq!(reader.recv().unwrap().reason.as_deref(), Some("closed"));
    assert!(broker.subscribe("retired", "ui", "controllers.inventory").is_err());
}
