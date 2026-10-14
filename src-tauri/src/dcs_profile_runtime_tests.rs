use super::*;
use crate::dcs_profile::Profile;
use std::{fs, net::UdpSocket, path::PathBuf, sync::atomic::AtomicU64};

const A: &str = "stm32:00112233445566778899aabb";
const B: &str = "stm32:112233445566778899aabbcc";
const MODULE: &str = "00112233445566778899aabbccddeeff";

// These authored fixtures use the upstream JSON shapes and control names. The
// simulation catalogue deliberately uses synthetic addresses, never addresses
// assumed from a particular aircraft installation.
struct Fixture {
    path: PathBuf,
    receiver: UdpSocket,
    exporter: UdpSocket,
    listen_port: u16,
}
impl Fixture {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.research/m3/profile-runtime-tests")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&path).unwrap();
        let catalogue =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../simulation/dcs-bios/catalogue");
        for file in [
            "A-10C.json",
            "MetadataStart.json",
            "MetadataEnd.json",
            "AircraftAliases.json",
        ] {
            fs::copy(catalogue.join(file), path.join(file)).unwrap();
        }
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver.set_nonblocking(true).unwrap();
        let exporter = UdpSocket::bind("127.0.0.1:0").unwrap();
        let listen_port = UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        Self {
            path,
            receiver,
            exporter,
            listen_port,
        }
    }
    fn compile(&self, inputs: Vec<Value>, outputs: Vec<Value>, freshness: u64) -> CompiledProfile {
        CompiledProfile::load(
            Profile::parse(&json!({
                "schemaVersion":2,"aircraftModule":"A-10C","cataloguePath":self.path,
                "commandHost":"127.0.0.1","commandPort":self.receiver.local_addr().unwrap().port(),
                "listenAddress":"127.0.0.1","listenPort":self.listen_port,"multicastGroup":"",
                "freshnessMs":freshness,"inputs":inputs,"outputs":outputs
            }))
            .unwrap(),
        )
        .unwrap()
    }
    fn send(&self, bytes: &[u8]) {
        self.exporter
            .send_to(bytes, ("127.0.0.1", self.listen_port))
            .unwrap();
    }
    fn commands(&self) -> Vec<String> {
        let mut commands = Vec::new();
        let mut data = [0; 256];
        loop {
            match self.receiver.recv(&mut data) {
                Ok(count) => commands.push(String::from_utf8(data[..count].to_vec()).unwrap()),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return commands,
                Err(e) => panic!("{e}"),
            }
        }
    }
    fn expect(&self, expected: &[&str]) {
        let mut received = Vec::new();
        wait("UDP command delivery", || {
            received.extend(self.commands());
            received.len() >= expected.len()
        });
        assert_eq!(received, expected);
    }
    fn transport(&self, profile: &CompiledProfile) -> Transport {
        Transport::open_network(&profile.profile.network).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
fn input(
    id: &str,
    gateway: &str,
    kind: &str,
    control: usize,
    command: &str,
    interface: &str,
) -> Value {
    json!({"id":id,"gatewayId":gateway,"moduleId":MODULE,"kind":kind,"control":control,
        "command":command,"interface":interface})
}
fn output(id: &str, gateway: &str, control: &str, action: &str, kind: &str) -> Value {
    json!({"id":id,"gatewayId":gateway,"moduleId":MODULE,"control":control,"actionId":action,"format":{"kind":kind}})
}
fn button() -> Value {
    input("ack", A, "button", 0, "UFC_MASTER_CAUTION", "set_state")
}
fn frequency(gateway: &str) -> Value {
    output(
        "frequency",
        gateway,
        "UHF_FREQUENCY",
        "display.frequency",
        "text",
    )
}
fn display(id: &str, endpoint: &str, columns: u8) -> Value {
    json!({"id":id,"label":"Display","endpoint":endpoint,"semantics":"setter","readable":true,
        "parameters":[{"id":"value","label":"Text","type":"string","maxBytes":columns}],
        "display":{"columns":columns,"rows":1,"encoding":"ascii"}})
}
fn block(bytes: &mut Vec<u8>, address: u16, data: &[u8]) {
    assert_eq!(data.len() % 2, 0);
    bytes.extend(address.to_le_bytes());
    bytes.extend((data.len() as u16).to_le_bytes());
    bytes.extend(data);
}
fn frame(
    counter: u8,
    frequency: Option<&str>,
    preset: Option<&str>,
    caution: Option<bool>,
) -> Vec<u8> {
    let mut bytes = vec![0x55; 4];
    let mut name = [b' '; 24];
    name[..5].copy_from_slice(b"A-10C");
    block(&mut bytes, 64, &name);
    if let Some(frequency) = frequency {
        assert_eq!(frequency.len(), 7);
        let mut data = frequency.as_bytes().to_vec();
        data.push(0);
        block(&mut bytes, 4704, &data);
    }
    if let Some(preset) = preset {
        block(&mut bytes, 4712, preset.as_bytes());
    }
    if let Some(caution) = caution {
        block(&mut bytes, 4672, &[if caution { 4 } else { 0 }, 0]);
    }
    block(&mut bytes, 65534, &[counter, 0]);
    bytes
}
fn wait(message: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(4);
    while !condition() {
        assert!(Instant::now() < deadline, "Timed out waiting for {message}");
        thread::sleep(Duration::from_millis(5));
    }
}

struct FakeDevice {
    module: Value,
    epoch: u32,
    revision: u32,
    stream: u32,
    held: Vec<bool>,
    axes: Vec<u16>,
    available: bool,
    events: Vec<Value>,
    actions: Vec<Value>,
    values: BTreeMap<String, Value>,
    lose_reply: bool,
    error: Option<String>,
    route_override: Option<Value>,
    capabilities: Option<Value>,
}
impl FakeDevice {
    fn new() -> Self {
        Self {
            module: json!({"id":MODULE,"online":true,"enabled":false,"published":false,
                "reportId":null,"directUsb":false,"node":1,"lease":9,"boot":1}),
            epoch: 1,
            revision: 1,
            stream: 1,
            held: vec![false; 6],
            axes: vec![0],
            available: true,
            events: Vec::new(),
            actions: vec![
                display("display.frequency", "frequency", 7),
                display("display.preset", "preset", 2),
                json!({"id":"led.caution","label":"Caution","endpoint":"caution","semantics":"setter","readable":true,
                    "parameters":[{"id":"value","label":"Enabled","type":"boolean"}]}),
            ],
            values: BTreeMap::from([
                ("display.frequency".into(), json!(["       "])),
                ("display.preset".into(), json!(["  "])),
                ("led.caution".into(), json!([false])),
            ]),
            lose_reply: false,
            error: None,
            route_override: None,
            capabilities: None,
        }
    }
    fn route(&self) -> Value {
        json!([
            self.epoch,
            self.module["node"],
            self.module["lease"],
            self.module["boot"]
        ])
    }
    fn edge(&mut self, control: usize, pressed: bool) {
        self.held[control] = pressed;
        self.events
            .push(json!({"sequence":self.events.len()+1,"control":control,"pressed":pressed}));
    }
    fn events(&self, after: Option<u32>) -> Value {
        let mut v = json!({"ok":true,"schemaVersion":1,"route":self.route_override.clone().unwrap_or_else(||self.route()),
            "stream":self.stream,"head":self.events.len(),"oldest":1,"overflow":0,"buttonCount":6,
            "held":self.held,"encoderButtons":[2,3,4,5],"contactButtonCount":2,"maintainedButtons":[1],
            "selectorButtons":[],"axisCount":1,"axisUsage":[48],"axisNeutral":[0],"axes":self.axes,
            "axisAvailable":[self.available],"sampledAtMs":1});
        if let Some(capabilities) = &self.capabilities {
            for (key, value) in capabilities.as_object().unwrap() {
                v[key] = value.clone();
            }
        }
        if let Some(after) = after {
            let page: Vec<_> = self
                .events
                .iter()
                .skip(after as usize)
                .take(16)
                .cloned()
                .collect();
            let next = after as usize + page.len();
            v["after"] = json!(after);
            v["next"] = json!(next);
            v["more"] = json!(next < self.events.len());
            v["events"] = json!(page);
        }
        v
    }
    fn state(&self) -> Value {
        let catalogue = json!({"status":"ready","route":self.route_override.clone().unwrap_or_else(||self.route()),
            "catalogueRevision":self.revision,"actions":self.actions,
            "states":self.values.iter().map(|(id,values)|json!({"id":id,"values":values})).collect::<Vec<_>>()});
        json!({"connected":true,"reconnecting":false,"layoutPending":false,"epoch":self.epoch,"modules":[self.module],
            "moduleActions":{MODULE:catalogue},"moduleEvents":{MODULE:self.events(None)}})
    }
}
#[derive(Default)]
struct Fake {
    devices: BTreeMap<String, FakeDevice>,
    writes: Vec<(String, String, Value)>,
    calls: Vec<Value>,
}
#[derive(Clone)]
struct Mock {
    fake: Arc<Mutex<Fake>>,
    delay: Arc<Mutex<Option<(String, String, u64)>>>,
    entered: Arc<AtomicU64>,
}
impl Mock {
    fn new() -> Self {
        Self {
            fake: Arc::new(Mutex::new(Fake {
                devices: BTreeMap::from([
                    (A.into(), FakeDevice::new()),
                    (B.into(), FakeDevice::new()),
                ]),
                ..Fake::default()
            })),
            delay: Arc::new(Mutex::new(None)),
            entered: Arc::new(AtomicU64::new(0)),
        }
    }
    fn device(&self, gateway: &str, edit: impl FnOnce(&mut FakeDevice)) {
        edit(self.fake.lock().unwrap().devices.get_mut(gateway).unwrap());
    }
    fn delay_once(&self, gateway: &str, command: &str, milliseconds: u64) {
        self.entered.store(0, Ordering::Release);
        *self.delay.lock().unwrap() = Some((gateway.into(), command.into(), milliseconds));
    }
    fn panel(&self) -> Panel {
        let mock = self.clone();
        Arc::new(move |request| {
            let gateway = request["gatewayId"].as_str().unwrap();
            let command = request["command"].as_str().unwrap();
            let delay = {
                let mut delay = mock.delay.lock().unwrap();
                if delay
                    .as_ref()
                    .is_some_and(|(g, c, _)| g == gateway && c == command)
                {
                    delay.take().unwrap().2
                } else {
                    0
                }
            };
            if delay > 0 {
                mock.entered.store(delay, Ordering::Release);
                thread::sleep(Duration::from_millis(delay));
                mock.entered.store(0, Ordering::Release);
            }
            let mut fake = mock.fake.lock().unwrap();
            fake.calls.push(request.clone());
            let device = fake.devices.get_mut(gateway).unwrap();
            if let Some(error) = &device.error {
                return json!({"ok":false,"error":error});
            }
            if command == "action_invoke" {
                let id = request["actionId"].as_str().unwrap().to_owned();
                let value = request["values"].clone();
                device.values.insert(id.clone(), value.clone());
                let lose = std::mem::take(&mut device.lose_reply);
                let state = device.state();
                fake.writes.push((gateway.into(), id, value));
                if lose {
                    json!({"ok":false,"error":"lost_reply"})
                } else {
                    json!({"ok":true,"state":state})
                }
            } else if command == "events_read" {
                let after = request["after"].as_u64().unwrap() as u32;
                if request["stream"] != device.stream || after as usize > device.events.len() {
                    return json!({"ok":false,"error":"eventGap"});
                }
                let mut state = device.state();
                state["moduleEvents"][MODULE] = device.events(Some(after));
                json!({"ok":true,"state":state})
            } else {
                json!({"ok":true,"state":device.state()})
            }
        })
    }
    fn writes(&self) -> Vec<(String, String, Value)> {
        self.fake.lock().unwrap().writes.clone()
    }
}
struct Running {
    state: Arc<Mutex<Value>>,
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Running {
    fn start(profile: CompiledProfile, mock: &Mock) -> Self {
        let state = Arc::new(Mutex::new(Value::Null));
        let cancel = Arc::new(AtomicBool::new(false));
        let worker = start(profile, mock.panel(), state.clone(), cancel.clone()).unwrap();
        Self {
            state,
            cancel,
            worker: Some(worker),
        }
    }
    fn value(&self) -> Value {
        self.state.lock().unwrap().clone()
    }
    fn armed(&self, id: &str) {
        wait("fresh armed input", || {
            self.value()["inputs"]
                .as_array()
                .is_some_and(|a| a.iter().any(|i| i["id"] == id && i["ready"] == true))
        });
    }
    fn applied(&self, id: &str, desired: Value) {
        wait("confirmed output", || {
            self.value()["outputs"].as_array().is_some_and(|a| {
                a.iter()
                    .any(|o| o["id"] == id && o["applied"] == desired && o["pending"] == false)
            })
        });
    }
    fn stop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
    }
}

#[test]
fn start_validates_hid_direct_routes_physical_kinds_and_live_output_capabilities() {
    let fixture = Fixture::new();
    let profile = fixture.compile(vec![button()], vec![frequency(A)], 1500);
    let mock = Mock::new();
    let target = profile.inputs[0].binding.target.clone();
    for (key, value) in [
        ("enabled", json!(true)),
        ("published", json!(true)),
        ("reportId", json!(1)),
        ("directUsb", json!(true)),
        ("online", json!(false)),
    ] {
        mock.device(A, |d| d.module[key] = value);
        assert!(
            prepare_one(&profile, &mock.panel(), &target).is_err(),
            "{key}"
        );
        mock.device(A, |d| *d = FakeDevice::new());
    }
    mock.device(A, |d| d.route_override = Some(json!([2, 1, 9, 1])));
    assert!(prepare_one(&profile, &mock.panel(), &target).is_err());
    mock.device(A, |d| *d = FakeDevice::new());
    mock.device(A, |d| d.revision = 0);
    assert!(prepare_one(&profile, &mock.panel(), &target).is_err());
    mock.device(A, |d| *d = FakeDevice::new());
    mock.device(A, |d| d.actions[0]["semantics"] = json!("oneShot"));
    assert!(prepare_one(&profile, &mock.panel(), &target).is_err());
    mock.device(A, |d| *d = FakeDevice::new());
    mock.device(A, |d| d.actions.clear());
    assert!(prepare_one(&profile, &mock.panel(), &target).is_err());
    mock.device(A, |d| *d = FakeDevice::new());
    mock.device(A, |d| d.actions[0]["parameters"][0]["maxBytes"] = json!(1));
    assert!(prepare_one(&profile, &mock.panel(), &target).is_err());
    mock.device(A, |d| *d = FakeDevice::new());
    let mut wrong = profile.clone();
    wrong.inputs[0].binding.control = 1;
    assert!(prepare_one(&wrong, &mock.panel(), &target)
        .err()
        .unwrap()
        .contains("physical"));
    wrong.inputs[0].binding.kind = InputKind::Switch;
    wrong.inputs[0].binding.control = 0;
    assert!(prepare_one(&wrong, &mock.panel(), &target).is_err());
    wrong.inputs[0].binding.kind = InputKind::Encoder;
    wrong.inputs[0].binding.control = 2;
    assert!(prepare_one(&wrong, &mock.panel(), &target).is_err());
    wrong.inputs[0].binding.kind = InputKind::Axis;
    wrong.inputs[0].binding.control = 1;
    assert!(prepare_one(&wrong, &mock.panel(), &target).is_err());
    let mut duplicate = profile.clone();
    let mut extra = duplicate.outputs[0].clone();
    extra.binding.id = "another".into();
    duplicate.outputs.push(extra);
    assert!(prepare_one(&duplicate, &mock.panel(), &target)
        .err()
        .unwrap()
        .contains("endpoint"));
    assert!(mock.writes().is_empty());
    let bad: Panel = Arc::new(|_| json!({"ok":false}));
    assert!(start(
        profile,
        bad,
        Arc::new(Mutex::new(Value::Null)),
        Arc::new(AtomicBool::new(false))
    )
    .is_err());
}

#[test]
fn start_requires_both_boolean_enum_states_without_requiring_a_board_specific_action() {
    let fixture = Fixture::new();
    let mock = Mock::new();
    let profile = fixture.compile(
        vec![],
        vec![output(
            "warning",
            A,
            "MASTER_CAUTION",
            "led.caution",
            "boolean",
        )],
        1500,
    );
    let target = profile.outputs[0].binding.target.clone();
    for options in [json!(["off"]), json!(["on"]), json!(["off", "on"])] {
        mock.device(A, |device| {
            device.actions[2]["parameters"][0] =
                json!({"id":"mode","label":"Mode","type":"enum","options":options});
            device.values.insert("led.caution".into(), json!(["off"]));
        });
        assert_eq!(
            prepare_one(&profile, &mock.panel(), &target).is_ok(),
            options.as_array().unwrap().len() == 2
        );
    }
    assert!(mock.writes().is_empty());
}

#[test]
fn stop_during_an_outstanding_event_read_discards_new_edges_and_only_releases_a_forwarded_press() {
    for already_pressed in [false, true] {
        let fixture = Fixture::new();
        let mock = Mock::new();
        let mut run = Running::start(fixture.compile(vec![button()], vec![], 1500), &mock);
        fixture.send(&frame(1, None, None, None));
        run.armed("ack");
        if already_pressed {
            mock.device(A, |device| device.edge(0, true));
            wait("forwarded press before Stop", || {
                run.value()["commandsSent"] == 1
            });
            fixture.expect(&["UFC_MASTER_CAUTION 1\n"]);
        }
        mock.delay_once(A, "events_read", 250);
        wait("outstanding event read before Stop", || {
            mock.entered.load(Ordering::Acquire) > 0
        });
        mock.device(A, |device| {
            if already_pressed {
                device.edge(0, false);
            }
            device.edge(0, true);
            device.edge(0, false);
        });
        run.stop();
        assert_eq!(
            fixture.commands(),
            if already_pressed {
                vec!["UFC_MASTER_CAUTION 0\n".to_owned()]
            } else {
                vec![]
            }
        );
        assert_eq!(run.value()["running"], false);
    }
}

#[test]
fn stop_during_output_reconciliation_discards_the_pending_value_and_applies_only_fallback() {
    let fixture = Fixture::new();
    let mock = Mock::new();
    let mut run = Running::start(fixture.compile(vec![], vec![frequency(A)], 1500), &mock);
    fixture.send(&frame(1, Some("251.000"), None, None));
    run.applied("frequency", json!(["251.000"]));
    mock.delay_once(A, "action_describe", 250);
    fixture.send(&frame(2, Some("260.000"), None, None));
    wait("outstanding output reconciliation before Stop", || {
        mock.entered.load(Ordering::Acquire) > 0
    });
    run.stop();
    assert!(!mock
        .writes()
        .iter()
        .any(|(_, _, value)| *value == json!(["260.000"])));
    assert_eq!(run.value()["outputs"][0]["applied"], json!(["       "]));
}

#[test]
fn momentary_held_start_needs_release_then_preserves_short_edges_and_stop_neutral_once() {
    let fixture = Fixture::new();
    let mock = Mock::new();
    mock.device(A, |d| d.held[0] = true);
    let mut run = Running::start(fixture.compile(vec![button()], vec![], 1500), &mock);
    fixture.send(&frame(1, None, None, None));
    wait("held startup baseline", || {
        run.value()["inputs"]
            .as_array()
            .is_some_and(|a| a.iter().any(|i| i["status"] == "waitingRelease"))
    });
    assert!(fixture.commands().is_empty());
    mock.device(A, |d| {
        d.edge(0, false);
        d.edge(0, true);
        d.edge(0, false);
        d.edge(0, true);
    });
    wait("ordered momentary commands", || {
        run.value()["commandsSent"] == 3
    });
    fixture.expect(&[
        "UFC_MASTER_CAUTION 1\n",
        "UFC_MASTER_CAUTION 0\n",
        "UFC_MASTER_CAUTION 1\n",
    ]);
    run.stop();
    fixture.expect(&["UFC_MASTER_CAUTION 0\n"]);
    assert_eq!(run.value()["commandsSent"], 4);
    run.stop();
    assert!(fixture.commands().is_empty());
    assert_eq!(run.value()["status"], "stopped");
}

#[test]
fn absolute_switch_encoder_detents_and_axis_scaling_have_distinct_delivery_semantics() {
    let fixture = Fixture::new();
    let profile = fixture.compile(
        vec![
            input("switch", A, "switch", 1, "UHF_SQUELCH", "set_state"),
            input("preset", A, "encoder", 0, "UHF_PRESET_SEL", "fixed_step"),
            input("volume", A, "encoder", 1, "UHF_VOL", "variable_step"),
            {
                let mut axis = input("axis", A, "axis", 0, "UHF_10MHZ_SEL", "set_state");
                axis["deadband"] = json!(2);
                axis
            },
        ],
        vec![],
        1500,
    );
    let transport = fixture.transport(&profile);
    let events = FakeDevice::new().events(None);
    let mut meter = Meter::default();
    let mut inputs: Vec<_> = profile
        .inputs
        .iter()
        .cloned()
        .map(InputState::new)
        .collect();
    let mut events = events;
    events["held"][1] = json!(true);
    events["axes"][0] = json!(32768);
    for input in &mut inputs {
        input
            .baseline(&events, true, &transport, &mut meter)
            .unwrap();
    }
    fixture.expect(&["UHF_SQUELCH 1\n", "UHF_10MHZ_SEL 5\n"]);
    for (control, pressed) in [
        (1, false),
        (2, true),
        (2, false),
        (2, true),
        (2, false),
        (3, true),
        (3, false),
        (4, true),
        (4, false),
        (5, true),
    ] {
        for input in &mut inputs {
            input
                .event(control, pressed, &events, &transport, &mut meter)
                .unwrap();
        }
    }
    fixture.expect(&[
        "UHF_SQUELCH 0\n",
        "UHF_PRESET_SEL INC\n",
        "UHF_PRESET_SEL INC\n",
        "UHF_PRESET_SEL DEC\n",
        "UHF_VOL +1\n",
        "UHF_VOL -1\n",
    ]);
    let axis = inputs.last_mut().unwrap();
    events["axes"][0] = json!(40000);
    axis.axis(&events, &transport, &mut meter).unwrap();
    assert!(fixture.commands().is_empty());
    events["axes"][0] = json!(65535);
    axis.axis(&events, &transport, &mut meter).unwrap();
    fixture.expect(&["UHF_10MHZ_SEL 9\n"]);
    events["axisAvailable"][0] = json!(false);
    axis.axis(&events, &transport, &mut meter).unwrap();
    assert!(!axis.armed);
    assert!(fixture.commands().is_empty());
    events["axisAvailable"][0] = json!(true);
    axis.axis(&events, &transport, &mut meter).unwrap();
    fixture.expect(&["UHF_10MHZ_SEL 9\n"]);
}

#[test]
fn command_uncertainty_disarms_without_replay_but_attempted_press_gets_one_cleanup_release() {
    let fixture = Fixture::new();
    let profile = fixture.compile(vec![button()], vec![], 1500);
    let transport = fixture.transport(&profile);
    let mut input = InputState::new(profile.inputs[0].clone());
    let mut meter = Meter::default();
    let events = FakeDevice::new().events(None);
    input
        .baseline(&events, true, &transport, &mut meter)
        .unwrap();
    // Transport rejects this argument before a datagram can be sent. The same
    // uncertainty path covers UDP send errors, which cannot be safely retried.
    input.binding.binding.pressed_value = u16::MAX;
    input.binding.binding.command = "invalid command".into();
    assert!(input
        .event(0, true, &events, &transport, &mut meter)
        .is_err());
    assert!(input.uncertain);
    input.binding.binding.command = "UFC_MASTER_CAUTION".into();
    input.release(&transport, &mut meter);
    input.release(&transport, &mut meter);
    input
        .baseline(&events, true, &transport, &mut meter)
        .unwrap();
    input
        .event(0, true, &events, &transport, &mut meter)
        .unwrap();
    fixture.expect(&["UFC_MASTER_CAUTION 0\n"]);
    assert_eq!(input.report(true)["status"], "error");
    assert!(input.report(true)["error"]
        .as_str()
        .unwrap()
        .contains("not replayed"));
}

#[test]
fn export_frames_commit_atomically_and_unknown_then_stale_outputs_use_bounded_fallbacks() {
    let fixture = Fixture::new();
    let mock = Mock::new();
    let mut freq = frequency(A);
    freq["format"]["unknownText"] = json!("UNKNOWN");
    freq["format"]["staleText"] = json!("STALE");
    let mut run = Running::start(
        fixture.compile(
            vec![],
            vec![
                freq,
                output("preset", A, "UHF_PRESET", "display.preset", "text"),
                output("caution", A, "MASTER_CAUTION", "led.caution", "boolean"),
            ],
            150,
        ),
        &mock,
    );
    fixture.send(&frame(1, None, None, None));
    run.applied("frequency", json!(["UNKNOWN"]));
    let bytes = frame(2, Some("251.000"), Some("20"), Some(true));
    let split = bytes.len() - 6;
    fixture.send(&bytes[..split]);
    thread::sleep(Duration::from_millis(30));
    assert_eq!(
        mock.fake.lock().unwrap().devices[A].values["display.frequency"],
        json!(["UNKNOWN"])
    );
    fixture.send(&bytes[split..]);
    run.applied("frequency", json!(["251.000"]));
    run.applied("preset", json!(["20"]));
    run.applied("caution", json!([true]));
    wait("stale output fallback", || {
        run.value()["feedbackFresh"] == false
    });
    run.applied("frequency", json!(["STALE  "]));
    run.applied("caution", json!([false]));
    fixture.send(&frame(3, Some("300.125"), Some("01"), Some(false)));
    run.applied("frequency", json!(["300.125"]));
    run.stop();
    assert_eq!(
        mock.fake.lock().unwrap().devices[A].values["display.frequency"],
        json!(["STALE  "])
    );
}

#[test]
fn slow_read_coalesces_latest_setter_and_lost_ack_is_reconciled_without_second_invoke() {
    let fixture = Fixture::new();
    let mock = Mock::new();
    let run = Running::start(fixture.compile(vec![], vec![frequency(A)], 1500), &mock);
    fixture.send(&frame(1, Some("251.000"), None, None));
    run.applied("frequency", json!(["251.000"]));
    mock.delay_once(A, "action_describe", 180);
    mock.device(A, |d| d.lose_reply = true);
    fixture.send(&frame(2, Some("260.000"), None, None));
    wait("blocked setter reconciliation", || {
        mock.entered.load(Ordering::Acquire) > 0
    });
    fixture.send(&frame(3, Some("270.000"), None, None));
    wait("first pending replacement", || {
        run.value()["outputs"][0]["desired"] == json!(["270.000"])
    });
    fixture.send(&frame(4, Some("280.000"), None, None));
    run.applied("frequency", json!(["280.000"]));
    let writes = mock.writes();
    assert_eq!(
        writes
            .iter()
            .filter(|(_, _, v)| *v == json!(["280.000"]))
            .count(),
        1
    );
    assert!(!writes
        .iter()
        .any(|(_, _, v)| *v == json!(["260.000"]) || *v == json!(["270.000"])));
    let state = run.value();
    assert!(state["gateways"][0]["coalesced"].as_u64().unwrap() >= 1);
    assert!(state["gateways"][0]["reconciled"].as_u64().unwrap() >= 1);
    assert_eq!(state["gateways"][0]["queueHighWater"], 1);
}

#[test]
fn blocked_gateway_output_does_not_stall_another_gateways_ordered_input() {
    let fixture = Fixture::new();
    let mock = Mock::new();
    let mut ack = button();
    ack["gatewayId"] = json!(B);
    let run = Running::start(fixture.compile(vec![ack], vec![frequency(A)], 1500), &mock);
    fixture.send(&frame(1, Some("251.000"), None, None));
    run.armed("ack");
    run.applied("frequency", json!(["251.000"]));
    mock.delay_once(A, "action_invoke", 350);
    fixture.send(&frame(2, Some("260.000"), None, None));
    wait("gateway A blocked output", || {
        mock.entered.load(Ordering::Acquire) > 0
    });
    let started = Instant::now();
    mock.device(B, |d| {
        d.edge(0, true);
        d.edge(0, false);
    });
    wait("gateway B commands", || run.value()["commandsSent"] == 2);
    assert!(started.elapsed() < Duration::from_millis(250));
    fixture.expect(&["UFC_MASTER_CAUTION 1\n", "UFC_MASTER_CAUTION 0\n"]);
    run.applied("frequency", json!(["260.000"]));
}

#[test]
fn slow_event_read_across_feedback_loss_discards_press_and_releases_prior_press_once() {
    let fixture = Fixture::new();
    let mock = Mock::new();
    let run = Running::start(fixture.compile(vec![button()], vec![], 200), &mock);
    fixture.send(&frame(1, None, None, None));
    run.armed("ack");
    mock.delay_once(A, "events_read", 450);
    mock.device(A, |d| {
        d.edge(0, true);
        d.edge(0, false);
    });
    wait("slow event read", || {
        mock.entered.load(Ordering::Acquire) > 0
    });
    wait("feedback invalidated", || {
        run.value()["feedbackFresh"] == false
    });
    assert_eq!(run.value()["inputs"][0]["ready"], false);
    assert_eq!(run.value()["inputs"][0]["armed"], false);
    assert_eq!(run.value()["inputs"][0]["needsBaseline"], true);
    wait("slow read completed", || {
        mock.entered.load(Ordering::Acquire) == 0
    });
    wait("worker disarmed", || {
        run.value()["inputs"][0]["ready"] == false
    });
    assert!(fixture.commands().is_empty());
    let baselines = mock
        .fake
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter(|c| c["command"] == "events_state")
        .count();
    fixture.send(&frame(2, None, None, None));
    wait("new route baseline", || {
        mock.fake
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|c| c["command"] == "events_state")
            .count()
            > baselines
    });
    run.armed("ack");
    mock.device(A, |d| d.edge(0, true));
    wait("new press sent", || run.value()["commandsSent"] == 1);
    fixture.expect(&["UFC_MASTER_CAUTION 1\n"]);
    wait("stale neutral release", || run.value()["commandsSent"] == 2);
    fixture.expect(&["UFC_MASTER_CAUTION 0\n"]);
    thread::sleep(Duration::from_millis(70));
    assert!(fixture.commands().is_empty());
}

#[test]
fn route_reboot_and_event_gap_rebaseline_without_historical_presses_and_revalidate_outputs() {
    let fixture = Fixture::new();
    let mock = Mock::new();
    let run = Running::start(
        fixture.compile(vec![button()], vec![frequency(A)], 1500),
        &mock,
    );
    fixture.send(&frame(1, Some("251.000"), None, None));
    run.armed("ack");
    run.applied("frequency", json!(["251.000"]));
    mock.device(A, |d| d.edge(0, true));
    wait("first press", || run.value()["commandsSent"] == 1);
    fixture.expect(&["UFC_MASTER_CAUTION 1\n"]);
    mock.device(A, |d| {
        d.module["boot"] = json!(2);
        d.stream = 2;
        d.events.clear();
        d.held[0] = true;
        d.values
            .insert("display.frequency".into(), json!(["       "]));
    });
    wait("route cleanup release", || run.value()["commandsSent"] == 2);
    fixture.expect(&["UFC_MASTER_CAUTION 0\n"]);
    run.applied("frequency", json!(["251.000"]));
    mock.device(A, |d| {
        d.edge(0, false);
        d.edge(0, true);
        d.edge(0, false);
    });
    wait("post reboot fresh edges", || {
        run.value()["commandsSent"] == 4
    });
    fixture.expect(&["UFC_MASTER_CAUTION 1\n", "UFC_MASTER_CAUTION 0\n"]);
    mock.device(A, |d| {
        d.stream = 3;
        d.events.clear();
        d.held[0] = true;
    });
    wait("event gap detected", || {
        run.value()["eventGaps"].as_u64().unwrap_or(0) > 0
    });
    wait("held after gap", || {
        run.value()["inputs"][0]["status"] == "waitingRelease"
    });
    assert!(fixture.commands().is_empty());
    mock.device(A, |d| {
        d.module["boot"] = json!(3);
        d.actions[0]["readable"] = json!(false);
    });
    wait("invalid reboot output descriptor", || {
        run.value()["outputs"][0]["error"].is_string()
    });
    assert!(fixture.commands().is_empty());
}

#[test]
fn live_typed_capabilities_are_rechecked_before_encoder_edges_can_reach_dcs() {
    let fixture = Fixture::new();
    let mock = Mock::new();
    let run = Running::start(
        fixture.compile(
            vec![input("volume", A, "encoder", 1, "UHF_VOL", "variable_step")],
            vec![],
            1500,
        ),
        &mock,
    );
    fixture.send(&frame(1, None, None, None));
    run.armed("volume");
    // This is structurally valid schema1 metadata but no longer advertises the
    // bound second encoder. A faulty unchanged stream must yield a row error,
    // rather than panicking the worker or reinterpreting a different source.
    mock.device(A, |d| {
        d.capabilities =
            Some(json!({"buttonCount":4,"held":[false,false,false,false],"encoderButtons":[2,3]}));
        d.edge(2, true);
        d.edge(2, false);
    });
    wait("capability mismatch reported", || {
        run.value()["inputs"].as_array().is_some_and(|a| {
            a.iter()
                .any(|i| i["error"].as_str().is_some_and(|e| e.contains("physical")))
        })
    });
    assert!(fixture.commands().is_empty());
    assert_eq!(run.value()["running"], true);
}

#[test]
fn transient_layout_or_disconnect_blocks_inputs_but_output_only_panels_may_keep_hid_enabled() {
    let fixture = Fixture::new();
    let profile = fixture.compile(vec![], vec![frequency(A)], 1500);
    let mock = Mock::new();
    let target = profile.outputs[0].binding.target.clone();
    mock.device(A, |d| {
        d.module["enabled"] = json!(true);
        d.module["published"] = json!(true);
        d.module["reportId"] = json!(1);
        d.module["directUsb"] = json!(true);
    });
    assert!(prepare_one(&profile, &mock.panel(), &target).is_ok());
    let state = mock.fake.lock().unwrap().devices[A].state();
    for (key, value) in [
        ("connected", json!(false)),
        ("reconnecting", json!(true)),
        ("layoutPending", json!(true)),
    ] {
        let mut state = state.clone();
        state[key] = value;
        assert!(module(&state, &target, false).is_err());
    }
    let mut missing = state.clone();
    missing["modules"] = json!([]);
    assert!(module(&missing, &target, false).is_err());
    let mut invalid = FakeDevice::new().events(None);
    invalid["contactButtonCount"] = Value::Null;
    let binding = fixture
        .compile(vec![button()], vec![], 1500)
        .inputs
        .remove(0);
    assert!(source(&binding, &invalid).is_err());
}
