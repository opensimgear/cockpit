use super::*;
use osg_companion_service::{InputValue, SourceChange, SourceSink, SourceToken};
use osg_virtual_controller::{
    feeder::Message,
    session::{Channel, Factory},
    vjoy::{Axis, Slot, Status},
};
use std::{
    sync::{mpsc, Condvar},
    time::Instant,
};

fn publication() -> Publication {
    Publication::validate(
        serde_json::from_slice(include_bytes!(
            "../../companion-service/tests/fixtures/c1-flight-publication.json"
        ))
        .unwrap(),
    )
    .unwrap()
}
fn revised(p: &Publication, change: impl FnOnce(&mut Value)) -> Publication {
    let mut d: Value = serde_json::from_str(&p.canonical_document).unwrap();
    change(&mut d);
    let canonical_document = serde_json::to_string(&d).unwrap();
    Publication::validate(PublicationUpload {
        publication_hash: format!("sha256:{}", osg_logic::worker::hash(&canonical_document)),
        canonical_document,
        capability_fingerprint: None,
        assets: p.assets.clone(),
    })
    .unwrap()
}
#[derive(Default)]
struct Gate {
    slot: u8,
    start: bool,
    report: bool,
    entered: bool,
    release: bool,
    fail_stop: bool,
    fail_start: bool,
}
type SharedGate = Arc<(Mutex<Gate>, Condvar)>;
struct Child {
    messages: Arc<Mutex<Vec<Value>>>,
    gate: SharedGate,
    slot: u8,
    alive: bool,
}
impl Channel for Child {
    fn request(&mut self, message: &Message) -> Result<(), String> {
        if let Message::Start { bindings, .. } = message {
            self.slot = bindings[0].slot;
        }
        let (lock, cv) = &*self.gate;
        let mut g = lock.lock().unwrap();
        if g.slot == self.slot
            && ((g.start && matches!(message, Message::Start { .. }))
                || (g.report
                    && matches!(message,Message::Report{report,..} if report.state.buttons.first()==Some(&true))))
        {
            g.entered = true;
            cv.notify_all();
            let (deadline, mut remaining) = (
                Instant::now() + Duration::from_secs(4),
                Duration::from_secs(4),
            );
            while !g.release {
                let (next, result) = cv.wait_timeout(g, remaining).unwrap();
                g = next;
                assert!(!result.timed_out(), "test feeder gate timed out");
                remaining = deadline.saturating_duration_since(Instant::now());
            }
        }
        if g.fail_stop && matches!(message, Message::Stop { .. }) {
            return Err("lost Stop acknowledgement".into());
        }
        if g.fail_start && matches!(message, Message::Start { .. }) {
            return Err("uncertain native acquisition".into());
        }
        drop(g);
        self.messages
            .lock()
            .unwrap()
            .push(json!({"slot":self.slot,"message":message}));
        Ok(())
    }
    fn alive(&mut self) -> bool {
        self.alive
    }
    fn terminate(&mut self) {
        self.alive = false;
    }
}
struct Fixture {
    service: Arc<Service>,
    _root: tempfile::TempDir,
    provider: Arc<crate::companions::CompanionProvider>,
    publications: Arc<Mutex<BTreeMap<String, Publication>>>,
    messages: Arc<Mutex<Vec<Value>>>,
    gate: SharedGate,
    offset: Arc<AtomicU64>,
    slots: Arc<Mutex<Vec<Slot>>>,
}
impl Fixture {
    fn new(qualified: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let capture = Arc::new(ControllerService::new(None, false));
        // Only explicit test steps advance leases. Coverage/CPU scheduling must
        // not expire an unrelated instance while a source-loss test is paused.
        let initial_time = capture.stream_page("", 0).captured_us;
        let offset = Arc::new(AtomicU64::new(0));
        let delta = offset.clone();
        let provider = Arc::new(crate::companions::CompanionProvider::new(Arc::new(
            move || initial_time + delta.load(Ordering::Acquire),
        )));
        let messages = Arc::new(Mutex::new(vec![]));
        let gate = Arc::new((Mutex::new(Gate::default()), Condvar::new()));
        let calls = messages.clone();
        let blocked = gate.clone();
        let factory: Factory = Arc::new(move || {
            Ok(Box::new(Child {
                messages: calls.clone(),
                gate: blocked.clone(),
                slot: 0,
                alive: true,
            }))
        });
        let slots = Arc::new(Mutex::new(vec![]));
        let inventory = slots.clone();
        let service = Arc::new(Service::with_output(
            capture,
            Some(root.path().into()),
            Arc::new(move || Ok(inventory.lock().unwrap().clone())),
            factory,
            qualified,
        ));
        service.set_companion_provider(provider.clone());
        let publications = Arc::new(Mutex::new(BTreeMap::<String, Publication>::new()));
        let current = publications.clone();
        service.set_companion_blueprint_provider(Arc::new(move |id| {
            current
                .lock()
                .unwrap()
                .get(id)
                .cloned()
                .ok_or("Assignment was revoked".into())
        }));
        Self {
            service,
            _root: root,
            provider,
            publications,
            messages,
            gate,
            offset,
            slots,
        }
    }
    fn add(&self, id: &str, p: Publication) {
        self.provider.register(p.definition(id)).unwrap();
        self.publications.lock().unwrap().insert(id.into(), p);
    }
    fn token(&self, id: &str, generation: u64) -> SourceToken {
        SourceToken {
            source_id: id.into(),
            session_id: format!("session-{id}"),
            generation,
            publication_hash: self.publications.lock().unwrap()[id]
                .publication_hash
                .clone(),
        }
    }
    fn baseline(&self, id: &str, generation: u64) {
        let p = self.publications.lock().unwrap()[id].clone();
        let changes = p
            .controls
            .iter()
            .map(|c| SourceChange {
                control_id: c.id.clone(),
                value: match c.kind {
                    SourceKind::Button => InputValue::Button(false),
                    SourceKind::Hat => InputValue::Number(-1.),
                    SourceKind::Axis { fallback, .. } => InputValue::Number(fallback),
                },
            })
            .collect::<Vec<_>>();
        self.provider
            .baseline(&self.token(id, generation), 0, &changes)
            .unwrap();
    }
    fn change(&self, id: &str, generation: u64, sequence: u64, control: &str, value: InputValue) {
        self.provider
            .changes(
                &self.token(id, generation),
                sequence,
                &[SourceChange {
                    control_id: control.into(),
                    value,
                }],
            )
            .unwrap();
    }
    fn request(&self, id: &str, action: &str, extra: Value) -> Result<Value, String> {
        let mut companion = extra;
        companion["instanceId"] = json!(id);
        companion["action"] = json!(action);
        self.service
            .request(&json!({"command":"mapping_companion_route","companion":companion}))
            .map(|v| v["companionRoute"].clone())
    }
    fn save(&self, id: &str, slot: Option<u8>) -> Value {
        let p = self.publications.lock().unwrap()[id].clone();
        let c = author(id, &p).unwrap();
        let bindings = slot
            .map(|first| {
                c.profile
                    .targets
                    .iter()
                    .enumerate()
                    .map(|(i, t)| Binding {
                        target: t.id.clone(),
                        slot: first + i as u8,
                        axes: osg_virtual_controller::vjoy::AXIS_USAGES[..t.layout.axes.len()]
                            .to_vec(),
                        layout: t.layout.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for b in &bindings {
            self.slots.lock().unwrap().push(Slot {
                id: b.slot,
                status: Status::Free,
                owner_pid: None,
                buttons: b.layout.buttons,
                axes: b
                    .axes
                    .iter()
                    .map(|u| Axis {
                        usage: *u,
                        min: 0,
                        max: 32767,
                    })
                    .collect(),
                continuous_hats: b.layout.hats,
                discrete_hats: 0,
                force_feedback: false,
            });
        }
        self.request(
            id,
            "save",
            json!({"publicationHash":p.publication_hash,"bindings":bindings}),
        )
        .unwrap()
    }
    fn start(&self, id: &str, mode: &str) -> Value {
        self.request(id, "start", json!({"mode":mode})).unwrap()
    }
    fn submission(&self, id: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match self.service.companion_submission(id) {
                Ok(proof) => return proof,
                Err(error) if error == "Companion submission proof is temporarily busy." => {
                    assert!(
                        Instant::now() < deadline,
                        "Submission inspector remained busy"
                    );
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(error) => panic!("Unexpected submission inspection failure: {error}"),
            }
        }
    }
    fn output(&self, id: &str) -> Value {
        let route = self.service.companion_runs.lock().unwrap().routes[id].clone();
        let router = route.router.lock().unwrap();
        router
            .runtime
            .engine()
            .map(|e| e.inspect(0)["outputs"].clone())
            .unwrap_or(Value::Null)
    }
    fn wait(&self, id: &str, predicate: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let v = self.request(id, "inspect", json!({})).unwrap();
            if predicate(&v) {
                return v;
            }
            assert!(
                Instant::now() < deadline,
                "route did not reach desired state: {v}"
            );
            std::thread::sleep(Duration::from_millis(3));
        }
    }
    fn recording(&self, id: &str, after: Value) -> Value {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match self.request(id, "recording", json!({"after":after})) {
                Ok(page) => return page,
                Err(error) if error.contains("feeder is busy") => {
                    assert!(
                        Instant::now() < deadline,
                        "Recording remained busy: {error}"
                    );
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(error) => panic!("Recording failed: {error}"),
            }
        }
    }
    fn wait_gate(&self) {
        let (lock, cv) = &*self.gate;
        let g = lock.lock().unwrap();
        let (g, r) = cv
            .wait_timeout_while(g, Duration::from_secs(3), |g| !g.entered)
            .unwrap();
        assert!(g.entered && !r.timed_out());
    }
    fn release_gate(&self) {
        let (lock, cv) = &*self.gate;
        lock.lock().unwrap().release = true;
        cv.notify_all();
    }
    fn enable(&self) {
        self.service
            .request(&json!({"command":"mapping_virtual_enable","enabled":true}))
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.release_gate();
        self.service.shutdown();
    }
}

#[test]
fn authored_profiles_preserve_primitives_fallbacks_and_stable_instance_identity() {
    let f = Fixture::new(false);
    let p = revised(&publication(), |d| {
        d["ui"]["controls"][2]["fallback"] = json!(0.25)
    });
    f.add("left", p.clone());
    f.add("right", p.clone());
    let left = author("left", &p).unwrap();
    let right = author("right", &p).unwrap();
    assert_ne!(left.profile.id, right.profile.id);
    assert_ne!(left.profile.targets[0].id, right.profile.targets[0].id);
    assert_eq!(left.profile.targets[0].layout.axes[0].fallback, 8192);
    assert_eq!(left.profile.roles[0].origin, Origin::Companion);
    assert_eq!(left.profile.roles[0].controls.len(), p.controls.len());
    assert_eq!(left.assignments[0].source.id, "left");
    let v = f.save("left", None);
    assert!(v["preflight"]["recordingReady"].as_bool().unwrap());
    assert_eq!(v["preflight"]["liveReady"], false);
    assert!(f.messages.lock().unwrap().is_empty());
    f.start("left", "recording");
    f.baseline("left", 1);
    f.wait("left", |v| v["session"]["ready"] == true);
    let target = left.profile.targets[0].id.clone();
    assert_eq!(f.output("left")[&target]["axes"][0], 8192);
    f.change("left", 1, 1, "flight-input-0", InputValue::Button(true));
    f.change("left", 1, 2, "flight-input-0", InputValue::Button(false));
    f.change("left", 1, 3, "flight-input-0", InputValue::Button(true));
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let records = f.recording("left", json!("0"));
        let edges = records["records"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["report"]["state"]["buttons"][0].clone())
            .collect::<Vec<_>>();
        if edges
            .windows(3)
            .any(|e| e == [json!(true), json!(false), json!(true)])
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(3));
    }
    f.request("left", "stop", json!({})).unwrap();
    f.wait("left", |v| v["session"]["cleanupComplete"] == true);
    assert!(f.messages.lock().unwrap().is_empty());
}
#[test]
fn source_loss_requires_explicit_restart_and_does_not_stop_other_routes() {
    let f = Fixture::new(false);
    for id in ["left", "right"] {
        f.add(id, publication());
        f.baseline(id, 1);
        f.save(id, None);
        f.start(id, "recording");
    }
    f.change("right", 1, 1, "flight-input-0", InputValue::Button(true));
    f.provider.loss(&f.token("left", 1)).unwrap();
    f.baseline("left", 2);
    f.change("left", 2, 1, "flight-input-0", InputValue::Button(true));
    f.wait("left", |v| v["session"]["cleanupComplete"] == true);
    assert_eq!(
        f.request("right", "inspect", json!({})).unwrap()["session"]["running"],
        true
    );
    let rows = f.request("left", "recording", json!({})).unwrap();
    assert!(rows["records"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["report"]["state"]["buttons"][0] != true));
    f.start("left", "recording");
    f.wait("left", |v| v["session"]["ready"] == true);
    let target = f.request("left", "inspect", json!({})).unwrap()["profile"]["targets"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(f.output("left")[target]["buttons"][0], true);
    f.provider.heartbeat(&f.token("right", 1)).unwrap();
    f.offset
        .store(crate::companions::LEASE_US, Ordering::Release);
    f.wait("left", |v| v["session"]["cleanupComplete"] == true);
}
#[test]
fn immutable_revision_needs_explicit_reconciliation_and_old_feed_cannot_revive() {
    let f = Fixture::new(false);
    let p = publication();
    f.add("panel", p.clone());
    f.baseline("panel", 1);
    let old = f.save("panel", None);
    f.start("panel", "recording");
    let next = revised(&p, |d| d["ui"]["title"] = json!("New panel style"));
    f.add("panel", next.clone());
    f.baseline("panel", 2);
    f.change("panel", 2, 1, "flight-input-0", InputValue::Button(true));
    f.wait("panel", |v| v["session"]["cleanupComplete"] == true);
    let view = f.request("panel", "inspect", json!({})).unwrap();
    assert_eq!(view["expectedPublicationHash"], old["publicationHash"]);
    assert_eq!(view["preflight"]["recordingReady"], false);
    assert!(f
        .request("panel", "start", json!({"mode":"recording"}))
        .is_err());
    assert!(f
        .request(
            "panel",
            "reconcile",
            json!({"publicationHash":p.publication_hash})
        )
        .is_err());
    let reconciled = f
        .request(
            "panel",
            "reconcile",
            json!({"publicationHash":next.publication_hash}),
        )
        .unwrap();
    assert_eq!(reconciled["profile"]["id"], old["profile"]["id"]);
    f.start("panel", "recording");
    f.publications.lock().unwrap().remove("panel");
    f.provider
        .loss(&f.token_for_revoked("panel", 2, &next))
        .unwrap();
    f.wait("panel", |v| v["session"]["cleanupComplete"] == true);
    assert_eq!(
        f.request("panel", "recording", json!({})).unwrap()["preflight"]["recordingReady"],
        false
    );
    f.request("panel", "stop", json!({})).unwrap();
}
impl Fixture {
    fn token_for_revoked(&self, id: &str, generation: u64, p: &Publication) -> SourceToken {
        SourceToken {
            source_id: id.into(),
            session_id: format!("session-{id}"),
            generation,
            publication_hash: p.publication_hash.clone(),
        }
    }
}

fn small(p: &Publication) -> Publication {
    revised(p, |d| {
        let c = d["ui"]["controls"][0].clone();
        let w = d["presentation"]["widgets"][0].clone();
        let item = d["ui"]["pages"][0]["items"][0].clone();
        d["ui"]["controls"] = json!([c]);
        d["ui"]["feedbackWidgets"] = json!([]);
        d["ui"]["pages"][0]["items"] = json!([item]);
        d["presentation"]["widgets"] = json!([w]);
        d["bindings"].as_array_mut().unwrap().truncate(1);
        d["targets"][0]["axes"] = json!([]);
        d["targets"][0]["hats"] = json!(0);
        d["targets"][0]["buttons"] = json!(1);
    })
}
#[test]
fn exact_target_validation_and_enablement_block_before_any_factory_side_effect() {
    let f = Fixture::new(false);
    f.add("panel", publication());
    f.baseline("panel", 1);
    let saved = f.save("panel", Some(2));
    let mut bad = saved["bindings"].clone();
    bad[0]["layout"]["buttons"] = json!(7);
    assert!(f
        .request(
            "panel",
            "save",
            json!({"publicationHash":saved["publicationHash"],"bindings":bad})
        )
        .is_err());
    let mut bad = saved["bindings"].clone();
    bad[0]["axes"][0] = bad[0]["axes"][1].clone();
    assert!(f
        .request(
            "panel",
            "save",
            json!({"publicationHash":saved["publicationHash"],"bindings":bad})
        )
        .is_err());
    assert!(f.request("panel", "start", json!({"mode":"live"})).is_err());
    f.enable();
    assert_eq!(
        f.request("panel", "inspect", json!({})).unwrap()["preflight"]["liveReady"],
        false
    );
    assert!(f.request("panel", "recover", json!({})).is_err());
    assert!(f.messages.lock().unwrap().is_empty());
    assert!(f
        .request(
            "panel",
            "save",
            json!({"publicationHash":saved["publicationHash"],"bindings":{}})
        )
        .is_err());
    assert!(f
        .request("panel", "start", json!({"mode":"unexpected"}))
        .is_err());
    assert!(f.request("panel", "unknown", json!({})).is_err());
    assert!(f
        .request("panel", "recording", json!({"after":"-1"}))
        .is_err());
    f.start("panel", "recording");
    assert!(f
        .request(
            "panel",
            "reconcile",
            json!({"publicationHash":saved["publicationHash"]})
        )
        .is_err());
    f.request("panel", "stop", json!({})).unwrap();
    f.wait("panel", |v| v["session"]["cleanupComplete"] == true);
    assert_eq!(
        f.request("panel", "inspect", json!({})).unwrap()["bindings"],
        saved["bindings"]
    );
}
#[test]
fn live_fixture_checks_full_baseline_and_preserves_declared_fallback_through_stop() {
    let f = Fixture::new(true);
    f.add(
        "panel",
        revised(&publication(), |d| {
            d["ui"]["controls"][2]["fallback"] = json!(0.25)
        }),
    );
    f.save("panel", Some(2));
    f.enable();
    assert!(f.request("panel", "start", json!({"mode":"live"})).is_err());
    assert!(f.messages.lock().unwrap().is_empty());
    f.baseline("panel", 1);
    f.change("panel", 1, 1, "flight-input-0", InputValue::Button(true));
    assert!(f.request("panel", "start", json!({"mode":"live"})).is_err());
    f.change("panel", 1, 2, "flight-input-0", InputValue::Button(false));
    let live = f.start("panel", "live");
    assert_eq!(live["session"]["mode"], "live");
    f.change("panel", 1, 3, "flight-input-0", InputValue::Button(true));
    let deadline = Instant::now() + Duration::from_secs(2);
    while !f.messages.lock().unwrap().iter().any(|m| {
        m["message"]["command"] == "report" && m["message"]["report"]["state"]["buttons"][0] == true
    }) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    f.request("panel", "stop", json!({})).unwrap();
    f.wait("panel", |v| v["session"]["cleanupComplete"] == true);
    let messages = f.messages.lock().unwrap();
    assert!(messages.iter().any(|m| m["message"]["command"] == "stop"));
    assert!(messages
        .iter()
        .filter(|m| m["message"]["command"] == "report")
        .any(|m| m["message"]["report"]["state"]["axes"][0] == 8192));
}
#[test]
fn pending_acquisition_can_be_stopped_without_waiting_and_cannot_flush_a_stale_report() {
    for fault in ["stop", "loss", "held", "revision", "revoke"] {
        let f = Fixture::new(true);
        f.add("panel", publication());
        f.baseline("panel", 1);
        f.save("panel", Some(2));
        f.enable();
        {
            let mut gate = f.gate.0.lock().unwrap();
            gate.slot = 2;
            gate.start = true;
        }
        let service = f.service.clone();
        let (tx, rx) = mpsc::channel();
        let start = std::thread::spawn(move || {
            tx.send(service.request(&json!({"command":"mapping_companion_route","companion":{"action":"start","instanceId":"panel","mode":"live"}}))).unwrap();
        });
        f.wait_gate();
        match fault {
            "stop" => {
                let before = Instant::now();
                let stopped = f.request("panel", "stop", json!({})).unwrap();
                assert!(before.elapsed() < Duration::from_millis(500));
                assert_eq!(stopped["session"]["cleanupComplete"], false);
                assert_eq!(stopped["session"]["stopRequested"], true);
            }
            "loss" => f.provider.loss(&f.token("panel", 1)).unwrap(),
            "held" => f.change("panel", 1, 1, "flight-input-0", InputValue::Button(true)),
            "revision" => {
                let p = revised(&publication(), |d| {
                    d["ui"]["title"] = json!("Changed during acquire")
                });
                f.add("panel", p);
            }
            _ => {
                f.publications.lock().unwrap().remove("panel");
            }
        }
        f.release_gate();
        assert!(
            rx.recv_timeout(Duration::from_secs(3)).unwrap().is_err(),
            "{fault} must cancel admission"
        );
        start.join().unwrap();
        assert!(!f
            .messages
            .lock()
            .unwrap()
            .iter()
            .any(|m| m["message"]["command"] == "report"));
        let v = f.wait("panel", |v| v["session"]["cleanupComplete"] == true);
        assert_eq!(v["session"]["running"], false);
    }
}
#[cfg(windows)]
#[test]
fn stalled_route_report_does_not_block_other_companion_or_ordinary_mapping_clocks() {
    let f = Fixture::new(true);
    for (id, slot) in [("slow", 2), ("healthy", 3)] {
        f.add(id, publication());
        f.baseline(id, 1);
        f.save(id, Some(slot));
    }
    f.enable();
    f.start("slow", "live");
    f.start("healthy", "live");
    let profile =
        Profile::parse(include_bytes!("../../mapping/fixtures/direct-profile.json")).unwrap();
    let assignments = profile
        .roles
        .iter()
        .map(|r| Assignment {
            role: r.id.clone(),
            source: Source {
                origin: r.origin,
                id: r.id.clone(),
            },
            controls: r
                .controls
                .iter()
                .map(|c| osg_mapping::engine::ControlAssignment {
                    control: c.id.clone(),
                    source_control: c.id.clone(),
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    let store = f.service.store().unwrap();
    store.save(&profile).unwrap();
    store.save_assignments(&profile.id, &assignments).unwrap();
    let ordinary = f
        .service
        .request(&json!({"command":"mapping_preview_start","profileId":profile.id}))
        .unwrap();
    {
        let mut gate = f.gate.0.lock().unwrap();
        gate.slot = 2;
        gate.report = true;
    }
    f.change("slow", 1, 1, "flight-input-0", InputValue::Button(true));
    f.wait_gate();
    assert!(
        f.request("slow", "recording", json!({})).is_err(),
        "recording polling must not wait on a slow feeder"
    );
    let stopped = f.request("slow", "stop", json!({})).unwrap();
    assert_eq!(stopped["session"]["cleanupComplete"], false);
    f.change("healthy", 1, 1, "flight-input-0", InputValue::Button(true));
    f.service.request(&json!({"command":"mapping_simulate","token":ordinary["token"],"source":assignments[0].source,"changes":[{"control":"trigger","value":{"kind":"button","value":true}}]})).unwrap();
    // Longer than the native feeder lease: healthy routes must keep renewing
    // while this one route's native request is blocked.
    let deadline = Instant::now() + Duration::from_millis(350);
    while Instant::now() < deadline {
        f.provider.heartbeat(&f.token("healthy", 1)).unwrap();
        std::thread::sleep(Duration::from_millis(10));
    }
    let messages = f.messages.lock().unwrap();
    assert!(messages
        .iter()
        .any(|m| m["slot"] == 3 && m["message"]["command"] == "renew"));
    assert!(messages
        .iter()
        .any(|m| m["slot"] == 3 && m["message"]["report"]["state"]["buttons"][0] == true));
    drop(messages);
    let status = f
        .service
        .request(&json!({"command":"mapping_status","inspect":true}))
        .unwrap();
    assert_eq!(status["running"], true);
    assert_eq!(status["inspector"]["outputs"]["main"]["buttons"][0], true);
    assert_eq!(
        f.request("healthy", "inspect", json!({})).unwrap()["session"]["running"],
        true
    );
    f.release_gate();
    f.wait("slow", |v| v["session"]["cleanupComplete"] == true);
    assert_eq!(
        f.request("healthy", "inspect", json!({})).unwrap()["session"]["running"],
        true
    );
}
#[test]
fn eight_independent_recording_routes_share_budgets_and_slotless_preview() {
    let f = Fixture::new(false);
    let p = small(&publication());
    for i in 0..9 {
        let id = format!("panel-{i}");
        f.add(&id, p.clone());
        f.save(&id, Some(2));
        if i < 8 {
            f.baseline(&id, 1);
            f.start(&id, "recording");
        }
    }
    assert!(f
        .request("panel-8", "start", json!({"mode":"recording"}))
        .is_err());
    assert_eq!(f.service.companion_runs.lock().unwrap().routes.len(), 8);
    f.request("panel-0", "stop", json!({})).unwrap();
    f.wait("panel-0", |v| v["session"]["cleanupComplete"] == true);
    f.start("panel-8", "recording");
    assert!(f.messages.lock().unwrap().is_empty());
}
#[test]
fn saved_configuration_and_pending_recovery_survive_restart_without_activation() {
    let f = Fixture::new(true);
    f.add("old", publication());
    let saved = f.save("old", Some(2));
    let original = saved["profile"]["id"].clone();
    f.add("new", publication());
    f.save("new", Some(2));
    let c = f.service.companion_runs.lock().unwrap().configurations["old"].clone();
    let path = f
        .service
        .companion_runs
        .lock()
        .unwrap()
        .journal("old")
        .unwrap();
    let token = RunToken {
        session: uuid::Uuid::new_v4().to_string(),
        generation: Counter(1),
    };
    {
        let mut journal =
            osg_virtual_controller::output_journal::Store::open(path.clone()).unwrap();
        journal.begin(token, c.bindings.clone()).unwrap();
        journal
            .transition(osg_virtual_controller::output_journal::Phase::Confirmed)
            .unwrap();
    }
    *f.service.companion_runs.lock().unwrap() = Manager::new(Some(f._root.path().into()));
    let view = f.request("old", "inspect", json!({})).unwrap();
    assert_eq!(view["profile"]["id"], original);
    assert_eq!(view["session"]["running"], false);
    assert_eq!(view["session"]["recoveryRequired"], true);
    assert_eq!(view["session"]["cleanupComplete"], false);
    let new = f.service.companion_runs.lock().unwrap().configurations["new"].clone();
    assert!(f
        .service
        .reserve_manual(&new.profile, &new.bindings, true)
        .is_err());
    assert_eq!(
        f.request("new", "inspect", json!({})).unwrap()["preflight"]["liveReady"],
        false
    );
    assert!(f.messages.lock().unwrap().is_empty());
    f.request("old", "recover", json!({})).unwrap();
    assert!(!osg_virtual_controller::output_journal::inspect(&path)
        .unwrap()
        .unwrap()
        .pending());
    assert!(f
        .service
        .reserve_manual(&new.profile, &new.bindings, true)
        .is_ok());
    assert_eq!(
        f.request("old", "inspect", json!({})).unwrap()["session"]["cleanupComplete"],
        true
    );
    f.publications.lock().unwrap().clear();
    f.service
        .set_companion_provider(Arc::new(crate::companions::CompanionProvider::new(
            Arc::new(|| 1),
        )));
    let list = f
        .service
        .request(&json!({"command":"mapping_companion_sources"}))
        .unwrap();
    assert_eq!(list["companionSources"].as_array().unwrap().len(), 2);
    assert!(list["companionSources"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["online"] == false));
    assert_eq!(
        f.request("old", "inspect", json!({})).unwrap()["profile"]["id"],
        original
    );
}
#[test]
fn corrupt_configuration_and_uncertain_acquisition_preserve_recovery_ownership() {
    let f = Fixture::new(true);
    f.add("panel", publication());
    f.baseline("panel", 1);
    f.save("panel", Some(2));
    f.enable();
    f.gate.0.lock().unwrap().fail_start = true;
    assert!(f.request("panel", "start", json!({"mode":"live"})).is_err());
    let view = f.request("panel", "inspect", json!({})).unwrap();
    assert_eq!(view["session"]["recoveryRequired"], true);
    assert_eq!(view["session"]["cleanupComplete"], false);
    let bytes = b"{invalid saved state";
    std::fs::write(f._root.path().join("companion-routes.json"), bytes).unwrap();
    *f.service.companion_runs.lock().unwrap() = Manager::new(Some(f._root.path().into()));
    let corrupted = f.request("panel", "inspect", json!({})).unwrap();
    assert_eq!(corrupted["preflight"]["recordingReady"], false);
    assert_eq!(corrupted["session"]["recoveryRequired"], true);
    assert_eq!(corrupted["session"]["cleanupComplete"], false);
    let c = author("panel", &publication()).unwrap();
    assert!(f.service.reserve_manual(&c.profile, &[], true).is_err());
    assert!(f
        .request(
            "panel",
            "save",
            json!({"publicationHash":publication().publication_hash})
        )
        .is_err());
    assert_eq!(
        std::fs::read(f._root.path().join("companion-routes.json")).unwrap(),
        bytes
    );
}

#[test]
fn maximal_portable_controls_and_recording_pages_fit_the_private_gateway_response_budget() {
    let f = Fixture::new(false);
    let p = revised(&small(&publication()), |d| {
        let mut controls = vec![];
        let mut widgets = vec![];
        let mut items = vec![];
        let mut bindings = vec![];
        let mut targets = vec![];
        for t in 0..8 {
            targets.push(json!({"id":format!("target-{t}"),"label":"T".repeat(120),"axes":vec!["bipolar";16],"buttons":128,"hats":4}));
        }
        for i in 0..128 {
            let id = format!("i{i:03}{}", "x".repeat(60));
            let mut c = d["ui"]["controls"][0].clone();
            c["id"] = json!(id);
            c["label"] = json!("C".repeat(120));
            controls.push(c);
            let mut w = d["presentation"]["widgets"][0].clone();
            w["id"] = json!(format!("w{i}"));
            w["controlIds"] = json!([id]);
            widgets.push(w);
            let mut item = d["ui"]["pages"][0]["items"][0].clone();
            item["refId"] = json!(id);
            items.push(item);
            bindings.push(json!({"controlId":id,"targetId":format!("target-{}",i/16),"kind":"button","index":i%16}));
        }
        d["ui"]["controls"] = json!(controls);
        d["ui"]["pages"][0]["items"] = json!(items);
        d["presentation"]["widgets"] = json!(widgets);
        d["targets"] = json!(targets);
        d["bindings"] = json!(bindings);
    });
    f.add("maximum", p.clone());
    f.baseline("maximum", 1);
    let descriptor = f.save("maximum", Some(1));
    assert_eq!(
        descriptor["profile"]["roles"][0]["controls"]
            .as_array()
            .unwrap()
            .len(),
        128
    );
    assert!(
        serde_json::to_vec(&json!({"result":{"companionRoute":descriptor}}))
            .unwrap()
            .len()
            <= 66_560
    );
    f.start("maximum", "recording");
    let control = p.controls[0].id.clone();
    let changes = (0..64)
        .map(|i| SourceChange {
            control_id: control.clone(),
            value: InputValue::Button(i % 2 == 0),
        })
        .collect::<Vec<_>>();
    f.provider
        .changes(&f.token("maximum", 1), 1, &changes)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let r = f.service.companion_runs.lock().unwrap().routes["maximum"].clone();
        if r.router.lock().unwrap().runtime.recording.cursor().0 >= 64 {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut after = json!("0");
    let mut seen = 0;
    loop {
        let page = f.recording("maximum", after.clone());
        assert!(
            serde_json::to_vec(
                &json!({"requestId":"x".repeat(128),"result":{"companionRoute":page}})
            )
            .unwrap()
            .len()
                <= 66_560
        );
        let count = page["records"].as_array().unwrap().len();
        seen += count;
        if count == 0 {
            break;
        }
        assert_ne!(page["next"], after);
        after = page["next"].clone();
    }
    assert!(seen >= 64);
}
#[test]
fn stop_cancels_start_waiting_for_authority_before_resource_admission() {
    let f = Fixture::new(false);
    f.add("panel", publication());
    f.save("panel", None);
    let (lock, _) = &*f.gate;
    lock.lock().unwrap().release = false;
    let gate = f.gate.clone();
    let p = publication();
    f.service
        .set_companion_blueprint_provider(Arc::new(move |_| {
            let (lock, cv) = &*gate;
            let mut g = lock.lock().unwrap();
            g.entered = true;
            cv.notify_all();
            while !g.release {
                let (next, r) = cv.wait_timeout(g, Duration::from_secs(3)).unwrap();
                g = next;
                assert!(!r.timed_out());
            }
            Ok(p.clone())
        }));
    let service = f.service.clone();
    let start = std::thread::spawn(move || {
        service.request(&json!({"command":"mapping_companion_route","companion":{"instanceId":"panel","action":"start","mode":"recording"}}))
    });
    f.wait_gate();
    // Stop itself invalidates the epoch before its inspect response consults
    // authority, so allow that response to finish on another request thread.
    let service = f.service.clone();
    let stop = std::thread::spawn(move || {
        service.request(&json!({"command":"mapping_companion_route","companion":{"instanceId":"panel","action":"stop"}}))
    });
    let deadline = Instant::now() + Duration::from_secs(1);
    while f.service.companion_runs.lock().unwrap().epochs.get("panel") != Some(&1) {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    f.release_gate();
    assert!(start.join().unwrap().is_err());
    assert!(stop.join().unwrap().is_ok());
    assert!(f.service.companion_runs.lock().unwrap().routes.is_empty());
    assert!(f.messages.lock().unwrap().is_empty());
}
#[test]
fn ordinary_interrupted_output_also_reserves_slots_against_companion_admission() {
    let f = Fixture::new(true);
    f.add("panel", small(&publication()));
    f.baseline("panel", 1);
    let view = f.save("panel", Some(2));
    f.enable();
    let bindings: Vec<Binding> = serde_json::from_value(view["bindings"].clone()).unwrap();
    let path = f.service.recovery_path().unwrap();
    {
        let mut store = osg_virtual_controller::output_journal::Store::open(path).unwrap();
        store
            .begin(
                RunToken {
                    session: uuid::Uuid::new_v4().to_string(),
                    generation: Counter(1),
                },
                bindings,
            )
            .unwrap();
        store
            .transition(osg_virtual_controller::output_journal::Phase::Confirmed)
            .unwrap();
    }
    let view = f.request("panel", "inspect", json!({})).unwrap();
    assert_eq!(view["preflight"]["recordingReady"], true);
    assert_eq!(view["preflight"]["liveReady"], false);
    assert!(f.request("panel", "start", json!({"mode":"live"})).is_err());
    assert!(f.messages.lock().unwrap().is_empty());
}

#[test]
fn deleted_configuration_does_not_discard_orphaned_driver_ownership() {
    let f = Fixture::new(true);
    f.add("orphan", small(&publication()));
    let saved = f.save("orphan", Some(2));
    let c = f.service.companion_runs.lock().unwrap().configurations["orphan"].clone();
    let path = f
        .service
        .companion_runs
        .lock()
        .unwrap()
        .journal("orphan")
        .unwrap();
    {
        let mut store = osg_virtual_controller::output_journal::Store::open(path).unwrap();
        store
            .begin(
                RunToken {
                    session: uuid::Uuid::new_v4().to_string(),
                    generation: Counter(1),
                },
                c.bindings.clone(),
            )
            .unwrap();
        store
            .transition(osg_virtual_controller::output_journal::Phase::Confirmed)
            .unwrap();
    }
    std::fs::remove_file(f._root.path().join("companion-routes.json")).unwrap();
    *f.service.companion_runs.lock().unwrap() = Manager::new(Some(f._root.path().into()));
    assert_eq!(f.service.companion_runs.lock().unwrap().orphaned.len(), 1);
    let view = f.request("orphan", "inspect", json!({})).unwrap();
    assert_eq!(view["session"]["cleanupComplete"], false);
    assert_eq!(view["session"]["recoveryRequired"], true);
    assert!(f
        .service
        .reserve_manual(&c.profile, &c.bindings, true)
        .is_err());
    f.request("orphan", "recover", json!({})).unwrap();
    assert!(f.service.companion_runs.lock().unwrap().orphaned.is_empty());
    assert!(f
        .service
        .reserve_manual(&c.profile, &c.bindings, true)
        .is_ok());
    assert_eq!(
        f.request("orphan", "inspect", json!({})).unwrap()["session"]["cleanupComplete"],
        true
    );
    assert_eq!(saved["profile"]["id"], json!(c.profile.id));
    std::fs::write(
        f.service
            .companion_runs
            .lock()
            .unwrap()
            .journal("orphan")
            .unwrap(),
        b"corrupt journal",
    )
    .unwrap();
    *f.service.companion_runs.lock().unwrap() = Manager::new(Some(f._root.path().into()));
    let view = f.request("orphan", "inspect", json!({})).unwrap();
    assert_eq!(view["session"]["cleanupComplete"], false);
    assert_eq!(view["session"]["recoveryRequired"], true);
    assert!(f.service.reserve_manual(&c.profile, &[], true).is_err());
    assert!(f.service.reserve_manual(&c.profile, &[], false).is_ok());
}

#[test]
fn submission_feedback_needs_acknowledged_live_output_and_is_read_only() {
    let f = Fixture::new(true);
    f.add("panel", publication());
    f.save("panel", Some(2));
    f.baseline("panel", 1);
    f.start("panel", "recording");
    assert_eq!(f.submission("panel")["confirmed"], false);
    f.request("panel", "stop", json!({})).unwrap();
    f.wait("panel", |v| v["session"]["cleanupComplete"] == true);
    f.enable();
    f.start("panel", "live");
    f.change("panel", 1, 1, "flight-input-0", InputValue::Button(true));
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let proof = f.submission("panel");
        if proof["confirmed"] == true && proof["values"]["flight-input-0"] == true {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    // The independent live worker may renew its lease during a read. Count
    // output mutations instead, so healthy renewal is not mistaken for a write.
    let mutations = || {
        f.messages
            .lock()
            .unwrap()
            .iter()
            .filter(|message| message["message"]["command"] != "renew")
            .count()
    };
    let before = mutations();
    let proof = f.submission("panel");
    assert_eq!(proof["values"]["flight-input-0"], true);
    assert_eq!(mutations(), before);
    f.request("panel", "stop", json!({})).unwrap();
    f.wait("panel", |v| v["session"]["cleanupComplete"] == true);
    assert_eq!(f.submission("panel")["confirmed"], false);
}

#[test]
fn submission_inspection_is_nonblocking_while_the_route_worker_owns_its_router() {
    let f = Fixture::new(false);
    f.add("panel", publication());
    f.save("panel", None);
    f.baseline("panel", 1);
    f.start("panel", "recording");
    let route = f.service.companion_runs.lock().unwrap().routes["panel"].clone();
    let guard = route.router.lock().unwrap();
    let before = f.messages.lock().unwrap().clone();
    let service = f.service.clone();
    let (send, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        send.send(service.companion_submission("panel")).unwrap();
    });
    // Keep the router locked until after the result is observed. A blocking
    // inspector fails this bound rather than waiting for an accidental unlock.
    let result = receive.recv_timeout(Duration::from_secs(2));
    let unchanged = *f.messages.lock().unwrap() == before;
    drop(guard);
    worker.join().unwrap();
    assert_eq!(
        result.unwrap().unwrap_err(),
        "Companion submission proof is temporarily busy."
    );
    assert!(unchanged);
    assert_eq!(f.submission("panel")["confirmed"], false);
}
