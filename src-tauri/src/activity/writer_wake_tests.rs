use super::*;
use crate::profile_session::{RunOwner, State, Status};

// Read committed storage only: Flush would itself wake the writer and hide
// the defect these tests exercise. The old lifecycle path waits 750 ms.
fn await_committed(runtime: &Runtime, entity: &str, expected: u64) {
    let deadline = std::time::Instant::now() + Duration::from_millis(500);
    loop {
        let page = runtime.request(&json!({
            "command":"activity_query", "committedOnly":true,
            "activityFilter":{"entityId":entity}
        })).unwrap();
        if page["activity"]["total"].as_u64() == Some(expected) { return; }
        assert!(std::time::Instant::now() < deadline, "Writer did not commit without a flush: {page}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn parked_writer_commits_brief_lifecycle_transitions_without_a_metadata_message() {
    let directory = crate::test_directory();
    let runtime = Runtime::new(&directory.path().join("config.json"));
    runtime.flush().unwrap();
    let observer = runtime.session_observer();
    let mut observer = observer.lock().unwrap();
    let mut status = Status { state:State::Stopped, owner:None, applied_snapshot:None,
        degradation:vec![], fault:None };
    observer.observe(&status);
    status.owner = Some(RunOwner { session:"lifecycle-wake".into(), generation:1,
        profile_id:"lifecycle-profile".into(), profile_revision:1, game_id:None });
    status.state = State::Running;
    observer.observe(&status);
    status.state = State::Degraded;
    status.degradation.push("Input unavailable".into());
    observer.observe(&status);
    status.state = State::Running;
    status.degradation.clear();
    observer.observe(&status);
    drop(observer);
    await_committed(&runtime, "lifecycle-profile", 3);
    let events = runtime.request(&json!({"command":"activity_query","committedOnly":true,
        "activityFilter":{"entityId":"lifecycle-profile"}})).unwrap();
    let mut kinds = events["activity"]["events"].as_array().unwrap().iter()
        .map(|event|event["eventType"].as_str().unwrap()).collect::<Vec<_>>();
    kinds.sort_unstable();
    assert_eq!(kinds, ["profile_session_activated", "profile_session_degraded", "profile_session_recovered"]);
}

#[test]
fn abandoning_an_action_commits_its_deferred_observation_without_another_message() {
    let directory = crate::test_directory();
    let runtime = Runtime::new(&directory.path().join("config.json"));
    let action = runtime.begin(&json!({"command":"profile_create","name":"Deferred"})).unwrap();
    let mut after = Snapshot::default();
    after.profile_value("deferred-profile", &json!({"id":"deferred-profile","name":"Deferred","revision":1}));
    let batch = observations::Batch::between(&Snapshot::default(), &after, std::time::Instant::now());
    assert!(runtime.registry.lock().unwrap().observe(batch).is_empty());
    runtime.flush().unwrap();
    drop(action);
    await_committed(&runtime, "deferred-profile", 1);
}
