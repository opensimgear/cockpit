use super::*;
use osg_desktop_output::{coordinator::ScopeUpdate, protocol::{ScopeEvidence, ScopeReceipt}};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

struct Source {
    loss: Arc<AtomicU64>,
    eligible: Arc<AtomicBool>,
    events: Events,
}
impl Adapter for Source {
    fn scope_evidence(&mut self, _: u64, _: &Scope) -> Result<ScopeEvidence, String> {
        Ok(ScopeEvidence { loss_epoch: self.loss.load(Ordering::Acquire), eligible: self.eligible.load(Ordering::Acquire) })
    }
    fn eligible(&mut self, _: &Scope) -> Result<bool, String> { Ok(self.eligible.load(Ordering::Acquire)) }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
        self.events.lock().unwrap().extend_from_slice(events);
        Ok(native_count(events))
    }
}
struct SourceChannel {
    worker: Worker<Source>,
    requests: Requests,
    now: Arc<AtomicU64>,
    block: Option<(SyncSender<ScopeReceipt>, Receiver<()>)>,
}
impl Channel for SourceChannel {
    fn request(&mut self, request: &Request) -> Result<Ack, String> {
        self.requests.lock().unwrap().push(request.clone());
        let ack = self.worker.receive(request.clone(), self.now.load(Ordering::Acquire));
        if matches!(request.command, Command::Arm { .. }) {
            if let Some((entered, resume)) = self.block.take() {
                entered.send(ack.scopes[0]).unwrap();
                resume.recv().unwrap();
            }
        }
        Ok(ack)
    }
}
fn source() -> (Output, Arc<AtomicU64>, Arc<AtomicBool>, Arc<AtomicU64>, Requests, Events) {
    let loss = Arc::new(AtomicU64::new(0));
    let eligible = Arc::new(AtomicBool::new(true));
    let now = Arc::new(AtomicU64::new(10));
    let events = Arc::new(Mutex::new(vec![]));
    let requests = Arc::new(Mutex::new(vec![]));
    let channel = SourceChannel { worker: Worker::new(Source { loss: loss.clone(), eligible: eligible.clone(), events: events.clone() }), requests: requests.clone(), now: now.clone(), block: None };
    let output = Output::inline(Coordinator::with_channel(Box::new(channel), || 10));
    output.activate(run(), Scope::DesktopWide).unwrap();
    (output, loss, eligible, now, requests, events)
}
fn renew_now(output: &Output) {
    let Mode::Inline(owner) = &output.mode else { unreachable!() };
    owner.lock().unwrap().last_renew = Some(Instant::now() - Duration::from_millis(101));
    output.renew(run()).unwrap();
}
fn proof(output: &Output) -> Option<ScopeAdmission> { output.child_scope().map(|receipt| receipt.admission) }

#[test]
fn worker_loss_receipt_revokes_already_armed_input_without_parent_scope_change() {
    let (output, loss, _, _, requests, events) = source();
    let old = proof(&output);
    output.dispatch(vec![held("key", 1)], 10, 0, old).unwrap();
    output.take_completion().unwrap();
    assert_eq!(events.lock().unwrap().len(), 1);
    loss.store(1, Ordering::Release); // A collapsed native away/back cycle.
    renew_now(&output);
    let new = proof(&output);
    assert_ne!(old, new);
    assert!(output.shared.lock().authorization.is_none());
    output.dispatch(vec![held("key", 2), pointer("x", 3)], 10, 0, old).unwrap();
    let completion = output.take_completion().unwrap();
    assert_eq!(completion.dropped.len(), 2);
    assert!(completion.error.is_none());
    assert_eq!(events.lock().unwrap().len(), 2, "Only native cleanup follows loss");
    output.acknowledge_child(new.unwrap());
    output.dispatch(vec![held("key", 4)], 10, 0, new).unwrap();
    assert!(output.take_completion().unwrap().error.is_none());
    assert_eq!(requests.lock().unwrap().iter().filter(|request| matches!(request.command, Command::Arm { .. })).count(), 2,
        "The same producer token must be armed again under its fresh proof");
    assert_eq!(events.lock().unwrap().len(), 3);
}

#[test]
fn skipped_arm_never_populates_owner_cache_or_delivers_the_rest_of_the_job() {
    let (output, _, eligible, _, requests, events) = source();
    eligible.store(false, Ordering::Release);
    output.dispatch(vec![held("key", 1), pointer("x", 2)], 10, 0, proof(&output)).unwrap();
    let completion = output.take_completion().unwrap();
    assert!(completion.error.is_none());
    assert_eq!(completion.dropped.len(), 2);
    assert!(events.lock().unwrap().is_empty());
    assert!(!requests.lock().unwrap().iter().any(|request| matches!(request.command, Command::Deliver { .. } | Command::DeliverPointerBatch { .. })));
}

#[test]
fn expired_worker_receipt_faults_and_cleans_while_paused_live_receipts_can_renew() {
    let (output, _, _, now, _, _) = source();
    output.pause(run()).unwrap();
    renew_now(&output);
    assert!(output.child_scope().unwrap().live);
    now.store(250_010, Ordering::Release);
    renew_now(&output);
    assert!(output.snapshot().error.is_some());
    assert!(output.snapshot().cleanup_complete);
    assert!(!output.shared.lock().authorization.is_some());
}

#[test]
fn gate_requires_an_immutable_executing_proof_and_never_accepts_missing_proof() {
    let (output, _, _) = inline();
    let old = proof(&output).unwrap();
    let delivery = held("key", 1);
    let mut request = Request { sequence: Counter(2), deadline_us: None, admission: None,
        command: Command::Arm { producer: delivery.producer, action_id: delivery.action_id } };
    assert!(!output.shared.permitted(&request));
    request.admission = Some(old);
    assert!(!output.shared.permitted(&request));
    output.shared.lock().executing_admission = Some(old);
    assert!(output.shared.permitted(&request));
    let new = ScopeAdmission { epoch: Counter(old.epoch.0 + 1), ..old };
    output.shared.lock().child_scope = Some(ScopeReceipt { admission: new, live: true, eligible: true });
    output.acknowledge_child(new);
    request.admission = Some(new);
    assert!(!output.shared.permitted(&request), "Sampling the latest receipt cannot upgrade an executing job");
}

#[test]
fn retired_and_other_run_callbacks_cannot_revoke_current_scope() {
    let (output, _, _) = inline();
    let receipt = output.child_scope().unwrap();
    let epoch = output.shared.lock().epoch;
    let mut other = run();
    other.generation = Counter(2);
    super::super::child::observe(&output.shared, ScopeUpdate { run: other, receipt: ScopeReceipt { live: false, ..receipt } });
    assert_eq!(output.shared.lock().epoch, epoch);
    output.stop_all().unwrap();
    super::super::child::observe(&output.shared, ScopeUpdate { run: run(), receipt: ScopeReceipt { live: false, ..receipt } });
    assert!(output.snapshot().error.is_none());
}

#[test]
fn helper_callback_revokes_without_waiting_for_a_blocked_ordered_owner() {
    let loss = Arc::new(AtomicU64::new(0));
    let events = Arc::new(Mutex::new(vec![]));
    let requests = Arc::new(Mutex::new(vec![]));
    let wakes = Arc::new(AtomicU64::new(0));
    let (entered, arrival) = mpsc::sync_channel(1);
    let (resume, release) = mpsc::sync_channel(1);
    let channel = SourceChannel {
        worker: Worker::new(Source { loss: loss.clone(), eligible: Arc::new(AtomicBool::new(true)), events: events.clone() }),
        requests: requests.clone(), now: Arc::new(AtomicU64::new(10)), block: Some((entered, release)),
    };
    let notified = wakes.clone();
    let output = Output::threaded(Coordinator::with_channel(Box::new(channel), || 10),
        Arc::new(move || { notified.fetch_add(1, Ordering::Relaxed); }), BTreeMap::new()).unwrap();
    output.activate(run(), Scope::DesktopWide).unwrap();
    let old = proof(&output);
    loss.store(1, Ordering::Release);
    let deliveries = vec![held("key", 1), pointer("x", 2)];
    reconcile(&output, &deliveries);
    output.dispatch(deliveries, 10, 0, old).unwrap();
    let receipt = arrival.recv_timeout(Duration::from_secs(1)).unwrap();
    // Exercise the trusted receipt callback independently of the blocked IPC
    // owner, using the exact receipt the actual Worker is about to acknowledge.
    let before = wakes.load(Ordering::Relaxed);
    let started = Instant::now();
    super::super::child::observe(&output.shared, ScopeUpdate { run: run(), receipt });
    assert!(started.elapsed() < Duration::from_millis(100));
    assert!(wakes.load(Ordering::Relaxed) > before);
    assert!(output.shared.lock().authorization.is_none());
    output.renew(run()).unwrap();
    assert!(output.shared.lock().authorization.is_none());
    resume.send(()).unwrap();
    wait_until(|| output.shared.lock().completion.is_some());
    let completion = output.take_completion().unwrap();
    assert!(completion.error.is_none());
    assert_eq!(completion.dropped.len(), 2);
    assert!(events.lock().unwrap().is_empty());
    assert_eq!(requests.lock().unwrap().iter().filter(|request| matches!(request.command, Command::Arm { .. })).count(), 1);
    output.stop_all().unwrap();
}
