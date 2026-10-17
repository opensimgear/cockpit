use super::*;
use std::time::{Duration, Instant};
const SERIAL: &str = "00112233445566778899aabb";
fn port(path: &str, strict: bool) -> PortInfo {
    PortInfo {
        path: path.into(),
        serial_number: Some(SERIAL.into()),
        vendor_id: Some("1209".into()),
        product_id: Some("0001".into()),
        manufacturer: strict.then(|| "OpenSimGear".into()),
        friendly_name: strict.then(|| "Device Template v0.1".into()),
    }
}
fn route() -> UsbRouteToken {
    UsbRouteToken {
        module_id: format!("4f534744{SERIAL}"),
        serial: SERIAL.into(),
        connection_generation: 1,
        boot: 10,
        configuration_generation: 1,
    }
}
#[derive(Default)]
struct Fake {
    snapshot: Mutex<UsbSnapshot>,
    calls: Mutex<Vec<String>>,
    stopped: AtomicBool,
    hold_refresh: Mutex<
        Option<(
            std::sync::mpsc::SyncSender<()>,
            std::sync::mpsc::Receiver<()>,
        )>,
    >,
    hold_shutdown: Mutex<
        Option<(
            std::sync::mpsc::SyncSender<()>,
            std::sync::mpsc::Receiver<()>,
        )>,
    >,
}
impl Endpoint for Fake {
    fn snapshot(&self) -> UsbSnapshot {
        self.snapshot.lock().unwrap().clone()
    }
    fn request(&self, request: UsbRequest) -> Result<UsbSnapshot, String> {
        if matches!(&request, UsbRequest::Refresh) {
            if let Some((entered, resume)) = self.hold_refresh.lock().unwrap().take() {
                entered.send(()).unwrap();
                resume
                    .recv_timeout(Duration::from_secs(2))
                    .map_err(|_| "fixture request timeout")?;
            }
        }
        let mut snapshot = self.snapshot.lock().unwrap();
        if self.stopped.load(Ordering::Acquire) {
            return Err("stopped".into());
        }
        let mut calls = self.calls.lock().unwrap();
        let check = |expected: &UsbRouteToken, snapshot: &UsbSnapshot| {
            if snapshot.route.as_ref() == Some(expected) {
                Ok(())
            } else {
                Err("stale route".to_owned())
            }
        };
        match request {
            UsbRequest::Connect { port } | UsbRequest::ConnectExplicit { port } => {
                calls.push(
                    if module_usb::candidate(&port) {
                        "connect"
                    } else {
                        "explicit"
                    }
                    .into(),
                );
                snapshot.connected = true;
                snapshot.port = Some(port.path);
                snapshot.route = Some(route());
                snapshot.identity = Some(crate::module_console::UsbIdentity {
                    model: "device-template".into(),
                    board: "test".into(),
                    profile: "Fixture".into(),
                    module_id: route().module_id,
                    serial: SERIAL.into(),
                    protocol: 3,
                    button_count: 1,
                    axis_count: 0,
                    hat_count: 0,
                });
            }
            UsbRequest::Disconnect => {
                calls.push("disconnect".into());
                *snapshot = offline(snapshot.clone());
            }
            UsbRequest::Refresh => {
                calls.push("refresh".into());
            }
            UsbRequest::ConfigRead { route } => {
                check(&route, &snapshot)?;
                calls.push("read".into());
            }
            UsbRequest::Claim { route, client_id } => {
                check(&route, &snapshot)?;
                snapshot.claimed_by = Some(client_id);
                calls.push("claim".into());
            }
            UsbRequest::Release { route, client_id } => {
                check(&route, &snapshot)?;
                if snapshot.claimed_by.as_ref() != Some(&client_id) {
                    return Err("wrong writer".into());
                }
                snapshot.claimed_by = None;
                calls.push("release".into());
            }
            UsbRequest::ConfigChange {
                route,
                client_id,
                change,
            } => {
                check(&route, &snapshot)?;
                if snapshot.claimed_by.as_ref() != Some(&client_id) {
                    return Err("claim first".into());
                }
                calls.push(
                    match change {
                        ConfigChange::Apply { config, save } => {
                            snapshot.config = Some(config);
                            if save {
                                "apply-save"
                            } else {
                                "apply"
                            }
                        }
                        ConfigChange::Save => "save",
                        ConfigChange::Discard => "discard",
                        ConfigChange::Reset => "reset",
                    }
                    .into(),
                );
            }
            UsbRequest::Action { route, action } => {
                check(&route, &snapshot)?;
                calls.push(
                    match action {
                        ActionRequest::Describe => {
                            snapshot.actions =
                                Some(json!({"status":"ready","route":route,"actions":[]}));
                            "describe"
                        }
                        ActionRequest::State => "state",
                        ActionRequest::Invoke {
                            catalogue_revision,
                            action_id,
                            values,
                        } => {
                            if catalogue_revision != 1
                                || action_id != "led.set"
                                || values != vec![json!(true)]
                            {
                                return Err("invalid arguments".into());
                            }
                            "invoke"
                        }
                        _ => return Err("owned action is not an operator command".into()),
                    }
                    .into(),
                );
            }
            UsbRequest::EventsState { route } => {
                check(&route, &snapshot)?;
                calls.push("events-state".into());
                snapshot.events = Some(json!({"route":route,"head":0}));
            }
            UsbRequest::EventsRead {
                route,
                stream,
                after,
            } => {
                check(&route, &snapshot)?;
                if stream != 1 || after > 10 {
                    return Err("stale stream".into());
                }
                calls.push("events-read".into());
            }
        }
        Ok(snapshot.clone())
    }
    fn shutdown(&self) {
        if let Some((entered, resume)) = self.hold_shutdown.lock().unwrap().take() {
            entered.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        self.stopped.store(true, Ordering::Release);
    }
}
fn fixture(
    ports: Vec<PortInfo>,
    native: bool,
) -> (Fleet, Arc<Mutex<Vec<PortInfo>>>, Arc<Mutex<Vec<Arc<Fake>>>>) {
    let observed = Arc::new(Mutex::new(ports));
    let read = observed.clone();
    let created = Arc::new(Mutex::new(Vec::new()));
    let save = created.clone();
    (
        Fleet::with_create(
            Arc::new(move || Ok(read.lock().unwrap().clone())),
            native,
            Arc::new(move || {
                let fake = Arc::new(Fake::default());
                save.lock().unwrap().push(fake.clone());
                fake
            }),
        ),
        observed,
        created,
    )
}
fn wait(test: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !test() {
        assert!(Instant::now() < deadline, "fixture timeout");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn connect(fleet: &Fleet, path: &str) -> Value {
    fleet
        .request(&json!({"command":"module_usb_connect","port":path}))
        .unwrap()["module"]
        .clone()
}
/// A handshaken but not capability-ready module for Manager ownership tests.
pub(crate) fn manager_fixture() -> Fleet {
    let (fleet, _, _) = fixture(vec![port("COM8", false)], false);
    connect(&fleet, "COM8");
    fleet
}
fn operation(fleet: &Fleet, key: &Value, command: &str, fields: Value) -> Result<Value, String> {
    let mut request = fields;
    request["command"] = json!(command);
    request["key"] = key.clone();
    request["clientId"] = json!("fixture-operator");
    fleet.request(&request)
}

#[test]
fn enumeration_is_nonopening_and_cu_tty_aliases_have_one_preferred_worker() {
    let (fleet, _, created) = fixture(
        vec![
            port("/dev/tty.usbmodemA", true),
            port("/dev/cu.usbmodemA", true),
        ],
        true,
    );
    let ports = fleet
        .request(&json!({"command":"module_usb_ports"}))
        .unwrap();
    assert_eq!(ports["ports"].as_array().unwrap().len(), 1);
    assert_eq!(ports["ports"][0]["port"]["path"], "/dev/cu.usbmodemA");
    assert!(created.lock().unwrap().is_empty());
    fleet.scan().unwrap();
    wait(|| {
        created
            .lock()
            .unwrap()
            .first()
            .is_some_and(|s| s.calls.lock().unwrap().contains(&"describe".into()))
    });
    assert_eq!(created.lock().unwrap().len(), 1);
    fleet.scan().unwrap();
    assert_eq!(created.lock().unwrap().len(), 1);
    assert_eq!(fleet.snapshots().len(), 1);
}

#[test]
fn unbranded_plausible_ports_need_explicit_connection_and_fresh_writer_route() {
    let (fleet, _, created) = fixture(vec![port("COM8", false)], true);
    fleet.scan().unwrap();
    assert!(created.lock().unwrap().is_empty());
    let module = connect(&fleet, "COM8");
    assert_eq!(
        created.lock().unwrap()[0].calls.lock().unwrap().as_slice(),
        ["explicit"]
    );
    let key = &module["key"];
    let route = module["snapshot"]["route"].clone();
    assert!(operation(
        &fleet,
        key,
        "module_usb_config_apply",
        json!({"route":route,"config":{"name":"Changed"}})
    )
    .is_err());
    operation(&fleet, key, "module_usb_claim", json!({"route":route})).unwrap();
    for command in [
        "module_usb_config_read",
        "module_usb_config_save",
        "module_usb_config_discard",
        "module_usb_config_reset",
    ] {
        operation(&fleet, key, command, json!({"route":route})).unwrap();
    }
    let changed = operation(
        &fleet,
        key,
        "module_usb_config_apply",
        json!({"route":route,"config":{"name":"Changed"},"save":true}),
    )
    .unwrap();
    assert_eq!(changed["module"]["snapshot"]["config"]["name"], "Changed");
    operation(&fleet, key, "module_usb_release", json!({"route":route})).unwrap();
    let mut stale = route;
    stale["boot"] = json!(99);
    assert!(
        operation(&fleet, key, "module_usb_claim", json!({"route":stale}))
            .unwrap_err()
            .contains("stale")
    );
}

#[test]
fn duplicate_serials_block_auto_opening_and_parallel_physical_routes() {
    let (fleet, ports, created) = fixture(vec![port("COM8", false)], true);
    let module = connect(&fleet, "COM8");
    ports.lock().unwrap().push(port("COM9", true));
    fleet.scan().unwrap();
    assert_eq!(created.lock().unwrap().len(), 1);
    assert!(fleet
        .snapshots()
        .iter()
        .all(|s| !s.admissible && s.snapshot.route.is_none() && !s.snapshot.connected));
    for path in ["COM8", "COM9"] {
        assert!(fleet
            .request(&json!({"command":"module_usb_connect","port":path}))
            .is_err());
    }
    assert!(operation(
        &fleet,
        &module["key"],
        "module_usb_claim",
        json!({"route":module["snapshot"]["route"]})
    )
    .is_err());
    let status = operation(&fleet, &module["key"], "module_usb_status", json!({})).unwrap();
    assert!(!status["module"]["admissible"].as_bool().unwrap());
    operation(&fleet, &module["key"], "module_usb_disconnect", json!({})).unwrap();
    ports.lock().unwrap().retain(|p| p.path == "COM9");
    fleet.scan().unwrap();
    wait(|| {
        !fleet.state.lock().unwrap().slots[module["key"].as_str().unwrap()]
            .closing
            .load(Ordering::Acquire)
    });
    connect(&fleet, "COM9");
    assert_eq!(created.lock().unwrap().len(), 2);
}

#[test]
fn absent_endpoints_close_workers_and_keep_only_offline_metadata() {
    let (fleet, ports, created) = fixture(vec![port("COM8", false)], false);
    let module = connect(&fleet, "COM8");
    let original = created.lock().unwrap()[0].clone();
    ports.lock().unwrap().clear();
    fleet.scan().unwrap();
    wait(|| {
        original.stopped.load(Ordering::Acquire)
            && !fleet.state.lock().unwrap().slots[module["key"].as_str().unwrap()]
                .closing
                .load(Ordering::Acquire)
    });
    let slots = fleet.snapshots();
    assert!(!slots[0].present);
    assert!(!slots[0].snapshot.connected);
    assert!(slots[0].snapshot.route.is_none());
    assert!(slots[0].snapshot.identity.is_some());
    assert!(operation(
        &fleet,
        &module["key"],
        "module_usb_claim",
        json!({"route":module["snapshot"]["route"]})
    )
    .is_err());
    ports.lock().unwrap().push(port("COM8", false));
    connect(&fleet, "COM8");
    assert_eq!(created.lock().unwrap().len(), 2);
    fleet.shutdown();
    assert!(fleet
        .request(&json!({"command":"module_usb_list"}))
        .is_err());
}

#[test]
fn operator_action_and_event_commands_are_exact_and_never_accept_ownership_secrets() {
    let (fleet, _, _) = fixture(vec![port("COM8", false)], false);
    let module = connect(&fleet, "COM8");
    let key = &module["key"];
    let route = &module["snapshot"]["route"];
    operation(&fleet, key, "module_usb_status", json!({})).unwrap();
    for command in [
        "module_usb_actions_describe",
        "module_usb_actions_state",
        "module_usb_events_state",
    ] {
        operation(&fleet, key, command, json!({"route":route})).unwrap();
    }
    operation(
        &fleet,
        key,
        "module_usb_actions_invoke",
        json!({"route":route,"catalogueRevision":1,"actionId":"led.set","values":[true]}),
    )
    .unwrap();
    operation(
        &fleet,
        key,
        "module_usb_events_read",
        json!({"route":route,"stream":1,"after":0}),
    )
    .unwrap();
    assert!(operation(&fleet,key,"module_usb_actions_invoke",json!({"route":route,"catalogueRevision":1,"actionId":"led.set","values":[true],"ownershipToken":"secret"})).is_err());
    assert!(operation(&fleet,key,"module_usb_actions_invoke",json!({"route":route,"catalogueRevision":1,"actionId":"led.set","values":[true,true,true,true,true]})).is_err());
    assert!(operation(
        &fleet,
        key,
        "module_usb_events_read",
        json!({"route":route,"stream":0,"after":0})
    )
    .is_err());
    assert!(!fleet
        .request(&json!({"command":"module_usb_list"}))
        .unwrap()
        .to_string()
        .contains("secret"));
}

#[test]
fn malformed_requests_and_ambiguous_unobserved_ports_never_open_services() {
    let (fleet, _, created) = fixture(vec![port("COM8", false)], false);
    for request in [
        json!(null),
        json!({"command":"module_usb_unknown"}),
        json!({"command":"module_usb_ports","extra":true}),
        json!({"command":"module_usb_connect","port":"COM999"}),
        json!({"command":"module_usb_connect","port":42}),
        json!({"command":"module_usb_connect","port":"COM8","key":"wrong-key"}),
        json!({"command":"module_usb_list","offset":-1}),
        json!({"command":"module_usb_list","limit":0}),
        json!({"command":"module_usb_list","limit":33}),
    ] {
        assert!(fleet.request(&request).is_err());
    }
    assert!(created.lock().unwrap().is_empty());
    assert!(deduplicate(
        (1..=33)
            .map(|index| {
                let mut p = port(&format!("COM{index}"), true);
                p.serial_number = Some(format!("{index:024x}"));
                p
            })
            .collect()
    )
    .is_err());
}

#[test]
fn explicit_disconnect_stays_disconnected_during_background_scan() {
    let (fleet, _, created) = fixture(vec![port("COM8", true)], true);
    let module = connect(&fleet, "COM8");
    operation(&fleet, &module["key"], "module_usb_disconnect", json!({})).unwrap();
    fleet.scan().unwrap();
    assert!(!fleet.snapshots()[0].snapshot.connected);
    assert_eq!(created.lock().unwrap().len(), 1);
    assert!(fleet.snapshots()[0]
        .diagnostics
        .iter()
        .any(|d| d.contains("Disconnected explicitly")));
    connect(&fleet, "COM8");
    assert!(fleet.snapshots()[0].snapshot.connected);
}

#[test]
fn previously_verified_explicit_selection_reconnects_after_com_renumbering() {
    let (fleet, ports, created) = fixture(vec![port("COM8", false)], true);
    fleet.scan().unwrap();
    assert!(created.lock().unwrap().is_empty());
    let original = connect(&fleet, "COM8");
    ports.lock().unwrap().clear();
    fleet.scan().unwrap();
    wait(|| {
        !fleet.state.lock().unwrap().slots[original["key"].as_str().unwrap()]
            .closing
            .load(Ordering::Acquire)
    });
    ports.lock().unwrap().push(port("COM9", false));
    fleet.scan().unwrap();
    wait(|| {
        let workers = created.lock().unwrap();
        workers.len() == 2 && workers[1].snapshot().connected
    });
    let new = fleet
        .snapshots()
        .into_iter()
        .find(|s| s.port.path == "COM9")
        .unwrap();
    assert!(new.automatic && new.admissible);
    assert_ne!(original["key"], json!(new.key));
    assert_eq!(new.snapshot.identity.unwrap().module_id, route().module_id);
    let other = PortInfo {
        serial_number: Some("00112233445566778899aacc".into()),
        ..port("COM10", false)
    };
    ports.lock().unwrap().push(other);
    fleet.scan().unwrap();
    assert_eq!(
        created.lock().unwrap().len(),
        2,
        "a new generic serial must not auto-probe"
    );
}

#[test]
fn list_pages_omit_large_caches_and_status_retains_one_full_snapshot() {
    let (fleet, ports, created) = fixture(vec![port("COM8", false)], false);
    let module = connect(&fleet, "COM8");
    let fake = created.lock().unwrap()[0].clone();
    fake.snapshot.lock().unwrap().config = Some(json!({"data":"x".repeat(4096)}));
    fake.snapshot.lock().unwrap().actions =
        Some(json!({"status":"unsupported","route":route(),"boot":10}));
    ports.lock().unwrap().push(PortInfo {
        serial_number: Some("00112233445566778899aacc".into()),
        ..port("COM9", false)
    });
    fleet.scan().unwrap();
    let first = fleet
        .request(&json!({"command":"module_usb_list","offset":0,"limit":1}))
        .unwrap();
    assert_eq!(first["total"], 2);
    assert_eq!(first["modules"].as_array().unwrap().len(), 1);
    assert!(first["modules"][0]["snapshot"].get("config").is_none());
    assert!(first["modules"][0]["snapshot"].get("actions").is_none());
    let second = fleet
        .request(&json!({"command":"module_usb_list","offset":1,"limit":1}))
        .unwrap();
    assert_ne!(first["modules"][0]["key"], second["modules"][0]["key"]);
    let status = operation(&fleet, &module["key"], "module_usb_status", json!({})).unwrap();
    assert_eq!(
        status["module"]["snapshot"]["config"]["data"]
            .as_str()
            .unwrap()
            .len(),
        4096
    );
    assert_eq!(
        status["module"]["snapshot"]["actions"]["status"],
        "unsupported"
    );
    assert!(bounded_reply(json!({"x":"x".repeat(MAX_REPLY_BYTES)})).is_err());
}

#[test]
fn enumeration_failure_withdraws_routes_without_treating_failure_as_removal() {
    let (mut fleet, _, created) = fixture(vec![port("COM8", false)], false);
    let module = connect(&fleet, "COM8");
    let failed = Arc::new(AtomicBool::new(true));
    let read = failed.clone();
    fleet.serial = Arc::new(move || {
        if read.load(Ordering::Acquire) {
            Err("é".repeat(1024))
        } else {
            Ok(vec![port("COM8", false)])
        }
    });
    assert!(fleet.scan().unwrap_err().len() <= 512);
    let unavailable = fleet.snapshots().remove(0);
    assert!(!unavailable.present && !unavailable.admissible && !unavailable.snapshot.connected);
    assert!(unavailable.snapshot.route.is_none() && unavailable.snapshot.identity.is_some());
    assert!(!created.lock().unwrap()[0].stopped.load(Ordering::Acquire));
    assert!(operation(
        &fleet,
        &module["key"],
        "module_usb_claim",
        json!({"route":module["snapshot"]["route"]})
    )
    .is_err());
    failed.store(false, Ordering::Release);
    fleet.scan().unwrap();
    assert!(fleet.snapshots()[0].admissible && fleet.snapshots()[0].snapshot.connected);
    assert_eq!(created.lock().unwrap().len(), 1);
}

#[test]
fn a_pending_request_rejects_parallel_mutation_without_replay() {
    let (fleet, _, created) = fixture(vec![port("COM8", false)], false);
    let module = connect(&fleet, "COM8");
    let fake = created.lock().unwrap()[0].clone();
    let (entered, observed) = std::sync::mpsc::sync_channel(1);
    let (resume, blocked) = std::sync::mpsc::sync_channel(1);
    *fake.hold_refresh.lock().unwrap() = Some((entered, blocked));
    let fleet = Arc::new(fleet);
    let background = fleet.clone();
    let key = module["key"].clone();
    let pending =
        std::thread::spawn(move || operation(&background, &key, "module_usb_status", json!({})));
    observed.recv_timeout(Duration::from_secs(1)).unwrap();
    let error = operation(
        &fleet,
        &module["key"],
        "module_usb_claim",
        json!({"route":module["snapshot"]["route"]}),
    )
    .unwrap_err();
    assert!(error.contains("busy") && error.contains("No command was replayed"));
    resume.send(()).unwrap();
    pending.join().unwrap().unwrap();
    assert!(!fake.calls.lock().unwrap().contains(&"claim".to_owned()));
    operation(
        &fleet,
        &module["key"],
        "module_usb_claim",
        json!({"route":module["snapshot"]["route"]}),
    )
    .unwrap();
    assert_eq!(
        fake.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.as_str() == "claim")
            .count(),
        1
    );
}

#[test]
fn matching_unsupported_capability_markers_do_not_repeat_automatic_probes() {
    let (fleet, _, created) = fixture(vec![port("COM8", true)], true);
    connect(&fleet, "COM8");
    let fake = created.lock().unwrap()[0].clone();
    {
        let mut state = fake.snapshot.lock().unwrap();
        let unsupported = json!({"boot":10,"route":route(),"status":"unsupported"});
        state.actions = Some(unsupported.clone());
        state.events = Some(unsupported);
    }
    let before = fake.calls.lock().unwrap().clone();
    fleet.scan().unwrap();
    assert_eq!(*fake.calls.lock().unwrap(), before);
    fake.snapshot.lock().unwrap().actions.as_mut().unwrap()["route"]["configurationGeneration"] =
        json!(0);
    fleet.scan().unwrap();
    wait(|| fake.calls.lock().unwrap().contains(&"describe".to_owned()));
}

#[test]
fn conflicting_serial_metadata_on_one_endpoint_never_opens_two_workers() {
    let (fleet, _, created) = fixture(
        vec![
            port("COM8", true),
            PortInfo {
                serial_number: Some("00112233445566778899aacc".into()),
                ..port("COM8", true)
            },
        ],
        true,
    );
    fleet.scan().unwrap();
    assert!(created.lock().unwrap().is_empty());
    assert!(fleet.snapshots().iter().all(|slot| !slot.admissible));
    assert!(fleet
        .request(&json!({"command":"module_usb_connect","port":"COM8"}))
        .is_err());
}

#[test]
fn retained_inventory_and_paged_reply_budgets_are_enforced() {
    let (fleet, ports, created) = fixture(vec![], false);
    for batch in 0..4 {
        *ports.lock().unwrap() = (1..=32)
            .map(|index| {
                let id = batch * 32 + index;
                PortInfo {
                    serial_number: Some(format!("{id:024x}")),
                    ..port(&format!("COM{id}"), false)
                }
            })
            .collect();
        fleet.scan().unwrap();
    }
    assert!(created.lock().unwrap().is_empty());
    assert_eq!(fleet.snapshots().len(), MAX_RETAINED);
    let page = fleet
        .request(&json!({"command":"module_usb_list","offset":96,"limit":32}))
        .unwrap();
    assert_eq!(page["total"], MAX_RETAINED);
    assert_eq!(page["modules"].as_array().unwrap().len(), 32);
    *ports.lock().unwrap() = vec![port("COM999", false)];
    assert!(fleet.scan().unwrap_err().contains("full"));
    assert!(fleet.snapshots().iter().all(|slot| !slot.admissible));
    let (large, _, _) = fixture(vec![port("COM8", false)], false);
    large.scan().unwrap();
    {
        let mut state = large.state.lock().unwrap();
        let slot = state.slots.values_mut().next().unwrap();
        slot.cached.error = Some("x".repeat(MAX_REPLY_BYTES));
    }
    assert!(large
        .request(&json!({"command":"module_usb_list"}))
        .is_err());
}

#[test]
fn a_scan_finishing_after_shutdown_cannot_adopt_or_open_endpoints() {
    let (mut fleet, _, created) = fixture(vec![], true);
    let (entered, observed) = std::sync::mpsc::sync_channel(1);
    let (resume, blocked) = std::sync::mpsc::sync_channel(1);
    let blocked = Mutex::new(Some((entered, blocked)));
    fleet.serial = Arc::new(move || {
        if let Some((entered, resume)) = blocked.lock().unwrap().take() {
            entered.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        Ok(vec![port("COM8", true)])
    });
    let fleet = Arc::new(fleet);
    let scan = fleet.clone();
    let pending = std::thread::spawn(move || scan.scan());
    observed.recv_timeout(Duration::from_secs(1)).unwrap();
    fleet.shutdown();
    resume.send(()).unwrap();
    assert!(pending.join().unwrap().is_err());
    assert!(fleet.snapshots().is_empty() && created.lock().unwrap().is_empty());
}

#[test]
fn a_retiring_usb_writer_remains_pinned_until_its_worker_has_closed() {
    let (fleet, ports, created) = fixture(vec![port("COM8", false)], false);
    let module = connect(&fleet, "COM8");
    operation(
        &fleet,
        &module["key"],
        "module_usb_claim",
        json!({"route":module["snapshot"]["route"]}),
    )
    .unwrap();
    assert_eq!(fleet.configuration_claims().len(), 1);
    let fake = created.lock().unwrap()[0].clone();
    let (entered, observed) = std::sync::mpsc::sync_channel(1);
    let (resume, blocked) = std::sync::mpsc::sync_channel(1);
    *fake.hold_shutdown.lock().unwrap() = Some((entered, blocked));
    ports.lock().unwrap().clear();
    fleet.scan().unwrap();
    observed.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(fleet.snapshots()[0].snapshot.route.is_none());
    assert!(fleet.snapshots()[0].snapshot.claimed_by.is_none());
    assert_eq!(
        fleet.configuration_claims().len(),
        1,
        "public route withdrawal must not authorize transfer while the old worker can still write"
    );
    resume.send(()).unwrap();
    wait(|| {
        !fleet.state.lock().unwrap().slots[module["key"].as_str().unwrap()]
            .closing
            .load(Ordering::Acquire)
    });
    assert!(fleet.configuration_claims().is_empty());
}

#[test]
fn native_profile_calls_are_typed_route_bound_and_never_open_or_persist() {
    let (fleet, _, created) = fixture(vec![port("COM8", false)], false);
    let module = connect(&fleet, "COM8");
    let key = module["key"].as_str().unwrap();
    fleet
        .request_native(key, UsbRequest::ConfigRead { route: route() })
        .unwrap();
    let mut old = route();
    old.boot += 1;
    assert!(fleet
        .request_native(key, UsbRequest::ConfigRead { route: old })
        .is_err());
    assert!(fleet.request_native(key, UsbRequest::Refresh).is_err());
    assert!(fleet
        .request_native(
            key,
            UsbRequest::ConfigChange {
                route: route(),
                client_id: "profile".into(),
                change: ConfigChange::Save
            }
        )
        .is_err());
    assert_eq!(
        *created.lock().unwrap()[0].calls.lock().unwrap(),
        vec!["explicit", "read"]
    );
    assert!(fleet
        .request(&json!({"command":"module_usb_actions_claim","key":key,"route":route()}))
        .is_err());
    fleet.shutdown();
    assert!(fleet
        .request_native(key, UsbRequest::ConfigRead { route: route() })
        .is_err());
}
#[test]
fn native_profile_call_uses_existing_busy_guard_without_replaying() {
    let (fleet, _, created) = fixture(vec![port("COM8", false)], false);
    let module = connect(&fleet, "COM8");
    let key = module["key"].as_str().unwrap().to_owned();
    let fleet = Arc::new(fleet);
    let (entered, notify) = std::sync::mpsc::sync_channel(1);
    let (resume, release) = std::sync::mpsc::channel();
    *created.lock().unwrap()[0].hold_refresh.lock().unwrap() = Some((entered, release));
    let worker = fleet.clone();
    let worker_key = key.clone();
    let pending = std::thread::spawn(move || {
        worker.request(&json!({"command":"module_usb_status","key":worker_key}))
    });
    notify.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(fleet
        .request_native(&key, UsbRequest::ConfigRead { route: route() })
        .unwrap_err()
        .contains("busy"));
    resume.send(()).unwrap();
    pending.join().unwrap().unwrap();
    assert_eq!(
        *created.lock().unwrap()[0].calls.lock().unwrap(),
        vec!["explicit", "refresh"]
    );
}
