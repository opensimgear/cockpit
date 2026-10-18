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
fn handoff_input(task: &Task, job: &str, record: Record) -> HandoffInput {
    let Task::Apply {
        preferences,
        capture,
        expected,
        ..
    } = task
    else {
        panic!("Apply fixture required")
    };
    HandoffInput {
        job: job.into(),
        record,
        preferences: preferences.clone(),
        sources: expected.clone(),
        capture: capture.clone(),
    }
}
#[test]
fn retained_read_tickets_reject_wrong_job_session_cancellation_and_expired_queued_replies() {
    let (task, factory, observe, state, _) = fixture();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tickets.json");
    let mut worker = Worker::default();
    assert!(worker.handoff_status().is_err());
    let status = worker
        .start(task.clone(), path.clone(), factory, observe)
        .unwrap();
    wait(&worker, |s| s.phase == "active");
    let record = journal::inspect(&path).unwrap().unwrap();
    let mut wrong = handoff_input(&task, "wrong-job", record.clone());
    assert!(worker.prepare_handoff(wrong).is_err());
    wrong = handoff_input(&task, &status.job_id, record.clone());
    wrong.record.id = uuid::Uuid::new_v4().to_string();
    assert!(worker.prepare_handoff(wrong).is_err());
    let read = worker
        .prepare_handoff(handoff_input(&task, &status.job_id, record.clone()))
        .unwrap()
        .wait()
        .unwrap();
    read.validate_live().unwrap();
    assert_eq!(read.actual, *state.lock().unwrap());
    let mut wrong = handoff_input(&task, &status.job_id, record.clone());
    wrong.record.position = 0;
    assert!(worker
        .prepare_handoff(wrong)
        .unwrap()
        .wait()
        .err()
        .unwrap()
        .contains("journal no longer matches"));
    let queued = worker
        .prepare_handoff(handoff_input(&task, &status.job_id, record.clone()))
        .unwrap();
    worker.stop(Some(&status.job_id)).unwrap();
    assert!(queued.wait().err().unwrap().contains("cancelled"));
    assert!(read.validate_live().unwrap_err().contains("cancelled"));
    assert!(worker.handoff_status().is_err());
    wait(&worker, |s| !s.active);
    let (reply, receive) = mpsc::sync_channel(1);
    reply
        .send(Ok((record.plan.after.clone(), record.plan.clone())))
        .unwrap();
    let expired = ReadTicket {
        receive,
        deadline: Instant::now() - Duration::from_millis(1),
        cancelled: Arc::new(AtomicBool::new(false)),
        abandoned: Arc::new(AtomicBool::new(false)),
    };
    assert!(expired.wait().err().unwrap().contains("expired"));
}
#[test]
fn retained_inspection_fences_source_policy_executable_journal_and_expiry_without_writes() {
    use std::sync::atomic::AtomicUsize;
    struct Counted(
        Box<dyn Adapter>,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
    );
    impl Backend for Counted {
        fn read(&mut self) -> Result<State, String> {
            self.1.fetch_add(1, Ordering::AcqRel);
            self.0.read()
        }
        fn change(&mut self, old: &State, new: &State) -> Result<(), String> {
            self.1.fetch_add(1, Ordering::AcqRel);
            self.2.fetch_add(1, Ordering::AcqRel);
            self.0.change(old, new)
        }
    }
    impl Adapter for Counted {
        fn executable(&mut self, path: &str) -> Result<String, String> {
            self.1.fetch_add(1, Ordering::AcqRel);
            if self.3.load(Ordering::Acquire) == 2 {
                return Ok(r"\Device\Volume1\Replacement.exe".into());
            }
            self.0.executable(path)
        }
        fn instance(&mut self, path: &str) -> Result<String, String> {
            self.1.fetch_add(1, Ordering::AcqRel);
            if self.3.load(Ordering::Acquire) == 1 {
                return Ok(r"HID\FOREIGN\INSTANCE".into());
            }
            self.0.instance(path)
        }
    }
    let (task, factory, observe, state, broken) = fixture();
    let Task::Apply {
        expected,
        ids,
        capture,
        ..
    } = &task
    else {
        unreachable!()
    };
    let before = state.lock().unwrap().clone();
    let applied = Plan::build(
        before.clone(),
        vec![Selection {
            instance: r"HID\EXACT\CONTROLLER".into(),
            policy: super::super::policy::Policy::HideAll,
        }],
        r"\Device\Volume1\Cockpit.exe".into(),
    )
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inspect.json");
    let calls = Arc::new(AtomicUsize::new(0));
    let writes = Arc::new(AtomicUsize::new(0));
    let drift = Arc::new(AtomicUsize::new(0));
    let mut session = Session::start(
        Box::new(Counted(
            factory().unwrap(),
            calls.clone(),
            writes.clone(),
            drift.clone(),
        )) as Box<dyn Adapter>,
        Store::open(path.clone()).unwrap(),
        applied,
        expected.clone(),
        |_| observe(ids),
    )
    .unwrap();
    let active = state.lock().unwrap().clone();
    let initial = std::fs::read(&path).unwrap();
    let initial_writes = writes.load(Ordering::Acquire);
    let make = || {
        let (reply, _) = mpsc::sync_channel(1);
        ReadRequest {
            input: handoff_input(
                &task,
                "exact-job",
                journal::inspect(&path).unwrap().unwrap(),
            ),
            deadline: Instant::now() + READ_BUDGET,
            abandoned: Arc::new(AtomicBool::new(false)),
            reply,
        }
    };
    let cancel = AtomicBool::new(false);
    let paths = vec![(capture.clone(), r"\Device\Volume1\Cockpit.exe".into())];
    let inspect = |session: &mut Session<Box<dyn Adapter>>, request: &ReadRequest| {
        inspect_handoff(session, request, "exact-job", &cancel, &observe, &paths)
    };
    assert_eq!(inspect(&mut session, &make()).unwrap().0, active);
    let mut request = make();
    request.input.job = "foreign-job".into();
    let before_calls = calls.load(Ordering::Acquire);
    assert!(inspect(&mut session, &request)
        .unwrap_err()
        .contains("no longer owns"));
    assert_eq!(calls.load(Ordering::Acquire), before_calls);
    let mut request = make();
    request.input.sources[0].generation += 1;
    assert!(inspect(&mut session, &request)
        .unwrap_err()
        .contains("lost its identity"));
    let mut request = make();
    request.input.preferences.enabled = false;
    assert!(inspect(&mut session, &request)
        .unwrap_err()
        .contains("Enable optional"));
    broken.store(true, Ordering::Release);
    assert!(inspect(&mut session, &make())
        .unwrap_err()
        .contains("relocated"));
    broken.store(false, Ordering::Release);
    drift.store(1, Ordering::Release);
    assert!(inspect(&mut session, &make())
        .unwrap_err()
        .contains("instance changed"));
    drift.store(2, Ordering::Release);
    assert!(inspect(&mut session, &make())
        .unwrap_err()
        .contains("capture executable changed"));
    drift.store(0, Ordering::Release);
    let mut request = make();
    request.deadline = Instant::now() - Duration::from_millis(1);
    let before_calls = calls.load(Ordering::Acquire);
    assert!(inspect(&mut session, &request)
        .unwrap_err()
        .contains("expired"));
    assert_eq!(calls.load(Ordering::Acquire), before_calls);
    let request = make();
    request.abandoned.store(true, Ordering::Release);
    assert!(inspect(&mut session, &request)
        .unwrap_err()
        .contains("no longer owns"));
    assert_eq!(calls.load(Ordering::Acquire), before_calls);
    assert_eq!(*state.lock().unwrap(), active);
    assert_eq!(std::fs::read(&path).unwrap(), initial);
    assert_eq!(writes.load(Ordering::Acquire), initial_writes);
    state
        .lock()
        .unwrap()
        .applications
        .push(r"\Device\Volume1\Foreign.exe".into());
    let foreign = state.lock().unwrap().clone();
    assert!(inspect(&mut session, &make())
        .unwrap_err()
        .contains("driver policy changed"));
    assert_eq!(*state.lock().unwrap(), foreign);
    assert_eq!(std::fs::read(&path).unwrap(), initial);
    assert_eq!(writes.load(Ordering::Acquire), initial_writes);
    // Only reset this injected backend to permit ordinary fixture teardown.
    *state.lock().unwrap() = active;
    session.stop().unwrap();
    assert_eq!(*state.lock().unwrap(), before);
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
        |_| panic!("not active"),
        mpsc::sync_channel(1).1,
        "unstarted".into()
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
        |_| panic!("not active"),
        mpsc::sync_channel(1).1,
        "unstarted".into()
    )
    .is_err());
    assert!(!state.lock().unwrap().active);
    let mut worker = Worker::default();
    worker
        .start(task, path, Arc::new(|| panic!("fixture panic")), observe)
        .unwrap();
    assert!(wait(&worker, |s| !s.active).message.contains("failed"));
}
