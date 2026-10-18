use super::*;
use crate::visibility::policy::Plan;
#[test]
fn active_handoff_uses_the_retained_exclusive_backend_without_another_open_or_write() {
    use crate::visibility::{journal::Backend, runtime::Adapter};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    struct Exclusive {
        state: Arc<Mutex<State>>,
        opened: Arc<AtomicBool>,
        writes: Arc<AtomicUsize>,
    }
    impl Drop for Exclusive {
        fn drop(&mut self) {
            self.opened.store(false, Ordering::Release);
        }
    }
    impl Backend for Exclusive {
        fn read(&mut self) -> Result<State, String> {
            Fixture(self.state.clone()).read()
        }
        fn change(&mut self, before: &State, after: &State) -> Result<(), String> {
            self.writes.fetch_add(1, Ordering::AcqRel);
            Fixture(self.state.clone()).change(before, after)
        }
    }
    impl Adapter for Exclusive {
        fn executable(&mut self, path: &str) -> Result<String, String> {
            if path == r"C:\Incoming.exe" {
                return Ok(r"\Device\Volume\Incoming.exe".into());
            }
            Fixture(self.state.clone()).executable(path)
        }
        fn instance(&mut self, path: &str) -> Result<String, String> {
            Fixture(self.state.clone()).instance(path)
        }
    }
    let (mut service, _temp, driver, registry, id) = setup();
    let opened = Arc::new(AtomicBool::new(false));
    let opens = Arc::new(AtomicUsize::new(0));
    let writes = Arc::new(AtomicUsize::new(0));
    let (owned, calls, mutations, state) = (
        opened.clone(),
        opens.clone(),
        writes.clone(),
        driver.clone(),
    );
    service.factory = Arc::new(move || {
        calls.fetch_add(1, Ordering::AcqRel);
        if owned.swap(true, Ordering::AcqRel) {
            return Err("Exclusive visibility control endpoint already open".into());
        }
        Ok(Box::new(Exclusive {
            state: state.clone(),
            opened: owned.clone(),
            writes: mutations.clone(),
        }))
    });
    let original = driver.lock().unwrap().clone();
    let intent = captured(&service, &id);
    let owner = uuid::Uuid::new_v4().to_string();
    let active =
        json!({"owner":owner,"prepared":service.game_request("preflight",&intent).unwrap()});
    service.game_request("start", &active).unwrap();
    wait(&service, &active, &registry, &id, true);
    let (before_opens, before_writes) = (
        opens.load(Ordering::Acquire),
        writes.load(Ordering::Acquire),
    );
    let applied = driver.lock().unwrap().clone();
    let next = service.game_request("handoff_preflight", &json!({"owner":owner,"intent":intent}));
    let mut changed = intent.clone();
    changed["policies"][&id] = json!({"mode":"hide-from-process","executable":r"C:\Incoming.exe"});
    let different = service.game_request(
        "handoff_preflight",
        &json!({"owner":owner,"intent":changed}),
    );
    let (after_opens, after_writes) = (
        opens.load(Ordering::Acquire),
        writes.load(Ordering::Acquire),
    );
    assert_eq!(*driver.lock().unwrap(), applied);
    service.game_request("stop", &active).unwrap();
    wait(&service, &active, &registry, &id, false);
    service.game_request("stop", &active).unwrap();
    assert_eq!(*driver.lock().unwrap(), original);
    let next = next.expect("Active handoff must read the same retained exclusive backend");
    assert_eq!(next["policy"]["before"], json!(original));
    let different = different.unwrap();
    assert_eq!(different["policy"]["before"], json!(original));
    assert_eq!(
        different["policy"]["selections"][0]["policy"]["executable"],
        r"\Device\Volume\Incoming.exe"
    );
    assert_eq!(
        after_opens, before_opens,
        "Handoff must not open the exclusive endpoint again"
    );
    assert_eq!(
        after_writes, before_writes,
        "Handoff preparation must not mutate policy"
    );
    assert!(!opened.load(Ordering::Acquire));
}
#[test]
fn active_native_profile_preflights_against_original_then_stops_both_owners_to_original() {
    let (s, temp, driver, registry, id) = setup();
    let original = driver.lock().unwrap().clone();
    let intent = captured(&s, &id);
    let prepared = s.game_request("preflight", &intent).unwrap();
    let old_owner = uuid::Uuid::new_v4().to_string();
    let a = json!({"owner":old_owner,"prepared":prepared});
    s.game_request("start", &a).unwrap();
    wait(&s, &a, &registry, &id, true);
    let active = driver.lock().unwrap().clone();
    assert_ne!(active, original);
    assert!(s.game_request("preflight", &intent).is_err());
    assert!(s
        .game_request(
            "handoff_preflight",
            &json!({"owner":uuid::Uuid::new_v4().to_string(),"intent":intent})
        )
        .is_err());
    let next = s
        .game_request(
            "handoff_preflight",
            &json!({"owner":old_owner,"intent":intent}),
        )
        .unwrap();
    assert_eq!(next["policy"]["before"], json!(original));
    assert_eq!(*driver.lock().unwrap(), active);
    assert_eq!(
        s.game_request("handoff_validate", &json!({"proof":next["handoff"]}))
            .unwrap(),
        json!({"valid":true})
    );
    let b = json!({"owner":uuid::Uuid::new_v4().to_string(),"prepared":next});
    assert!(s.game_request("start", &b).is_err());
    s.game_request("stop", &a).unwrap();
    wait(&s, &a, &registry, &id, false);
    s.game_request("stop", &a).unwrap();
    assert_eq!(*driver.lock().unwrap(), original);
    assert!(s
        .game_request("handoff_validate", &json!({"proof":next["handoff"]}))
        .is_err());
    s.game_request("start", &b).unwrap();
    wait(&s, &b, &registry, &id, true);
    let path = temp.path().join(format!(
        "game-visibility-{}.json",
        b["owner"].as_str().unwrap()
    ));
    assert_eq!(
        journal::inspect(&path).unwrap().unwrap().plan.before,
        original
    );
    s.game_request("stop", &b).unwrap();
    wait(&s, &b, &registry, &id, false);
    s.game_request("stop", &b).unwrap();
    assert_eq!(*driver.lock().unwrap(), original);
}
#[test]
fn external_change_after_capture_and_unqualified_foreign_scope_never_transfer() {
    let (mut s, _temp, driver, registry, id) = setup();
    let intent = captured(&s, &id);
    let old_owner = uuid::Uuid::new_v4().to_string();
    let a = json!({"owner":old_owner,"prepared":s.game_request("preflight",&intent).unwrap()});
    s.game_request("start", &a).unwrap();
    wait(&s, &a, &registry, &id, true);
    let next = s
        .game_request(
            "handoff_preflight",
            &json!({"owner":old_owner,"intent":intent}),
        )
        .unwrap();
    s.qualified = false;
    assert!(s
        .game_request(
            "handoff_preflight",
            &json!({"owner":old_owner,"intent":intent})
        )
        .is_err());
    assert!(s
        .game_request("handoff_validate", &json!({"proof":next["handoff"]}))
        .is_err());
    s.qualified = true;
    driver
        .lock()
        .unwrap()
        .applications
        .push(r"\Device\Volume\External.exe".into());
    let external = driver.lock().unwrap().clone();
    // Cached metadata may still be healthy until the worker observes the edit.
    // It never asserts that this call obtained a fresh driver sample.
    let _ = s.game_request("handoff_validate", &json!({"proof":next["handoff"]}));
    assert_eq!(*driver.lock().unwrap(), external);
    let _ = s.game_request("stop", &a);
    let b = json!({"owner":uuid::Uuid::new_v4().to_string(),"prepared":next});
    assert!(s.game_request("start", &b).is_err());
    assert_eq!(*driver.lock().unwrap(), external);
}
#[test]
fn restart_cannot_reuse_handoff_proof_and_explicit_recovery_only_restores() {
    let (s, temp, driver, registry, id) = setup();
    let intent = captured(&s, &id);
    let captured = s.game_request("preflight", &intent).unwrap();
    let original = driver.lock().unwrap().clone();
    let old_owner = uuid::Uuid::new_v4().to_string();
    let policy: Plan = serde_json::from_value(captured["policy"].clone()).unwrap();
    let path = temp
        .path()
        .join(format!("game-visibility-{old_owner}.json"));
    let mut store = journal::Store::open(path).unwrap();
    journal::apply(&mut Fixture(driver.clone()), &mut store, policy, || Ok(())).unwrap();
    drop(store);
    assert!(s
        .game_request(
            "handoff_preflight",
            &json!({"owner":old_owner,"intent":intent})
        )
        .is_err());
    assert!(s.game_request("preflight", &intent).is_err());
    let recover = json!({"owner":old_owner});
    s.game_request("stop", &recover).unwrap();
    wait(&s, &recover, &registry, &id, false);
    s.game_request("stop", &recover).unwrap();
    assert_eq!(*driver.lock().unwrap(), original);
    assert!(
        journal::inspect(
            &temp
                .path()
                .join(format!("game-visibility-{old_owner}.json"))
        )
        .unwrap()
        .unwrap()
        .clean
    );
}

#[test]
fn slow_readonly_proof_cannot_block_stop_or_admit_an_old_owner_after_replacement() {
    use crate::visibility::{journal::Backend, runtime::Adapter};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Condvar,
    };
    struct Release(Arc<(Mutex<bool>, Condvar)>);
    impl Drop for Release {
        fn drop(&mut self) {
            let (lock, wake) = &*self.0;
            *lock.lock().unwrap() = true;
            wake.notify_all();
        }
    }
    struct Slow {
        driver: Arc<Mutex<State>>,
        entered: std::sync::mpsc::Sender<()>,
        released: Arc<(Mutex<bool>, Condvar)>,
        once: Arc<AtomicBool>,
    }
    impl Backend for Slow {
        fn read(&mut self) -> Result<State, String> {
            Ok(self.driver.lock().unwrap().clone())
        }
        fn change(&mut self, before: &State, after: &State) -> Result<(), String> {
            Fixture(self.driver.clone()).change(before, after)
        }
    }
    impl Adapter for Slow {
        fn executable(&mut self, path: &str) -> Result<String, String> {
            // Only this incoming preparation resolves this executable. Normal
            // active health/Stop never shares the artificial blocked read.
            if path == r"C:\Incoming.exe" && !self.once.swap(true, Ordering::AcqRel) {
                self.entered.send(()).unwrap();
                let (lock, wake) = &*self.released;
                let mut released = lock.lock().unwrap();
                while !*released {
                    released = wake.wait(released).unwrap();
                }
                return Ok(r"\Device\Volume\Incoming.exe".into());
            }
            Fixture(self.driver.clone()).executable(path)
        }
        fn instance(&mut self, path: &str) -> Result<String, String> {
            Fixture(self.driver.clone()).instance(path)
        }
    }
    let (mut s, _temp, driver, registry, id) = setup();
    let original = driver.lock().unwrap().clone();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let released = Arc::new((Mutex::new(false), Condvar::new()));
    let release = Release(released.clone());
    let once = Arc::new(AtomicBool::new(false));
    let shared = driver.clone();
    s.factory = Arc::new(move || {
        Ok(Box::new(Slow {
            driver: shared.clone(),
            entered: entered_tx.clone(),
            released: released.clone(),
            once: once.clone(),
        }))
    });
    let intent = captured(&s, &id);
    let owner = uuid::Uuid::new_v4().to_string();
    let a = json!({"owner":owner,"prepared":s.game_request("preflight",&intent).unwrap()});
    s.game_request("start", &a).unwrap();
    wait(&s, &a, &registry, &id, true);
    let earlier = s
        .game_request("handoff_preflight", &json!({"owner":owner,"intent":intent}))
        .unwrap();
    let s = Arc::new(s);
    let pending = s.clone();
    let mut captured_intent = intent.clone();
    captured_intent["policies"][&id] =
        json!({"mode":"hide-from-process","executable":r"C:\Incoming.exe"});
    let thread = std::thread::Builder::new()
        .name("slow-handoff-proof".into())
        .spawn(move || {
            pending.game_request(
                "handoff_preflight",
                &json!({"owner":owner,"intent":captured_intent}),
            )
        })
        .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let (poll_tx, poll_rx) = std::sync::mpsc::channel();
    let polling = s.clone();
    let selected = a.clone();
    let poll = std::thread::spawn(move || {
        poll_tx
            .send(polling.game_request("poll", &selected))
            .unwrap();
    });
    assert!(poll_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .is_ok());
    poll.join().unwrap();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel();
    let stopping = s.clone();
    let selected = a.clone();
    let stop = std::thread::spawn(move || {
        stop_tx
            .send(stopping.game_request("stop", &selected))
            .unwrap();
    });
    assert!(stop_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .is_ok());
    stop.join().unwrap();
    // The status is intentionally still active until the blocked retained I/O
    // returns, but cancellation already invalidates final metadata admission.
    assert!(s
        .game_request("handoff_validate", &json!({"proof":earlier["handoff"]}))
        .is_err());
    drop(release);
    assert!(thread.join().unwrap().unwrap_err().contains("cancelled"));
    wait(&s, &a, &registry, &id, false);
    s.game_request("stop", &a).unwrap();
    let b = json!({"owner":uuid::Uuid::new_v4().to_string(),"prepared":s.game_request("preflight",&intent).unwrap()});
    s.game_request("start", &b).unwrap();
    wait(&s, &b, &registry, &id, true);
    s.game_request("stop", &b).unwrap();
    wait(&s, &b, &registry, &id, false);
    s.game_request("stop", &b).unwrap();
    assert_eq!(*driver.lock().unwrap(), original);
}

#[test]
fn final_metadata_admission_never_opens_a_driver_even_when_driver_io_is_unavailable() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (mut s, _temp, _driver, registry, id) = setup();
    let intent = captured(&s, &id);
    let owner = uuid::Uuid::new_v4().to_string();
    let a = json!({"owner":owner,"prepared":s.game_request("preflight",&intent).unwrap()});
    s.game_request("start", &a).unwrap();
    wait(&s, &a, &registry, &id, true);
    let next = s
        .game_request("handoff_preflight", &json!({"owner":owner,"intent":intent}))
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    s.factory = Arc::new(move || {
        observed.fetch_add(1, Ordering::AcqRel);
        Err(
            "This driver connection could block and must not be opened under final admission"
                .into(),
        )
    });
    assert_eq!(
        s.game_request("handoff_validate", &json!({"proof":next["handoff"]}))
            .unwrap(),
        json!({"valid":true})
    );
    assert_eq!(calls.load(Ordering::Acquire), 0);
    let mut changed = next["handoff"].clone();
    changed["jobId"] = uuid::Uuid::new_v4().to_string().into();
    assert!(s
        .game_request("handoff_validate", &json!({"proof":changed}))
        .is_err());
    assert_eq!(calls.load(Ordering::Acquire), 0);
    s.game_request("stop", &a).unwrap();
    wait(&s, &a, &registry, &id, false);
    s.game_request("stop", &a).unwrap();
}
