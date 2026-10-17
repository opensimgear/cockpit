use super::*;
use crate::visibility::policy::Plan;
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
            if std::thread::current().name() == Some("slow-handoff-proof")
                && !self.once.swap(true, Ordering::AcqRel)
            {
                self.entered.send(()).unwrap();
                let (lock, wake) = &*self.released;
                let mut released = lock.lock().unwrap();
                while !*released {
                    released = wake.wait(released).unwrap();
                }
            }
            Ok(self.driver.lock().unwrap().clone())
        }
        fn change(&mut self, before: &State, after: &State) -> Result<(), String> {
            Fixture(self.driver.clone()).change(before, after)
        }
    }
    impl Adapter for Slow {
        fn executable(&mut self, path: &str) -> Result<String, String> {
            Fixture(self.driver.clone()).executable(path)
        }
        fn instance(&mut self, path: &str) -> Result<String, String> {
            Fixture(self.driver.clone()).instance(path)
        }
    }
    let (mut s, _temp, driver, registry, id) = setup();
    let original = driver.lock().unwrap().clone();
    let intent = captured(&s, &id);
    let owner = uuid::Uuid::new_v4().to_string();
    let a = json!({"owner":owner,"prepared":s.game_request("preflight",&intent).unwrap()});
    s.game_request("start", &a).unwrap();
    wait(&s, &a, &registry, &id, true);
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
    let s = Arc::new(s);
    let pending = s.clone();
    let captured_intent = intent.clone();
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
    wait(&s, &a, &registry, &id, false);
    s.game_request("stop", &a).unwrap();
    let b = json!({"owner":uuid::Uuid::new_v4().to_string(),"prepared":s.game_request("preflight",&intent).unwrap()});
    s.game_request("start", &b).unwrap();
    wait(&s, &b, &registry, &id, true);
    drop(release);
    assert!(
        thread.join().unwrap().is_err(),
        "A new worker with the same driver after-state cannot validate an outgoing proof"
    );
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
