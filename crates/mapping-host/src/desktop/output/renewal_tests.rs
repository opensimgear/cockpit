use super::*;
use crate::desktop::output::{Mode, Output};
use osg_desktop_output::{Scope, coordinator::Channel, protocol::{Ack, Adapter, Command, NativeEvent, Request}, worker::Worker};
use osg_mapping::engine::Counter;
use std::sync::{Mutex, atomic::{AtomicUsize, Ordering}};

struct Native;
impl Adapter for Native {
    fn eligible(&mut self, _: &Scope) -> Result<bool, String> { Ok(true) }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
        Ok(osg_desktop_output::protocol::native_count(events))
    }
}
struct Recording {
    worker: Worker<Native>,
    renewals: Arc<AtomicUsize>,
}
impl Channel for Recording {
    fn request(&mut self, request: &Request) -> Result<Ack, String> {
        if matches!(request.command, Command::Renew { .. }) { self.renewals.fetch_add(1, Ordering::SeqCst); }
        Ok(self.worker.receive(request.clone(), 10))
    }
}
fn run() -> RunToken { RunToken { session: "renewal-rendezvous".into(), generation: Counter(1) } }
fn fixture() -> (Output, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let wakes = Arc::new(AtomicUsize::new(0));
    let renewals = Arc::new(AtomicUsize::new(0));
    let channel = Recording { worker: Worker::new(Native), renewals: renewals.clone() };
    let wake = wakes.clone();
    let (shared, owner) = Output::prepare(Coordinator::with_channel(Box::new(channel), || 10),
        Arc::new(move || { wake.fetch_add(1, Ordering::SeqCst); }), true, BTreeMap::new());
    let output = Output { shared, mode: Mode::Inline(Mutex::new(owner)) };
    output.activate(run(), Scope::DesktopWide).unwrap();
    wakes.store(0, Ordering::SeqCst);
    (output, wakes, renewals)
}
fn owner(output: &Output) -> std::sync::MutexGuard<'_, Owner> {
    let Mode::Inline(owner) = &output.mode else { unreachable!() };
    owner.lock().unwrap()
}
fn due(output: &Output) {
    owner(output).last_renew = Some(Instant::now() - RENEW_INTERVAL - Duration::from_millis(1));
}

#[test]
fn due_idle_lease_requests_one_parent_wake_and_never_renews_without_a_response() {
    let (output, wakes, renewals) = fixture();
    assert!(owner(&output).renewal_wait(&output.shared.lock()).is_some());
    due(&output);
    assert_eq!(owner(&output).renewal_wait(&output.shared.lock()), Some(Duration::ZERO));
    output.drive();
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    assert!(owner(&output).renewal_wait(&output.shared.lock()).is_none());
    for _ in 0..10 { output.drive(); }
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    assert_eq!(renewals.load(Ordering::SeqCst), 0);
}

#[test]
fn queued_neutral_arm_waits_for_outstanding_lease_authorization() {
    let (output, wakes, renewals) = fixture();
    due(&output);
    output.drive();
    let admission = output.child_scope().map(|receipt| receipt.admission);
    let producer = Token { run: run(), node_id: "source".into(), dependencies: vec![] };
    output.dispatch_prepared(vec![], Some(("key".into(), producer)), 10, 0, admission).unwrap();
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    assert!(output.shared.lock().job.is_some());
    assert!(output.snapshot().owners.is_empty());

    output.renew(run()).unwrap();
    assert_eq!(renewals.load(Ordering::SeqCst), 1);
    assert_eq!(output.snapshot().owners.len(), 1);
    assert!(output.snapshot().held.is_empty());
    assert!(output.take_completion().unwrap().error.is_none());
}

#[test]
fn early_authorization_that_expires_before_due_requests_fresh_parent_evaluation() {
    let (output, wakes, renewals) = fixture();
    output.renew(run()).unwrap();
    assert_eq!(renewals.load(Ordering::SeqCst), 0);
    output.shared.lock().authorization.as_mut().unwrap().1 = Instant::now() - AUTHORIZATION_FRESHNESS - Duration::from_millis(1);
    due(&output);
    output.drive();
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    assert!(output.shared.lock().authorization.is_none());
    assert_eq!(renewals.load(Ordering::SeqCst), 0);
    output.renew(run()).unwrap();
    assert_eq!(renewals.load(Ordering::SeqCst), 1);
    assert!(!owner(&output).renewal_requested);
    due(&output);
    output.drive();
    assert_eq!(renewals.load(Ordering::SeqCst), 1, "Consumed authorization cannot renew again");
}

#[test]
fn response_that_stalls_after_a_request_gets_one_replacement_wake() {
    let (output, wakes, renewals) = fixture();
    due(&output);
    output.drive();
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    {
        let mut state = output.shared.lock();
        state.authorization_id += 1;
        state.authorization = Some((run(), Instant::now() - AUTHORIZATION_FRESHNESS - Duration::from_millis(1), state.authorization_id));
    }
    output.drive();
    assert_eq!(wakes.load(Ordering::SeqCst), 2);
    for _ in 0..10 { output.drive(); }
    assert_eq!(wakes.load(Ordering::SeqCst), 2);
    assert_eq!(renewals.load(Ordering::SeqCst), 0);
    output.renew(run()).unwrap();
    assert_eq!(renewals.load(Ordering::SeqCst), 1);
}

#[test]
fn stop_or_fault_retires_renewal_requests() {
    let (output, wakes, renewals) = fixture();
    due(&output);
    output.stop_all().unwrap();
    wakes.store(0, Ordering::SeqCst);
    for _ in 0..10 { output.drive(); }
    assert_eq!(wakes.load(Ordering::SeqCst), 0);
    assert!(owner(&output).renewal_wait(&output.shared.lock()).is_none());
    assert_eq!(renewals.load(Ordering::SeqCst), 0);

    let (output, wakes, renewals) = fixture();
    due(&output);
    output.shared.lock().fault = Some("Injected terminal fault".into());
    output.drive();
    wakes.store(0, Ordering::SeqCst);
    for _ in 0..10 { output.drive(); }
    assert_eq!(wakes.load(Ordering::SeqCst), 0);
    assert!(owner(&output).renewal_wait(&output.shared.lock()).is_none());
    assert_eq!(renewals.load(Ordering::SeqCst), 0);
}

#[test]
fn healthy_paused_registration_still_requests_and_accepts_parent_renewal() {
    let (output, wakes, renewals) = fixture();
    output.pause(run()).unwrap();
    wakes.store(0, Ordering::SeqCst);
    due(&output);
    output.drive();
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    output.renew(run()).unwrap();
    assert_eq!(renewals.load(Ordering::SeqCst), 1);
}
