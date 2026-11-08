use super::*;
use crate::game_discovery::ProcessIdentity;
use std::sync::mpsc;

struct PlatformFixture {
    enabled: AtomicBool,
    blocked: Mutex<BTreeMap<u32, (mpsc::Sender<()>, mpsc::Receiver<()>)>>,
}

fn game(id: &str, integration: &str) -> Value {
    json!({
        "id": id, "integrationKey": integration, "integrationCompatible": true,
        "provider": "x-plane", "installed": true, "processMonitoringSupported": true,
        "executable": format!("/fixture/{id}.exe"),
    })
}

fn process(game: &Value) -> ProcessIdentity {
    ProcessIdentity {
        pid: if game["id"] == "x-plane-11" { 41 } else { 42 },
        path: game["executable"].as_str().unwrap().into(),
        epoch: 12,
    }
}

impl Platform for PlatformFixture {
    fn integration_setup_supported(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        vec![
            game("x-plane-11", "x-plane-11"),
            game("x-plane-12", "x-plane-12"),
        ]
    }

    fn validate(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }

    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> {
        Err("Context qualification cannot launch a game".into())
    }

    fn find(&self, _: &Value) -> Result<Option<ProcessIdentity>, String> {
        Ok(None)
    }

    fn alive(&self, process: &ProcessIdentity) -> Result<bool, String> {
        let blocked = self.blocked.lock().unwrap().remove(&process.pid);
        if let Some((entered, released)) = blocked {
            entered.send(()).unwrap();
            let _ = released.recv_timeout(Duration::from_secs(8));
        }
        Ok(true)
    }
}

fn wait_verified(inner: &Inner, id: &str) -> bool {
    let wake = &inner.changes.wake;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let observed = wake.observed();
        if inner.contexts.lock().unwrap().view(id)["processVerified"] == true {
            return true;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        wake.wait(observed, remaining);
    }
}

#[test]
fn blocked_process_qualification_does_not_hold_up_another_integration() {
    verify_isolation(false);
    verify_isolation(true);
}

fn verify_isolation(fallback_replacement: bool) {
    let directory = crate::test_directory();
    let (entered, entered_rx) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let (replacement_entered, replacement_entered_rx) = mpsc::channel();
    let (release_replacement, release_replacement_rx) = mpsc::channel();
    let platform = Arc::new(PlatformFixture {
        enabled: AtomicBool::new(false),
        blocked: Mutex::new(BTreeMap::from([
            (41, (entered, release_rx)),
            (43, (replacement_entered, release_replacement_rx)),
        ])),
    });
    let runtime = GamesRuntime::test_runtime(
        Arc::new(|_| panic!("Context qualification cannot submit output")),
        Paths::in_directory(directory.path()),
        platform.clone(),
    );
    platform.enabled.store(true, Ordering::Release);
    let inner = &runtime.inner;
    inner.changes.wake.bind_thread(thread::current()).unwrap();
    let workers = Arc::new(Workers::default());
    let threads = workers.start(inner);
    let submitted = inner.collect_game_contexts(
        SetupRefresh::Cached,
        |games| games.iter().map(|game| Ok(Some(process(game)))).collect(),
        Some((&workers, inner.context_epochs())),
    );
    let entered = entered_rx.recv_timeout(Duration::from_secs(2)).is_ok();
    let independent = entered && wait_verified(inner, "x-plane-12");

    let mut pending = Vec::new();
    let epoch = inner.contexts.lock().unwrap().begin_observation().unwrap();
    if fallback_replacement {
        let game = game("x-plane-11", "x-plane-11");
        let replacement = ProcessIdentity {
            pid: 43,
            ..process(&game)
        };
        pending.push(Work {
            process: Ok(Some(replacement)),
            game,
            epoch,
            supported: true,
            eligible: Ok(true),
        });
    } else {
        inner.changes.context_process_wake.notify();
    }
    // The sentinel confirms completion of prior work on the blocked lane.
    let sentinel = game("fixture-sentinel", "x-plane-11");
    pending.push(Work {
        process: Ok(Some(process(&sentinel))),
        game: sentinel,
        epoch,
        supported: true,
        eligible: Ok(true),
    });
    workers.submit(
        Scope {
            process: inner.context_epochs(),
            setup: inner.changes.vehicle_scope_wake.observed(),
            watched: false,
        },
        pending,
    );
    let _ = release.send(());

    let stale = if fallback_replacement {
        let replacement_started = replacement_entered_rx
            .recv_timeout(Duration::from_secs(2))
            .is_ok();
        let prior_published =
            inner.contexts.lock().unwrap().view("x-plane-11")["processVerified"] == true;
        let _ = release_replacement.send(());
        !replacement_started || prior_published
    } else {
        false
    };
    let finished = wait_verified(inner, "fixture-sentinel");
    let stale = stale
        || (!fallback_replacement
            && inner.contexts.lock().unwrap().view("x-plane-11")["processVerified"] == true);

    inner.closed.store(true, Ordering::Release);
    workers.stop();
    for worker in threads {
        worker.join().unwrap();
    }
    runtime.shutdown();
    assert!(
        submitted && entered,
        "The shared inventory must reach the blocked process check"
    );
    assert!(
        independent,
        "A second integration must publish before the first check finishes"
    );
    assert!(
        finished && !stale,
        "A superseded check cannot publish after returning"
    );
}
