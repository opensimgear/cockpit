use super::*;
use crate::tests::fixture;
use osg_streams::Reader;

#[test]
fn exhausted_native_generation_withdraws_values_without_wrapping_wire_identity() {
    let mut registry=Registry::default();
    let id=registry.connect(fixture("last-generation",None)).unwrap();
    baseline(&mut registry,&id);
    registry.sources.get_mut(&id).unwrap().generation=u64::MAX-1;
    let broker=Broker::new("last-generation".into()).unwrap();
    let mut reader=subscribe(&mut registry,&broker,&id);
    assert_eq!(reader.recv().unwrap().generation,u64::MAX.to_string());
    registry.fault(&id,"Bad report");
    let unavailable=reader.recv().unwrap();
    assert_eq!(unavailable.generation,u64::MAX.to_string());
    assert_eq!(unavailable.payload.unwrap()["source"]["online"],false);
    assert!(registry.sources[&id].values.is_empty());
    assert!(registry.connect(fixture("last-generation",None)).unwrap_err().contains("generation exhausted"));
}

#[test]
fn exhausted_native_sequence_rejects_report_without_committing_unpublished_values() {
    let mut registry=Registry::default();
    let id=registry.connect(fixture("last-report",None)).unwrap();
    baseline(&mut registry,&id);
    registry.sources.get_mut(&id).unwrap().sequence=u64::MAX;
    let before=registry.inspect(&id,0).unwrap();
    assert!(registry.report(&id,1,&BTreeMap::from([(0,1),(1,555)])).unwrap_err().contains("sequence exhausted"));
    assert_eq!(registry.inspect(&id,0).unwrap(),before);
}

fn setup() -> (Registry, Broker, String, Reader) {
    let mut registry = Registry::default();
    let id = registry.connect(fixture("controller", None)).unwrap();
    let broker = Broker::new("observers".into()).unwrap();
    let reader = subscribe(&mut registry, &broker, &id);
    (registry, broker, id, reader)
}
fn subscribe(registry: &mut Registry, broker: &Broker, id: &str) -> Reader {
    registry
        .observe(broker, id, || {
            broker
                .subscribe("tester", "ui", &format!("controllers.input.{id}"))
                .map_err(|e| e.to_string())
        })
        .unwrap()
}
fn baseline(registry: &mut Registry, id: &str) {
    registry
        .report(id, 1, &BTreeMap::from([(0, 1), (1, 65535)]))
        .unwrap();
    registry.report(id, 2, &BTreeMap::from([(2, 1)])).unwrap();
}
#[test]
fn exact_domains_partial_baseline_atomic_report_and_capture() {
    let (mut registry, _, id, mut reader) = setup();
    let snapshot = reader.recv().unwrap();
    assert_eq!(snapshot.kind, "snapshot");
    assert_eq!(
        snapshot.payload.as_ref().unwrap()["source"]["baselineReady"],
        false
    );
    assert_eq!(
        snapshot.payload.as_ref().unwrap()["controls"][1]["capability"]["min"],
        -32768
    );
    assert_eq!(
        snapshot.payload.as_ref().unwrap()["controls"][1]["value"],
        Value::Null
    );
    registry
        .report(&id, 1, &BTreeMap::from([(0, 1), (1, 65535)]))
        .unwrap();
    let partial = reader.recv().unwrap();
    assert_eq!(partial.payload.as_ref().unwrap()["values"], json!({}));
    assert_eq!(
        partial.payload.as_ref().unwrap()["source"]["baselineReady"],
        false
    );
    registry.report(&id, 2, &BTreeMap::from([(2, 1)])).unwrap();
    let baseline = reader.recv().unwrap();
    assert_eq!(baseline.payload.as_ref().unwrap()["reason"], "baseline");
    assert_eq!(
        baseline.payload.as_ref().unwrap()["values"]
            .as_object()
            .unwrap()
            .len(),
        3
    );
    assert!(baseline.payload.as_ref().unwrap().get("controls").is_none());
    let event = registry
        .stream_page(&registry.session, 0)
        .events
        .pop()
        .unwrap();
    assert_eq!(baseline.captured_us, event.captured_us.to_string());
    assert_eq!(
        baseline.payload.as_ref().unwrap()["cursor"],
        event.cursor.to_string()
    );
    registry
        .report(&id, 1, &BTreeMap::from([(1, 123)]))
        .unwrap();
    let change = reader.recv().unwrap();
    assert_eq!(change.payload.as_ref().unwrap()["reason"], "change");
    assert_eq!(
        change.payload.as_ref().unwrap()["values"]["hid:0009:0001:1:0:0"]["native"],
        0
    );
    assert_eq!(
        change.payload.as_ref().unwrap()["values"]["hid:0001:0030:1:0:1"]["native"],
        123
    );
}
#[test]
fn speculative_metadata_isolated_and_committed_metadata_keeps_current_samples() {
    let (mut registry, _, id, mut reader) = setup();
    reader.recv().unwrap();
    baseline(&mut registry, &id);
    reader.recv().unwrap();
    reader.recv().unwrap();
    let before = registry.clone();
    let mut staged = before.clone();
    staged.update(&id, "Updated", "joystick").unwrap();
    staged.fault(&id, "speculative");
    assert!(reader.try_recv().unwrap().is_none());
    registry.commit_metadata(&before, &staged);
    let metadata = reader.recv().unwrap();
    assert_eq!(
        metadata.payload.as_ref().unwrap()["source"]["name"],
        "Updated"
    );
    assert_eq!(
        metadata.payload.as_ref().unwrap()["source"]["baselineReady"],
        true
    );
    assert_eq!(
        metadata.payload.as_ref().unwrap()["values"]
            .as_object()
            .unwrap()
            .len(),
        3
    );
}
#[test]
fn disconnect_reconnect_fault_and_layout_replacement_fence_values() {
    let (mut registry, broker, id, mut reader) = setup();
    reader.recv().unwrap();
    baseline(&mut registry, &id);
    reader.recv().unwrap();
    reader.recv().unwrap();
    registry.disconnect(&id);
    let offline = reader.recv().unwrap();
    assert_eq!(offline.payload.as_ref().unwrap()["values"], json!({}));
    assert_eq!(offline.payload.as_ref().unwrap()["source"]["online"], false);
    registry.connect(fixture("controller", None)).unwrap();
    assert_eq!(reader.recv().unwrap().kind, "loss");
    let snapshot = reader.recv().unwrap();
    assert_eq!(snapshot.kind, "snapshot");
    assert_eq!(
        snapshot.payload.as_ref().unwrap()["source"]["baselineReady"],
        false
    );
    baseline(&mut registry, &id);
    reader.recv().unwrap();
    reader.recv().unwrap();
    registry.fault(&id, "native decode fault");
    assert_eq!(reader.recv().unwrap().kind, "loss");
    let fault = reader.recv().unwrap();
    assert_eq!(
        fault.payload.as_ref().unwrap()["diagnostics"],
        json!(["native decode fault"])
    );
    assert!(fault.payload.as_ref().unwrap()["controls"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c["value"].is_null()));
    let mut replacement = fixture("controller", None);
    replacement.controls[1].max = 100;
    registry.connect(replacement).unwrap();
    assert_eq!(reader.recv().unwrap().kind, "loss");
    assert_eq!(
        reader.recv().unwrap().payload.as_ref().unwrap()["controls"][1]["capability"]["max"],
        100
    );
    let mut later = subscribe(&mut registry, &broker, &id);
    assert_eq!(
        later.recv().unwrap().payload.as_ref().unwrap()["source"]["baselineReady"],
        false
    );
}
#[test]
fn bounded_topics_recycle_only_idle_bindings_and_cancelled_readers() {
    let mut registry = Registry::default();
    let broker = Broker::new("bounds".into()).unwrap();
    let mut live = Vec::new();
    for index in 0..64 {
        let id = registry
            .connect(fixture(&format!("device{index}"), None))
            .unwrap();
        // Global budget permits 64 readers, client budget permits 16 each.
        let reader = registry
            .observe(&broker, &id, || {
                broker
                    .subscribe(
                        &format!("client{index}"),
                        "ui",
                        &format!("controllers.input.{id}"),
                    )
                    .map_err(|e| e.to_string())
            })
            .unwrap();
        live.push(reader);
    }
    let id = registry.connect(fixture("extra", None)).unwrap();
    assert!(registry.observe(&broker, &id, || Ok(())).is_err());
    live.pop().unwrap().cancellation_handle().cancel();
    registry.observe(&broker, &id, || Ok(())).unwrap();
    assert_eq!(registry.observations.bindings.len(), 64);
    assert_eq!(live[0].recv().unwrap().kind, "snapshot");
}
#[test]
fn overflow_is_explicit_and_new_subscription_has_current_level() {
    let (mut registry, broker, id, mut reader) = setup();
    reader.recv().unwrap();
    baseline(&mut registry, &id);
    reader.recv().unwrap();
    reader.recv().unwrap();
    for value in 0..300 {
        registry
            .report(&id, 1, &BTreeMap::from([(1, value)]))
            .unwrap();
    }
    assert_eq!(reader.recv().unwrap().kind, "loss");
    assert!(reader.try_recv().unwrap().is_none());
    let mut later = subscribe(&mut registry, &broker, &id);
    let snapshot = later.recv().unwrap();
    assert_eq!(
        snapshot.payload.as_ref().unwrap()["controls"][1]["value"]["native"],
        299
    );
    reader.cancellation_handle().cancel();
    assert!(reader.try_recv().is_err());
}
#[test]
fn observation_action_sees_authoritative_topic_before_any_report() {
    let mut registry = Registry::default();
    let id = registry.connect(fixture("atomic", None)).unwrap();
    let broker = Broker::new("atomic".into()).unwrap();
    assert!(registry.observe(&broker, "missing", || Ok(())).is_err());
    let mut reader = subscribe(&mut registry, &broker, &id);
    baseline(&mut registry, &id);
    assert_eq!(
        reader.recv().unwrap().payload.as_ref().unwrap()["source"]["sequence"],
        "0"
    );
    assert_eq!(
        reader.recv().unwrap().payload.as_ref().unwrap()["source"]["sequence"],
        "1"
    );
    assert_eq!(
        reader.recv().unwrap().payload.as_ref().unwrap()["source"]["sequence"],
        "2"
    );
}
#[test]
fn quiet_subscription_and_metadata_preserve_native_capture_age() {
    let (mut registry, broker, id, mut reader) = setup();
    reader.recv().unwrap();
    baseline(&mut registry, &id);
    reader.recv().unwrap();
    let captured = reader.recv().unwrap();
    registry.update(&id, "Renamed", "joystick").unwrap();
    let metadata = reader.recv().unwrap();
    assert_eq!(metadata.captured_us, captured.captured_us);
    assert_eq!(
        metadata.payload.as_ref().unwrap()["cursor"],
        captured.payload.as_ref().unwrap()["cursor"]
    );
    let mut late = subscribe(&mut registry, &broker, &id);
    assert_eq!(late.recv().unwrap().captured_us, captured.captured_us);
}

#[test]
fn concurrent_subscription_has_no_report_boundary_hole() {
    use std::sync::{Arc, Barrier, Mutex};
    let mut registry = Registry::default();
    let id = registry.connect(fixture("race", None)).unwrap();
    baseline(&mut registry, &id);
    let registry = Arc::new(Mutex::new(registry));
    let ready = Arc::new(Barrier::new(2));
    let writer = {
        let registry = registry.clone();
        let ready = ready.clone();
        let id = id.clone();
        std::thread::spawn(move || {
            ready.wait();
            for value in 0..64 {
                registry
                    .lock()
                    .unwrap()
                    .report(&id, 1, &BTreeMap::from([(1, value)]))
                    .unwrap();
            }
        })
    };
    let broker = Broker::new("race".into()).unwrap();
    ready.wait();
    let mut reader = subscribe(&mut registry.lock().unwrap(), &broker, &id);
    writer.join().unwrap();
    let first = reader.recv().unwrap();
    let mut sequence = first.payload.unwrap()["source"]["sequence"]
        .as_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    while let Some(frame) = reader.try_recv().unwrap() {
        sequence += 1;
        assert_eq!(
            frame.payload.unwrap()["source"]["sequence"],
            sequence.to_string()
        );
    }
    assert_eq!(sequence, 66);
}

#[test]
fn reassignment_after_remembered_reconnect_fences_live_observers_and_staged_baselines() {
    let mut registry = Registry::default();
    let remembered = registry.connect(fixture("remembered", None)).unwrap();
    baseline(&mut registry, &remembered);
    registry.disconnect(&remembered);
    let observed = registry.connect(fixture("replacement", None)).unwrap();
    baseline(&mut registry, &observed);
    let before = registry.clone();
    let mut staged = before.clone();
    staged.reassign(&remembered, &observed).unwrap();
    let desired_generation = staged.sources[&remembered].generation;

    // A reconnect and real reports arrive while the authoring database write runs.
    assert_eq!(
        registry.connect(fixture("remembered", None)).unwrap(),
        remembered
    );
    baseline(&mut registry, &remembered);
    let live_generation = registry.sources[&remembered].generation;
    let broker = Broker::new("reassignment".into()).unwrap();
    let mut reader = subscribe(&mut registry, &broker, &remembered);
    let prior = reader.recv().unwrap();
    assert_eq!(prior.payload.unwrap()["source"]["baselineReady"], true);
    // The replacement itself may also reconnect during the database write.
    registry.disconnect(&observed);
    registry.connect(fixture("replacement", None)).unwrap();
    baseline(&mut registry, &observed);
    let observed_generation = registry.sources[&observed].generation;

    registry.commit_metadata(&before, &staged);
    let loss = reader.recv().unwrap();
    assert_eq!(loss.kind, "loss");
    assert_eq!(loss.reason.as_deref(), Some("generation-replaced"));
    let snapshot = reader.recv().unwrap();
    assert_eq!(snapshot.kind, "snapshot");
    let payload = snapshot.payload.unwrap();
    assert_eq!(payload["identity"]["path"], "replacement");
    let generation = payload["source"]["generation"]
        .as_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert!(
        generation > live_generation
            && generation > observed_generation
            && generation > desired_generation
    );
    assert_eq!(payload["source"]["sequence"], "0");
    assert_eq!(payload["source"]["baselineReady"], false);
    assert!(payload["controls"]
        .as_array()
        .unwrap()
        .iter()
        .all(|slot| slot["value"].is_null()));
    assert!(!registry.sources.contains_key(&observed));
    assert!(reader.try_recv().unwrap().is_none());
    // Neither the old remembered samples nor staged/live B samples form this
    // new domain's baseline. Only its following native reports can restore it.
    baseline(&mut registry, &remembered);
    assert_eq!(reader.recv().unwrap().payload.unwrap()["values"], json!({}));
    assert_eq!(
        reader.recv().unwrap().payload.unwrap()["reason"],
        "baseline"
    );
}

#[test]
fn reassignment_generation_exhaustion_withdraws_samples_and_reports_failure() {
    let mut registry = Registry::default();
    let remembered = registry.connect(fixture("remembered", None)).unwrap();
    registry.disconnect(&remembered);
    let observed = registry.connect(fixture("replacement", None)).unwrap();
    let before = registry.clone();
    let mut staged = before.clone();
    staged.reassign(&remembered, &observed).unwrap();
    registry.connect(fixture("remembered", None)).unwrap();
    baseline(&mut registry, &remembered);
    registry.sources.get_mut(&remembered).unwrap().generation = u64::MAX - 1;
    let broker = Broker::new("exhaustion".into()).unwrap();
    let mut reader = subscribe(&mut registry, &broker, &remembered);
    reader.recv().unwrap();
    registry.commit_metadata(&before, &staged);
    let current = &registry.sources[&remembered];
    assert_eq!(current.saved.identity.path, "remembered");
    assert!(!current.online && current.values.is_empty());
    assert!(registry.dirty);
    assert!(registry.diagnostic.contains("generation exhausted"));
    assert!(current.diagnostics[0].contains("generation exhausted"));
    while let Some(frame) = reader.try_recv().unwrap() {
        let payload = frame.payload.unwrap();
        assert_eq!(payload["values"], json!({}));
        assert_eq!(payload["source"]["baselineReady"], false);
    }
}
