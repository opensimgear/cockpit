use super::*;

#[test]
fn blocked_native_channel_does_not_block_snapshot_renew_or_reserved_pause() {
    let (mut channel, requests, events) = channel();
    let (arrival, resume) = block(&mut channel, "arm");
    let output = threaded(channel);
    let delivery = held("key", 1);
    reconcile(&output, &[delivery.clone()]);
    output.dispatch(vec![delivery], 10, 0, output.child_scope().map(|receipt| receipt.admission)).unwrap();
    arrival.recv_timeout(Duration::from_secs(1)).unwrap();
    let started = Instant::now();
    output.snapshot();
    output.renew(run()).unwrap();
    output.pause(run()).unwrap();
    assert!(started.elapsed() < Duration::from_millis(100));
    assert!(output.pending());
    resume.send(()).unwrap();
    wait_until(|| output.shared.lock().completion.is_some());
    let completion = output.take_completion().unwrap();
    assert_eq!(completion.dropped.len(), 1);
    assert_eq!(completion.dropped[0].1, 4);
    wait_until(|| !output.pending());
    assert!(requests.lock().unwrap().iter().any(|request| kind(request) == "pause"));
    assert!(events.lock().unwrap().is_empty());
    output.stop_all().unwrap();
}

#[test]
fn revocation_during_arm_drops_entire_pointer_vector() {
    let (mut channel, requests, events) = channel();
    let (arrival, resume) = block(&mut channel, "arm");
    let output = threaded(channel);
    let deliveries = vec![pointer("x", 1), pointer("y", 2)];
    reconcile(&output, &deliveries);
    output.dispatch(deliveries.clone(), 10, 0, output.child_scope().map(|receipt| receipt.admission)).unwrap();
    arrival.recv_timeout(Duration::from_secs(1)).unwrap();
    let mut current = deliveries.iter().map(|d| (d.action_id.clone(), d.producer.clone())).collect::<BTreeMap<_, _>>();
    current.get_mut("y").unwrap().dependencies[0].epoch = Counter(2);
    output.reconcile(&current);
    resume.send(()).unwrap();
    wait_until(|| output.shared.lock().completion.is_some());
    let completion = output.take_completion().unwrap();
    assert_eq!(completion.dropped.len(), 2);
    assert!(completion.outcomes.is_empty());
    assert!(events.lock().unwrap().is_empty());
    assert!(!requests.lock().unwrap().iter().any(|request| kind(request) == "pointer"));
    output.stop_all().unwrap();
}

#[test]
fn stop_is_a_priority_barrier_and_waits_for_native_ack() {
    let (mut channel, requests, events) = channel();
    let (arrival, resume) = block(&mut channel, "arm");
    let output = Arc::new(threaded(channel));
    let deliveries = vec![held("key", 1), pointer("x", 2), pointer("y", 3)];
    reconcile(&output, &deliveries);
    output.dispatch(deliveries, 10, 0, output.child_scope().map(|receipt| receipt.admission)).unwrap();
    arrival.recv_timeout(Duration::from_secs(1)).unwrap();
    let (done, answer) = mpsc::sync_channel(1);
    let stopping = output.clone();
    std::thread::spawn(move || done.send(stopping.stop_all()).unwrap());
    wait_until(|| output.shared.lock().closed);
    assert!(answer.try_recv().is_err());
    resume.send(()).unwrap();
    answer.recv_timeout(Duration::from_secs(1)).unwrap().unwrap();
    assert_eq!(output.snapshot().phase, "stopped");
    assert!(output.snapshot().cleanup_complete);
    assert!(events.lock().unwrap().is_empty());
    let kinds: Vec<_> = requests.lock().unwrap().iter().map(kind).collect();
    assert_eq!(kinds, ["activate", "arm", "stop"]);
    assert_eq!(output.take_completion().unwrap().dropped.len(), 3);
}

#[test]
fn drop_signals_shutdown_without_waiting_for_blocked_native_channel() {
    let (mut channel, requests, _) = channel();
    let (arrival, resume) = block(&mut channel, "arm");
    let output = threaded(channel);
    let delivery = held("key", 1);
    reconcile(&output, &[delivery.clone()]);
    output.dispatch(vec![delivery], 10, 0, output.child_scope().map(|receipt| receipt.admission)).unwrap();
    arrival.recv_timeout(Duration::from_secs(1)).unwrap();
    let started = Instant::now();
    drop(output);
    assert!(started.elapsed() < Duration::from_millis(100));
    resume.send(()).unwrap();
    wait_until(|| requests.lock().unwrap().iter().any(|request| kind(request) == "stop"));
}

#[test]
fn renewal_requires_a_fresh_new_host_authorization_and_never_repeats_it() {
    let (output, requests, _) = inline();
    let Mode::Inline(owner) = &output.mode else { unreachable!() };
    owner.lock().unwrap().last_renew = Some(Instant::now() - Duration::from_millis(101));
    {
        let mut state = output.shared.lock();
        state.authorization = Some((run(), Instant::now() - Duration::from_millis(6), 1));
    }
    output.drive();
    assert!(!requests.lock().unwrap().iter().any(|request| kind(request) == "renew"));
    output.renew(run()).unwrap();
    assert_eq!(requests.lock().unwrap().iter().filter(|request| kind(request) == "renew").count(), 1);
    owner.lock().unwrap().last_renew = Some(Instant::now() - Duration::from_millis(101));
    output.drive();
    assert_eq!(requests.lock().unwrap().iter().filter(|request| kind(request) == "renew").count(), 1);
}

#[test]
fn heartbeat_failure_is_visible_and_prevents_new_data() {
    let (mut channel, _, _) = channel();
    channel.fail = Some("renew");
    let output = threaded(channel);
    std::thread::sleep(Duration::from_millis(105));
    output.renew(run()).unwrap();
    wait_until(|| output.snapshot().error.is_some());
    assert!(output.dispatch(vec![held("key", 1)], 10, 0, output.child_scope().map(|receipt| receipt.admission)).is_err());
}

#[test]
fn panic_resolves_accepted_ticket_and_lifecycle_waiter() {
    let (mut channel, _, _) = channel();
    let (arrival, resume) = block(&mut channel, "arm");
    channel.panic = Some("arm");
    let output = Arc::new(threaded(channel));
    let delivery = held("key", 1);
    reconcile(&output, &[delivery.clone()]);
    output.dispatch(vec![delivery], 10, 0, output.child_scope().map(|receipt| receipt.admission)).unwrap();
    arrival.recv_timeout(Duration::from_secs(1)).unwrap();
    let (done, answer) = mpsc::sync_channel(1);
    let stopping = output.clone();
    std::thread::spawn(move || done.send(stopping.stop_all()).unwrap());
    wait_until(|| output.shared.lock().closed);
    resume.send(()).unwrap();
    assert!(answer.recv_timeout(Duration::from_secs(1)).unwrap().is_err());
    let completion = output.take_completion().unwrap();
    assert!(completion.error.unwrap().contains("panicked"));
    assert!(!output.busy());
    assert!(output.snapshot().uncertain);
}
