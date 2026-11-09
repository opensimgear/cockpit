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
fn native_key_pair_has_one_bounded_intent_and_exact_logical_receipts() {
    let (_directory, mut coordinator, seen, second) = pair_fixture();
    let before = coordinator.persist_attempts.get();
    let [first, second] = coordinator.try_submit_native_key_pair([delivery(), second.clone()], 0).unwrap().unwrap();
    assert_eq!(first.sequence, second.sequence);
    assert_eq!([first.operation_id, second.operation_id], [Some(Counter(1)), Some(Counter(2))]);
    assert_eq!([first.confirmed, second.confirmed], [1, 1]);
    assert_eq!(coordinator.persist_attempts.get() - before, 2);
    let receipts = seen.lock().unwrap();
    assert_eq!(receipts.len(), 1);
    let prepared = &receipts[0];
    assert!(prepared.held.is_empty());
    let expected = vec![key(), Control::Key { key: Key(5) }];
    assert_eq!(crate::reducer::recovery_releases(&prepared.possible_held).unwrap().len(), 2);
    assert!(expected.iter().all(|control| prepared.possible_held.contains(control)));
    assert!(matches!(&prepared.in_flight.as_ref().unwrap().command,
        Command::DeliverKeyPair { deliveries } if deliveries[0].operation_id == Counter(1)
            && deliveries[1].operation_id == Counter(2)));
    let durable = Coordinator::inspect_journal(coordinator.journal.as_ref().unwrap()).unwrap();
    assert_eq!(durable.held, expected);
    assert_eq!(durable.possible_held, durable.held);
    assert!(durable.in_flight.is_none());
}

#[test]
fn revoked_native_key_pair_leaves_both_operations_available_for_individual_validation() {
    let (_directory, mut coordinator, seen, second) = pair_fixture();
    coordinator.set_dispatch_gate(Arc::new(|request| !matches!(request.command, Command::DeliverKeyPair { .. })));
    assert!(coordinator.try_submit_native_key_pair([delivery(), second.clone()], 0).unwrap().is_none());
    assert!(coordinator.snapshot().held.is_empty());
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(coordinator.submit(delivery(), 0).unwrap().confirmed, 1);
    assert_eq!(coordinator.submit(second, 0).unwrap().confirmed, 1);
}

#[test]
fn failed_native_pair_receipt_leaves_both_exact_operations_in_durable_recovery() {
    let (_directory, mut coordinator, seen, second) = pair_fixture();
    let before = coordinator.persist_attempts.get();
    coordinator.fail_persist_at.set(Some(before + 2));
    assert!(coordinator.try_submit_native_key_pair([delivery(), second], 0).is_err());
    assert!(coordinator.snapshot().uncertain);
    assert_eq!(seen.lock().unwrap().len(), 1);
    let durable = Coordinator::inspect_journal(coordinator.journal.as_ref().unwrap()).unwrap();
    assert!(matches!(durable.in_flight.unwrap().command, Command::DeliverKeyPair { .. }));
    assert_eq!(crate::reducer::recovery_releases(&durable.possible_held).unwrap().len(), 2);
    coordinator.stop_all().unwrap();
    assert!(coordinator.snapshot().cleanup_complete);
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

fn releases(coordinator: &mut Coordinator, seen: &Mutex<Vec<Snapshot>>, second: Delivery) -> (Delivery, Delivery) {
    let first = delivery();
    coordinator.submit(first.clone(), 0).unwrap();
    coordinator.submit(second.clone(), 0).unwrap();
    seen.lock().unwrap().clear();
    let release = |delivery: Delivery, operation| Delivery {
        operation_id: Counter(operation),
        sequence: Counter(2),
        intent: Intent::Held { controls: vec![] },
        ..delivery
    };
    (release(first, 3), release(second, 4))
}

#[test]
fn two_releases_keep_old_durable_holds_until_the_combined_receipts_commit() {
    let (_directory, mut coordinator, seen, second) = pair_fixture();
    let (first, second) = releases(&mut coordinator, &seen, second);
    let before = coordinator.persist_attempts.get();
    let (first_ack, second_ack) = coordinator.submit_key_pair(first, 0, |_| Some((second, 0))).unwrap();
    assert_eq!(first_ack.attempted, 1);
    assert_eq!(second_ack.unwrap().unwrap().attempted, 1);
    assert_eq!(coordinator.persist_attempts.get() - before, 1);
    let receipts = seen.lock().unwrap();
    assert_eq!(receipts.len(), 2);
    for receipt in receipts.iter() {
        assert_eq!(receipt.held, vec![key(), Control::Key { key: Key(5) }]);
        assert!(receipt.in_flight.is_none());
    }
    let durable = Coordinator::inspect_journal(coordinator.journal.as_ref().unwrap()).unwrap();
    assert!(durable.held.is_empty() && durable.possible_held.is_empty());
}

#[test]
fn failed_release_receipt_commit_preserves_the_original_recovery_set() {
    let (_directory, mut coordinator, seen, second) = pair_fixture();
    let (first, second) = releases(&mut coordinator, &seen, second);
    let before = coordinator.persist_attempts.get();
    coordinator.fail_persist_at.set(Some(before + 1));
    assert!(coordinator.submit_key_pair(first, 0, |_| Some((second, 0))).is_err());
    assert_eq!(seen.lock().unwrap().len(), 2);
    assert!(coordinator.snapshot().uncertain);
    let durable = Coordinator::inspect_journal(coordinator.journal.as_ref().unwrap()).unwrap();
    assert_eq!(durable.held, vec![key(), Control::Key { key: Key(5) }]);
    coordinator.stop_all().unwrap();
    assert!(coordinator.snapshot().cleanup_complete);
}

#[test]
fn external_journal_change_blocks_the_second_release_before_ipc() {
    let (_directory, mut coordinator, seen, second) = pair_fixture();
    let (first, second) = releases(&mut coordinator, &seen, second);
    let path = coordinator.journal.clone().unwrap();
    let durable = sql_read(&path).unwrap();
    let before = coordinator.sequence;
    assert!(coordinator.submit_key_pair(first, 0, |_| {
        sql_write(&path, &durable).unwrap();
        Some((second, 0))
    }).is_err());
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(coordinator.sequence, before + 1);
    assert_eq!(sql_read(&path).unwrap(), durable);
}
