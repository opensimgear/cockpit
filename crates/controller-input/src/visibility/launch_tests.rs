use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
fn request() -> Request {
    Request::parse(&json!({"launchKind":"direct-child","executable":"/app/game","directory":"/app","arguments":[]})).unwrap()
}
fn sources() -> Vec<Source> {
    vec![Source {
        id: uuid::Uuid::new_v4().to_string(),
        session: uuid::Uuid::new_v4().to_string(),
        generation: 1,
        sequence: 1,
        interface: "usb:phys".into(),
    }]
}
fn observe(sources: Vec<Source>) -> Observe {
    Arc::new(move |_| Ok(sources.clone()))
}
fn wait(worker: &Worker) {
    let until = std::time::Instant::now() + Duration::from_secs(3);
    while worker.active() && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!worker.active(), "{}", worker.snapshot());
}
struct Mock {
    polls: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
    mode: u8,
}
impl Child for Mock {
    fn poll(&mut self) -> Result<Poll, String> {
        let n = self.polls.fetch_add(1, Ordering::SeqCst);
        match self.mode {
            1 => Err("device inventory changed".into()),
            2 => Ok(Poll::Exited(9)),
            3 => panic!("worker panic"),
            4 => Ok(Poll::Running),
            _ => Ok(if n == 0 {
                Poll::Starting
            } else if n == 1 {
                Poll::Running
            } else {
                Poll::Exited(0)
            }),
        }
    }
    fn stop(&mut self) -> Result<(), String> {
        self.stops.fetch_add(1, Ordering::SeqCst);
        if self.mode == 5 {
            Err("termination unconfirmed".into())
        } else {
            Ok(())
        }
    }
}
#[test]
fn strict_requests_and_production_gate_cannot_be_overridden() {
    let mut v=serde_json::to_value(json!({"launchKind":"direct-child","executable":"/app/game","directory":"/app","arguments":[]})).unwrap();
    for key in [
        "pid",
        "alreadyRunning",
        "qualified",
        "helper",
        "environment",
    ] {
        v[key] = json!(true);
        assert!(Request::parse(&v).is_err());
        v.as_object_mut().unwrap().remove(key);
    }
    for path in ["game", "/app/../game", "/app/./game", "/app/\0"] {
        v["executable"] = json!(path);
        assert!(Request::parse(&v).is_err());
    }
    v["executable"] = json!("/app/game");
    v["arguments"] = json!(vec!["x"; 65]);
    assert!(Request::parse(&v).is_err());
    v["arguments"] = json!(["\0"]);
    assert!(Request::parse(&v).is_err());
    v["arguments"] = json!(["x".repeat(4097)]);
    assert!(Request::parse(&v).is_err());
    v["arguments"] = json!(vec!["x".repeat(4096); 5]);
    assert!(Request::parse(&v).is_err());
    v["arguments"] = json!([]);
    v["launchKind"] = json!("steam");
    assert!(Request::parse(&v).unwrap_err().contains("Unsupported"));
    let mut worker = Worker::default();
    worker.factory = Arc::new(|_, _| panic!("closed gate reached factory"));
    let s = sources();
    assert!(worker
        .start(request(), s.clone(), observe(s.clone()))
        .unwrap_err()
        .contains("disabled"));
    assert!(native(request(), s).is_err());
    assert_eq!(worker.snapshot()["qualified"], false);
    assert!(!worker.active());
    assert!(worker.stop(Some("stale")).is_err());
    worker.stop(None).unwrap();
}
#[test]
fn worker_owns_child_completion_failure_stop_and_source_loss() {
    for mode in [0, 1, 2, 4] {
        let mut worker = Worker::default();
        let polls = Arc::new(AtomicUsize::new(0));
        let stops = Arc::new(AtomicUsize::new(0));
        let p = polls.clone();
        let s = stops.clone();
        worker.factory = Arc::new(move |_, _| {
            Ok(Box::new(Mock {
                polls: p.clone(),
                stops: s.clone(),
                mode,
            }))
        });
        let sources = sources();
        let observing = observe(sources.clone());
        worker
            .start_owned(request(), sources.clone(), observing.clone())
            .unwrap();
        assert!(worker
            .start_owned(request(), sources.clone(), observing)
            .is_err());
        assert!(worker.stop(Some("stale")).is_err());
        if mode == 4 {
            while polls.load(Ordering::SeqCst) == 0 {
                std::thread::sleep(Duration::from_millis(2));
            }
            let id = worker.snapshot()["job"]["jobId"]
                .as_str()
                .unwrap()
                .to_owned();
            worker.stop(Some(&id)).unwrap();
        }
        wait(&worker);
        assert_eq!(stops.load(Ordering::SeqCst), 1);
        worker.shutdown();
        assert_eq!(
            worker.snapshot()["job"]["phase"],
            if mode == 1 || mode == 2 {
                "attention"
            } else {
                "stopped"
            }
        );
    }
    let mut worker = Worker::default();
    let sources = sources();
    let counter = Arc::new(AtomicUsize::new(0));
    let s = sources.clone();
    let obs: Observe = Arc::new(move |_| {
        if counter.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(s.clone())
        } else {
            Err("capture stopped".into())
        }
    });
    let stopped = Arc::new(AtomicUsize::new(0));
    let signal = stopped.clone();
    worker.factory = Arc::new(move |_, _| {
        Ok(Box::new(Mock {
            polls: Default::default(),
            stops: signal.clone(),
            mode: 4,
        }))
    });
    worker.start_owned(request(), sources, obs).unwrap();
    wait(&worker);
    assert_eq!(stopped.load(Ordering::SeqCst), 1);
    assert!(worker.snapshot()["job"]["message"]
        .as_str()
        .unwrap()
        .contains("capture stopped"));
}
#[test]
fn start_refuses_stale_sources_and_reports_preflight_failure() {
    let mut worker = Worker::default();
    let s = sources();
    assert!(worker
        .start_owned(request(), vec![], observe(s.clone()))
        .is_err());
    assert!(worker
        .start_owned(
            request(),
            s.clone(),
            Arc::new(|_| Err("capture unavailable".into()))
        )
        .is_err());
    worker.factory = Arc::new(|_, _| Err("missing sandbox permission".into()));
    worker
        .start_owned(request(), s.clone(), observe(s.clone()))
        .unwrap();
    wait(&worker);
    assert_eq!(
        worker.snapshot()["job"]["message"],
        "missing sandbox permission"
    );
    worker
        .start_owned(request(), s.clone(), observe(s))
        .unwrap();
    wait(&worker);
}
#[test]
fn uncertain_cleanup_and_panics_keep_ownership_blocked() {
    for mode in [3, 5] {
        let mut worker = Worker::default();
        let stops = Arc::new(AtomicUsize::new(0));
        worker.factory = Arc::new(move |_, _| {
            Ok(Box::new(Mock {
                polls: Default::default(),
                stops: stops.clone(),
                mode,
            }))
        });
        let s = sources();
        worker
            .start_owned(request(), s.clone(), observe(s.clone()))
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while worker.snapshot()["job"]["phase"] != "attention" {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(worker.active());
        assert!(worker
            .start_owned(request(), s.clone(), observe(s))
            .is_err());
        worker.stop(None).unwrap();
        worker.shutdown();
        assert!(worker.active());
    }
}
