use super::*;
use crate::desktop::tests::{fixture, frame};
use osg_mapping::{desktop::{Action, Key}, engine::Reason};

fn notice(sequence: u64, loss_epoch: u64, eligible: bool) -> ScopeNotice {
    ScopeNotice { watch_id: 1, sequence, loss_epoch, eligible, error: None }
}
fn install(desktop: &Desktop) {
    desktop.output.install_scope_watch(1);
    let initial = notice(1, 0, true);
    desktop.output.scope_revoker(desktop.run.clone()).observe(initial.clone());
    desktop.output.acknowledge_scope(&initial);
}

#[test]
fn collapsed_loss_and_return_requires_fresh_neutral_and_does_not_replay_current_admissions() {
    let (mut desktop, mut phase, _, events) = fixture(Action::KeyHold { key: Key(4) }, false);
    install(&desktop);
    let frames = frame(&mut phase, 2, 2, Reason::Change, true);
    desktop.poll_focused(&frames, &phase, true).unwrap();
    assert!(!desktop.output.snapshot().held.is_empty());
    let revoke = desktop.output.scope_revoker(desktop.run.clone());
    revoke.observe(notice(2, 1, false));
    revoke.observe(notice(3, 1, true));
    let admissions = frames.into_iter().map(Into::into).collect::<Vec<Admission>>();
    desktop.poll_scope_notice(&admissions, &phase, notice(3, 1, true)).unwrap();
    assert_eq!(desktop.focus_boundary, Some(Counter(2)));
    assert_eq!(desktop.status()["state"], "waiting");
    assert!(desktop.output.snapshot().held.is_empty());
    let submitted = events.lock().unwrap().len();
    let held = frame(&mut phase, 3, 3, Reason::Change, true);
    let admissions = held.into_iter().map(Into::into).collect::<Vec<Admission>>();
    desktop.poll_scope_notice(&admissions, &phase, notice(3, 1, true)).unwrap();
    assert_eq!(events.lock().unwrap().len(), submitted);
    let neutral = frame(&mut phase, 4, 4, Reason::Change, false);
    let admissions = neutral.into_iter().map(Into::into).collect::<Vec<Admission>>();
    desktop.poll_scope_notice(&admissions, &phase, notice(3, 1, true)).unwrap();
    let pressed = frame(&mut phase, 5, 5, Reason::Change, true);
    let admissions = pressed.into_iter().map(Into::into).collect::<Vec<Admission>>();
    desktop.poll_scope_notice(&admissions, &phase, notice(3, 1, true)).unwrap();
    assert!(!desktop.output.snapshot().held.is_empty());
}

#[test]
fn scope_inspection_error_pauses_without_faulting_and_return_uses_a_new_baseline() {
    let (mut desktop, phase, _, _) = fixture(Action::KeyHold { key: Key(4) }, false);
    install(&desktop);
    let mut failed = notice(2, 1, false);
    failed.error = Some("Process identity unavailable".into());
    let diagnostics = desktop.poll_scope_notice(&[], &phase, failed).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(desktop.status()["state"], "paused");
    assert_eq!(desktop.status()["error"], Value::Null);
    assert_eq!(desktop.status()["scopeError"], "Process identity unavailable");
    desktop.poll_scope_notice(&[], &phase, notice(3, 1, true)).unwrap();
    assert_eq!(desktop.status()["state"], "waiting");
    assert_eq!(desktop.status()["scopeError"], Value::Null);
    assert!(desktop.focus_boundary.is_some());
}

#[cfg(windows)]
mod native {
    use super::*;
    use osg_desktop_output::{
        coordinator::Channel,
        protocol::{Ack, Adapter, Command, NativeEvent, Request},
        windows::focus::ScopeWatch,
        worker::Worker,
    };
    use std::sync::{atomic::{AtomicU64, Ordering}, Arc, Mutex};
    use std::time::{Duration, Instant};

    struct NoInput;
    impl Adapter for NoInput {
        fn eligible(&mut self, _: &Scope) -> Result<bool, String> { Ok(false) }
        fn submit(&mut self, _: &[NativeEvent]) -> Result<usize, String> {
            panic!("Scope lifecycle tests must never submit native input")
        }
    }
    type Requests = Arc<Mutex<Vec<(Request, Ack)>>>;
    struct Recording {
        worker: Worker<NoInput>,
        requests: Requests,
        clock: Arc<AtomicU64>,
    }
    impl Channel for Recording {
        fn request(&mut self, request: &Request) -> Result<Ack, String> {
            let ack = self.worker.receive(request.clone(), self.clock.load(Ordering::Acquire));
            self.requests.lock().unwrap().push((request.clone(), ack.clone()));
            Ok(ack)
        }
    }
    fn fixture() -> (Desktop, Phase, Requests, Arc<AtomicU64>) {
        let program = Program { sources: vec![], nodes: vec![], sinks: vec![], controllers: vec![] };
        let mut phase = Phase::new(program.clone(), Counter(1),
            "94a5ece4-4e3a-4395-a69b-3aa79c1ecf12".into()).unwrap();
        phase.drain();
        let requests = Arc::new(Mutex::new(vec![]));
        let clock = Arc::new(AtomicU64::new(10));
        let channel = Recording { worker: Worker::new(NoInput), requests: requests.clone(), clock: clock.clone() };
        let output = Output::threaded(Coordinator::with_channel(Box::new(channel), || 10),
            Arc::new(|| {}), BTreeMap::new()).unwrap();
        let run = phase.run_token();
        (Desktop {
            output, scheduler: Scheduler::from_program(run.clone(), &program).unwrap(), run,
            scope: None, focused: false, started: false, registered: false, last_error: None,
            outcomes: VecDeque::new(), dropped_outcomes: 0, diagnostics: VecDeque::new(),
            scope_error: None, focus_boundary: None, waiting_neutral: BTreeSet::new(),
            rearm_evidence: BTreeMap::new(), shadow: None, audit_shadow: 0,
            focus_watch: Default::default(), authorization_deadline: None,
        }, phase, requests, clock)
    }
    fn unrelated_application() -> Scope {
        Scope::Application { identity: osg_desktop_output::ProcessIdentity {
            executable: "C:\\ScopeLifecycleTest\\Absent.exe".into(),
            pid: u32::MAX, created: "1".into(),
        } }
    }
    fn renewals(requests: &Requests) -> usize {
        requests.lock().unwrap().iter().filter(|(request, ack)|
            matches!(request.command, Command::Renew { .. }) && ack.ok).count()
    }

    #[test]
    fn real_scope_subscription_lifecycle_and_paused_lease_never_submit_input() {
        // One real observer stays alive throughout the cases. These tests only
        // inspect its cached foreground evidence; they never switch focus or
        // broadcast synthetic WinEvents into another application's desktop.
        let observer = ScopeWatch::open(Scope::DesktopWide, Arc::new(|_| {})).unwrap();
        assert!(observer.snapshot().is_ok());
        {
            let (mut desktop, phase, requests, _) = fixture();
            assert!(!desktop.notification_driven());
            desktop.poll(&[], &phase).unwrap();
            assert!(desktop.register_scope_watch().unwrap_err().contains("verified desktop scope"));
            desktop.bind_scope_before_start(unrelated_application()).unwrap();
            let first = desktop.focus_watch.snapshot().unwrap();
            assert!(first.watch_id > 0);
            assert!(desktop.notification_driven());
            assert_eq!(desktop.register_scope_watch().unwrap().watch_id, first.watch_id,
                "Repeated setup must reuse the native lease");
            let old_lease = desktop.focus_watch.native.take().unwrap();
            desktop.bind_scope_before_start(unrelated_application()).unwrap();
            let replacement = desktop.focus_watch.snapshot().unwrap();
            assert_ne!(replacement.watch_id, first.watch_id);
            assert!(!replacement.eligible);
            assert_eq!(desktop.status()["scopeNotifications"]["watchId"], replacement.watch_id);
            assert_eq!(desktop.status()["scopeNotifications"]["nativeHealthy"], true);
            desktop.start(&phase, None).unwrap();
            desktop.poll(&[], &phase).unwrap();
            assert_eq!(desktop.status()["state"], "paused");
            assert!(requests.lock().unwrap().iter().all(|(_, ack)| ack.ok));
            desktop.stop().unwrap();
            assert!(!desktop.notification_driven());
            assert_eq!(desktop.status()["scopeNotifications"]["watchId"], Value::Null);
            desktop.poll(&[], &phase).unwrap();
            drop(old_lease);
        }
        {
            let (mut desktop, phase, _, _) = fixture();
            desktop.scope = Some(unrelated_application());
            desktop.start_waiting_for_neutral().unwrap();
            assert!(desktop.notification_driven());
            desktop.poll(&[], &phase).unwrap();
            assert!(!desktop.focused);
            // Removing this consumer's native lease models a terminal observer
            // becoming unavailable without touching the shared native producer.
            drop(desktop.focus_watch.native.take());
            assert_eq!(desktop.status()["scopeNotifications"]["nativeHealthy"], false);
            assert!(desktop.poll(&[], &phase).unwrap_err().contains("observer is unavailable"));
            assert!(!desktop.notification_driven());
            assert_eq!(desktop.status()["state"], "faulted");
            assert!(desktop.status()["error"].as_str().unwrap().contains("observer is unavailable"));
            desktop.stop().unwrap();
        }
        {
            let (mut desktop, mut phase, requests, clock) = fixture();
            desktop.start(&phase, Some(unrelated_application())).unwrap();
            assert!(!desktop.focused);
            // A paused application remains registered across its original
            // 250 ms lease. Each renewal requires a fresh parent admission.
            for (now, expected) in [(100_010, 1), (300_010, 2)] {
                clock.store(now, Ordering::Release);
                phase.advance(Counter(now)).unwrap();
                let began = Instant::now();
                while renewals(&requests) < expected {
                    desktop.poll(&[], &phase).unwrap();
                    assert!(began.elapsed() < Duration::from_secs(3), "Paused renewal was not authorized");
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert_eq!(desktop.authorization_deadline, Some(Counter(now + 100_000)));
                assert_eq!(desktop.next_deadline_us(), desktop.authorization_deadline);
            }
            assert!(requests.lock().unwrap().iter().all(|(_, ack)| ack.ok));
            desktop.stop().unwrap();
        }
    }
}
