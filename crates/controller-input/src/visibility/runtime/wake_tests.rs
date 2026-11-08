use super::*;
use crate::registry::Registry;
use std::collections::BTreeMap;

struct ReadyCapture {
    registry: Arc<Mutex<Registry>>,
    task: Task,
    factory: Factory,
    capture: Capture,
    state: Arc<Mutex<State>>,
    fresh_read: Receiver<()>,
    resume_read: SyncSender<()>,
}

impl ReadyCapture {
    fn new() -> Self {
        let (mut task, factory, _, state, _) = fixture();
        let mut registry = Registry::default();
        let mut definition = crate::tests::fixture("exact-interface", None);
        definition.identity.backend = "windows-raw-input".into();
        let id = registry.connect(definition).unwrap();
        registry.report(&id, 1, &BTreeMap::from([(0, 0), (1, 0)])).unwrap();
        registry.report(&id, 2, &BTreeMap::from([(2, 0)])).unwrap();
        let Task::Apply { ids, expected, preferences, .. } = &mut task else { unreachable!() };
        *ids = vec![id.clone()];
        *expected = registry.visibility_sources(ids).unwrap();
        preferences.devices.clear();
        preferences.devices.insert(id, Intent::HideAll);
        let registry = Arc::new(Mutex::new(registry));
        let mut capture = Capture::registry(registry.clone());
        let observe = capture.observe.clone();
        let hidden = state.clone();
        let reads = AtomicU64::new(0);
        let (observed, fresh_read) = mpsc::sync_channel(1);
        let (resume_read, resume) = mpsc::sync_channel(1);
        let resume = Mutex::new(resume);
        capture.observe = Arc::new(move |ids| {
            let snapshot = observe(ids);
            // After the final driver mutation the journal makes one safety read,
            // then Session records a baseline, then waits for a newer report.
            if hidden.lock().unwrap().active && reads.fetch_add(1, Ordering::AcqRel) == 2 {
                observed.send(()).unwrap();
                resume.lock().unwrap().recv_timeout(Duration::from_secs(4)).unwrap();
            }
            snapshot
        });
        Self { registry, task, factory, capture, state, fresh_read, resume_read }
    }
}

#[test]
fn unchanged_report_between_read_and_wait_arms_and_topology_loss_restores() {
    let ReadyCapture { registry, task, factory, capture, state, fresh_read, resume_read } = ReadyCapture::new();
    let temp = crate::test_directory();
    let path = temp.path().join("visibility.json");
    let mut worker = Worker::default();
    let Task::Apply { ids, .. } = &task else { unreachable!() };
    let id = ids[0].clone();
    let status = worker.start(task, path.clone(), factory, capture).unwrap();
    fresh_read.recv_timeout(Duration::from_secs(4)).unwrap();
    assert_eq!(worker.snapshot().unwrap().phase, "preflight");
    let report = || {
        registry.lock().unwrap().report(&id, 1, &BTreeMap::from([(0, 0), (1, 0)])).unwrap();
    };
    let before = worker.wake.observed();
    report();
    assert!(worker.wake.observed() > before);
    resume_read.send(()).unwrap();
    assert!(worker.completion().wait(&status.job_id, false, Duration::from_secs(1)).unwrap());
    assert_eq!(worker.snapshot().unwrap().phase, "active");

    let before = worker.wake.observed();
    report();
    assert_eq!(worker.wake.observed(), before, "active capture must not wake on every report");
    registry.lock().unwrap().disconnect(&id);
    assert!(worker.wake.observed() > before);
    assert!(worker.completion().wait(&status.job_id, true, Duration::from_secs(1)).unwrap());
    assert!(worker.snapshot().unwrap().message.contains("complete current baseline"));
    assert!(!state.lock().unwrap().active);
    assert!(journal::inspect(&path).unwrap().unwrap().clean);
}

#[test]
fn cancelled_or_expired_fresh_wait_restores_owned_policy() {
    for cancel in [true, false] {
        let ReadyCapture { task, factory, capture, state, fresh_read, resume_read, .. } = ReadyCapture::new();
        let temp = crate::test_directory();
        let path = temp.path().join("visibility.json");
        let mut worker = Worker::default();
        let status = worker.start(task, path.clone(), factory, capture).unwrap();
        fresh_read.recv_timeout(Duration::from_secs(4)).unwrap();
        assert!(state.lock().unwrap().active);
        resume_read.send(()).unwrap();
        let budget = if cancel {
            let before = worker.wake.observed();
            worker.stop(Some(&status.job_id)).unwrap();
            assert!(worker.wake.observed() > before);
            Duration::from_secs(1)
        } else {
            Duration::from_secs(4)
        };
        assert!(worker.completion().wait(&status.job_id, true, budget).unwrap());
        let expected = if cancel { "cancelled" } else { "No fresh capture report" };
        assert!(worker.snapshot().unwrap().message.contains(expected));
        assert!(!state.lock().unwrap().active);
        assert!(journal::inspect(&path).unwrap().unwrap().clean);
    }
}

#[test]
fn unavailable_inventory_watch_fails_before_policy_mutation() {
    let ReadyCapture { registry, task, factory, capture, state, .. } = ReadyCapture::new();
    let retained = Arc::new(NativeWake::default());
    registry.lock().unwrap().watch_visibility_inventory(&retained).unwrap();
    let temp = crate::test_directory();
    let path = temp.path().join("visibility.json");
    let mut worker = Worker::default();
    let status = worker.start(task, path.clone(), factory, capture).unwrap();
    assert!(worker.completion().wait(&status.job_id, true, Duration::from_secs(1)).unwrap());
    assert!(worker.snapshot().unwrap().message.contains("already has a native inventory observer"));
    assert!(!state.lock().unwrap().active);
    assert!(journal::inspect(&path).unwrap().is_none());
}
