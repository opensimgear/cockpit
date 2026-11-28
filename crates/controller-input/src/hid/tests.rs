use super::*;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
struct Fixture(AtomicUsize);

#[test]
fn replacement_and_restart_never_reactivate_cached_contracts() {
    struct Changing(AtomicUsize);
    impl Backend for Changing {
        fn inventory(&self, target: Option<&str>) -> Result<Value, String> {
            let epoch = self.0.load(Ordering::Acquire);
            let id = format!("handle-{epoch}");
            let summary =
                json!({"interfaceId":id,"pathFingerprint":"same-interface-and-collection"});
            match target {
                None if epoch == 0 => Ok(json!({"interfaces":[],"truncated":false,"failures":[]})),
                None => Ok(json!({"interfaces":[summary],"truncated":false,"failures":[]})),
                Some(selected) if selected == id && epoch != 0 => {
                    let mut detail = summary;
                    detail["descriptorHex"] = json!(format!("{epoch:02x}"));
                    Ok(detail)
                }
                _ => Err("Interface was replaced".into()),
            }
        }
        fn open(&self, _: &str) -> Result<Box<dyn Reader>, String> {
            Ok(Box::new(Input { remaining: 2 }))
        }
    }
    let temp = crate::test_directory();
    let path = temp.path().join("settings.json");
    let backend = Arc::new(Changing(AtomicUsize::new(1)));
    let service = Service {
        backend: Some(backend.clone()),
        ..Service::new(false, Some(&path))
    };
    let request =
        |command: &str, args: Value| service.request(&format!("controller_hid_{command}"), &args);
    let first = request("open", json!({"interfaceId":"handle-1"})).unwrap();
    let listed = request("list", json!({})).unwrap();
    assert_eq!(
        listed["interfaces"][0]["captureGeneration"],
        first["generation"]
    );
    assert_eq!(
        request("state", first.clone()).unwrap()["contractFingerprint"],
        first["contractFingerprint"]
    );
    backend.0.store(2, Ordering::Release);
    let next = request("list", json!({})).unwrap();
    assert!(next["interfaces"][0].get("captureGeneration").is_none());
    assert!(request("reports", first.clone()).is_err());
    assert!(request(
        "open",
        json!({"interfaceId":"handle-2","contractFingerprint":first["contractFingerprint"]})
    )
    .is_err());
    let second = request("open", json!({"interfaceId":"handle-2"})).unwrap();
    assert_ne!(first["contractFingerprint"], second["contractFingerprint"]);
    backend.0.store(0, Ordering::Release);
    assert!(request("list", json!({})).unwrap()["interfaces"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(request("state", second.clone()).is_err());
    service.shutdown();
    let restarted = Service::new(false, Some(&path));
    let catalog = restarted
        .request("controller_hid_catalog", &json!({}))
        .unwrap();
    let rows = catalog["snapshots"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["availability"], "offline");
    assert_eq!(
        rows[0]["contractFingerprint"],
        second["contractFingerprint"]
    );
    let snapshot = restarted
        .request(
            "controller_hid_snapshot",
            &json!({"interfaceId":rows[0]["snapshotId"]}),
        )
        .unwrap();
    assert_eq!(snapshot["detail"]["descriptorHex"], "02");
    assert!(restarted.request("controller_hid_state", &second).is_err());
}

#[test]
fn blocked_native_open_reserves_only_its_own_interface() {
    use std::sync::mpsc;
    struct Slow {
        entered: mpsc::SyncSender<()>,
        resume: Mutex<mpsc::Receiver<()>>,
    }
    impl Backend for Slow {
        fn inventory(&self, target: Option<&str>) -> Result<Value, String> {
            Ok(json!({"interfaceId":target}))
        }
        fn open(&self, target: &str) -> Result<Box<dyn Reader>, String> {
            if target == "slow" {
                self.entered.send(()).unwrap();
                self.resume
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(3))
                    .unwrap();
            }
            Ok(Box::new(Input { remaining: 0 }))
        }
    }
    let (entered, started) = mpsc::sync_channel(1);
    let (go, resume) = mpsc::channel();
    let service = Arc::new(Service {
        stopped: AtomicBool::default(),
        backend: Some(Arc::new(Slow {
            entered,
            resume: Mutex::new(resume),
        })),
        sessions: Mutex::default(),
        catalog: Mutex::new(catalog::Catalog::new(None)),
    });
    let slow = service.clone();
    let open = std::thread::spawn(move || {
        slow.request("controller_hid_open", &json!({"interfaceId":"slow"}))
    });
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(service
        .request("controller_hid_open", &json!({"interfaceId":"slow"}))
        .is_err());
    let other = service
        .request("controller_hid_open", &json!({"interfaceId":"other"}))
        .unwrap();
    service.request("controller_hid_close", &other).unwrap();
    service.shutdown();
    go.send(()).unwrap();
    assert!(open.join().unwrap().is_err());
}
struct Input {
    remaining: u8,
}
impl Reader for Input {
    fn poll(&mut self, _: Duration) -> Result<Option<Vec<u8>>, Value> {
        if self.remaining > 0 {
            self.remaining -= 1;
            return Ok(Some(vec![7, 255]));
        }
        std::thread::sleep(Duration::from_millis(1));
        Ok(None)
    }
    fn get(&mut self, get: &Get, _: Duration) -> Result<Value, Value> {
        if get.feature {
            Ok(json!({"wireHex":"0701"}))
        } else {
            Err(json!({"kind":"unsupported"}))
        }
    }
    fn project(&self, bytes: &[u8]) -> Value {
        json!({"delta":bytes[1] as i8})
    }
}
impl Backend for Fixture {
    fn inventory(&self, target: Option<&str>) -> Result<Value, String> {
        Ok(match target {
            Some(id) => json!({"interfaceId":id}),
            None => json!({"interfaces":[{"interfaceId":"a"}],"failures":[],"truncated":false}),
        })
    }
    fn open(&self, target: &str) -> Result<Box<dyn Reader>, String> {
        if target == "denied" {
            return Err("AccessDenied".into());
        }
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(Input { remaining: 2 }))
    }
}
#[test]
fn selected_generation_preserves_equal_reports_and_fences_reopens() {
    let service = Service {
        stopped: AtomicBool::default(),
        backend: Some(Arc::new(Fixture(AtomicUsize::new(0)))),
        sessions: Mutex::default(),
        catalog: Mutex::new(catalog::Catalog::new(None)),
    };
    let run = |name: &str, p: Value| service.request(&format!("controller_hid_{name}"), &p);
    assert!(run("list", json!({})).is_ok());
    assert!(run("inspect", json!({})).is_err());
    assert_eq!(
        run("inspect", json!({"interfaceId":"a"})).unwrap()["interfaceId"],
        "a"
    );
    assert!(run("open", json!({"interfaceId":"denied"})).is_err());
    let opened = run("open", json!({"interfaceId":"a"})).unwrap();
    assert!(run("open", opened.clone()).is_err());
    let start = Instant::now();
    loop {
        let page = run("reports", opened.clone()).unwrap();
        if page["result"]["latest"] == "2" {
            let rows = page["result"]["reports"].as_array().unwrap();
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0]["wireHex"], "07ff");
            assert_eq!(rows[1]["sequence"], "2");
            assert_eq!(rows[1]["samples"]["delta"], -1);
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::yield_now();
    }
    let mut req = opened.clone();
    req["after"] = json!("1");
    assert_eq!(
        run("reports", req.clone()).unwrap()["result"]["reports"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    req["after"] = json!("3");
    assert!(run("reports", req.clone()).is_err());
    for (k, v) in [
        ("requestId", json!("r")),
        ("kind", json!("feature")),
        ("reportId", json!(7)),
        ("deadlineMs", json!(1000)),
        ("maxBytes", json!(2)),
    ] {
        req[k] = v;
    }
    assert_eq!(
        run("get", req.clone()).unwrap()["result"]["wireHex"],
        "0701"
    );
    req["kind"] = json!("input");
    assert_eq!(
        run("get", req.clone()).unwrap()["result"]["error"]["kind"],
        "unsupported"
    );
    req["kind"] = json!("output");
    assert!(run("get", req).is_err());
    run("close", opened.clone()).unwrap();
    assert!(run("reports", opened.clone()).is_err());
    let again = run("open", json!({"interfaceId":"a"})).unwrap();
    assert_ne!(opened["generation"], again["generation"]);
    assert!(run("close", opened).is_err());
    service.shutdown();
    assert!(Service::new(false, None)
        .request("controller_hid_list", &json!({}))
        .is_err());
}
