use super::*;
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
};
const SERIAL: &str = "00112233445566778899aabb";
const MODULE: &str = "4f53474400112233445566778899aabb";
const TOKEN: &str = "11111111-2222-4333-8444-555555555555";
fn port() -> PortInfo {
    PortInfo {
        path: "COM8".into(),
        serial_number: Some(SERIAL.into()),
        vendor_id: Some("1209".into()),
        product_id: Some("0001".into()),
        manufacturer: Some("OpenSimGear".into()),
        friendly_name: Some("Device Template v0.1.0".into()),
    }
}
fn definition() -> Value {
    json!({"name":"Panel with spaces","expanderMask":0,"buttons":[[0,0,0]],"axes":[],"hats":[],"encoders":[],"selectors":[]})
}
fn layout(config: &Value) -> Value {
    let n = |key: &str| config[key].as_array().unwrap().len();
    let buttons = n("buttons") + n("encoders") * 2 + n("selectors") * 3;
    let axes = n("axes");
    let hats = n("hats");
    json!({"buttons":buttons,"axes":axes,"hats":hats,"reportBytes":buttons.div_ceil(8)+axes*2+hats.div_ceil(2)})
}
fn descriptor() -> Value {
    json!({"id":"led.boolean.set_enabled","label":"LED","endpoint":"led.boolean","semantics":"setter","readable":true,"parameters":[{"id":"enabled","label":"Enabled","type":"boolean"}]})
}
fn timing() -> Timing {
    Timing {
        read: Duration::from_millis(1),
        command: Duration::from_millis(15),
        refresh: Duration::from_secs(60),
        reconnect: Duration::from_millis(80),
        settle: Duration::ZERO,
    }
}
struct Device {
    ports: Vec<PortInfo>,
    boot: u32,
    generation: u32,
    saved: u32,
    owner: u32,
    active: Value,
    draft: Value,
    dirty: bool,
    opens: usize,
    writes: Vec<String>,
    controls: Vec<bool>,
    drop_command: Option<String>,
    ignore_apply: bool,
    malformed_status: bool,
    identity_module: String,
    identity_model: String,
    led: bool,
    token: Option<String>,
    lease: u32,
    fallback: bool,
    head: u32,
    response_override: Option<(String, Value)>,
    action_revision: u32,
    ownership: bool,
    guarded_restore: bool,
    last_owned: Option<bool>,
    actions_supported: bool,
    events_supported: bool,
}
impl Default for Device {
    fn default() -> Self {
        Self {
            ports: vec![port()],
            boot: 100,
            generation: 1,
            saved: 1,
            owner: 0,
            active: definition(),
            draft: definition(),
            dirty: false,
            opens: 0,
            writes: vec![],
            controls: vec![],
            drop_command: None,
            ignore_apply: false,
            malformed_status: false,
            identity_module: MODULE.into(),
            identity_model: "device-template".into(),
            led: false,
            token: None,
            lease: 0,
            fallback: false,
            head: 10,
            response_override: None,
            action_revision: 1,
            ownership: true,
            guarded_restore: false,
            last_owned: None,
            actions_supported: true,
            events_supported: true,
        }
    }
}
impl Device {
    fn snapshot(&self, active: bool) -> Value {
        let draft = self.owner != 0 && !active;
        let config = if draft { &self.draft } else { &self.active };
        let shape = layout(config);
        let empty = shape["reportBytes"] == 0;
        json!({"generation":self.generation,"savedGeneration":self.saved,"owner":self.owner,"claimExpiresMs":if self.owner==0{0}else{30000},"dirty":self.dirty,"pendingApply":false,"view":if draft{"draft"}else{"active"},"valid":true,"config":config,"layout":shape,"descriptorHex":if empty{""}else{"0501"}})
    }
    fn header(&self) -> Value {
        let mut value =
            json!({"ok":true,"schemaVersion":1,"catalogueRevision":self.action_revision});
        if self.ownership {
            value["ownership"] = json!({"schemaVersion":1,"scope":"endpoint","maxLeaseMs":5000});
            if self.guarded_restore {
                value["ownership"]["restorePolicy"] = json!("if-unchanged");
            }
        }
        value
    }
    fn run(&mut self, command: &str) -> Vec<u8> {
        self.writes.push(command.into());
        if command.starts_with("action ") && !self.actions_supported
            || command.starts_with("events ") && !self.events_supported
        {
            return b"ERR unknown command\r\n".to_vec();
        }
        let mut response = if command == "info" {
            let l = layout(&self.active);
            format!("model={} board=0.1.0 profile={} module={} protocol=3 buttons={} axes={} hats={}\r\n",self.identity_model,self.active["name"].as_str().unwrap(),self.identity_module,l["buttons"],l["axes"],l["hats"])
        } else if command == "can status" {
            if self.malformed_status {
                "invalid status\r\n".into()
            } else {
                format!("can=cyphal node=0 route=0 lease=0 boot={} usb=1 rx=0 tx=0 rejected=0 recoveries=0\r\n",self.boot)
            }
        } else if let Some(child) = command.strip_prefix("config ") {
            let args: Vec<_> = child.split(' ').collect();
            let number = |i: usize| args[i].parse::<u32>().unwrap();
            let mut error = None;
            match args[0] {
                "get" | "get_active" => {}
                "claim" => {
                    let owner = number(1);
                    if self.owner != 0 && self.owner != owner {
                        error = Some("writer_busy");
                    } else {
                        if self.owner == 0 {
                            self.draft = self.active.clone();
                            self.dirty = false;
                        }
                        self.owner = owner;
                    }
                }
                _ if self.owner == 0 || number(1) != self.owner => error = Some("writer_required"),
                "release" => {
                    self.owner = 0;
                    self.draft = self.active.clone();
                    self.dirty = false;
                }
                "discard" => {
                    self.draft = self.active.clone();
                    self.dirty = false;
                }
                "reset" => {
                    self.draft = definition();
                    self.dirty = true;
                }
                "counts" => {
                    for (key, count, row) in [
                        ("buttons", number(2), json!([0, 0, 0])),
                        ("axes", number(3), json!([0, 48, 0, 4095, 0, 0, 0])),
                        ("hats", number(4), json!([0, 1, 2, 3])),
                        ("encoders", number(5), json!([0, 1])),
                        ("selectors", number(6), json!([0, 1])),
                    ] {
                        self.draft[key]
                            .as_array_mut()
                            .unwrap()
                            .resize(count as usize, row);
                    }
                    self.draft["expanderMask"] = json!(number(7));
                    self.dirty = true;
                }
                "name" => {
                    let text: Vec<u8> = args[2]
                        .as_bytes()
                        .chunks(2)
                        .map(|v| u8::from_str_radix(std::str::from_utf8(v).unwrap(), 16).unwrap())
                        .collect();
                    self.draft["name"] = json!(String::from_utf8(text).unwrap());
                    self.dirty = true;
                }
                "button" | "axis" | "hat" | "encoder" | "selector" => {
                    let key = match args[0] {
                        "button" => "buttons",
                        "axis" => "axes",
                        "hat" => "hats",
                        "encoder" => "encoders",
                        _ => "selectors",
                    };
                    let index = number(2) as usize;
                    self.draft[key][index] = json!(args[3..]
                        .iter()
                        .map(|n| n.parse::<u32>().unwrap())
                        .collect::<Vec<_>>());
                    self.dirty = true;
                }
                "save" => self.saved = self.generation,
                "apply" | "apply_save" => {
                    if !self.ignore_apply {
                        self.active = self.draft.clone();
                        self.generation += 1;
                        self.dirty = false;
                        if args[0] == "apply_save" {
                            self.saved = self.generation;
                        }
                    }
                }
                _ => error = Some("invalid_request"),
            }
            format!(
                "{}\n{}\r\n",
                error.map_or_else(|| "OK".into(), |e| format!("ERR {e}")),
                self.snapshot(args[0] == "get_active")
            )
        } else if let Some(child) = command.strip_prefix("action ") {
            let args: Vec<_> = child.split(' ').collect();
            let mut result = self.header();
            match args[0] {
                "describe" => {
                    let offset = args[1].parse::<usize>().unwrap();
                    result["total"] = json!(1);
                    result["next"] = Value::Null;
                    result["actions"] = if offset == 0 {
                        json!([descriptor()])
                    } else {
                        json!([])
                    };
                }
                "state" => {
                    result["states"] = json!([{"id":"led.boolean.set_enabled","values":[self.led]}])
                }
                "invoke" => {
                    if self.token.is_some() {
                        result = json!({"ok":false,"error":"outputBusy"});
                    } else {
                        self.led = args[3] == "1";
                        result["id"] = json!(args[2]);
                        result["values"] = json!([self.led]);
                    }
                }
                "claim" => {
                    if self.token.as_deref().is_some_and(|token| token != args[3]) {
                        result = json!({"ok":false,"error":"outputBusy"});
                    } else {
                        if self.token.is_none() {
                            self.lease += 1;
                        }
                        self.token = Some(args[3].into());
                        self.fallback = args[5] == "1";
                        self.last_owned = Some(self.led);
                        result["leaseId"] = json!(self.lease);
                        result["endpoint"] = json!("led.boolean");
                        result["remainingMs"] = json!(args[4].parse::<u32>().unwrap());
                    }
                }
                "renew" | "release" => {
                    if self.token.as_deref() != Some(args[1]) || self.lease.to_string() != args[2] {
                        result = json!({"ok":false,"error":"outputNotOwned"});
                    } else {
                        result["leaseId"] = json!(self.lease);
                        result["endpoint"] = json!("led.boolean");
                        if args[0] == "release" {
                            self.token = None;
                            let preserve =
                                self.guarded_restore && self.last_owned != Some(self.led);
                            if !preserve {
                                self.led = self.fallback;
                            }
                            if self.guarded_restore {
                                result["fallbackStatus"] =
                                    json!(if preserve { "preserved" } else { "applied" });
                            }
                            result["released"] = json!(true);
                        } else {
                            result["remainingMs"] = json!(args[3].parse::<u32>().unwrap());
                        }
                    }
                }
                "invoke_owned" => {
                    if self.token.as_deref() != Some(args[1]) || self.lease.to_string() != args[2] {
                        result = json!({"ok":false,"error":"outputNotOwned"});
                    } else {
                        self.led = args[5] == "1";
                        self.last_owned = Some(self.led);
                        result["id"] = json!(args[4]);
                        result["values"] = json!([self.led]);
                    }
                }
                "ownership" => {
                    result.as_object_mut().unwrap().remove("ownership");
                    result["total"] = json!(1);
                    result["next"] = Value::Null;
                    result["ownership"] = json!([{"endpoint":"led.boolean","actionId":"led.boolean.set_enabled","owned":self.token.is_some(),"leaseId":self.lease,"remainingMs":if self.token.is_some(){5000}else{0},"fallback":if self.lease>0{vec![json!(self.fallback)]}else{vec![]},"fallbackStatus":"none"}]);
                }
                _ => result = json!({"ok":false,"error":"unknownCommand"}),
            }
            format!("{result}\r\n")
        } else if let Some(child) = command.strip_prefix("events ") {
            let args: Vec<_> = child.split(' ').collect();
            let buttons = layout(&self.active)["buttons"].as_u64().unwrap() as usize;
            let mut result = json!({"ok":true,"schemaVersion":1,"stream":self.generation,"head":self.head,"oldest":1,"overflow":0,"buttonCount":buttons,"held":vec![false;buttons],"encoderButtons":[]});
            if args[0] == "read" {
                let stream = args[1].parse::<u32>().unwrap();
                let after = args[2].parse::<u32>().unwrap();
                if stream != self.generation {
                    result = json!({"ok":false,"error":"staleStream"});
                } else if after > self.head {
                    result = json!({"ok":false,"error":"invalidCursor"});
                } else {
                    let next = (after + 16).min(self.head);
                    result["after"] = json!(after);
                    result["next"] = json!(next);
                    result["more"] = json!(next < self.head);
                    result["events"]=json!(((after+1)..=next).map(|sequence|json!({"sequence":sequence,"control":0,"pressed":sequence%2==1})).collect::<Vec<_>>());
                }
            }
            format!("{result}\r\n")
        } else {
            "ERR unknown command\r\n".into()
        };
        if self
            .response_override
            .as_ref()
            .is_some_and(|(prefix, _)| command.starts_with(prefix))
        {
            let (_, value) = self.response_override.take().unwrap();
            response = format!("{value}\r\n");
        }
        if self
            .drop_command
            .as_ref()
            .is_some_and(|prefix| command.starts_with(prefix))
        {
            self.drop_command = None;
            response.clear();
        }
        response.into_bytes()
    }
}
struct FakeFactory(Arc<Mutex<Device>>);
impl Factory for FakeFactory {
    fn ports(&mut self) -> Result<Vec<PortInfo>, String> {
        Ok(self.0.lock().unwrap().ports.clone())
    }
    fn open(
        &mut self,
        _: &PortInfo,
        _: Duration,
    ) -> Result<Box<dyn crate::transport::Connection>, String> {
        self.0.lock().unwrap().opens += 1;
        Ok(Box::new(FakeConnection {
            device: self.0.clone(),
            output: format!("{}\r\n", module_console::CONSOLE_BANNER)
                .into_bytes()
                .into(),
            input: vec![],
        }))
    }
}
struct FakeConnection {
    device: Arc<Mutex<Device>>,
    output: VecDeque<u8>,
    input: Vec<u8>,
}
impl Read for FakeConnection {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.output.is_empty() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let count = out.len().min(13).min(self.output.len());
        for byte in &mut out[..count] {
            *byte = self.output.pop_front().unwrap();
        }
        Ok(count)
    }
}
impl Write for FakeConnection {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        for byte in bytes {
            if *byte == b'\n' {
                let command = String::from_utf8(std::mem::take(&mut self.input)).unwrap();
                self.output
                    .extend(self.device.lock().unwrap().run(&command));
            } else {
                self.input.push(*byte);
            }
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl crate::transport::Connection for FakeConnection {
    fn set_control_lines(&mut self, dtr: bool, _: bool) -> Result<(), String> {
        let mut d = self.device.lock().unwrap();
        d.controls.push(dtr);
        if !dtr {
            d.owner = 0;
            if d.token.take().is_some() {
                d.led = d.fallback;
            }
        }
        Ok(())
    }
}
fn setup() -> (ModuleUsbService, Arc<Mutex<Device>>) {
    let d = Arc::new(Mutex::new(Device::default()));
    (
        ModuleUsbService::with_factory(Box::new(FakeFactory(d.clone())), timing()),
        d,
    )
}
fn connect(service: &ModuleUsbService) -> UsbSnapshot {
    service
        .request(UsbRequest::Connect { port: port() })
        .unwrap()
}
fn actions(service: &ModuleUsbService, route: &UsbRouteToken) -> UsbSnapshot {
    service
        .request(UsbRequest::Action {
            route: route.clone(),
            action: ActionRequest::Describe,
        })
        .unwrap()
}
fn claim(service: &ModuleUsbService, route: &UsbRouteToken) {
    service
        .request(UsbRequest::Claim {
            route: route.clone(),
            client_id: "editor-a".into(),
        })
        .unwrap();
}

#[test]
fn discovery_is_persistent_verified_and_read_only() {
    let (service, d) = setup();
    assert!(!service.snapshot().connected);
    let snapshot = connect(&service);
    let route = snapshot.route.unwrap();
    assert_eq!(route.module_id, MODULE);
    assert_eq!(route.boot, 100);
    assert_eq!(route.configuration_generation, 1);
    assert_eq!(snapshot.identity.unwrap().profile, "Panel with spaces");
    assert!(snapshot.claimed_by.is_none());
    service.request(UsbRequest::Refresh).unwrap();
    connect(&service);
    let device = d.lock().unwrap();
    assert_eq!(device.opens, 1);
    assert_eq!(device.controls, vec![true]);
    assert!(device
        .writes
        .iter()
        .all(|line| line == "info" || line == "can status" || line == "config get_active"));
    drop(device);
    service.shutdown();
    assert!(!service.snapshot().connected);
    assert!(service.request(UsbRequest::Refresh).is_err());
    assert_eq!(d.lock().unwrap().controls, vec![true, false]);
}
#[test]
fn wrong_metadata_identity_replaced_port_and_boot_are_rejected() {
    let (service, d) = setup();
    let mut wrong = port();
    wrong.friendly_name = Some("Gateway".into());
    assert!(service
        .request(UsbRequest::Connect { port: wrong })
        .is_err());
    assert_eq!(d.lock().unwrap().opens, 0);
    d.lock().unwrap().identity_module = "4f53474411112233445566778899aabb".into();
    assert!(service
        .request(UsbRequest::Connect { port: port() })
        .is_err());
    assert!(!service.snapshot().connected);
    d.lock().unwrap().identity_module = MODULE.into();
    let route = connect(&service).route.unwrap();
    d.lock().unwrap().boot = 101;
    assert!(service
        .request(UsbRequest::Claim {
            route,
            client_id: "editor-a".into()
        })
        .is_err());
    assert!(!service.snapshot().connected);
    assert!(!d
        .lock()
        .unwrap()
        .writes
        .iter()
        .any(|line| line.starts_with("config claim")));
}
#[test]
fn output_only_modules_remain_visible_without_hid_inputs() {
    let (service, d) = setup();
    {
        let mut device = d.lock().unwrap();
        device.active["buttons"] = json!([]);
        device.draft = device.active.clone();
    }
    let snapshot = connect(&service);
    assert_eq!(snapshot.identity.unwrap().button_count, 0);
    let route = snapshot.route.unwrap();
    let described = actions(&service, &route);
    assert_eq!(
        described.actions.unwrap()["actions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let events = service.request(UsbRequest::EventsState { route }).unwrap();
    assert_eq!(events.events.unwrap()["buttonCount"], 0);
}
#[test]
fn writer_is_exclusive_reads_do_not_reclaim_and_stale_routes_never_write() {
    let (service, d) = setup();
    let route = connect(&service).route.unwrap();
    claim(&service, &route);
    assert!(service
        .request(UsbRequest::Claim {
            route: route.clone(),
            client_id: "editor-b".into()
        })
        .is_err());
    service
        .request(UsbRequest::ConfigRead {
            route: route.clone(),
        })
        .unwrap();
    assert_eq!(
        d.lock()
            .unwrap()
            .writes
            .iter()
            .filter(|line| line.starts_with("config claim"))
            .count(),
        1
    );
    d.lock().unwrap().owner = 0;
    service.request(UsbRequest::Refresh).unwrap();
    assert!(service.snapshot().claimed_by.is_none());
    assert!(service
        .request(UsbRequest::ConfigChange {
            route: route.clone(),
            client_id: "editor-a".into(),
            change: ConfigChange::Reset
        })
        .is_err());
    claim(&service, &route);
    service
        .request(UsbRequest::Release {
            route: route.clone(),
            client_id: "editor-a".into(),
        })
        .unwrap();
    assert!(service.snapshot().claimed_by.is_none());
    service.request(UsbRequest::Disconnect).unwrap();
    let next = connect(&service).route.unwrap();
    assert_ne!(route.connection_generation, next.connection_generation);
    assert!(service
        .request(UsbRequest::Claim {
            route,
            client_id: "editor-a".into()
        })
        .is_err());
}
#[test]
fn explicit_config_apply_save_reconciles_a_lost_commit_without_replay() {
    let (service, d) = setup();
    let route = connect(&service).route.unwrap();
    claim(&service, &route);
    let mut config = definition();
    config["name"] = json!("Renamed USB panel");
    d.lock().unwrap().drop_command = Some("config apply_save".into());
    let applied = service
        .request(UsbRequest::ConfigChange {
            route: route.clone(),
            client_id: "editor-a".into(),
            change: ConfigChange::Apply {
                config: config.clone(),
                save: true,
            },
        })
        .unwrap();
    assert_eq!(applied.config.as_ref().unwrap()["config"], config);
    assert_eq!(applied.config.as_ref().unwrap()["generation"], 2);
    assert_eq!(applied.config.as_ref().unwrap()["savedGeneration"], 2);
    assert_ne!(
        applied.route.unwrap().connection_generation,
        route.connection_generation
    );
    assert!(applied.claimed_by.is_none());
    assert_eq!(
        d.lock()
            .unwrap()
            .writes
            .iter()
            .filter(|line| line.starts_with("config apply_save"))
            .count(),
        1
    );
}
#[test]
fn staging_and_unconfirmed_commits_fail_without_replaying_mutations() {
    let (service, d) = setup();
    let route = connect(&service).route.unwrap();
    claim(&service, &route);
    d.lock().unwrap().drop_command = Some("config counts".into());
    assert!(service
        .request(UsbRequest::ConfigChange {
            route: route.clone(),
            client_id: "editor-a".into(),
            change: ConfigChange::Apply {
                config: definition(),
                save: false
            }
        })
        .is_err());
    assert!(!service.snapshot().connected);
    assert!(!d
        .lock()
        .unwrap()
        .writes
        .iter()
        .any(|line| line.starts_with("config apply")));
    let route = connect(&service).route.unwrap();
    claim(&service, &route);
    {
        let mut device = d.lock().unwrap();
        device.ignore_apply = true;
        device.drop_command = Some("config apply_save".into());
    }
    let mut config = definition();
    config["name"] = json!("Not applied");
    assert!(service
        .request(UsbRequest::ConfigChange {
            route,
            client_id: "editor-a".into(),
            change: ConfigChange::Apply { config, save: true }
        })
        .unwrap_err()
        .contains("not repeated"));
    assert_eq!(
        d.lock()
            .unwrap()
            .writes
            .iter()
            .filter(|line| line.starts_with("config apply_save"))
            .count(),
        1
    );
}
#[test]
fn reset_discard_and_save_keep_the_active_configuration_separate() {
    let (service, d) = setup();
    let route = connect(&service).route.unwrap();
    claim(&service, &route);
    let reset = service
        .request(UsbRequest::ConfigChange {
            route: route.clone(),
            client_id: "editor-a".into(),
            change: ConfigChange::Reset,
        })
        .unwrap();
    assert_eq!(reset.config.as_ref().unwrap()["config"], definition());
    assert_eq!(reset.draft.as_ref().unwrap()["view"], "draft");
    service
        .request(UsbRequest::ConfigChange {
            route: route.clone(),
            client_id: "editor-a".into(),
            change: ConfigChange::Discard,
        })
        .unwrap();
    let saved = service
        .request(UsbRequest::ConfigChange {
            route,
            client_id: "editor-a".into(),
            change: ConfigChange::Save,
        })
        .unwrap();
    assert_eq!(saved.config.unwrap()["savedGeneration"], 1);
    assert_eq!(d.lock().unwrap().generation, 1);
}
#[test]
fn actions_validate_catalogue_arguments_readback_and_never_claim_configuration() {
    let (service, d) = setup();
    let route = connect(&service).route.unwrap();
    let found = actions(&service, &route);
    assert_eq!(
        found.actions.as_ref().unwrap()["states"][0]["values"],
        json!([false])
    );
    let before = d.lock().unwrap().writes.len();
    assert!(service
        .request(UsbRequest::Action {
            route: route.clone(),
            action: ActionRequest::Invoke {
                catalogue_revision: 2,
                action_id: "led.boolean.set_enabled".into(),
                values: vec![json!(true)]
            }
        })
        .is_err());
    assert!(service
        .request(UsbRequest::Action {
            route: route.clone(),
            action: ActionRequest::Invoke {
                catalogue_revision: 1,
                action_id: "led.boolean.set_enabled".into(),
                values: vec![json!("bad")]
            }
        })
        .is_err());
    assert!(!d.lock().unwrap().writes[before..]
        .iter()
        .any(|line| line.starts_with("action invoke")));
    let result = service
        .request(UsbRequest::Action {
            route,
            action: ActionRequest::Invoke {
                catalogue_revision: 1,
                action_id: "led.boolean.set_enabled".into(),
                values: vec![json!(true)],
            },
        })
        .unwrap();
    assert_eq!(
        result.actions.unwrap()["states"][0]["values"],
        json!([true])
    );
    let device = d.lock().unwrap();
    assert_eq!(device.owner, 0);
    assert_eq!(device.generation, 1);
    assert!(device.led);
}
#[test]
fn endpoint_tokens_are_input_only_and_leases_do_not_bypass_writer_or_state_guards() {
    let (service, d) = setup();
    let route = connect(&service).route.unwrap();
    actions(&service, &route);
    let claimed = service
        .request(UsbRequest::Action {
            route: route.clone(),
            action: ActionRequest::Claim {
                catalogue_revision: 1,
                action_id: "led.boolean.set_enabled".into(),
                values: vec![json!(false)],
                token: TOKEN.into(),
                ttl_ms: 5000,
            },
        })
        .unwrap();
    let lease = claimed.actions.as_ref().unwrap()["lastLease"]["leaseId"]
        .as_u64()
        .unwrap() as u32;
    assert!(!serde_json::to_string(&claimed).unwrap().contains(TOKEN));
    assert!(!d.lock().unwrap().led);
    service
        .request(UsbRequest::Action {
            route: route.clone(),
            action: ActionRequest::Renew {
                token: TOKEN.into(),
                lease_id: lease,
                ttl_ms: 5000,
            },
        })
        .unwrap();
    service
        .request(UsbRequest::Action {
            route: route.clone(),
            action: ActionRequest::InvokeOwned {
                catalogue_revision: 1,
                action_id: "led.boolean.set_enabled".into(),
                values: vec![json!(true)],
                token: TOKEN.into(),
                lease_id: lease,
            },
        })
        .unwrap();
    assert!(d.lock().unwrap().led);
    let ownership = service
        .request(UsbRequest::Action {
            route: route.clone(),
            action: ActionRequest::Ownership { offset: 0 },
        })
        .unwrap();
    assert_eq!(
        ownership.actions.unwrap()["ownershipPage"]["ownership"][0]["owned"],
        true
    );
    service
        .request(UsbRequest::Action {
            route,
            action: ActionRequest::Release {
                token: TOKEN.into(),
                lease_id: lease,
            },
        })
        .unwrap();
    assert!(!d.lock().unwrap().led);
    assert!(d.lock().unwrap().token.is_none());
}
#[test]
fn lost_action_reply_is_uncertain_without_an_automatic_second_invocation() {
    let (service, d) = setup();
    let route = connect(&service).route.unwrap();
    actions(&service, &route);
    d.lock().unwrap().drop_command = Some("action invoke ".into());
    assert_eq!(
        service
            .request(UsbRequest::Action {
                route,
                action: ActionRequest::Invoke {
                    catalogue_revision: 1,
                    action_id: "led.boolean.set_enabled".into(),
                    values: vec![json!(true)]
                }
            })
            .unwrap_err(),
        crate::actions::UNKNOWN
    );
    assert!(!service.snapshot().connected);
    assert_eq!(service.snapshot().actions.unwrap()["status"], "uncertain");
    assert!(d.lock().unwrap().led);
    assert_eq!(
        d.lock()
            .unwrap()
            .writes
            .iter()
            .filter(|line| line.starts_with("action invoke "))
            .count(),
        1
    );
    let route = connect(&service).route.unwrap();
    let fresh = actions(&service, &route);
    assert_eq!(fresh.actions.unwrap()["states"][0]["values"], json!([true]));
}
#[test]
fn events_require_a_current_baseline_and_never_replay_preconnection_history() {
    let (service, d) = setup();
    let route = connect(&service).route.unwrap();
    assert!(service
        .request(UsbRequest::EventsRead {
            route: route.clone(),
            stream: 1,
            after: 0
        })
        .is_err());
    let base = service
        .request(UsbRequest::EventsState {
            route: route.clone(),
        })
        .unwrap();
    assert_eq!(base.events.unwrap()["head"], 10);
    assert!(service
        .request(UsbRequest::EventsRead {
            route: route.clone(),
            stream: 1,
            after: 0
        })
        .is_err());
    d.lock().unwrap().head = 12;
    let page = service
        .request(UsbRequest::EventsRead {
            route: route.clone(),
            stream: 1,
            after: 10,
        })
        .unwrap();
    assert_eq!(
        page.events.as_ref().unwrap()["events"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(page.events.unwrap()["next"], 12);
    service.request(UsbRequest::Disconnect).unwrap();
    let next = connect(&service).route.unwrap();
    assert!(service.snapshot().events.is_none());
    assert!(service
        .request(UsbRequest::EventsRead {
            route: next.clone(),
            stream: 1,
            after: 10
        })
        .is_err());
    service
        .request(UsbRequest::EventsState {
            route: next.clone(),
        })
        .unwrap();
    assert!(service
        .request(UsbRequest::EventsRead {
            route: next,
            stream: 1,
            after: 10
        })
        .is_err());
}
#[test]
fn invalid_status_closes_access_and_external_configuration_change_invalidates_caches() {
    let (service, d) = setup();
    let route = connect(&service).route.unwrap();
    actions(&service, &route);
    service
        .request(UsbRequest::EventsState {
            route: route.clone(),
        })
        .unwrap();
    {
        let mut device = d.lock().unwrap();
        device.generation = 2;
        device.saved = 2;
    }
    let fresh = service.request(UsbRequest::Refresh).unwrap();
    assert_eq!(fresh.route.unwrap().configuration_generation, 2);
    assert!(fresh.actions.is_none());
    assert!(fresh.events.is_none());
    assert!(service
        .request(UsbRequest::Action {
            route,
            action: ActionRequest::Describe
        })
        .is_err());
    d.lock().unwrap().malformed_status = true;
    assert!(service.request(UsbRequest::Refresh).is_err());
    assert!(!service.snapshot().connected);
}

#[test]
fn an_unchanged_configuration_still_requires_a_new_generation_after_apply() {
    let (service, device) = setup();
    let route = connect(&service).route.unwrap();
    claim(&service, &route);
    device.lock().unwrap().ignore_apply = true;
    assert!(service
        .request(UsbRequest::ConfigChange {
            route,
            client_id: "editor-a".into(),
            change: ConfigChange::Apply {
                config: definition(),
                save: false
            },
        })
        .unwrap_err()
        .contains("not repeated"));
    assert_eq!(
        device
            .lock()
            .unwrap()
            .writes
            .iter()
            .filter(|line| line.starts_with("config apply "))
            .count(),
        1
    );
}

#[test]
fn save_persists_active_settings_while_retaining_a_separate_dirty_draft() {
    let (service, device) = setup();
    {
        let mut device = device.lock().unwrap();
        device.active["name"] = json!("Active name");
        device.draft = device.active.clone();
        device.generation = 3;
        device.saved = 2;
    }
    let route = connect(&service).route.unwrap();
    claim(&service, &route);
    service
        .request(UsbRequest::ConfigChange {
            route: route.clone(),
            client_id: "editor-a".into(),
            change: ConfigChange::Reset,
        })
        .unwrap();
    let saved = service
        .request(UsbRequest::ConfigChange {
            route,
            client_id: "editor-a".into(),
            change: ConfigChange::Save,
        })
        .unwrap();
    assert_eq!(
        saved.config.as_ref().unwrap()["config"]["name"],
        "Active name"
    );
    assert_eq!(saved.config.as_ref().unwrap()["savedGeneration"], 3);
    assert_eq!(
        saved.draft.as_ref().unwrap()["config"]["name"],
        "Panel with spaces"
    );
    assert_eq!(saved.draft.as_ref().unwrap()["dirty"], true);
}

#[test]
fn malformed_applied_action_readback_blocks_more_mutations_until_explicit_refresh() {
    let (service, device) = setup();
    let route = connect(&service).route.unwrap();
    actions(&service, &route);
    device.lock().unwrap().response_override = Some((
        "action invoke ".into(),
        json!({"ok":true,"schemaVersion":1,"catalogueRevision":1,"id":"different-action","values":[true]}),
    ));
    let invoke = || UsbRequest::Action {
        route: route.clone(),
        action: ActionRequest::Invoke {
            catalogue_revision: 1,
            action_id: "led.boolean.set_enabled".into(),
            values: vec![json!(true)],
        },
    };
    assert_eq!(
        service.request(invoke()).unwrap_err(),
        crate::actions::UNKNOWN
    );
    assert_eq!(service.snapshot().actions.unwrap()["status"], "uncertain");
    assert!(service.request(invoke()).is_err());
    assert_eq!(
        device
            .lock()
            .unwrap()
            .writes
            .iter()
            .filter(|line| line.starts_with("action invoke "))
            .count(),
        1
    );
    let fresh = service
        .request(UsbRequest::Action {
            route,
            action: ActionRequest::State,
        })
        .unwrap();
    assert_eq!(fresh.actions.unwrap()["states"][0]["values"], json!([true]));
}

#[test]
fn bad_catalogue_or_event_pages_never_retain_a_usable_old_cache() {
    let (service, device) = setup();
    let route = connect(&service).route.unwrap();
    actions(&service, &route);
    device.lock().unwrap().response_override = Some((
        "action describe ".into(),
        json!({"ok":true,"schemaVersion":1,"catalogueRevision":2,"total":1,"next":null,"actions":[{}]}),
    ));
    assert!(service
        .request(UsbRequest::Action {
            route: route.clone(),
            action: ActionRequest::Describe
        })
        .is_err());
    assert!(service.snapshot().actions.is_none());
    service
        .request(UsbRequest::EventsState {
            route: route.clone(),
        })
        .unwrap();
    device.lock().unwrap().response_override = Some((
        "events read ".into(),
        json!({"ok":true,"schemaVersion":1,"stream":1,"head":12,"oldest":1,"overflow":0,"buttonCount":1,"held":[false],"encoderButtons":[],"after":10,"next":12,"more":false,"events":[{"sequence":12,"control":0,"pressed":true}]}),
    ));
    assert!(service
        .request(UsbRequest::EventsRead {
            route: route.clone(),
            stream: 1,
            after: 10
        })
        .is_err());
    assert!(service.snapshot().events.is_none());
    assert!(service
        .request(UsbRequest::EventsRead {
            route,
            stream: 1,
            after: 10
        })
        .is_err());
}

fn generic_port() -> PortInfo {
    let mut port = port();
    port.manufacturer = Some("(Standard port types)".into());
    port.friendly_name = Some("USB Serial Device (COM8)".into());
    port
}
#[test]
fn explicit_generic_windows_port_is_verified_with_no_automatic_probing() {
    let (service, device) = setup();
    device.lock().unwrap().ports = vec![generic_port()];
    assert!(service
        .request(UsbRequest::Connect {
            port: generic_port()
        })
        .is_err());
    assert_eq!(device.lock().unwrap().opens, 0);
    let snapshot = service
        .request(UsbRequest::ConnectExplicit {
            port: generic_port(),
        })
        .unwrap();
    assert!(snapshot.connected);
    assert_eq!(snapshot.identity.unwrap().module_id, MODULE);
    let d = device.lock().unwrap();
    assert!(d
        .writes
        .iter()
        .all(|v| ["info", "can status", "config get_active"].contains(&v.as_str())));
}
#[test]
fn explicit_port_still_rejects_known_roles_forged_metadata_and_wrong_wire_identity() {
    let (service, device) = setup();
    let mut actual = generic_port();
    actual.friendly_name = Some("OpenSimGear CAN Gateway".into());
    device.lock().unwrap().ports = vec![actual];
    assert!(service
        .request(UsbRequest::ConnectExplicit {
            port: generic_port()
        })
        .is_err());
    assert_eq!(device.lock().unwrap().opens, 0);
    device.lock().unwrap().ports = vec![generic_port()];
    device.lock().unwrap().identity_model = "stm32-can-gateway".into();
    assert!(service
        .request(UsbRequest::ConnectExplicit {
            port: generic_port()
        })
        .is_err());
    assert!(!service.snapshot().connected);
    assert_eq!(device.lock().unwrap().writes, vec!["info"]);
    device.lock().unwrap().identity_model = "device-template".into();
    device.lock().unwrap().identity_module = "4f53474411112233445566778899aabb".into();
    assert!(service
        .request(UsbRequest::ConnectExplicit {
            port: generic_port()
        })
        .is_err());
    assert!(!service.snapshot().connected);
    assert_eq!(device.lock().unwrap().writes, vec!["info", "info"]);
}
#[test]
fn explicit_verified_uid_reconnects_on_a_new_port_after_one_commit() {
    let (service, device) = setup();
    device.lock().unwrap().ports = vec![generic_port()];
    let route = service
        .request(UsbRequest::ConnectExplicit {
            port: generic_port(),
        })
        .unwrap()
        .route
        .unwrap();
    claim(&service, &route);
    let mut expected = definition();
    expected["name"] = json!("Explicit Windows module");
    let mut moved = generic_port();
    moved.path = "COM9".into();
    device.lock().unwrap().ports = vec![moved];
    device.lock().unwrap().drop_command = Some("config apply_save ".into());
    let snapshot = service
        .request(UsbRequest::ConfigChange {
            route: route.clone(),
            client_id: "editor-a".into(),
            change: ConfigChange::Apply {
                config: expected.clone(),
                save: true,
            },
        })
        .unwrap();
    assert_eq!(snapshot.config.unwrap()["config"], expected);
    assert_eq!(snapshot.port.as_deref(), Some("COM9"));
    assert_eq!(snapshot.route.unwrap().module_id, route.module_id);
    assert_eq!(
        device
            .lock()
            .unwrap()
            .writes
            .iter()
            .filter(|v| v.starts_with("config apply_save "))
            .count(),
        1
    );
}

#[test]
fn complete_negative_capability_replies_preserve_base_connection_and_do_not_invent_ports() {
    let (service, device) = setup();
    device.lock().unwrap().actions_supported = false;
    device.lock().unwrap().events_supported = false;
    let route = connect(&service).route.unwrap();
    let snapshot = service
        .request(UsbRequest::Action {
            route: route.clone(),
            action: ActionRequest::Describe,
        })
        .unwrap();
    assert!(snapshot.connected);
    let marker = snapshot.actions.unwrap();
    assert_eq!(marker["status"], "unsupported");
    assert_eq!(marker["route"], json!(route));
    assert!(marker.get("actions").is_none() && marker.get("ok").is_none());
    let snapshot = service
        .request(UsbRequest::EventsState {
            route: route.clone(),
        })
        .unwrap();
    let marker = snapshot.events.unwrap();
    assert!(snapshot.connected && snapshot.config.is_some());
    assert_eq!(marker["status"], "unsupported");
    assert_eq!(marker["route"], json!(route));
    assert!(marker.get("buttons").is_none() && marker.get("ok").is_none());
    assert!(service
        .request(UsbRequest::EventsRead {
            route: route.clone(),
            stream: 1,
            after: 10
        })
        .is_err());
    assert!(service
        .request(UsbRequest::Action {
            route: route.clone(),
            action: ActionRequest::Invoke {
                catalogue_revision: 1,
                action_id: "led.boolean.set_enabled".into(),
                values: vec![json!(true)]
            }
        })
        .is_err());
    claim(&service, &route);
    assert_eq!(device.lock().unwrap().opens, 1);
    assert!(!device
        .lock()
        .unwrap()
        .writes
        .iter()
        .any(|v| v.starts_with("action invoke ")));
}
#[test]
fn malformed_or_timed_out_capability_replies_are_never_classified_as_unsupported() {
    for malformed in [true, false] {
        let (service, device) = setup();
        let route = connect(&service).route.unwrap();
        if malformed {
            device.lock().unwrap().response_override =
                Some(("action describe ".into(), json!("ERR unknown command")));
        } else {
            device.lock().unwrap().drop_command = Some("action describe ".into());
        }
        assert!(service
            .request(UsbRequest::Action {
                route,
                action: ActionRequest::Describe
            })
            .is_err());
        let snapshot = service.snapshot();
        assert!(!snapshot.connected);
        assert!(snapshot.actions.is_none());
        assert!(snapshot.events.is_none());
    }
}

#[test]
fn guarded_policy_and_preserved_receipt_survive_actual_usb_worker_without_replaying_release() {
    let (service, device) = setup();
    device.lock().unwrap().guarded_restore = true;
    let route = connect(&service).route.unwrap();
    let described = actions(&service, &route);
    assert_eq!(
        described.actions.as_ref().unwrap()["ownership"]["restorePolicy"],
        "if-unchanged"
    );
    let projected =
        crate::profile_resource_ports::actions(described.actions.as_ref().unwrap()).unwrap();
    assert!(projected[0].contract.supports_guarded_setter());
    let claimed = service
        .request(UsbRequest::Action {
            route: route.clone(),
            action: ActionRequest::Claim {
                catalogue_revision: 1,
                action_id: "led.boolean.set_enabled".into(),
                values: vec![json!(true)],
                token: TOKEN.into(),
                ttl_ms: 5000,
            },
        })
        .unwrap();
    let lease_id = claimed.actions.as_ref().unwrap()["lastLease"]["leaseId"]
        .as_u64()
        .unwrap() as u32;
    service
        .request(UsbRequest::Action {
            route: route.clone(),
            action: ActionRequest::InvokeOwned {
                catalogue_revision: 1,
                action_id: "led.boolean.set_enabled".into(),
                values: vec![json!(true)],
                token: TOKEN.into(),
                lease_id,
            },
        })
        .unwrap();
    device.lock().unwrap().led = false;
    let released = service
        .request(UsbRequest::Action {
            route,
            action: ActionRequest::Release {
                token: TOKEN.into(),
                lease_id,
            },
        })
        .unwrap();
    assert_eq!(
        released.actions.as_ref().unwrap()["lastLease"]["fallbackStatus"],
        "preserved"
    );
    assert!(!device.lock().unwrap().led);
    assert!(!serde_json::to_string(&released).unwrap().contains(TOKEN));
    assert_eq!(
        device
            .lock()
            .unwrap()
            .writes
            .iter()
            .filter(|line| line.starts_with("action release "))
            .count(),
        1
    );
}
