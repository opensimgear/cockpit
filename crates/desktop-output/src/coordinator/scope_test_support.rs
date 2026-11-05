use super::*;
use crate::worker::Worker;
use osg_mapping::desktop::Key;
use std::sync::{Mutex, atomic::{AtomicU64, Ordering}};

#[derive(Clone, Copy, Debug, Default)]
pub(super) enum Forgery {
    #[default] None, Missing, Duplicate, Unknown, Regress, InitialEpoch,
    EligibleWithoutEpoch, ExpiryWithoutEpoch, ExpiryEligible, Budget,
    MissingHeld, RejectCleanup, Rejected, ZeroCounts,
}
#[derive(Clone, Default)]
pub(super) struct Source {
    pub evidence: Arc<Mutex<BTreeMap<u64, ScopeEvidence>>>,
    pub events: Arc<Mutex<Vec<NativeEvent>>>,
    pub retired: Arc<Mutex<Vec<u64>>>,
}
impl Source {
    pub fn lose(&self, registration: u64) {
        self.evidence.lock().unwrap().get_mut(&registration).unwrap().loss_epoch += 1;
    }
}
impl Adapter for Source {
    fn bind_scope(&mut self, registration: u64, _: &Scope) -> Result<ScopeEvidence, String> {
        let evidence = ScopeEvidence { loss_epoch: 0, eligible: true };
        self.evidence.lock().unwrap().insert(registration, evidence);
        Ok(evidence)
    }
    fn scope_evidence(&mut self, registration: u64, _: &Scope) -> Result<ScopeEvidence, String> {
        self.evidence.lock().unwrap().get(&registration).copied().ok_or("unbound scope".into())
    }
    fn retire_scope(&mut self, registration: u64) {
        self.evidence.lock().unwrap().remove(&registration);
        self.retired.lock().unwrap().push(registration);
    }
    fn notification_driven(&self) -> bool { true }
    fn eligible(&mut self, _: &Scope) -> Result<bool, String> { Ok(true) }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
        self.events.lock().unwrap().extend_from_slice(events);
        Ok(native_count(events))
    }
}
struct RecordingChannel {
    worker: Worker<Source>,
    fault: Arc<Mutex<Forgery>>,
    now: Arc<AtomicU64>,
    acks: Arc<Mutex<Vec<Ack>>>,
}
impl Channel for RecordingChannel {
    fn request(&mut self, request: &Request) -> Result<Ack, String> {
        let fault = std::mem::take(&mut *self.fault.lock().unwrap());
        let mut actual = request.clone();
        if matches!(fault, Forgery::RejectCleanup) {
            let delivery = request.command.delivery().unwrap();
            actual.command = Command::Renew { run: delivery.producer.run.clone() };
        }
        let mut ack = self.worker.receive(actual, self.now.load(Ordering::Acquire));
        match fault {
            Forgery::None => {},
            Forgery::Missing => { ack.scopes.pop(); },
            Forgery::Duplicate => { ack.scopes[1] = ack.scopes[0]; },
            Forgery::Unknown => {
                ack.scopes[0].admission.epoch.0 += 1;
                ack.scopes[1].admission.registration = Counter(999);
            },
            Forgery::Regress => ack.scopes[0].admission.epoch = Counter(1),
            Forgery::InitialEpoch => ack.scopes.last_mut().unwrap().admission.epoch = Counter(2),
            Forgery::EligibleWithoutEpoch => ack.scopes[0].eligible = false,
            Forgery::ExpiryWithoutEpoch => { ack.scopes[0].live = false; ack.scopes[0].eligible = false; },
            Forgery::ExpiryEligible => { ack.scopes[0].live = false; ack.scopes[0].admission.epoch.0 += 1; },
            Forgery::Budget => ack.scopes = vec![ack.scopes[0]; 65],
            Forgery::MissingHeld => ack.held.clear(),
            Forgery::ZeroCounts => { ack.attempted = 0; ack.confirmed = 0; },
            Forgery::RejectCleanup => {
                ack.operation_id = request.command.delivery().map(|delivery| delivery.operation_id);
                ack.skipped = true;
                ack.scope_rejected = true;
            },
            Forgery::Rejected => { ack.ok = false; ack.error = Some("recording rejection".into()); },
        }
        self.acks.lock().unwrap().push(ack.clone());
        Ok(ack)
    }
}
pub(super) struct Fixture {
    pub coordinator: Coordinator,
    pub source: Source,
    pub fault: Arc<Mutex<Forgery>>,
    pub now: Arc<AtomicU64>,
    pub acks: Arc<Mutex<Vec<Ack>>>,
    pub updates: Arc<Mutex<Vec<ScopeUpdate>>>,
}
impl Fixture {
    pub fn new() -> Self {
        let source = Source::default();
        let fault = Arc::new(Mutex::new(Forgery::None));
        let now = Arc::new(AtomicU64::new(0));
        let acks = Arc::new(Mutex::new(vec![]));
        let mut coordinator = Coordinator::with_channel(Box::new(RecordingChannel {
            worker: Worker::new(source.clone()), fault: fault.clone(), now: now.clone(), acks: acks.clone(),
        }), || 0);
        let updates = Arc::new(Mutex::new(vec![]));
        let observed = updates.clone();
        coordinator.set_scope_observer(Arc::new(move |update| observed.lock().unwrap().push(update)));
        Self { coordinator, source, fault, now, acks, updates }
    }
    pub fn two_runs() -> Self {
        let mut fixture = Self::new();
        for index in 1..=2 {
            fixture.coordinator.activate(run(index), Scope::DesktopWide).unwrap();
            fixture.coordinator.arm(token(index), "action".into()).unwrap();
        }
        fixture.updates.lock().unwrap().clear();
        fixture
    }
    pub fn registration(&self, index: u64) -> u64 {
        self.coordinator.scope_receipt(&run(index)).unwrap().admission.registration.0
    }
}
pub(super) fn run(index: u64) -> RunToken {
    RunToken { session: format!("scope-receipt-{index}"), generation: Counter(1) }
}
pub(super) fn token(index: u64) -> Token {
    Token { run: run(index), node_id: "button".into(), dependencies: vec![] }
}
pub(super) fn key(index: u64) -> Control { Control::Key { key: Key(index as u16 + 3) } }
pub(super) fn delivery(index: u64, operation: u64, intent: Intent) -> Delivery {
    Delivery { producer: token(index), action_id: "action".into(), operation_id: Counter(operation),
        sequence: Counter(operation), at_us: Counter(0), deadline_us: None, intent }
}
pub(super) fn down(index: u64, operation: u64) -> Delivery {
    delivery(index, operation, Intent::Held { controls: vec![key(index)] })
}
pub(super) fn journal_dir() -> tempfile::TempDir {
    tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap()
}
