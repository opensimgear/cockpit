use super::*;
use crate::registry::{Control, Identity};
use std::{collections::VecDeque, sync::atomic::AtomicUsize};
fn definition(path: &str) -> Definition {
    let mut button = Control::new(9, 1, 0, 0, 1);
    button.bits = 8;
    Definition {
        identity: Identity {
            backend: "fixture".into(),
            vendor_id: 1,
            product_id: 2,
            serial: None,
            collection: "0".into(),
            fingerprint: String::new(),
            path: path.into(),
        },
        name: "Fixture".into(),
        controls: vec![button, Control::new(9, 2, 0, 0, 2)],
        diagnostics: vec![],
    }
}
#[derive(Default)]
struct State {
    paths: Vec<String>,
    events: VecDeque<Result<Vec<Event>, String>>,
    baseline: VecDeque<Result<Vec<Event>, String>>,
    fail_open: bool,
    fail_scan: bool,
}
struct Fake {
    state: Arc<Mutex<State>>,
    drops: Arc<AtomicUsize>,
    path: String,
}
impl Drop for Fake {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
    }
}
impl Input for Fake {
    fn definition(&self) -> Definition {
        definition(&self.path)
    }
    fn baseline(&mut self) -> Result<Vec<Event>, String> {
        self.state
            .lock()
            .unwrap()
            .baseline
            .pop_front()
            .unwrap_or(Ok(vec![Event::Values(0, BTreeMap::from([(1, 0), (2, 1)]))]))
    }
    fn poll(&mut self) -> Result<Vec<Event>, String> {
        self.state
            .lock()
            .unwrap()
            .events
            .pop_front()
            .unwrap_or(Ok(vec![]))
    }
}
struct Backend {
    state: Arc<Mutex<State>>,
    drops: Arc<AtomicUsize>,
}
impl Platform for Backend {
    fn paths(&mut self) -> Result<Vec<String>, String> {
        let s = self.state.lock().unwrap();
        if s.fail_scan {
            Err("scan failure".into())
        } else {
            Ok(s.paths.clone())
        }
    }
    fn open(&mut self, path: &str) -> Result<Box<dyn Input>, String> {
        if self.state.lock().unwrap().fail_open {
            return Err(permission_error("linux", "Permission denied"));
        }
        Ok(Box::new(Fake {
            state: self.state.clone(),
            drops: self.drops.clone(),
            path: path.into(),
        }))
    }
}
fn setup() -> (
    Capture<Backend>,
    Arc<Mutex<State>>,
    Shared,
    Arc<AtomicUsize>,
) {
    let state = Arc::new(Mutex::new(State {
        paths: vec!["a".into()],
        ..Default::default()
    }));
    let drops = Arc::new(AtomicUsize::new(0));
    let registry = Arc::new(Mutex::new(Registry::default()));
    (
        Capture::new(
            Backend {
                state: state.clone(),
                drops: drops.clone(),
            },
            registry.clone(),
        ),
        state,
        registry,
        drops,
    )
}
#[test]
fn lifecycle_preserves_held_buttons_and_changes_generation_on_gap() {
    let (mut c, state, r, drops) = setup();
    c.scan().unwrap();
    c.scan().unwrap();
    let id = r.lock().unwrap().id_for_path("a").unwrap();
    assert_eq!(
        r.lock().unwrap().inspect(&id, 0).unwrap()["source"]["baselineReady"],
        false
    );
    c.tick();
    assert_eq!(
        r.lock().unwrap().inspect(&id, 0).unwrap()["source"]["baselineReady"],
        true
    );
    state
        .lock()
        .unwrap()
        .events
        .push_back(Ok(vec![Event::Values(0, BTreeMap::from([(1, 1)]))]));
    c.tick();
    let detail = r.lock().unwrap().inspect(&id, 0).unwrap();
    assert_eq!(detail["controls"][1]["value"]["normalized"], 1);
    state.lock().unwrap().events.push_back(Ok(vec![
        Event::Gap("lost".into()),
        Event::Values(0, BTreeMap::from([(1, 1)])),
    ]));
    c.tick();
    let detail = r.lock().unwrap().inspect(&id, 0).unwrap();
    assert_eq!(detail["source"]["generation"], "2");
    assert_eq!(detail["source"]["baselineReady"], false);
    state
        .lock()
        .unwrap()
        .baseline
        .push_back(Ok(vec![Event::PendingBaseline]));
    c.tick();
    assert_eq!(
        r.lock().unwrap().inspect(&id, 0).unwrap()["source"]["baselineReady"],
        false
    );
    c.tick();
    assert_eq!(
        r.lock().unwrap().inspect(&id, 0).unwrap()["source"]["baselineReady"],
        true
    );
    state.lock().unwrap().paths.clear();
    c.scan().unwrap();
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(
        r.lock().unwrap().inspect(&id, 0).unwrap()["source"]["online"],
        false
    );
    state.lock().unwrap().paths.push("a".into());
    c.scan().unwrap();
    c.tick();
    assert_eq!(
        r.lock().unwrap().inspect(&id, 0).unwrap()["source"]["generation"],
        "3"
    );
    drop(c);
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    assert_eq!(
        r.lock().unwrap().inspect(&id, 0).unwrap()["source"]["online"],
        false
    );
}
#[test]
fn failure_isolated_retry_and_overflow() {
    let (mut c, state, r, _) = setup();
    state.lock().unwrap().fail_open = true;
    c.scan().unwrap();
    assert!(r.lock().unwrap().diagnostic.contains("Permission denied"));
    state.lock().unwrap().fail_open = false;
    c.scan().unwrap();
    c.tick();
    let id = r.lock().unwrap().id_for_path("a").unwrap();
    state
        .lock()
        .unwrap()
        .events
        .push_back(Ok(vec![Event::Values(0, BTreeMap::new()); MAX_EVENTS + 1]));
    c.tick();
    assert_eq!(
        r.lock().unwrap().inspect(&id, 0).unwrap()["source"]["baselineReady"],
        false
    );
    c.tick();
    state
        .lock()
        .unwrap()
        .events
        .push_back(Ok(vec![Event::Values(0, BTreeMap::from([(1, 2)]))]));
    c.tick();
    assert_eq!(
        r.lock().unwrap().inspect(&id, 0).unwrap()["source"]["baselineReady"],
        false
    );
    c.tick();
    state
        .lock()
        .unwrap()
        .events
        .push_back(Err("device removed".into()));
    c.tick();
    assert_eq!(
        r.lock().unwrap().inspect(&id, 0).unwrap()["source"]["online"],
        false
    );
    c.scan().unwrap();
    state
        .lock()
        .unwrap()
        .baseline
        .push_back(Err("permission revoked".into()));
    c.tick();
    assert_eq!(
        r.lock().unwrap().inspect(&id, 0).unwrap()["source"]["online"],
        false
    );
    state.lock().unwrap().fail_scan = true;
    assert!(c.scan().is_err());
    state.lock().unwrap().fail_scan = false;
    state.lock().unwrap().paths = vec!["a".into(); 1025];
    assert!(c.scan().is_err());
}
#[test]
fn reassignment_and_inventory_limits() {
    let (mut c, state, r, _) = setup();
    c.scan().unwrap();
    c.tick();
    let id = r.lock().unwrap().id_for_path("a").unwrap();
    r.lock().unwrap().disconnect(&id);
    c.tick();
    assert!(c.devices.is_empty());
    state.lock().unwrap().paths = vec!["b".into(), "c".into()];
    c.scan().unwrap();
    c.tick();
    let observed = r.lock().unwrap().id_for_path("b").unwrap();
    r.lock().unwrap().reassign(&id, &observed).unwrap();
    c.tick();
    assert_eq!(r.lock().unwrap().id_for_path("b").unwrap(), id);
    assert_eq!(r.lock().unwrap().list(0)["total"], 2);
    state.lock().unwrap().paths = (0..260).map(|n| format!("device{n}")).collect();
    c.scan().unwrap();
    assert!(r.lock().unwrap().diagnostic.contains("Forget an offline"));
}
#[test]
fn worker_shutdown_and_diagnostics() {
    assert!(owned_virtual("OpenSimGear-Virtual:one"));
    assert!(owned_virtual("osg-virtual:a"));
    assert!(!owned_virtual("USB"));
    assert!(permission_error("macos", "denied").contains("Privacy & Security"));
    let (_, state, r, drops) = setup();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let handle = std::thread::spawn(move || run(Backend { state, drops }, r, stopped));
    std::thread::sleep(Duration::from_millis(520));
    stop.store(true, Ordering::Release);
    assert!(handle.join().unwrap().is_ok());
}
