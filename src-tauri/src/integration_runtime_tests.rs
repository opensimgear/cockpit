use super::*;
use std::{collections::BTreeMap, thread};

const A: &str = "stm32:00112233445566778899aabb";
const B: &str = "stm32:112233445566778899aabbcc";
const MODULE: &str = "00112233445566778899aabbccddeeff";

fn wait(label: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("Timed out waiting for {label}");
}
fn display(name: &str, cells: usize) -> Value {
    json!({"id":format!("display.{name}.set"),"endpoint":format!("display.{name}"),"label":name,"readable":true,"semantics":"setter",
    "parameters":[{"id":"text","label":"Text","type":"string","maxBytes":cells}],"display":{"columns":cells,"rows":1,"encoding":"ascii"}})
}
fn output(id: &str, role: &str, channel: &str, name: &str, kind: &str) -> Value {
    json!({"id":id,"role":role,"channel":channel,"endpoint":format!("display.{name}"),"actionId":format!("display.{name}.set"),
        "format":{"kind":kind,"unknownText":"?","staleText":"-"}})
}
fn config() -> Value {
    json!({"document":{"schemaVersion":1,"id":"rally","name":"Rally dashboard","provider":"assetto-corsa-rally","context":{},
        "roles":[{"id":"radio","label":"Dashboard"}],"inputs":[],"outputs":[output("rpm","radio","rpm","rpm","integer")]},
        "connection":{"source":"synthetic","freshnessMs":1500},"assignments":[{"role":"radio","gatewayId":A,"moduleId":MODULE}]})
}
fn cohort() -> Value {
    let mut value = config();
    value["document"]["roles"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"second","label":"Second gateway"}));
    value["assignments"]
        .as_array_mut()
        .unwrap()
        .push(json!({"role":"second","gatewayId":B,"moduleId":MODULE}));
    value["document"]["outputs"]
        .as_array_mut()
        .unwrap()
        .push(output("rpm2", "second", "rpm", "rpm", "integer"));
    value
}
fn all_channels() -> Value {
    let mut value = config();
    let mut speed = output("speed", "radio", "speed_kmh", "speed", "fixed");
    speed["format"]["decimals"] = json!(1);
    value["document"]["outputs"] = json!([
        output("rpm","radio","rpm","rpm","integer"),speed,output("gear","radio","gear","gear","integer"),
        output("maximum","radio","max_rpm","maximum","integer"),
        {"id":"threshold","role":"radio","channel":"rev_ratio","endpoint":"led.rev","actionId":"led.rev.set","format":{"kind":"threshold","threshold":0.8}},
        {"id":"bands","role":"radio","channel":"rev_ratio","endpoint":"led.rgb","actionId":"led.rgb.set","format":{"kind":"rgb","bands":[{"minimum":0,"color":"00FF00"},{"minimum":0.8,"color":"FF0000"}],"unknownColor":"010203","staleColor":"000000"}}
    ]);
    value
}
fn frame(packet: u32, rpm: i32) -> Value {
    let mut physics = include_bytes!("../../tests/fixtures/acr/live.physics.bin").to_vec();
    physics[0..4].copy_from_slice(&packet.to_le_bytes());
    physics[20..24].copy_from_slice(&rpm.to_le_bytes());
    json!({"sourceId":"synthetic","sourceEpoch":"1","physics":physics,
        "graphics":include_bytes!("../../tests/fixtures/acr/live.graphics.bin").to_vec(),
        "staticPage":include_bytes!("../../tests/fixtures/acr/live.static.bin").to_vec()})
}

#[derive(Clone)]
struct Owned {
    id: u64,
    secret: String,
    fallback: Value,
    expires: Instant,
}
struct Device {
    boot: u64,
    revision: u64,
    online: bool,
    actions: Vec<Value>,
    values: BTreeMap<String, Value>,
    leases: BTreeMap<String, Owned>,
    last_lease: Value,
    lose_reply: bool,
    held: Vec<bool>,
    events: Vec<Value>,
    axis: u16,
}
impl Device {
    fn new() -> Self {
        let actions = vec![
            display("rpm", 6),
            display("speed", 6),
            display("gear", 2),
            display("maximum", 6),
            json!({"id":"led.rev.set","endpoint":"led.rev","label":"Rev","readable":true,"semantics":"setter","parameters":[{"id":"on","label":"On","type":"boolean"}]}),
            json!({"id":"led.rgb.set","endpoint":"led.rgb","label":"RGB","readable":true,"semantics":"setter","parameters":[{"id":"color","label":"Color","type":"rgb"}]}),
        ];
        let values = actions
            .iter()
            .map(|a| {
                let value = match a["parameters"][0]["type"].as_str().unwrap() {
                    "boolean" => json!([false]),
                    "rgb" => json!(["000000"]),
                    _ => json!([" ".repeat(a["display"]["columns"].as_u64().unwrap() as usize)]),
                };
                (a["id"].as_str().unwrap().into(), value)
            })
            .collect();
        Self {
            boot: 1,
            revision: 1,
            online: true,
            actions,
            values,
            leases: BTreeMap::new(),
            last_lease: Value::Null,
            lose_reply: false,
            held: vec![false; 3],
            events: Vec::new(),
            axis: 0,
        }
    }
    fn state(&self) -> Value {
        json!({"connected":true,"reconnecting":false,"layoutPending":false,"epoch":1,
            "modules":[{"id":MODULE,"online":self.online,"enabled":false,"published":false,"directUsb":false,"reportId":null,"boot":self.boot,"node":1,"lease":9}],
            "moduleActions":{MODULE:{"status":"ready","route":[1,1,9,self.boot],"catalogueRevision":self.revision,"ownership":{"schemaVersion":1,"scope":"endpoint","maxLeaseMs":5000},"actions":self.actions,
                "states":self.values.iter().map(|(id,v)|json!({"id":id,"values":v})).collect::<Vec<_>>(),"lastLease":self.last_lease,"lastLeaseRoute":[1,1,9,self.boot]}},
            "moduleEvents":{MODULE:self.input_state(None)}})
    }
    fn input_state(&self, after: Option<u64>) -> Value {
        let mut result = json!({"ok":true,"schemaVersion":1,"route":[1,1,9,self.boot],"stream":1,"head":self.events.len(),"oldest":1,"overflow":0,
            "buttonCount":3,"held":self.held,"contactButtonCount":1,"maintainedButtons":[],"encoderButtons":[1,2],"selectorButtons":[],
            "axisCount":1,"axes":[self.axis],"axisNeutral":[0],"axisUsage":[48],"axisAvailable":[true],"sampledAtMs":1});
        if let Some(after) = after {
            let events: Vec<_> = self
                .events
                .iter()
                .skip(after as usize)
                .take(16)
                .cloned()
                .collect();
            let next = after + events.len() as u64;
            result["after"] = json!(after);
            result["next"] = json!(next);
            result["more"] = json!(next < self.events.len() as u64);
            result["events"] = json!(events);
        }
        result
    }
    fn edge(&mut self, control: usize, pressed: bool) {
        self.held[control] = pressed;
        self.events
            .push(json!({"sequence":self.events.len()+1,"control":control,"pressed":pressed}));
    }
    fn expire(&mut self) {
        let expired: Vec<_> = self
            .leases
            .iter()
            .filter(|(_, l)| Instant::now() >= l.expires)
            .map(|(endpoint, l)| (endpoint.clone(), l.fallback.clone()))
            .collect();
        for (endpoint, fallback) in expired {
            self.leases.remove(&endpoint);
            for action in &self.actions {
                if action["endpoint"] == endpoint {
                    self.values
                        .insert(action["id"].as_str().unwrap().into(), fallback.clone());
                }
            }
        }
    }
}
struct Fake {
    devices: BTreeMap<String, Device>,
    calls: Vec<Value>,
    writes: Vec<(String, String, Value)>,
    next_lease: u64,
}
#[derive(Clone)]
struct Mock {
    fake: Arc<Mutex<Fake>>,
    delay: Arc<Mutex<Option<(String, String, u64)>>>,
    entered: Arc<AtomicBool>,
}
impl Mock {
    fn new() -> Self {
        Self {
            fake: Arc::new(Mutex::new(Fake {
                devices: BTreeMap::from([(A.into(), Device::new()), (B.into(), Device::new())]),
                calls: vec![],
                writes: vec![],
                next_lease: 1,
            })),
            delay: Arc::new(Mutex::new(None)),
            entered: Arc::new(AtomicBool::new(false)),
        }
    }
    fn delay(&self, gateway: &str, command: &str, milliseconds: u64) {
        self.entered.store(false, Ordering::Release);
        *self.delay.lock().unwrap() = Some((gateway.into(), command.into(), milliseconds));
    }
    fn panel(&self) -> Panel {
        let mock = self.clone();
        Arc::new(move |request| mock.reply(request))
    }
    fn reply(&self, request: Value) -> Value {
        let gateway = request["gatewayId"].as_str().unwrap().to_owned();
        let command = request["command"].as_str().unwrap();
        let delay = {
            let mut delay = self.delay.lock().unwrap();
            if delay
                .as_ref()
                .is_some_and(|(g, c, _)| g == &gateway && c == command)
            {
                delay.take()
            } else {
                None
            }
        };
        if let Some((_, _, ms)) = delay {
            self.entered.store(true, Ordering::Release);
            thread::sleep(Duration::from_millis(ms));
            self.entered.store(false, Ordering::Release);
        }
        let mut fake = self.fake.lock().unwrap();
        fake.calls.push(request.clone());
        let lease_id = fake.next_lease;
        if command == "action_claim" {
            fake.next_lease += 1;
        }
        let device = fake.devices.get_mut(&gateway).unwrap();
        device.expire();
        if command.starts_with("action_")
            && !matches!(command, "action_describe" | "action_state")
            && request["boot"] != device.boot
        {
            return json!({"ok":false,"error":"staleModuleBoot"});
        }
        let mut write = None;
        match command {
            "action_claim" => {
                let Some(action) = device
                    .actions
                    .iter()
                    .find(|a| a["id"] == request["actionId"])
                else {
                    return json!({"ok":false,"error":"unknownAction"});
                };
                let endpoint = action["endpoint"].as_str().unwrap().to_owned();
                if device.leases.contains_key(&endpoint) {
                    return json!({"ok":false,"error":"outputOwned"});
                }
                device.leases.insert(
                    endpoint.clone(),
                    Owned {
                        id: lease_id,
                        secret: request["ownershipToken"].as_str().unwrap().into(),
                        fallback: request["values"].clone(),
                        expires: Instant::now() + Duration::from_millis(5000),
                    },
                );
                device.last_lease =
                    json!({"leaseId":lease_id,"endpoint":endpoint,"remainingMs":5000});
            }
            "action_renew" | "action_release" => {
                let owned = device
                    .leases
                    .iter()
                    .find(|(_, l)| {
                        request["leaseId"] == l.id && request["ownershipToken"] == l.secret
                    })
                    .map(|(e, l)| (e.clone(), l.clone()));
                let Some((endpoint, owned)) = owned else {
                    return json!({"ok":false,"error":"staleOutputLease"});
                };
                if command == "action_release" {
                    device.leases.remove(&endpoint);
                    for action in &device.actions {
                        if action["endpoint"] == endpoint {
                            device.values.insert(
                                action["id"].as_str().unwrap().into(),
                                owned.fallback.clone(),
                            );
                        }
                    }
                } else {
                    device.leases.get_mut(&endpoint).unwrap().expires =
                        Instant::now() + Duration::from_millis(5000);
                }
                device.last_lease = json!({"leaseId":owned.id,"endpoint":endpoint,"remainingMs":5000,"released":command=="action_release"});
            }
            "action_invoke_owned" | "action_invoke" => {
                let Some(action) = device
                    .actions
                    .iter()
                    .find(|a| a["id"] == request["actionId"])
                else {
                    return json!({"ok":false,"error":"unknownAction"});
                };
                let endpoint = action["endpoint"].as_str().unwrap();
                if command == "action_invoke_owned" {
                    if !device.leases.get(endpoint).is_some_and(|l| {
                        request["leaseId"] == l.id && request["ownershipToken"] == l.secret
                    }) {
                        return json!({"ok":false,"error":"staleOutputLease"});
                    }
                } else if device.leases.contains_key(endpoint) {
                    return json!({"ok":false,"error":"outputOwned"});
                }
                if let Err(error) = crate::actions::arguments(action, &request["values"]) {
                    return json!({"ok":false,"error":error});
                }
                let id = action["id"].as_str().unwrap().to_owned();
                device.values.insert(id.clone(), request["values"].clone());
                write = Some((gateway.clone(), id, request["values"].clone()));
            }
            "events_read" => {
                let after = request["after"].as_u64().unwrap();
                let mut state = device.state();
                state["moduleEvents"][MODULE] = device.input_state(Some(after));
                return json!({"ok":true,"state":state});
            }
            "status" | "action_describe" | "action_state" | "events_state" => {}
            _ => return json!({"ok":false,"error":"unsupportedMockCommand"}),
        }
        let lost = write.is_some() && device.lose_reply;
        if lost {
            device.lose_reply = false;
        }
        let state = device.state();
        if let Some(write) = write {
            fake.writes.push(write);
        }
        if lost {
            json!({"ok":false,"error":"lostReply"})
        } else {
            json!({"ok":true,"state":state})
        }
    }
    fn plain_invocations(&self) -> usize {
        self.fake
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|r| r["command"] == "action_invoke")
            .count()
    }
}
struct Fixture {
    runtime: Arc<IntegrationRuntime>,
    path: PathBuf,
    mock: Mock,
}
impl Fixture {
    fn new() -> Self {
        Self::with_mock(Mock::new())
    }
    fn with_mock(mock: Mock) -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.research/milestone4/integration-runtime-tests")
            .join(uuid::Uuid::new_v4().to_string())
            .join("settings.json");
        Self {
            runtime: Arc::new(IntegrationRuntime::new(mock.panel(), path.clone())),
            path,
            mock,
        }
    }
    fn request(&self, command: &str, extra: Value) -> Result<(), String> {
        let mut request = json!({"command":command});
        for (key, value) in extra.as_object().unwrap() {
            request[key] = value.clone();
        }
        self.runtime.request(&request)
    }
    fn save(&self, config: Value) {
        self.request("integration_save", json!({"integrationConfig":config}))
            .unwrap();
    }
    fn start(&self) {
        self.request("integration_start", json!({})).unwrap();
    }
    fn simulate(&self, frame: Value) {
        self.request("integration_simulate", json!({"telemetryFrame":frame}))
            .unwrap();
    }
    fn fresh(&self, packet: u32, rpm: i32) {
        self.simulate(frame(packet, rpm));
        self.simulate(frame(packet + 1, rpm));
        wait("fresh telemetry", || {
            self.runtime.status()["feedbackFresh"] == true
        });
    }
    fn applied(&self, id: &str, value: Value) {
        wait(&format!("{id} applied as {value}"), || {
            self.runtime.status()["outputs"]
                .as_array()
                .is_some_and(|rows| {
                    rows.iter()
                        .any(|r| r["id"] == id && r["applied"] == value && r["pending"] == false)
                })
        });
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.runtime.stop(true);
        if let Some(parent) = self.path.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}

#[test]
fn saved_profiles_are_explicit_atomic_and_revalidated() {
    let f = Fixture::new();
    assert!(f.request("integration_start", json!({})).is_err());
    assert!(f
        .request(
            "integration_simulate",
            json!({"telemetryFrame":frame(1,6300)})
        )
        .is_err());
    assert!(f
        .request(
            "integration_override",
            json!({"bindingId":"rpm","values":["123"],"durationMs":100})
        )
        .is_err());
    f.save(config());
    assert_eq!(f.runtime.status()["revision"], 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&f.path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let loaded = IntegrationRuntime::new(f.mock.panel(), f.path.clone());
    assert_eq!(loaded.status()["config"], config());
    assert!(!loaded.running());
    let saved = fs::read(&f.path).unwrap();
    let mut missing = config();
    missing["assignments"] = json!([]);
    f.request("integration_validate", json!({"integrationConfig":missing}))
        .unwrap();
    assert_eq!(f.runtime.status()["validation"]["valid"], false);
    assert!(f
        .request("integration_save", json!({"integrationConfig":missing}))
        .is_err());
    assert_eq!(fs::read(&f.path).unwrap(), saved);
    let mut missing = config();
    missing["document"]["outputs"][0]["channel"] = json!("missing_channel");
    f.request("integration_validate", json!({"integrationConfig":missing}))
        .unwrap();
    assert_eq!(f.runtime.status()["validation"]["valid"], false);
    let mut wrong_endpoint = config();
    wrong_endpoint["document"]["outputs"][0]["endpoint"] = json!("display.other");
    f.request(
        "integration_validate",
        json!({"integrationConfig":wrong_endpoint}),
    )
    .unwrap();
    assert_eq!(f.runtime.status()["validation"]["valid"], false);
    assert!(f
        .request(
            "integration_save",
            json!({"integrationConfig":config(),"revision":0})
        )
        .is_err());
    assert!(f
        .request("integration_start", json!({"revision":0}))
        .is_err());
    assert!(f.request("not_a_command", json!({})).is_err());
    f.request("integration_get_state", json!({})).unwrap();
    fs::write(&f.path, b"bad settings").unwrap();
    assert!(
        IntegrationRuntime::new(f.mock.panel(), f.path.clone()).status()["error"]
            .as_str()
            .unwrap()
            .contains("invalid")
    );
}

#[test]
fn all_channels_use_real_prefixes_unknown_and_pause_fallback() {
    let f = Fixture::new();
    f.save(all_channels());
    f.start();
    f.fresh(10, 6300);
    f.applied("rpm", json!(["6300  "]));
    f.applied("speed", json!(["124.2 "]));
    f.applied("gear", json!(["4 "]));
    f.applied("maximum", json!(["7500  "]));
    f.applied("threshold", json!([true]));
    f.applied("bands", json!(["FF0000"]));
    let mut unknown = frame(12, 6300);
    unknown["staticPage"] =
        json!(include_bytes!("../../tests/fixtures/acr/unknown-max.static.bin").to_vec());
    f.simulate(unknown);
    f.applied("maximum", json!(["?     "]));
    f.applied("threshold", json!([false]));
    f.applied("bands", json!(["010203"]));
    assert_eq!(f.runtime.status()["feedbackFresh"], true);
    let mut paused = frame(12, 6300);
    paused["graphics"] =
        json!(include_bytes!("../../tests/fixtures/acr/paused.graphics.bin").to_vec());
    f.simulate(paused);
    wait("paused source", || {
        f.runtime.status()["providerState"]["status"] == "inactive"
    });
    f.applied("rpm", json!(["-     "]));
    f.applied("bands", json!(["000000"]));
    f.request("integration_stop", json!({})).unwrap();
    assert_eq!(f.runtime.status()["feedbackFresh"], false);
    assert_ne!(f.runtime.status()["providerState"]["fresh"], true);
    assert!(f.mock.fake.lock().unwrap().devices[A].leases.is_empty());
    assert_eq!(f.mock.plain_invocations(), 0);
}

#[test]
fn route_changes_require_confirmation_and_running_reboot_reclaims() {
    let f = Fixture::new();
    f.save(config());
    f.start();
    f.fresh(10, 6300);
    f.applied("rpm", json!(["6300  "]));
    f.runtime.stop(false);
    {
        let mut fake = f.mock.fake.lock().unwrap();
        let d = fake.devices.get_mut(A).unwrap();
        d.boot = 2;
        d.leases.clear();
    }
    assert!(f
        .request("integration_start", json!({}))
        .unwrap_err()
        .contains("Revalidate"));
    f.request(
        "integration_validate",
        json!({"integrationConfig":config()}),
    )
    .unwrap();
    f.start();
    f.fresh(20, 7000);
    f.applied("rpm", json!(["7000  "]));
    {
        let mut fake = f.mock.fake.lock().unwrap();
        let d = fake.devices.get_mut(A).unwrap();
        d.boot = 3;
        d.leases.clear();
        d.values.insert("display.rpm.set".into(), json!(["      "]));
    }
    wait("new boot ownership", || {
        f.mock
            .fake
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|r| r["command"] == "action_claim" && r["boot"] == 3)
    });
    f.applied("rpm", json!(["7000  "]));
    f.runtime.stop(false);
    {
        let mut fake = f.mock.fake.lock().unwrap();
        fake.devices.get_mut(A).unwrap().actions[0]["parameters"][0]["maxBytes"] = json!(1);
    }
    assert!(f.request("integration_start", json!({})).is_err());
    assert_eq!(f.mock.plain_invocations(), 0);
}

#[test]
fn another_client_and_partial_claim_failure_do_not_steal_outputs() {
    let mock = Mock::new();
    let first = Fixture::with_mock(mock.clone());
    let second = Fixture::with_mock(mock.clone());
    first.save(config());
    second.save(config());
    first.start();
    assert!(second
        .request("integration_start", json!({}))
        .unwrap_err()
        .contains("outputOwned"));
    assert!(!second.runtime.running());
    first.runtime.stop(false);
    second.save(cohort());
    mock.fake
        .lock()
        .unwrap()
        .devices
        .get_mut(B)
        .unwrap()
        .leases
        .insert(
            "display.rpm".into(),
            Owned {
                id: 999,
                secret: "other-client".into(),
                fallback: json!(["other "]),
                expires: Instant::now() + Duration::from_secs(30),
            },
        );
    assert!(second.request("integration_start", json!({})).is_err());
    let fake = mock.fake.lock().unwrap();
    assert!(fake.devices[A].leases.is_empty());
    assert_eq!(fake.devices[B].leases["display.rpm"].id, 999);
    drop(fake);
    assert_eq!(mock.plain_invocations(), 0);
}

#[test]
fn blocked_read_coalesces_latest_and_lost_reply_is_reconciled() {
    let f = Fixture::new();
    f.save(config());
    f.start();
    f.fresh(10, 6300);
    f.applied("rpm", json!(["6300  "]));
    f.mock.delay(A, "action_describe", 220);
    f.mock
        .fake
        .lock()
        .unwrap()
        .devices
        .get_mut(A)
        .unwrap()
        .lose_reply = true;
    f.simulate(frame(12, 6400));
    wait("blocked read", || f.mock.entered.load(Ordering::Acquire));
    f.simulate(frame(13, 6800));
    wait("coalesced provider update", || {
        f.runtime.status()["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == "rpm" && r["desired"] == json!(["6800  "]))
    });
    f.simulate(frame(14, 7200));
    f.applied("rpm", json!(["7200  "]));
    let fake = f.mock.fake.lock().unwrap();
    assert_eq!(
        fake.writes
            .iter()
            .filter(|(_, _, v)| *v == json!(["7200  "]))
            .count(),
        1
    );
    assert!(!fake
        .writes
        .iter()
        .any(|(_, _, v)| *v == json!(["6400  "]) || *v == json!(["6800  "])));
    drop(fake);
    assert_eq!(f.mock.plain_invocations(), 0);
}

#[test]
fn override_expiry_uses_latest_value_and_frozen_packets_stay_stale() {
    let f = Fixture::new();
    let mut c = config();
    c["connection"]["freshnessMs"] = json!(400);
    f.save(c);
    f.start();
    f.fresh(10, 6300);
    f.applied("rpm", json!(["6300  "]));
    f.request(
        "integration_override",
        json!({"bindingId":"rpm","values":["9999  "],"durationMs":200}),
    )
    .unwrap();
    f.applied("rpm", json!(["9999  "]));
    f.simulate(frame(12, 7200));
    f.applied("rpm", json!(["7200  "]));
    for _ in 0..12 {
        f.simulate(frame(12, 7200));
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(f.runtime.status()["feedbackFresh"], false);
    assert_eq!(f.runtime.status()["providerState"]["status"], "stale");
    f.applied("rpm", json!(["-     "]));
    f.simulate(frame(13, 7300));
    f.applied("rpm", json!(["7300  "]));
    assert!(f
        .request(
            "integration_override",
            json!({"bindingId":"rpm","values":["TOO LONG"],"durationMs":100})
        )
        .is_err());
    f.request(
        "integration_override",
        json!({"bindingId":"rpm","resume":true}),
    )
    .unwrap();
    assert_eq!(f.mock.plain_invocations(), 0);
}

#[test]
fn blocked_gateway_does_not_stall_peer_and_stop_cancels_pending_setter() {
    let f = Fixture::new();
    f.save(cohort());
    f.start();
    f.fresh(10, 6300);
    f.applied("rpm", json!(["6300  "]));
    f.applied("rpm2", json!(["6300  "]));
    f.mock.delay(A, "action_invoke_owned", 600);
    f.simulate(frame(12, 6800));
    wait("blocked gateway A", || {
        f.mock.entered.load(Ordering::Acquire)
    });
    f.applied("rpm2", json!(["6800  "]));
    assert!(f.mock.entered.load(Ordering::Acquire));
    f.applied("rpm", json!(["6800  "]));
    f.mock.delay(A, "action_describe", 300);
    f.simulate(frame(13, 7000));
    wait("pending output read", || {
        f.mock.entered.load(Ordering::Acquire)
    });
    let cancel = f.runtime.settings.lock().unwrap().cancel.clone();
    let runtime = f.runtime.clone();
    let stopped = thread::spawn(move || runtime.stop(false));
    wait("cancellation", || cancel.load(Ordering::Acquire));
    stopped.join().unwrap();
    assert!(!f
        .mock
        .fake
        .lock()
        .unwrap()
        .writes
        .iter()
        .any(|(g, _, v)| g == A && *v == json!(["7000  "])));
    assert_eq!(f.mock.plain_invocations(), 0);
    assert!(!f.runtime.running());
    f.runtime.stop(true);
    assert!(f.request("integration_start", json!({})).is_err());
    assert!(f
        .request("integration_save", json!({"integrationConfig":config()}))
        .is_err());
}

#[test]
fn dcs_named_catalogue_query_and_alias_validation_use_same_entrypoint() {
    let f = Fixture::new();
    let mut c = config();
    c["document"]["provider"] = json!("dcs-bios");
    c["document"]["context"] = json!({"aircraftModule":"A-10C"});
    c["document"]["outputs"][0]["channel"] = json!("UHF_FREQUENCY");
    c["document"]["outputs"][0]["format"]["kind"] = json!("text");
    c["connection"] = json!({"cataloguePath":PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../simulation/dcs-bios/catalogue"),
        "commandHost":"127.0.0.1","commandPort":7778,"listenAddress":"127.0.0.1","listenPort":5010,"multicastGroup":"","multicastInterface":"127.0.0.1","freshnessMs":1500});
    f.request(
        "integration_catalogue",
        json!({"integrationConfig":c,"query":"UHF","kind":"output","limit":16}),
    )
    .unwrap();
    assert!(f.runtime.status()["catalogue"]["controls"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["id"] == "UHF_FREQUENCY"));
    f.save(c.clone());
    c["document"]["roles"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"alias","label":"Alias"}));
    c["assignments"]
        .as_array_mut()
        .unwrap()
        .push(json!({"role":"alias","gatewayId":A,"moduleId":MODULE}));
    let mut aliased = c["document"]["outputs"][0].clone();
    aliased["id"] = json!("alias");
    aliased["role"] = json!("alias");
    c["document"]["outputs"]
        .as_array_mut()
        .unwrap()
        .push(aliased);
    assert!(f
        .request("integration_save", json!({"integrationConfig":c}))
        .is_err());
}

// This test keeps real UDP exports advancing while a leased panel read exceeds
// its five-second TTL. Reception of a late event page is not ownership proof.
#[test]
fn late_dcs_event_page_cannot_forward_commands_after_output_lease_expiry() {
    use std::net::UdpSocket;
    struct Feed {
        cancel: Arc<AtomicBool>,
        worker: Option<JoinHandle<()>>,
    }
    impl Drop for Feed {
        fn drop(&mut self) {
            self.cancel.store(true, Ordering::Release);
            if let Some(worker) = self.worker.take() {
                worker.join().unwrap();
            }
        }
    }
    fn commands(receiver: &UdpSocket) -> Vec<String> {
        let mut result = Vec::new();
        let mut bytes = [0; 256];
        while let Ok(size) = receiver.recv(&mut bytes) {
            result.push(String::from_utf8(bytes[..size].to_vec()).unwrap());
        }
        result
    }
    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    receiver.set_nonblocking(true).unwrap();
    let listen_port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let f = Fixture::new();
    let mut c = config();
    c["document"]["provider"] = json!("dcs-bios");
    c["document"]["context"] = json!({"aircraftModule":"A-10C"});
    c["document"]["outputs"][0]["channel"] = json!("UHF_FREQUENCY");
    c["document"]["outputs"][0]["format"]["kind"] = json!("text");
    c["document"]["inputs"] = json!([
        {"id":"ack","role":"radio","kind":"button","control":0,"command":"UFC_MASTER_CAUTION","interface":"set_state"},
        {"id":"detent","role":"radio","kind":"encoder","control":0,"command":"UHF_PRESET_SEL","interface":"fixed_step"},
        {"id":"axis","role":"radio","kind":"axis","control":0,"command":"UHF_VOL","interface":"set_state"}
    ]);
    c["connection"] = json!({"cataloguePath":PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../simulation/dcs-bios/catalogue"),
        "commandHost":"127.0.0.1","commandPort":receiver.local_addr().unwrap().port(),"listenAddress":"127.0.0.1","listenPort":listen_port,
        "multicastGroup":"","multicastInterface":"127.0.0.1","freshnessMs":1500});
    f.save(c);
    f.start();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancelled = cancel.clone();
    let feed = Feed {
        cancel,
        worker: Some(thread::spawn(move || {
            let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
            let mut counter = 1u8;
            while !cancelled.load(Ordering::Acquire) {
                let mut bytes = vec![0x55; 4];
                let mut name = [b' '; 24];
                name[..5].copy_from_slice(b"A-10C");
                for (address, data) in [
                    (64u16, name.to_vec()),
                    (4704, b"251.000\0".to_vec()),
                    (65534, vec![counter, 0]),
                ] {
                    bytes.extend(address.to_le_bytes());
                    bytes.extend((data.len() as u16).to_le_bytes());
                    bytes.extend(data);
                }
                sender.send_to(&bytes, ("127.0.0.1", listen_port)).unwrap();
                counter = counter.wrapping_add(1);
                thread::sleep(Duration::from_millis(50));
            }
        })),
    };
    wait("owned DCS inputs armed", || {
        f.runtime.status()["inputs"]
            .as_array()
            .is_some_and(|a| a.len() == 3 && a.iter().all(|i| i["ready"] == true))
    });
    assert_eq!(commands(&receiver), vec!["UHF_VOL 0\n"]);
    f.mock
        .fake
        .lock()
        .unwrap()
        .devices
        .get_mut(A)
        .unwrap()
        .edge(0, true);
    let mut received = Vec::new();
    wait("initial DCS press", || {
        received.extend(commands(&receiver));
        received.contains(&"UFC_MASTER_CAUTION 1\n".into())
    });
    assert_eq!(received, vec!["UFC_MASTER_CAUTION 1\n"]);
    f.mock.delay(A, "events_read", 5200);
    {
        let mut fake = f.mock.fake.lock().unwrap();
        let device = fake.devices.get_mut(A).unwrap();
        device.edge(0, false);
        device.edge(0, true);
        device.edge(1, true);
        device.edge(1, false);
        device.axis = 60000;
    }
    wait("read admitted before expiry", || {
        f.mock.entered.load(Ordering::Acquire)
    });
    // Keep reacquisition blocked long enough to inspect the expired interval.
    f.mock.delay(A, "action_claim", 500);
    let before = f
        .mock
        .fake
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter(|r| r["command"] == "events_read")
        .count();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let fake = f.mock.fake.lock().unwrap();
        let returned = fake
            .calls
            .iter()
            .filter(|r| r["command"] == "events_read")
            .count()
            > before;
        let waiting = fake.devices[A].leases.is_empty() && returned;
        drop(fake);
        if waiting {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "leased event read did not finish"
        );
        thread::sleep(Duration::from_millis(5));
    }
    wait("expired ownership disarms input", || {
        f.runtime.status()["inputs"]
            .as_array()
            .is_some_and(|a| a.iter().all(|i| i["ready"] == false))
    });
    let neutral = commands(&receiver);
    assert_eq!(
        neutral,
        vec!["UFC_MASTER_CAUTION 0\n"],
        "Only cleanup release may cross ownership expiry"
    );
    wait("new lease confirmed", || {
        !f.mock.fake.lock().unwrap().devices[A].leases.is_empty()
    });
    wait("current head baseline", || {
        f.runtime.status()["inputs"].as_array().is_some_and(|a| {
            a.iter()
                .any(|i| i["id"] == "ack" && i["status"] == "waitingRelease")
        })
    });
    let recovered = commands(&receiver);
    assert!(!recovered
        .iter()
        .any(|c| c == "UFC_MASTER_CAUTION 1\n" || c == "UHF_PRESET_SEL INC\n"));
    assert!(recovered.iter().all(|c| c == "UHF_VOL 60000\n"));

    // A scope can disappear without changing this cached route. An explicit
    // owned-invoke rejection must disarm immediately, before a later renewal.
    f.mock
        .fake
        .lock()
        .unwrap()
        .devices
        .get_mut(A)
        .unwrap()
        .edge(0, false);
    wait("release arms current button", || {
        f.runtime.status()["inputs"]
            .as_array()
            .is_some_and(|a| a.iter().any(|i| i["id"] == "ack" && i["ready"] == true))
    });
    assert!(commands(&receiver).is_empty());
    f.mock.delay(A, "action_invoke_owned", 100);
    f.request(
        "integration_override",
        json!({"bindingId":"rpm","values":["33333 "],"durationMs":1000}),
    )
    .unwrap();
    wait("owned setter admitted", || {
        f.mock.entered.load(Ordering::Acquire)
    });
    {
        let mut fake = f.mock.fake.lock().unwrap();
        let d = fake.devices.get_mut(A).unwrap();
        d.leases.clear();
        d.edge(0, true);
        d.edge(1, true);
        d.edge(1, false);
        d.axis = 10000;
    }
    f.mock.delay(A, "action_claim", 500);
    wait("explicit ownership loss is visible", || {
        f.runtime.status()["outputs"]
            .as_array()
            .is_some_and(|rows| {
                rows.iter().any(|r| {
                    r["id"] == "rpm"
                        && r["error"]
                            .as_str()
                            .is_some_and(|e| e.contains("staleOutputLease"))
                })
            })
    });
    wait("ownership loss disarms before renewal", || {
        f.runtime.status()["inputs"]
            .as_array()
            .is_some_and(|a| a.iter().all(|i| i["ready"] == false))
    });
    assert!(
        commands(&receiver).is_empty(),
        "No command may follow confirmed ownership loss"
    );
    wait("new ownership restores baseline", || {
        f.runtime.status()["inputs"].as_array().is_some_and(|a| {
            a.iter()
                .any(|i| i["id"] == "ack" && i["status"] == "waitingRelease")
        })
    });
    let recovered = commands(&receiver);
    assert!(!recovered
        .iter()
        .any(|c| c == "UFC_MASTER_CAUTION 1\n" || c == "UHF_PRESET_SEL INC\n"));
    assert!(recovered.iter().all(|c| c == "UHF_VOL 10000\n"));
    drop(feed);
    assert_eq!(f.mock.plain_invocations(), 0);
}
