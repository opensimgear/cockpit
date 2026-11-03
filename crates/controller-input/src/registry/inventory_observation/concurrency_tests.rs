use super::{test_support::*, tests::{baseline, setup, subscribe}, *};
use std::{collections::BTreeMap, sync::{mpsc, Mutex}, time::Duration};

#[test]
fn blocked_inventory_encoder_does_not_block_capture_and_overwrite_resynchronizes_latest() {
    let (mut registry, broker, id, mut first) = setup();first.recv().unwrap();
    baseline(&mut registry, &id);wait_processed(&registry);first.recv().unwrap();
    let mut second = subscribe(&mut registry, &broker);second.recv().unwrap();
    let gate = block_publication(&registry.session);
    registry.set_diagnostic("Blocked publication".into());gate.entered();
    let registry = Arc::new(Mutex::new(registry));let (done, progress) = mpsc::channel();
    let writer = {
        let registry = registry.clone();let id = id.clone();
        std::thread::spawn(move || {
            for value in 0..1000 {
                registry.lock().unwrap().report(&id, 1, &BTreeMap::from([(0, value % 2), (1, value)])).unwrap();
            }
            for index in 0..100 {registry.lock().unwrap().set_diagnostic(format!("Latest diagnostic {index}"));}
            done.send(()).unwrap();
        })
    };
    let progressed = progress.recv_timeout(Duration::from_secs(2)).is_ok();
    gate.release();writer.join().unwrap();assert!(progressed, "capture waited on inventory encoding");
    wait_processed(&registry.lock().unwrap());
    for reader in [&mut first, &mut second] {
        assert_eq!(reader.recv().unwrap().kind, "loss");
        let snapshot = reader.recv().unwrap();assert_eq!(snapshot.kind, "snapshot");
        assert_eq!(snapshot.payload.unwrap()["diagnostic"], "Latest diagnostic 99");
        assert!(reader.try_recv().unwrap().is_none());
    }
}

#[test]
fn first_readiness_queues_only_its_row_while_inventory_publication_is_blocked() {
    let (mut registry, _broker, id, mut reader) = setup();reader.recv().unwrap();
    registry.connect(crate::tests::fixture("unrelated-inventory-row", None)).unwrap();wait_processed(&registry);
    while reader.try_recv().unwrap().is_some() {}
    let gate = block_publication(&registry.session);registry.set_diagnostic("Blocked encoder".into());gate.entered();
    let captures = full_capture_count(&registry.session);
    baseline(&mut registry, &id);
    assert_eq!(full_capture_count(&registry.session), captures, "first report captured unrelated rows");
    gate.release();wait_processed(&registry);
    let mut last = None;while let Some(frame) = reader.try_recv().unwrap() { if frame.payload.is_some() { last = frame.payload; } }
    let payload = last.unwrap();let sources = payload["sources"].as_array().unwrap();
    assert_eq!(sources.iter().find(|source| source["id"] == id).unwrap()["baselineReady"], true);
    assert_eq!(sources.iter().find(|source| source["id"] != id).unwrap()["baselineReady"], false);
}

#[test]
fn preparation_pin_covers_ready_to_subscribe_gap_and_waits_outside_capture() {
    let (mut registry, broker, id, mut first) = setup();first.recv().unwrap();
    let prepared = registry.prepare_inventory_observation(&broker).unwrap();
    prepared.shared.wait(prepared.epoch).unwrap();drop(first);
    registry.update(&id, "Pinned current metadata", "panel").unwrap();wait_processed(&registry);
    let mut reader = prepared.with_subscription(|| broker.subscribe("prepared", "ui", "controllers.inventory").map_err(|e| e.to_string())).unwrap();
    assert_eq!(reader.recv().unwrap().payload.unwrap()["sources"][0]["name"], "Pinned current metadata");
    let gate = block_publication(&registry.session);registry.set_diagnostic("Pending metadata".into());gate.entered();
    let preparation = registry.prepare_inventory_observation(&broker).unwrap();
    let (sent, received) = mpsc::channel();
    let waiter = std::thread::spawn(move || {sent.send(preparation.with_subscription(|| Ok(()))).unwrap();});
    assert!(received.recv_timeout(Duration::from_millis(25)).is_err());
    // The caller can continue committing reports while the preparation waits.
    registry.report(&id, 1, &BTreeMap::from([(0, 1), (1, 4)])).unwrap();
    gate.release();received.recv_timeout(Duration::from_secs(2)).unwrap().unwrap();waiter.join().unwrap();
}

#[test]
fn shutdown_fails_pending_preparation_and_retires_existing_reader() {
    let (mut registry, broker, _id, mut reader) = setup();reader.recv().unwrap();
    let gate = block_publication(&registry.session);registry.set_diagnostic("Blocked".into());gate.entered();
    let preparation = registry.prepare_inventory_observation(&broker).unwrap();
    let shutdown = registry.take_inventory_shutdown();
    let stopped = std::thread::spawn(move || shutdown.shutdown());
    assert!(preparation.with_subscription(|| Ok(())).unwrap_err().contains("stopped"));
    gate.release();stopped.join().unwrap();
    assert_eq!(reader.recv().unwrap().reason.as_deref(), Some("closed"));
    assert!(registry.prepare_inventory_observation(&broker).is_err());
    assert!(broker.subscribe("late", "ui", "controllers.inventory").is_err());
}

#[test]
fn encoder_panic_unblocks_waiters_and_withdraws_current_topic() {
    let (mut registry, broker, _id, mut reader) = setup();reader.recv().unwrap();
    panic_next(&registry.session);
    let preparation = registry.prepare_inventory_observation(&broker).unwrap();
    assert!(preparation.with_subscription(|| Ok(())).unwrap_err().contains("panicked"));
    assert_eq!(reader.recv().unwrap().reason.as_deref(), Some("closed"));
    assert!(broker.subscribe("failed", "ui", "controllers.inventory").is_err());
    assert!(registry.prepare_inventory_observation(&broker).is_err());
}
