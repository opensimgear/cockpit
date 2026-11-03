use super::*;
use crate::profile_session::{RunOwner, State};
use std::time::{Duration, Instant};

fn status(state: State) -> Status {
    Status { state, applied_snapshot: None, owner: None, degradation: vec![], fault: None }
}
fn automatic() -> AutomaticSnapshot {
    AutomaticSnapshot { schema_version: 1, preferred_profiles: Default::default(), mode: SelectionMode::Manual,
        manual_profile_id: osg_profiles::DEFAULT_PROFILE_ID.into(), rules: vec![], status: "manual".into(),
        message: String::new(), conflicts: vec![], last_attempt: None, target: None, read_only: false }
}
fn fixture() -> (Runtime, osg_profiles::service::Service) {
    let observations = observations::Runtime::new();
    let runtime = Runtime::new(observations);
    let mut service = osg_profiles::service::Service::new(None);
    service.observe_read_model(runtime.library_observer());
    runtime.native_observer()(status(State::Stopped));
    runtime.automatic_observer()(AutomaticChange::Snapshot(automatic()));
    runtime.automatic_observer()(AutomaticChange::Emergency { revision: 0, enabled: false });
    runtime.start().unwrap();
    (runtime, service)
}
fn next(reader: &mut osg_streams::Reader) -> osg_streams::WireFrame {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(frame) = reader.try_recv().unwrap() { return frame; }
        assert!(Instant::now() < deadline, "Profile event was not delivered");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn committed_second_client_changes_arrive_and_new_readers_start_current() {
    let (runtime, mut service) = fixture();
    let broker = runtime.observations.broker();
    let mut first = broker.subscribe("first", "ui", TOPICS[0]).unwrap();
    let initial = next(&mut first);
    assert_eq!(initial.kind, "snapshot");
    assert_eq!(initial.source, "profiles");
    assert!(initial.clock_id.starts_with("profiles:"));
    service.request(&json!({"command":"profile_create","name":"Second client"})).unwrap();
    let changed = next(&mut first);
    assert_eq!(changed.kind, "change");
    assert_eq!(changed.payload.as_ref().unwrap()["profiles"].as_array().unwrap().len(), 2);
    let mut late = broker.subscribe("late", "mcp", TOPICS[0]).unwrap();
    assert_eq!(next(&mut late).payload, changed.payload);
    first.close();
    late.close();
    // Passive subscribe/unsubscribe did not mutate the source, which needs no I/O.
    assert_eq!(service.read_model_snapshot().profiles.len(), 2);
    runtime.shutdown();
    assert!(broker.subscribe("after", "ui", TOPICS[0]).is_err());
    assert!(broker.subscribe("after", "ui", TOPICS[1]).is_err());
}

#[test]
fn ordered_background_lifecycle_and_preference_only_changes_are_delivered() {
    let (runtime, _) = fixture();
    let mut reader = runtime.observations.broker().subscribe("reader", "browser", TOPICS[1]).unwrap();
    next(&mut reader);
    let capture = runtime.native_observer();
    std::thread::spawn(move || {
        capture(status(State::Transitioning));
        let mut running = status(State::Running);
        running.owner = Some(RunOwner { session: uuid::Uuid::new_v4().to_string(), generation: 1,
            profile_id: osg_profiles::DEFAULT_PROFILE_ID.into(), profile_revision: 1, game_id: None });
        capture(running.clone());
        running.state = State::Degraded;
        running.degradation.push("Disconnected peripheral".into());
        capture(running);
    }).join().unwrap();
    for state in ["transitioning", "running", "degraded"] {
        let frame = next(&mut reader);
        let payload = frame.payload.unwrap();
        assert_eq!(payload["profileSession"]["state"], state);
        assert_eq!(payload["session"]["state"], state);
    }
    let mut preference = automatic();
    preference.preferred_profiles.insert("dcs".into(), osg_profiles::DEFAULT_PROFILE_ID.into());
    runtime.automatic_observer()(AutomaticChange::Snapshot(preference));
    let payload = next(&mut reader).payload.unwrap();
    assert_eq!(payload["preferredProfiles"]["dcs"], osg_profiles::DEFAULT_PROFILE_ID);
    assert!(payload["automatic"].get("preferredProfiles").is_none());
    runtime.shutdown();
}

#[test]
fn bounded_queue_overflow_explicitly_resets_continuity_and_reset_changes_generation() {
    let (runtime, mut service) = fixture();
    let mut reader = runtime.observations.broker().subscribe("reader", "ui", TOPICS[1]).unwrap();
    let initial = next(&mut reader);
    {
        // Hold the queue only to make saturation deterministic, without racing
        // a fast worker or relying on sleep/consumer timing.
        let mut queue = runtime.inner.queue.lock().unwrap();
        for index in 0..CAPACITY + 10 {
            let mut snapshot = status(State::Degraded);
            snapshot.degradation = vec![index.to_string()];
            queue.capture(Event::Native(snapshot));
        }
        assert!(queue.overflow);
        assert!(queue.events.is_empty());
        runtime.inner.wake.notify_one();
    }
    assert_eq!(next(&mut reader).kind, "loss");
    let snapshot = next(&mut reader);
    assert_eq!(snapshot.kind, "snapshot");
    assert_eq!(snapshot.payload.unwrap()["profileSession"]["degradation"][0], (CAPACITY + 9).to_string());
    service.request(&json!({"command":"profile_reset","confirm":true})).unwrap();
    assert_eq!(next(&mut reader).kind, "loss");
    let reset = next(&mut reader);
    assert_eq!(reset.kind, "snapshot");
    assert_ne!(reset.generation, initial.generation);
    runtime.shutdown();
}

#[test]
fn maximum_library_projection_uses_explicit_baseline_and_deduplicates_reads() {
    let (runtime, mut service) = fixture();
    for _ in 1..osg_profiles::MAX_PROFILES {
        service.request(&json!({"command":"profile_create","name":"X".repeat(80)})).unwrap();
    }
    let snapshot = service.read_model_snapshot();
    service.request(&json!({"command":"profile_list"})).unwrap();
    service.request(&json!({"command":"profile_status"})).unwrap();
    assert_eq!(snapshot, service.read_model_snapshot());
    let mut reader = runtime.observations.broker().subscribe("large", "ui", TOPICS[0]).unwrap();
    next(&mut reader);
    let mut large = snapshot;
    for row in &mut large.profiles {
        row.game_ids = (0..32).map(|index| format!("g{index:02}{}", "x".repeat(93))).collect();
    }
    let bytes = serde_json::to_vec(&large).unwrap();
    assert!(bytes.len() > osg_streams::MAX_CHANGE_BYTES);
    assert!(bytes.len() < osg_streams::MAX_SNAPSHOT_BYTES);
    runtime.library_observer()(large);
    // Previously queued creation events may precede this explicit reset.
    while next(&mut reader).kind != "loss" {}
    assert_eq!(next(&mut reader).kind, "snapshot");
    let mut native = runtime.observations.broker().subscribe("native", "ui", TOPICS[1]).unwrap();
    next(&mut native);
    runtime.native_observer()(status(State::Transitioning));
    next(&mut native);
    assert!(reader.try_recv().unwrap().is_none(), "Unchanged large library was reset by a native-only event");
    runtime.shutdown();
}

#[test]
fn large_valid_automatic_rules_resynchronize_and_late_emergency_events_cannot_regress() {
    let (runtime, _) = fixture();
    let mut reader = runtime.observations.broker().subscribe("large", "ui", TOPICS[1]).unwrap();
    next(&mut reader);
    let mut large = automatic();
    for index in 0..osg_profiles::activation::MAX_RULES {
        let game = format!("g{index:03}{}", "x".repeat(92));
        large.rules.push(Rule { identity: osg_profiles::activation::GameIdentity {
            game_id: game.clone(), executable: format!("C:\\{}", "x".repeat(300)) },
            profile_id: osg_profiles::DEFAULT_PROFILE_ID.into(), priority: 0, enabled: true });
        large.preferred_profiles.insert(game, osg_profiles::DEFAULT_PROFILE_ID.into());
    }
    osg_profiles::activation::validate_rules(&large.rules).unwrap();
    runtime.automatic_observer()(AutomaticChange::Snapshot(large));
    assert_eq!(next(&mut reader).kind, "loss");
    assert_eq!(next(&mut reader).kind, "snapshot");
    runtime.automatic_observer()(AutomaticChange::Emergency { revision: 2, enabled: false });
    runtime.automatic_observer()(AutomaticChange::Emergency { revision: 1, enabled: true });
    // A small source replacement also proves the topics remain registered.
    runtime.automatic_observer()(AutomaticChange::Snapshot(automatic()));
    let frame = next(&mut reader);
    let payload = if frame.kind == "loss" { next(&mut reader).payload.unwrap() } else { frame.payload.unwrap() };
    assert_eq!(payload["automatic"]["status"], "manual");
    runtime.shutdown();
}

#[test]
fn oversized_fault_withdraws_both_topics_without_blocking_source() {
    let (runtime, _) = fixture();
    let broker = runtime.observations.broker();
    let mut reader = broker.subscribe("oversized", "ui", TOPICS[1]).unwrap();
    next(&mut reader);
    let mut fault = status(State::Faulted);
    fault.fault = Some("x".repeat(bounds::MAX_SOURCE_BYTES + 1));
    runtime.native_observer()(fault);
    assert_eq!(next(&mut reader).kind, "loss");
    assert!(broker.subscribe("after", "ui", TOPICS[0]).is_err());
    assert!(broker.subscribe("after", "ui", TOPICS[1]).is_err());
    runtime.shutdown();
}

#[test]
fn valid_larger_than_one_mib_rules_bootstrap_and_reconstruct_exactly_without_io() {
    use sha2::{Digest, Sha256};
    let runtime = Runtime::new(observations::Runtime::new());
    let mut service = osg_profiles::service::Service::new(None);
    service.observe_read_model(runtime.library_observer());
    runtime.native_observer()(status(State::Stopped));
    let mut large = automatic();
    for index in 0..40 {
        large.rules.push(Rule { identity: osg_profiles::activation::GameIdentity {
            game_id: format!("game-{index}"), executable: format!("C:\\{}", "x".repeat(32760)) },
            profile_id: osg_profiles::DEFAULT_PROFILE_ID.into(), priority: 0, enabled: true });
    }
    let mut settings = crate::profile_automatic_store::Settings::default();
    settings.rules = large.rules.clone();
    settings.validate().unwrap();
    let expected = serde_json::to_string(&large.rules).unwrap();
    assert!(expected.len() > osg_streams::MAX_SNAPSHOT_BYTES);
    runtime.automatic_observer()(AutomaticChange::Snapshot(large));
    runtime.automatic_observer()(AutomaticChange::Emergency { revision: 0, enabled: false });
    runtime.start().unwrap();
    let mut reader = runtime.observations.broker().subscribe("pages", "mcp", TOPICS[1]).unwrap();
    let baseline = next(&mut reader).payload.unwrap();
    let manifest = &baseline["automatic"]["rules"]["pages"];
    assert_eq!(manifest["count"], 40);
    assert!(serde_json::to_vec(&baseline).unwrap().len() < osg_streams::MAX_CHANGE_BYTES);
    let id = manifest["id"].as_str().unwrap();
    let mut offset = 0;
    let mut bytes = String::new();
    while offset < manifest["bytes"].as_u64().unwrap() {
        let page = runtime.page(&json!({"command":"profile_read_model_page","id":id,"offset":offset})).unwrap();
        let chunk = page["chunk"].as_str().unwrap();
        assert!(chunk.len() <= 8192);
        bytes.push_str(chunk);
        offset = page["nextOffset"].as_u64().unwrap();
    }
    assert_eq!(bytes, expected);
    assert_eq!(id, format!("sha256:{:x}", Sha256::digest(bytes.as_bytes())));
    runtime.native_observer()(status(State::Transitioning));
    let change = next(&mut reader).payload.unwrap();
    assert_eq!(change["automatic"]["rules"]["pages"], *manifest);
    assert!(runtime.page(&json!({"command":"profile_read_model_page","id":id,"offset":0,"path":"forbidden"})).is_err());
    runtime.shutdown();
    assert!(runtime.page(&json!({"command":"profile_read_model_page","id":id,"offset":0})).is_err());
}
