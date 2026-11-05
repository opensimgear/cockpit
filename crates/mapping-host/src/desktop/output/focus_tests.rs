use super::*;
use crate::desktop::output::ScopeNotice;

fn notice(id: u64, sequence: u64, loss_epoch: u64, eligible: bool) -> ScopeNotice {
    ScopeNotice { watch_id: id, sequence, loss_epoch, eligible, error: None }
}
fn install(output: &Output, id: u64) {
    output.install_scope_watch(id);
    let snapshot = notice(id, 1, 0, true);
    output.scope_revoker(run()).observe(snapshot.clone());
    output.acknowledge_scope(&snapshot);
    output.activate_scope_watch(run());
}

#[test]
fn callback_revokes_blocked_batch_and_authorization_before_the_next_parent_poll() {
    let (mut channel, requests, events) = channel();
    let (arrival, resume) = block(&mut channel, "arm");
    let output = threaded(channel);
    install(&output, 1);
    output.renew(run()).unwrap();
    let deliveries = vec![held("key", 1), held("other", 2)];
    reconcile(&output, &deliveries);
    output.dispatch(deliveries, 10, 0).unwrap();
    arrival.recv_timeout(Duration::from_secs(1)).unwrap();
    let revoke = output.scope_revoker(run());
    let started = Instant::now();
    revoke.observe(notice(1, 2, 1, false));
    revoke.observe(notice(1, 3, 1, true));
    assert!(started.elapsed() < Duration::from_millis(100));
    assert!(output.shared.lock().authorization.is_none());
    output.renew(run()).unwrap();
    assert!(output.shared.lock().authorization.is_none(), "A return cannot authorize an unconsumed loss epoch");
    resume.send(()).unwrap();
    wait_until(|| output.shared.lock().completion.is_some());
    let completion = output.take_completion().unwrap();
    assert_eq!(completion.dropped.len(), 2);
    assert!(completion.error.is_none());
    assert!(events.lock().unwrap().is_empty());
    assert_eq!(requests.lock().unwrap().iter().filter(|request| kind(request) == "arm").count(), 1);
    output.stop_all().unwrap();
}

#[test]
fn old_watch_and_older_sequence_cannot_revoke_a_replacement_lease() {
    let (output, _, _) = inline();
    install(&output, 1);
    let revoke = output.scope_revoker(run());
    output.retire_scope_watch();
    install(&output, 2);
    output.renew(run()).unwrap();
    let epoch = output.shared.lock().epoch;
    revoke.observe(notice(1, 500, 50, false));
    revoke.observe(notice(2, 0, 1, false));
    assert_eq!(output.shared.lock().epoch, epoch);
    assert!(output.shared.lock().authorization.is_some());
}

#[test]
fn eligible_same_process_and_global_scope_events_preserve_current_output() {
    let (output, _, events) = inline();
    install(&output, 1);
    let epoch = output.shared.lock().epoch;
    output.scope_revoker(run()).observe(notice(1, 2, 0, true));
    assert_eq!(output.shared.lock().epoch, epoch);
    output.dispatch(vec![held("key", 1)], 10, 0).unwrap();
    assert_eq!(output.take_completion().unwrap().outcomes.len(), 1);
    assert!(!events.lock().unwrap().is_empty());
}
