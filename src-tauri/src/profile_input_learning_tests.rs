use super::*;
use osg_controller_input::registry::{Control, Sample};
use osg_profiles::resources::{Access, Observation, ProviderBatch, Registry, ResourceType};
use osg_profiles::resources_ports::PortUpdate;
use std::sync::atomic::AtomicUsize;

const PHYSICAL: &str = "11111111-1111-4111-8111-111111111111";
struct Fixture {
    runtime: Runtime,
    registry: Arc<Mutex<Registry>>,
    journal: Arc<Mutex<Vec<stream::Page>>>,
    calls: Arc<AtomicUsize>,
    epoch: Arc<AtomicU64>,
    resource: String,
    generation: String,
    capability: String,
}
fn controls() -> Vec<Control> {
    let mut axis = Control::new(1, 0x30, 0, 0, 2);
    axis.bits = 16;
    axis.min = 0;
    axis.max = 32767;
    vec![
        Control::new(9, 1, 0, 0, 1),
        axis,
        Control::new(1, 0x39, 0, 0, 3),
    ]
}
fn baseline() -> stream::Event {
    stream::Event {
        cursor: 1,
        source: PHYSICAL.into(),
        generation: 1,
        sequence: 1,
        captured_us: 100,
        reason: stream::Reason::Baseline,
        values: controls()
            .iter()
            .map(|control| {
                (
                    control.id.clone(),
                    Sample {
                        native: 0,
                        normalized: Some(0),
                    },
                )
            })
            .collect(),
    }
}
fn snapshot() -> stream::Snapshot {
    stream::Snapshot {
        session: "native-journal".into(),
        cursor: 1,
        captured_us: 100,
        sources: vec![stream::SourceSnapshot {
            id: PHYSICAL.into(),
            controls: controls(),
            baseline: baseline(),
        }],
    }
}
fn changed(indices: &[usize]) -> stream::Page {
    let mut event = baseline();
    event.cursor = 2;
    event.sequence = 2;
    event.captured_us = 110;
    event.reason = stream::Reason::Change;
    for &index in indices {
        let control = &controls()[index];
        event.values.get_mut(&control.id).unwrap().normalized =
            Some(if index == 1 { 3000 } else { 1 });
    }
    stream::Page {
        session: "native-journal".into(),
        cursor: 2,
        captured_us: 110,
        gap: false,
        events: vec![event],
    }
}
fn registry() -> Registry {
    let mut registry = Registry::default();
    registry
        .reconcile(ProviderBatch {
            scope: "host-controllers".into(),
            session: "provider".into(),
            revision: 1,
            complete: true,
            observations: vec![Observation {
                key: PHYSICAL.into(),
                name: "Physical controller".into(),
                resource_type: ResourceType::HostController,
                evidence: vec![],
                attributes: BTreeMap::new(),
                diagnostics: vec![],
                route: Some(Route {
                    id: format!("hid:{PHYSICAL}"),
                    transport: Transport::Hid,
                    provider_generation: "1".into(),
                    attributes: BTreeMap::new(),
                    baseline_ready: true,
                    writable: false,
                    priority: 1,
                }),
                ports: Some(
                    PortUpdate::complete(crate::profile_resource_ports::controls(&controls()))
                        .unwrap(),
                ),
            }],
        })
        .unwrap();
    registry
}
fn fixture() -> Fixture {
    let registry = Arc::new(Mutex::new(registry()));
    let resource = registry.lock().unwrap().list(0, 1).unwrap().resources[0].clone();
    let journal = Arc::new(Mutex::new(vec![]));
    let calls = Arc::new(AtomicUsize::new(0));
    let epoch = Arc::new(AtomicU64::new(1));
    let inventory = registry.clone();
    let validation = registry.clone();
    let pages = journal.clone();
    let page_calls = calls.clone();
    let capture = Capture {
        inventory: Arc::new(move |request| {
            let registry = inventory.lock().unwrap();
            match request["command"].as_str().unwrap() {
                "profile_resources_inspect" => Ok(serde_json::to_value(registry.inspect(
                    request["resourceId"].as_str().unwrap(),
                    request["offset"].as_u64().unwrap() as usize,
                    request["limit"].as_u64().unwrap() as usize,
                )?)
                .unwrap()),
                "profile_resources_token" => Ok(
                    json!({"resourceToken":registry.token(request["resourceId"].as_str().unwrap(),request["portId"].as_str().unwrap(),Access::Read)?}),
                ),
                _ => panic!("An input learner attempted a non-read command."),
            }
        }),
        snapshot: Arc::new(|ids| {
            assert_eq!(ids, [PHYSICAL]);
            Ok(snapshot())
        }),
        page: Arc::new(move |session, cursor| {
            assert_eq!(session, "native-journal");
            page_calls.fetch_add(1, Ordering::AcqRel);
            let mut pages = pages.lock().unwrap();
            if pages.is_empty() {
                stream::Page {
                    session: session.into(),
                    cursor,
                    captured_us: 110,
                    gap: false,
                    events: vec![],
                }
            } else {
                pages.remove(0)
            }
        }),
        validate: Arc::new(move |token| {
            Ok(validation
                .lock()
                .unwrap()
                .validate_token(token, &token.port_id, token.access)?
                .route)
        }),
    };
    Fixture {
        runtime: Runtime::with_capture(capture, epoch.clone()),
        registry,
        journal,
        calls,
        epoch,
        resource: resource.id,
        generation: resource.generation,
        capability: resource.capability_revision,
    }
}
fn start(fixture: &Fixture) -> Value {
    fixture
        .runtime
        .start(&fixture.resource, &fixture.generation, &fixture.capability)
        .unwrap()
}
fn wait(fixture: &Fixture, id: &str, phase: &str) -> Value {
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        let status = fixture.runtime.status(id).unwrap();
        if status["phase"] == phase {
            return status;
        }
        assert!(
            Instant::now() < until,
            "Learning phase never became {phase}: {status}"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn worker_learns_from_native_journal_without_status_pumping_or_profile_effects() {
    let fixture = fixture();
    assert_eq!(fixture.calls.load(Ordering::Acquire), 0);
    fixture.journal.lock().unwrap().push(changed(&[0]));
    let started = start(&fixture);
    let id = started["learnId"].as_str().unwrap();
    assert_eq!(started["phase"], "listening");
    assert_eq!(started["untilUs"], "30000100");
    let until = Instant::now() + Duration::from_secs(3);
    while fixture.calls.load(Ordering::Acquire) == 0 {
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(5));
    }
    let status = wait(&fixture, id, "learned");
    assert_eq!(status["schemaVersion"], 1);
    assert_eq!(status["resourceId"], fixture.resource);
    assert_eq!(status["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(
        status["candidates"][0]["descriptor"]["id"],
        controls()[0].id
    );
    assert_eq!(status["candidates"][0]["current"], true);
    assert!(sha256(
        status["candidates"][0]["contractHash"].as_str().unwrap()
    ));
    let after = fixture.calls.load(Ordering::Acquire);
    for _ in 0..10 {
        fixture.runtime.status(id).unwrap();
    }
    assert_eq!(fixture.calls.load(Ordering::Acquire), after);
}
#[test]
fn deliberate_axis_hat_changes_and_ambiguity_return_only_exact_candidates() {
    for indices in [vec![1], vec![2], vec![0, 1, 2]] {
        let fixture = fixture();
        fixture.journal.lock().unwrap().push(changed(&indices));
        let status = start(&fixture);
        let status = wait(&fixture, status["learnId"].as_str().unwrap(), "learned");
        assert_eq!(
            status["candidates"].as_array().unwrap().len(),
            indices.len()
        );
        if indices.len() > 1 {
            assert!(status["reason"].as_str().unwrap().contains("Several"));
        }
    }
}
#[test]
fn gaps_disconnect_generation_changes_native_timeout_and_control_epoch_stop_without_candidates() {
    for mode in 0..5 {
        let fixture = fixture();
        let mut page = changed(&[0]);
        match mode {
            0 => page.gap = true,
            1 => page.events[0].reason = stream::Reason::Disconnect,
            2 => page.events[0].generation = 2,
            3 => {
                page.events.clear();
                page.captured_us = 30_000_100;
            }
            _ => page.session = "replaced".into(),
        };
        fixture.journal.lock().unwrap().push(page);
        let status = start(&fixture);
        let status = wait(&fixture, status["learnId"].as_str().unwrap(), "stopped");
        assert!(status["candidates"].as_array().unwrap().is_empty());
    }
    let fixture = fixture();
    let status = start(&fixture);
    fixture.epoch.fetch_add(1, Ordering::AcqRel);
    let status = wait(&fixture, status["learnId"].as_str().unwrap(), "stopped");
    assert!(status["reason"]
        .as_str()
        .unwrap()
        .contains("control changed"));
}
#[test]
fn cancel_replacement_invalidation_and_shutdown_isolate_cached_results() {
    let fixture = fixture();
    let first = start(&fixture);
    let first_id = first["learnId"].as_str().unwrap();
    assert!(fixture.runtime.status("forged").is_err());
    assert!(fixture.runtime.cancel("forged").is_err());
    let status = fixture.runtime.cancel(first_id).unwrap();
    assert_eq!(status["phase"], "stopped");
    let second = start(&fixture);
    assert_ne!(first["learnId"], second["learnId"]);
    assert!(fixture.runtime.status(first_id).is_err());
    fixture
        .runtime
        .invalidate("Explicit native Apply invalidated learning.");
    let status = fixture
        .runtime
        .status(second["learnId"].as_str().unwrap())
        .unwrap();
    assert_eq!(status["phase"], "stopped");
    assert!(status["candidates"].as_array().unwrap().is_empty());
    fixture.runtime.shutdown();
    assert!(fixture
        .runtime
        .start(&fixture.resource, &fixture.generation, &fixture.capability)
        .is_err());
}
#[test]
fn stale_inventory_missing_baselines_and_nonphysical_or_remembered_ports_are_rejected() {
    let fixture = fixture();
    for (id, generation, hash) in [
        (
            fixture.resource.as_str(),
            "wrong",
            fixture.capability.as_str(),
        ),
        (fixture.resource.as_str(), "1", "sha256:bad"),
        ("", fixture.generation.as_str(), fixture.capability.as_str()),
        (fixture.resource.as_str(), "2", fixture.capability.as_str()),
    ] {
        assert!(fixture.runtime.start(id, generation, hash).is_err());
    }
    let mut capture = Capture {
        inventory: fixture.runtime.capture.inventory.clone(),
        snapshot: Arc::new(|_| {
            let mut snapshot = snapshot();
            snapshot.sources[0].baseline.values.clear();
            Ok(snapshot)
        }),
        page: fixture.runtime.capture.page.clone(),
        validate: fixture.runtime.capture.validate.clone(),
    };
    assert!(capture
        .prepare(&fixture.resource, &fixture.generation, &fixture.capability)
        .is_err());
    capture.snapshot = Arc::new(|_| {
        let mut snapshot = snapshot();
        snapshot.sources[0].controls.pop();
        Ok(snapshot)
    });
    assert!(capture
        .prepare(&fixture.resource, &fixture.generation, &fixture.capability)
        .is_err());
    let original = fixture.runtime.capture.inventory.clone();
    for mode in 0..6 {
        let original = original.clone();
        capture.inventory = Arc::new(move |request| {
            let mut value = original(request)?;
            match mode {
                0 => value["resource"]["resourceType"] = json!("osg-module"),
                1 => value["resource"]["availability"] = json!("offline"),
                2 => value["resource"]["connections"][0]["route"]["transport"] = json!("usb"),
                3 => value["resource"]["connections"][0]["route"]["id"] = json!("hid:virtual"),
                4 => value["ports"][0]["current"] = json!(false),
                _ => value["total"] = json!(0),
            }
            Ok(value)
        });
        assert!(capture
            .prepare(&fixture.resource, &fixture.generation, &fixture.capability)
            .is_err());
    }
    fixture
        .registry
        .lock()
        .unwrap()
        .reconcile(ProviderBatch {
            scope: "host-controllers".into(),
            session: "provider".into(),
            revision: 2,
            complete: true,
            observations: vec![],
        })
        .unwrap();
    assert!(fixture
        .runtime
        .capture
        .prepare(&fixture.resource, &fixture.generation, &fixture.capability)
        .is_err());
}

fn capture_clone(capture: &Capture) -> Capture {
    Capture {
        inventory: capture.inventory.clone(),
        snapshot: capture.snapshot.clone(),
        page: capture.page.clone(),
        validate: capture.validate.clone(),
    }
}
pub(super) fn manager_fixture(epoch:Arc<AtomicU64>,gate:Option<Arc<std::sync::Barrier>>)->(Runtime,Value){
    let fixture=fixture();let mut capture=capture_clone(&fixture.runtime.capture);
    if let Some(gate)=gate{let snapshot=capture.snapshot.clone();capture.snapshot=Arc::new(move|ids|{gate.wait();gate.wait();snapshot(ids)});}
    let source=json!({"resourceId":fixture.resource,"generation":fixture.generation,"capabilityRevision":fixture.capability});
    (Runtime::with_capture(capture,epoch),source)
}

#[test]
fn catalogue_pages_and_read_tokens_must_remain_exact_before_any_worker_is_started() {
    let fixture = fixture();
    let mut capture = capture_clone(&fixture.runtime.capture);
    let original = capture.inventory.clone();
    capture.inventory = Arc::new(move |mut request| {
        if request["command"] == "profile_resources_inspect" {
            request["limit"] = json!(1);
        }
        original(request)
    });
    let (pin, _) = capture
        .prepare(&fixture.resource, &fixture.generation, &fixture.capability)
        .unwrap();
    assert_eq!(pin.ports.len(), 3);
    for mode in 0..8 {
        let mut changed = pin.clone();
        match mode {
            0 => changed.token.resource_id = "other".into(),
            1 => changed.token.generation = "2".into(),
            2 => changed.token.capability_revision = "different".into(),
            3 => changed.token.route_id = "hid:other".into(),
            4 => changed.token.provider_generation = "2".into(),
            5 => changed.token.access = Access::Invoke,
            6 => changed.token.port_id = "remembered".into(),
            _ => changed.token.contract_hash = "changed".into(),
        };
        assert!(capture.validate_pin(&changed).is_err());
    }
    let original = capture.inventory.clone();
    capture.inventory = Arc::new(move |request| {
        let offset = request["offset"].as_u64().unwrap_or(0);
        let mut page = original(request)?;
        if offset > 0 {
            page["revision"] = json!("changed");
        }
        Ok(page)
    });
    assert!(capture
        .prepare(&fixture.resource, &fixture.generation, &fixture.capability)
        .is_err());
    let mut capture = capture_clone(&fixture.runtime.capture);
    capture.validate = Arc::new(|_| Err("Cached HID route expired.".into()));
    assert!(capture
        .prepare(&fixture.resource, &fixture.generation, &fixture.capability)
        .is_err());
}

#[test]
fn learned_candidates_are_withdrawn_after_catalogue_loss_and_wall_clock_expiry() {
    for expires in [false, true] {
        let fixture = fixture();
        fixture.journal.lock().unwrap().push(changed(&[0]));
        let status = start(&fixture);
        let id = status["learnId"].as_str().unwrap();
        wait(&fixture, id, "learned");
        if expires {
            fixture
                .runtime
                .shared
                .0
                .lock()
                .unwrap()
                .entry
                .as_mut()
                .unwrap()
                .started = Instant::now() - Duration::from_secs(31);
        } else {
            fixture
                .registry
                .lock()
                .unwrap()
                .reconcile(ProviderBatch {
                    scope: "host-controllers".into(),
                    session: "provider".into(),
                    revision: 2,
                    complete: true,
                    observations: vec![],
                })
                .unwrap();
        }
        let status = wait(&fixture, id, "stopped");
        assert!(status["candidates"].as_array().unwrap().is_empty());
    }
}

#[test]
fn a_late_worker_read_cannot_restore_a_cancelled_or_replaced_capture() {
    let fixture = fixture();
    let gate = Arc::new(std::sync::Barrier::new(2));
    let first = Arc::new(AtomicBool::new(true));
    let mut capture = capture_clone(&fixture.runtime.capture);
    let original = capture.page.clone();
    let reader_gate = gate.clone();
    capture.page = Arc::new(move |session, cursor| {
        if first.swap(false, Ordering::AcqRel) {
            reader_gate.wait();
            reader_gate.wait();
        }
        original(session, cursor)
    });
    let runtime = Runtime::with_capture(capture, fixture.epoch.clone());
    fixture.journal.lock().unwrap().push(changed(&[0]));
    fixture.journal.lock().unwrap().push(changed(&[1]));
    let first = runtime
        .start(&fixture.resource, &fixture.generation, &fixture.capability)
        .unwrap();
    gate.wait();
    assert_eq!(
        runtime.cancel(first["learnId"].as_str().unwrap()).unwrap()["phase"],
        "stopped"
    );
    let second = runtime
        .start(&fixture.resource, &fixture.generation, &fixture.capability)
        .unwrap();
    gate.wait();
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        let status = runtime.status(second["learnId"].as_str().unwrap()).unwrap();
        if status["phase"] == "learned" {
            assert_eq!(
                status["candidates"][0]["descriptor"]["id"],
                controls()[1].id
            );
            break;
        }
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(5));
    }
    assert!(runtime.status(first["learnId"].as_str().unwrap()).is_err());
}

#[test]
fn control_invalidation_while_preparing_a_baseline_prevents_late_start() {
    let fixture = fixture();
    let gate = Arc::new(std::sync::Barrier::new(2));
    let mut capture = capture_clone(&fixture.runtime.capture);
    let reader_gate = gate.clone();
    capture.snapshot = Arc::new(move |_| {
        reader_gate.wait();
        reader_gate.wait();
        Ok(snapshot())
    });
    let runtime = Arc::new(Runtime::with_capture(capture, fixture.epoch.clone()));
    let starter = runtime.clone();
    let (id, generation, capability) = (
        fixture.resource.clone(),
        fixture.generation.clone(),
        fixture.capability.clone(),
    );
    let starting = thread::spawn(move || starter.start(&id, &generation, &capability));
    gate.wait();
    runtime.invalidate("The native control epoch invalidated this pending learner.");
    gate.wait();
    assert!(starting
        .join()
        .unwrap()
        .unwrap_err()
        .contains("cancelled during native capture"));
    assert!(runtime.shared.0.lock().unwrap().entry.is_none());
}
