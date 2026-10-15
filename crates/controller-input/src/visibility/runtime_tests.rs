use super::super::policy::{tests::state, State};
use super::*;
use std::sync::atomic::AtomicU64;
struct AdapterFixture {
    state: Arc<Mutex<State>>,
    broken: Arc<AtomicBool>,
}
impl Backend for AdapterFixture {
    fn read(&mut self) -> Result<State, String> {
        Ok(self.state.lock().unwrap().clone())
    }
    fn change(&mut self, expected: &State, desired: &State) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if *state != *expected {
            return Err("external change".into());
        }
        *state = desired.clone();
        Ok(())
    }
}
impl Adapter for AdapterFixture {
    fn executable(&mut self, _: &str) -> Result<String, String> {
        if self.broken.load(Ordering::Acquire) {
            Err("executable relocated".into())
        } else {
            Ok(r"\Device\Volume1\Cockpit.exe".into())
        }
    }
    fn instance(&mut self, _: &str) -> Result<String, String> {
        Ok(r"HID\EXACT\CONTROLLER".into())
    }
}
fn fixture() -> (Task, Factory, Observe, Arc<Mutex<State>>, Arc<AtomicBool>) {
    let source = Source {
        id: uuid::Uuid::new_v4().to_string(),
        session: uuid::Uuid::new_v4().to_string(),
        generation: 1,
        sequence: 1,
        interface: "exact-interface".into(),
    };
    let mut preferences = Preferences {
        enabled: true,
        ..Preferences::default()
    };
    preferences
        .devices
        .insert(source.id.clone(), Intent::HideAll);
    let task = Task::Apply {
        expected_plan: None,
        preferences,
        ids: vec![source.id.clone()],
        capture: r"C:\Cockpit.exe".into(),
        expected: vec![source.clone()],
    };
    let state = Arc::new(Mutex::new(state()));
    let broken = Arc::new(AtomicBool::new(false));
    let shared = state.clone();
    let failure = broken.clone();
    let factory: Factory = Arc::new(move || {
        Ok(Box::new(AdapterFixture {
            state: shared.clone(),
            broken: failure.clone(),
        }))
    });
    let sequence = AtomicU64::new(1);
    let observe: Observe = Arc::new(move |_| {
        let mut source = source.clone();
        source.sequence = sequence.fetch_add(1, Ordering::AcqRel);
        Ok(vec![source])
    });
    (task, factory, observe, state, broken)
}
fn wait(worker: &Worker, predicate: impl Fn(&Status) -> bool) -> Status {
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if let Some(status) = worker.snapshot() {
            if predicate(&status) {
                return status;
            }
        }
        assert!(
            Instant::now() < deadline,
            "worker deadline: {:?}",
            worker.snapshot()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn game_preflight_plan_is_rechecked_in_the_mutating_worker() {
    let (mut task, factory, observe, state, _) = fixture();
    let before = state.lock().unwrap().clone();
    let mut planned_before = before.clone();
    planned_before
        .applications
        .push(r"\Device\Volume1\Old.exe".into());
    let plan = Plan::build(
        planned_before,
        vec![Selection {
            instance: r"HID\EXACT\CONTROLLER".into(),
            policy: super::super::policy::Policy::HideAll,
        }],
        r"\Device\Volume1\Cockpit.exe".into(),
    )
    .unwrap();
    if let Task::Apply { expected_plan, .. } = &mut task {
        *expected_plan = Some(plan);
    }
    let temp = tempfile::tempdir().unwrap();
    let mut worker = Worker::default();
    worker
        .start(task, temp.path().join("session.json"), factory, observe)
        .unwrap();
    let status = wait(&worker, |status| !status.active);
    assert!(status.message.contains("changed after game preflight"));
    assert_eq!(*state.lock().unwrap(), before);
    assert!(!temp.path().join("session.json").exists());
}
#[test]
fn independent_worker_requires_fresh_reports_and_stops_without_ui_polling() {
    let (task, factory, observe, state, _) = fixture();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("v.json");
    let mut worker = Worker::default();
    let status = worker
        .start(task.clone(), path.clone(), factory.clone(), observe.clone())
        .unwrap();
    wait(&worker, |s| s.phase == "active");
    assert!(state.lock().unwrap().active);
    assert!(worker.start(task, path.clone(), factory, observe).is_err());
    assert!(worker.stop(Some("stale")).is_err());
    worker.stop(Some(&status.job_id)).unwrap();
    assert_eq!(wait(&worker, |s| !s.active).phase, "restored");
    assert!(!state.lock().unwrap().active);
    assert!(journal::inspect(&path).unwrap().unwrap().clean);
}
#[test]
fn relocated_executable_shutdown_and_missing_dependencies_are_explicit() {
    let (task, factory, observe, state, broken) = fixture();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("v.json");
    let mut worker = Worker::default();
    worker
        .start(task.clone(), path.clone(), factory.clone(), observe.clone())
        .unwrap();
    wait(&worker, |s| s.phase == "active");
    broken.store(true, Ordering::Release);
    assert!(wait(&worker, |s| !s.active).message.contains("relocated"));
    assert!(!state.lock().unwrap().active);
    broken.store(false, Ordering::Release);
    worker
        .start(task.clone(), path.clone(), factory.clone(), observe.clone())
        .unwrap();
    wait(&worker, |s| s.phase == "active");
    worker.shutdown();
    wait(&worker, |s| !s.active);
    assert!(!state.lock().unwrap().active);
    worker
        .start(
            task,
            path.clone(),
            Arc::new(|| Err("HidHide absent".into())),
            observe.clone(),
        )
        .unwrap();
    assert!(wait(&worker, |s| !s.active).message.contains("absent"));
    worker
        .start(Task::Recover { id: "stale".into() }, path, factory, observe)
        .unwrap();
    assert!(wait(&worker, |s| !s.active).message.contains("Stale"));
}
#[test]
fn cancellation_before_preflight_disabled_intent_and_worker_panic_do_not_hide() {
    let (task, factory, observe, state, _) = fixture();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("v.json");
    assert!(run(
        task.clone(),
        path.clone(),
        factory.clone(),
        observe.clone(),
        Arc::new(AtomicBool::new(true)),
        |_| panic!("not active")
    )
    .is_err());
    let mut disabled = task.clone();
    if let Task::Apply { preferences, .. } = &mut disabled {
        preferences.enabled = false;
    }
    assert!(run(
        disabled,
        path.clone(),
        factory,
        observe.clone(),
        Arc::new(AtomicBool::new(false)),
        |_| panic!("not active")
    )
    .is_err());
    assert!(!state.lock().unwrap().active);
    let mut worker = Worker::default();
    worker
        .start(task, path, Arc::new(|| panic!("fixture panic")), observe)
        .unwrap();
    assert!(wait(&worker, |s| !s.active).message.contains("failed"));
}
