use super::{
    test_support::*,
    tests::{baseline, setup, subscribe},
    *,
};
use std::{
    sync::{mpsc, Mutex},
    time::Duration,
};

#[test]
fn blocked_publication_does_not_hold_native_capture_or_metadata_lock() {
    let (mut registry, _broker, id, mut reader) = setup();
    reader.recv().unwrap();
    baseline(&mut registry, &id);
    reader.recv().unwrap();
    reader.recv().unwrap();
    let gate = block_publication(&id);
    registry.report(&id, 1, &BTreeMap::from([(1, 10)])).unwrap();
    gate.entered();
    let registry = Arc::new(Mutex::new(registry));
    let (done, progress) = mpsc::channel();
    let writer = {
        let registry = registry.clone();
        let id = id.clone();
        std::thread::spawn(move || {
            for value in 11..75 {
                registry
                    .lock()
                    .unwrap()
                    .delta(&id, 1, &BTreeMap::from([(1, value)]))
                    .unwrap();
            }
            registry
                .lock()
                .unwrap()
                .update(&id, "Captured while publisher blocked", "joystick")
                .unwrap();
            done.send(()).unwrap();
        })
    };
    let progressed = progress.recv_timeout(Duration::from_secs(2)).is_ok();
    gate.release();
    writer.join().unwrap();
    assert!(
        progressed,
        "native capture waited for the blocked publisher"
    );
    let registry = registry.lock().unwrap();
    wait_processed(&registry, &id);
    assert_eq!(registry.sources[&id].sequence, 67);
    let mut last = None;
    while let Some(frame) = reader.try_recv().unwrap() {
        last = Some(frame);
    }
    assert_eq!(
        last.unwrap().payload.unwrap()["source"]["name"],
        "Captured while publisher blocked"
    );
}

#[test]
fn preparation_pin_preserves_reports_and_metadata_before_reader_opens() {
    let (mut registry, broker, id, mut first) = setup();
    first.recv().unwrap();
    baseline(&mut registry, &id);
    first.recv().unwrap();
    first.recv().unwrap();
    drop(first);
    let prepared = registry.prepare_observation(&broker, &id).unwrap();
    prepared
        .shared
        .wait(&prepared.slot, prepared.epoch)
        .unwrap();
    // This is the deliberate readiness-to-subscription gap. The pin keeps the
    // idle optimization from dropping reports, even after the fence completed.
    registry
        .update(&id, "Fence includes metadata", "wheelbase")
        .unwrap();
    registry.report(&id, 1, &BTreeMap::from([(1, 19)])).unwrap();
    wait_processed(&registry, &id);
    let mut reader = prepared
        .with_subscription(|| {
            broker
                .subscribe("gap", "ui", &format!("controllers.input.{id}"))
                .map_err(|e| e.to_string())
        })
        .unwrap();
    let snapshot = reader.recv().unwrap().payload.unwrap();
    assert_eq!(snapshot["source"]["name"], "Fence includes metadata");
    assert_eq!(snapshot["controls"][1]["value"]["native"], 19);
    registry.report(&id, 1, &BTreeMap::from([(1, 20)])).unwrap();
    let update = reader.recv().unwrap().payload.unwrap();
    assert_eq!(update["source"]["sequence"], "4");
}

#[test]
fn worker_overflow_explicitly_resynchronizes_both_readers_and_current_snapshot() {
    let (mut registry, broker, id, mut first) = setup();
    first.recv().unwrap();
    baseline(&mut registry, &id);
    first.recv().unwrap();
    first.recv().unwrap();
    let mut second = subscribe(&mut registry, &broker, &id);
    second.recv().unwrap();
    let gate = block_publication(&id);
    registry.report(&id, 1, &BTreeMap::from([(1, 1)])).unwrap();
    gate.entered();
    for value in 2..400 {
        registry
            .report(&id, 1, &BTreeMap::from([(1, value)]))
            .unwrap();
    }
    registry
        .update(&id, "Overflow metadata", "joystick")
        .unwrap();
    gate.release();
    wait_processed(&registry, &id);
    for reader in [&mut first, &mut second] {
        let loss = reader.recv().unwrap();
        assert_eq!(loss.kind, "loss");
        assert_eq!(loss.reason.as_deref(), Some("baseline-reset"));
        let snapshot = reader.recv().unwrap();
        assert_eq!(snapshot.kind, "snapshot");
        let payload = snapshot.payload.unwrap();
        assert_eq!(payload["controls"][1]["value"]["native"], 399);
        assert_eq!(payload["source"]["name"], "Overflow metadata");
        assert!(reader.try_recv().unwrap().is_none());
    }
    let mut late = subscribe(&mut registry, &broker, &id);
    assert_eq!(
        late.recv().unwrap().payload.unwrap()["controls"][1]["value"]["native"],
        399
    );
}

#[test]
fn metadata_epoch_waits_for_unpublished_edit_without_native_cursor_change() {
    let (mut registry, broker, id, mut reader) = setup();
    reader.recv().unwrap();
    baseline(&mut registry, &id);
    reader.recv().unwrap();
    reader.recv().unwrap();
    let cursor = registry.sources[&id].cursor;
    let gate = block_publication(&id);
    registry.update(&id, "Pending metadata", "pedals").unwrap();
    gate.entered();
    let prepared = registry.prepare_observation(&broker, &id).unwrap();
    let (sent, received) = mpsc::channel();
    let subscriber = std::thread::spawn(move || {
        let mut reader = prepared
            .with_subscription(|| {
                broker
                    .subscribe("metadata", "ui", &format!("controllers.input.{id}"))
                    .map_err(|e| e.to_string())
            })
            .unwrap();
        sent.send(reader.recv().unwrap()).unwrap();
    });
    assert!(received.recv_timeout(Duration::from_millis(25)).is_err());
    gate.release();
    let payload = received
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .payload
        .unwrap();
    subscriber.join().unwrap();
    assert_eq!(payload["source"]["name"], "Pending metadata");
    assert_eq!(payload["cursor"], cursor.to_string());
}

#[test]
fn shutdown_fails_waiting_preparation_and_closes_existing_reader() {
    let (mut registry, broker, id, mut reader) = setup();
    reader.recv().unwrap();
    let gate = block_publication(&id);
    registry.report(&id, 1, &BTreeMap::from([(1, 2)])).unwrap();
    gate.entered();
    let preparation = registry.prepare_observation(&broker, &id).unwrap();
    let shutdown = registry.take_observation_shutdown();
    let stopped = std::thread::spawn(move || shutdown.shutdown());
    assert!(preparation
        .with_subscription(|| Ok(()))
        .unwrap_err()
        .contains("stopped"));
    gate.release();
    stopped.join().unwrap();
    assert_eq!(reader.recv().unwrap().reason.as_deref(), Some("closed"));
    assert!(broker
        .subscribe("closed", "ui", &format!("controllers.input.{id}"))
        .is_err());
}

#[test]
fn oversized_typed_capture_withdraws_topic_instead_of_leaving_stale_publisher() {
    let (mut registry, broker, id, mut reader) = setup();
    reader.recv().unwrap();
    // Exercise the defensive terminal path beyond the validated native limits.
    registry.sources.get_mut(&id).unwrap().diagnostics = vec!["x".repeat(256 * 1024)];
    registry.report(&id, 1, &BTreeMap::from([(1, 22)])).unwrap();
    let loss = reader.recv().unwrap();
    assert_eq!(loss.reason.as_deref(), Some("closed"));
    assert_eq!(registry.sources[&id].sequence, 1);
    assert!(broker
        .subscribe("stale", "ui", &format!("controllers.input.{id}"))
        .is_err());
}

#[test]
fn late_preparation_cannot_restart_actor_after_shutdown_boundary() {
    use std::sync::Barrier;
    let (registry, broker, id, mut reader) = setup();
    reader.recv().unwrap();
    let registry = Arc::new(Mutex::new(registry));
    let boundary = Arc::new(Barrier::new(2));
    let late = {
        let registry = registry.clone();
        let boundary = boundary.clone();
        std::thread::spawn(move || {
            // The observer started before service stop, but has not yet reached
            // the registry boundary. Shutdown must permanently close admission.
            boundary.wait();
            boundary.wait();
            registry
                .lock()
                .unwrap()
                .prepare_observation(&broker, &id)
                .err()
                .unwrap()
        })
    };
    boundary.wait();
    let shutdown = registry.lock().unwrap().take_observation_shutdown();
    shutdown.shutdown();
    boundary.wait();
    assert!(late.join().unwrap().contains("stopped"));
    assert!(registry.lock().unwrap().observations.actor.is_none());
    assert_eq!(reader.recv().unwrap().reason.as_deref(), Some("closed"));
}
