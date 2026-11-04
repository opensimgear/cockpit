use super::*;
use std::time::Duration;

fn inner() -> Arc<Inner> {
    Arc::new(Inner { registry: Mutex::new(Registry::default()), changed: Condvar::new(),
        closed: AtomicBool::new(false), signal: Arc::new(Signal::default()) })
}

fn job(phase: &str, terminal: bool) -> Job {
    Job { key: "discovery".into(), run: "run-one".into(), phase: phase.into(), message: phase.into(),
        percent: None, terminal, truncated: false }
}

fn batch(events: Vec<Job>, jobs: Vec<Job>, lost: bool) -> Batch {
    Batch { revision: 1, dirty: Domain::Discovery.bit(), events, jobs, lost }
}

#[test]
fn bootstrap_jobs_use_current_baseline_and_subsequent_lifecycle_is_ordered() {
    let observations = observations::Runtime::new();
    let broker = observations.broker();
    let mut worker = Worker::default();
    let current = job("ready", true);
    worker.jobs(&broker, &observations, "games:clock", &batch(vec![job("scanning", false)], vec![current], false), true).unwrap();
    let mut reader = broker.subscribe("test", "mcp", "games.jobs").unwrap();
    let snapshot = reader.recv().unwrap();
    assert_eq!(snapshot.source, "games");
    assert_eq!(snapshot.payload.unwrap()["jobs"][0]["phase"], "ready");
    let scanning = job("scanning", false);
    let ready = job("ready", true);
    worker.jobs(&broker, &observations, "games:clock", &batch(vec![scanning, ready], vec![], false), false).unwrap();
    let first = reader.recv().unwrap();
    let second = reader.recv().unwrap();
    assert_eq!(first.kind, "change");
    assert_eq!(first.payload.unwrap()["jobs"][0]["phase"], "scanning");
    assert_eq!(second.kind, "change");
    assert_eq!(second.payload.unwrap()["jobs"][0]["phase"], "ready");
}

#[test]
fn overflow_replaces_job_map_and_explicitly_resynchronizes_readers() {
    let observations = observations::Runtime::new();
    let broker = observations.broker();
    let mut worker = Worker::default();
    worker.jobs(&broker, &observations, "games:clock", &batch(vec![], vec![job("scanning", false)], false), true).unwrap();
    let mut reader = broker.subscribe("test", "ui", "games.jobs").unwrap();
    reader.recv().unwrap();
    worker.jobs(&broker, &observations, "games:clock", &batch(vec![job("discarded", false)], vec![job("ready", true)], true), false).unwrap();
    assert_eq!(reader.recv().unwrap().kind, "loss");
    let baseline = reader.recv().unwrap();
    assert_eq!(baseline.kind, "snapshot");
    assert_eq!(baseline.payload.unwrap()["jobs"][0]["phase"], "ready");
}

#[test]
fn lease_keeps_prepared_topic_alive_until_subscribe_and_last_reader_retires() {
    let observations = observations::Runtime::new();
    let broker = observations.broker();
    let inner = inner();
    let lease = details::Lease::acquire(&inner, "dcs").unwrap();
    assert!(!inner.lock().prune(&broker));
    let task = details::tasks(&inner, &broker).0.pop().unwrap();
    let payload = json!({"gameId":"dcs","scope":"sha256:test","value":{"id":"dcs"}});
    let publisher = topic(&broker, &observations, "games:clock", "games.detail.dcs", payload.clone()).unwrap();
    assert!(details::accept(&inner, &task, publisher, payload));
    lease.wait_ready().unwrap();
    // The action has not subscribed yet, so this lease alone protects the topic.
    assert!(!inner.lock().prune(&broker));
    let reader = broker.subscribe("test", "mcp", "games.detail.dcs").unwrap();
    drop(lease);
    assert!(!inner.lock().prune(&broker));
    drop(reader);
    assert!(inner.lock().prune(&broker));
    assert!(inner.lock().slots.is_empty());
    assert!(broker.subscribe("test", "mcp", "games.detail.dcs").is_err());
}

#[test]
fn detail_admission_is_bounded_and_stale_leases_cannot_release_replacement() {
    let broker = Broker::new("test".into()).unwrap();
    let mut registry = Registry::default();
    let first = registry.acquire("dcs").unwrap();
    registry.release("dcs", first);
    assert!(registry.prune(&broker));
    let replacement = registry.acquire("dcs").unwrap();
    assert_ne!(first, replacement);
    registry.release("dcs", first);
    assert_eq!(registry.slots["dcs"].leases, 1);
    for index in 1..details::MAX_DETAILS { registry.acquire(&format!("game-{index}")).unwrap(); }
    assert!(registry.acquire("overflow").is_err());
    assert_eq!(registry.slots.len(), details::MAX_DETAILS);
    for _ in 1..64 { registry.acquire("dcs").unwrap(); }
    assert!(registry.acquire("dcs").is_err());
    registry.slots.get_mut("dcs").unwrap().ready = Some(Err("retired".into()));
    assert_eq!(registry.acquire("dcs").unwrap_err(), "retired");
}

#[test]
fn retired_game_wakes_detail_waiter_and_withdraws_exact_publisher() {
    let inner = inner();
    let observations = observations::Runtime::new();
    let broker = observations.broker();
    let lease = details::Lease::acquire(&inner, "dcs").unwrap();
    let task = details::tasks(&inner, &broker).0.pop().unwrap();
    let publisher = topic(&broker, &observations, "games:clock", "games.detail.dcs", json!({"value":1})).unwrap();
    details::accept(&inner, &task, publisher, json!({"value":1}));
    details::retire_missing(&inner, &broker, &task);
    assert!(lease.wait_ready().unwrap_err().contains("retired"));
    assert!(broker.subscribe("test", "ui", "games.detail.dcs").is_err());
    drop(lease);
    assert!(inner.lock().prune(&broker));
}

#[test]
fn source_failure_retires_all_topics_clears_cache_and_unblocks_waiters() {
    let inner = inner();
    let observations = observations::Runtime::new();
    let broker = observations.broker();
    let pages = Pages::default();
    let mut projection = pages.begin();
    let image = projection.asset(Arc::from("memory-only-image")).unwrap();
    projection.commit("games.state").unwrap();
    let lease = details::Lease::acquire(&inner, "dcs").unwrap();
    let mut worker = Worker::default();
    worker.state = Some(topic(&broker, &observations, "games:clock", "games.state", json!({"value":image})).unwrap());
    worker.jobs = Some(topic(&broker, &observations, "games:clock", "games.jobs", json!({"jobs":[]})).unwrap());
    worker.telemetry = Some(topic(&broker, &observations, "games:clock", "games.telemetry", json!({})).unwrap());
    let waiter = std::thread::spawn(move || lease.wait_ready());
    finish(&inner, &broker, &pages, worker, Err("explicit capture failure".into()));
    assert!(broker.subscribe("test", "mcp", "games.telemetry").is_err());
    assert_eq!(waiter.join().unwrap().unwrap_err(), "explicit capture failure");
    assert!(inner.closed.load(Ordering::Acquire));
    assert!(broker.subscribe("test", "ui", "games.state").is_err());
    assert!(broker.subscribe("test", "ui", "games.jobs").is_err());
    assert!(pages.get(image["$gamePage"]["id"].as_str().unwrap(), 0).is_err());
}

#[test]
fn worker_panic_is_reported_and_finishing_unblocks_bootstrap_and_detail_waiters() {
    let inner = inner();
    let observations = observations::Runtime::new();
    let broker = observations.broker();
    let pages = Pages::default();
    let lease = details::Lease::acquire(&inner, "dcs").unwrap();
    let waiter = std::thread::spawn(move || lease.wait_ready());
    let mut worker = Worker::default();
    worker.jobs = Some(topic(&broker, &observations, "games:clock", "games.jobs", json!({"jobs":[]})).unwrap());
    let result = catch_worker(|| panic!("capture panic"));
    finish(&inner, &broker, &pages, worker, result);
    let error = "Game read-model worker panicked: capture panic";
    assert_eq!(inner.lock().ready.as_ref().unwrap().as_ref().unwrap_err(), error);
    assert_eq!(waiter.join().unwrap().unwrap_err(), error);
    assert!(inner.closed.load(Ordering::Acquire));
    assert!(broker.subscribe("test", "ui", "games.jobs").is_err());
    assert_eq!(catch_worker(|| std::panic::panic_any(String::from("owned panic"))).unwrap_err(),
        "Game read-model worker panicked: owned panic");
    assert_eq!(catch_worker(|| std::panic::panic_any(7u8)).unwrap_err(),
        "Game read-model worker panicked: unknown panic payload");
    assert!(catch_worker(|| Ok(())).is_ok());
}

#[test]
fn page_command_accepts_only_issued_identity_and_offset_without_source_access() {
    let pages = Pages::default();
    let mut projection = pages.begin();
    let node = projection.asset(Arc::from("cached".repeat(2000))).unwrap();
    projection.commit("games.state").unwrap();
    let request = json!({"command":"game_read_model_page","id":node["$gamePage"]["id"],"offset":0});
    assert_eq!(page_request(&pages, false, &request).unwrap()["chunk"].as_str().unwrap().len(), 8192);
    assert!(page_request(&pages, true, &request).is_err());
    for field in ["path", "file", "gameId", "scope", "authority"] {
        let mut forged = request.clone();
        forged[field] = json!("forbidden");
        assert!(page_request(&pages, false, &forged).is_err());
    }
    let mut malformed = request.clone();
    malformed["command"] = json!("games_state");
    assert!(page_request(&pages, false, &malformed).is_err());
    malformed = request.clone();
    malformed["offset"] = json!(-1);
    assert!(page_request(&pages, false, &malformed).is_err());
    malformed["offset"] = json!(0.5);
    assert!(page_request(&pages, false, &malformed).is_err());
    malformed = request.clone();
    malformed["id"] = json!(format!("sha256:{}", "f".repeat(64)));
    assert!(page_request(&pages, false, &malformed).is_err());
    assert!(page_request(&pages, false, &json!([])).is_err());
}

#[test]
fn encoded_detail_scope_is_stable_for_equal_data_and_changes_for_new_data() {
    let pages = Pages::default();
    let mut projection = pages.begin();
    let node = projection.encode(&json!({"id":"dcs","metadata":"a".repeat(10_000)})).unwrap();
    let first = scope(&node).unwrap();
    assert!(first.starts_with("sha256:"));
    assert_eq!(first.len(), 71);
    assert_eq!(first, scope(&projection.encode(&node).unwrap()).unwrap());
    let changed = projection.encode(&json!({"id":"dcs","metadata":"b".repeat(10_000)})).unwrap();
    assert_ne!(first, scope(&changed).unwrap());
    projection.commit("games.state").unwrap();
}

#[test]
fn reached_source_deadline_invalidates_even_with_other_deadlines_or_renewal() {
    let now = Instant::now();
    let expired = now - Duration::from_millis(1);
    let renewed = now + Duration::from_secs(30);
    // The vehicle fence expires while the activation fence is still future.
    let scheduled = [expired, renewed].into_iter().min();
    assert!(expired_deadline(scheduled, now));
    // Renewal may race the wake, but must not suppress another source's expiry.
    assert!(expired_deadline(Some(expired), now));
    assert!(!expired_deadline(Some(renewed), now));
    assert!(!expired_deadline(None, now));
}

#[test]
fn capture_preserves_elapsed_fence_when_sources_omit_it_or_return_another_future_fence() {
    let before = Instant::now();
    let captured = before + Duration::from_millis(1);
    let after = before + Duration::from_millis(2);
    let future = before + Duration::from_secs(30);
    assert!(expired_deadline(Some(captured), after));
    assert_eq!(pending_deadline(Some(captured), None), Some(captured));
    assert_eq!(pending_deadline(Some(captured), Some(future)), Some(captured));
    assert!(expired_deadline(pending_deadline(Some(captured), Some(future)), after));
    // The rerun sees the expired fence consumed and can wait for future work.
    assert_eq!(pending_deadline(None, Some(future)), Some(future));
    assert!(!expired_deadline(pending_deadline(None, Some(future)), after));
}

#[test]
fn unchanged_root_does_not_advance_the_broker_revision() {
    let observations = observations::Runtime::new();
    let broker = observations.broker();
    let payload = json!({"value":{"catalogue":[],"discovery":{}}});
    let publisher = topic(&broker, &observations, "games:clock", "games.state", payload.clone()).unwrap();
    publish(&observations, &publisher, "games:clock", payload.clone(), Some(&payload), false, false).unwrap();
    assert_eq!(publisher.revision(), 1);
    publish(&observations, &publisher, "games:clock", json!({"value":2}), Some(&payload), false, false).unwrap();
    assert_eq!(publisher.revision(), 2);
}
