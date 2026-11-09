use super::*;

fn pair_fixture() -> (tempfile::TempDir, Coordinator, Arc<Mutex<Vec<Snapshot>>>, Delivery) {
    let directory = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let path = directory.path().join("journal.json");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut coordinator = Coordinator::with_channel(
        Box::new(Fixture {
            worker: crate::worker::Worker::new(Recording),
            fault: Fault::None,
            inspect: Some(path.clone()),
            seen: seen.clone(),
        }),
        clock,
    );
    coordinator.journal = Some(path);
    coordinator.activate(run(), Scope::DesktopWide).unwrap();
    coordinator.arm(token(), "action".into()).unwrap();
    coordinator.arm(token(), "next".into()).unwrap();
    seen.lock().unwrap().clear();
    let second = Delivery {
        action_id: "next".into(),
        operation_id: Counter(2),
        intent: Intent::Held { controls: vec![Control::Key { key: Key(5) }] },
        ..delivery()
    };
    (directory, coordinator, seen, second)
}

#[test]
fn next_native_request_sees_the_previous_receipt_and_its_own_intent() {
    let (_directory, mut coordinator, seen, second) = pair_fixture();
    let before = coordinator.persist_attempts.get();
    let (first_ack, second_ack) = coordinator
        .submit_key_pair(delivery(), 0, |_| Some((second, 0)))
        .unwrap();
    assert_eq!(first_ack.attempted, 1);
    assert_eq!(second_ack.unwrap().unwrap().attempted, 1);
    assert_eq!(coordinator.persist_attempts.get() - before, 3);
    let receipts = seen.lock().unwrap();
    assert_eq!(receipts.len(), 2);
    assert!(receipts[0].held.is_empty());
    assert_eq!(receipts[0].possible_held, vec![key()]);
    assert_eq!(receipts[1].held, vec![key()]);
    assert!(receipts[1].possible_held.contains(&Control::Key { key: Key(5) }));
    assert_eq!(receipts[1].in_flight.as_ref().unwrap().command.delivery().unwrap().operation_id, Counter(2));
    let durable = Coordinator::inspect_journal(coordinator.journal.as_ref().unwrap()).unwrap();
    assert_eq!(durable.held, vec![key(), Control::Key { key: Key(5) }]);
    assert!(durable.in_flight.is_none());
}

#[test]
fn failure_to_commit_the_combined_write_leaves_only_the_first_intent() {
    let (_directory, mut coordinator, seen, second) = pair_fixture();
    let before = coordinator.persist_attempts.get();
    coordinator.fail_persist_at.set(Some(before + 2));
    assert!(coordinator
        .submit_key_pair(delivery(), 0, |_| Some((second, 0)))
        .is_err());
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert!(coordinator.snapshot().uncertain);
    let durable = Coordinator::inspect_journal(coordinator.journal.as_ref().unwrap()).unwrap();
    assert!(durable.held.is_empty());
    assert_eq!(durable.possible_held, vec![key()]);
    assert_eq!(durable.in_flight.unwrap().command.delivery().unwrap().operation_id, Counter(1));
    coordinator.stop_all().unwrap();
    assert!(coordinator.snapshot().cleanup_complete);
}

#[test]
fn withdrawn_next_edge_still_commits_the_first_receipt() {
    let (_directory, mut coordinator, seen, _) = pair_fixture();
    let (first, second) = coordinator.submit_key_pair(delivery(), 0, |_| None).unwrap();
    assert_eq!(first.attempted, 1);
    assert!(second.unwrap().is_none());
    assert_eq!(seen.lock().unwrap().len(), 1);
    let durable = Coordinator::inspect_journal(coordinator.journal.as_ref().unwrap()).unwrap();
    assert_eq!(durable.held, vec![key()]);
    assert!(durable.in_flight.is_none());
}

#[test]
fn expired_next_edge_sees_the_first_receipt_already_committed() {
    let (_directory, mut coordinator, seen, second) = pair_fixture();
    let (_, second) = coordinator
        .submit_key_pair(delivery(), 0, |_| Some((second, 100)))
        .unwrap();
    assert!(second.unwrap().unwrap().skipped);
    let receipts = seen.lock().unwrap();
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[1].held, vec![key()]);
    assert!(receipts[1].in_flight.is_none());
}
