//! Run simulator bindings independently of frontend polling, through normal adapters.
use crate::dcs_bios::{Catalogue, Config, Intent, LedBinding, Session, Transport};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const CLIENT: &str = "cockpit-dcs-bios-integration";
type Panel = Arc<dyn Fn(Value) -> Value + Send + Sync>;
struct Settings {
    closed: bool,
    config: Option<Config>,
    worker: Option<JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
}
pub struct DcsRuntime {
    settings: Mutex<Settings>,
    state: Arc<Mutex<Value>>,
    path: PathBuf,
    panel: Panel,
}

pub fn config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("OSG_DCS_BIOS_CONFIG") {
        return PathBuf::from(path);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    #[cfg(target_os = "macos")]
    let root = home.join("Library/Application Support/org.opensimgear.cockpit");
    #[cfg(target_os = "windows")]
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or(home)
        .join("OpenSimGear/Cockpit");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"))
        .join("opensimgear/cockpit");
    root.join("dcs-bios.json")
}
fn stopped(error: Option<String>) -> Value {
    json!({"running":false,"aircraft":null,"missionActive":false,"feedbackFresh":false,"feedbackAgeMs":null,"status":if error.is_some(){"error"}else{"stopped"},"error":error,"eventOverflow":0,"commandsSent":0,"ledValue":null})
}
fn call(panel: &Panel, config: &Config, command: &str, extra: Value) -> Result<Value, String> {
    let mut request = json!({"command":command,"clientId":CLIENT,"gatewayId":config.gateway_id,"moduleId":config.module_id});
    for (key, value) in extra.as_object().unwrap() {
        request[key] = value.clone();
    }
    let reply = panel(request);
    if reply["ok"] != true {
        return Err(reply["error"]
            .as_str()
            .unwrap_or("The panel request failed.")
            .into());
    }
    Ok(reply["state"].clone())
}
fn connected_module(state: &Value, config: &Config) -> Result<Value, String> {
    let module = state["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|m| m["id"] == config.module_id)
        .ok_or("Reconnect the bound panel.")?;
    if state["connected"] != true
        || state["reconnecting"] == true
        || state["layoutPending"] == true
        || module["online"] != true
    {
        return Err("Reconnect the bound panel and wait for the gateway layout to settle.".into());
    }
    Ok(module.clone())
}
fn ready(state: &Value, config: &Config) -> Result<Value, String> {
    let module = connected_module(state, config)?;
    if module["enabled"] != false
        || module["published"] != false
        || !module["reportId"].is_null()
        || module["directUsb"] != false
    {
        return Err("Disable the bound panel's HID controller and disconnect its direct USB input before starting DCS-BIOS.".into());
    }
    Ok(module.clone())
}
struct Route {
    value: Value,
    events: Value,
    led: LedBinding,
}
fn prepare(panel: &Panel, config: &Config) -> Result<Route, String> {
    let state = call(panel, config, "status", json!({}))?;
    let module = ready(&state, config)?;
    let state = call(panel, config, "action_describe", json!({}))?;
    ready(&state, config)?;
    let led = crate::dcs_bios::validate_led(
        &state["moduleActions"][&config.module_id],
        &config.led_action_id,
    )?;
    let state = call(
        panel,
        config,
        "events_state",
        json!({"boot":module["boot"]}),
    )?;
    ready(&state, config)?;
    let events = state["moduleEvents"][&config.module_id].clone();
    crate::panel_events::validate(&events, None)?;
    if config.button as u64 >= events["buttonCount"].as_u64().unwrap()
        || events["encoderButtons"]
            .as_array()
            .unwrap()
            .contains(&json!(config.button))
    {
        return Err(
            "The master-caution binding requires a mapped button, not an encoder pulse.".into(),
        );
    }
    Ok(Route {
        value: events["route"].clone(),
        events,
        led,
    })
}

impl DcsRuntime {
    pub fn new(panel: Panel, path: PathBuf) -> Self {
        let (config, error) = match Config::load(&path) {
            Ok(config) => (config, None),
            Err(error) => (None, Some(error)),
        };
        Self {
            settings: Mutex::new(Settings {
                closed: false,
                config,
                worker: None,
                cancel: Arc::new(AtomicBool::new(false)),
            }),
            state: Arc::new(Mutex::new(stopped(error))),
            path,
            panel,
        }
    }
    pub fn status(&self) -> Value {
        let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        let mut value = self.state.lock().unwrap_or_else(|p| p.into_inner()).clone();
        value["configured"] = json!(settings.config.is_some());
        value["config"] = settings
            .config
            .as_ref()
            .map(Config::value)
            .unwrap_or(Value::Null);
        value
    }
    pub fn request(&self, request: &Value) -> Result<(), String> {
        match request["command"].as_str() {
            Some("dcs_status") => Ok(()),
            Some("dcs_stop") => {
                self.stop();
                Ok(())
            }
            Some("dcs_configure") => {
                let mut settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
                if settings.closed {
                    return Err("The DCS-BIOS runtime has closed.".into());
                }
                if settings
                    .worker
                    .as_ref()
                    .is_some_and(|worker| !worker.is_finished())
                {
                    return Err("Stop DCS-BIOS before changing its profile.".into());
                }
                let config = Config::parse(&request["dcsConfig"])?;
                Catalogue::load(&PathBuf::from(&config.catalogue_path))?;
                config.save(&self.path)?;
                settings.config = Some(config);
                *self.state.lock().unwrap_or_else(|p| p.into_inner()) = stopped(None);
                Ok(())
            }
            Some("dcs_start") => {
                let mut settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
                if settings.closed {
                    return Err("The DCS-BIOS runtime has closed.".into());
                }
                if settings
                    .worker
                    .as_ref()
                    .is_some_and(|worker| !worker.is_finished())
                {
                    return Err("DCS-BIOS is already running.".into());
                }
                if let Some(worker) = settings.worker.take() {
                    let _ = worker.join();
                }
                let config = settings
                    .config
                    .clone()
                    .ok_or("Save a DCS-BIOS profile before starting.")?;
                let catalogue = Catalogue::load(&PathBuf::from(&config.catalogue_path))?;
                let route = prepare(&self.panel, &config)?;
                let transport = Transport::open(&config)?;
                let session = Session::new(config.clone(), catalogue);
                settings.cancel = Arc::new(AtomicBool::new(false));
                let (panel, state, cancel) = (
                    self.panel.clone(),
                    self.state.clone(),
                    settings.cancel.clone(),
                );
                *state.lock().unwrap_or_else(|p| p.into_inner()) = json!({"running":true,"status":"waiting","error":null,"aircraft":null,"missionActive":false,"feedbackFresh":false,"feedbackAgeMs":null,"eventOverflow":0,"commandsSent":0,"ledValue":null});
                settings.worker = Some(thread::spawn(move || {
                    worker(panel, state, cancel, config, session, transport, route)
                }));
                Ok(())
            }
            _ => Err("Unknown DCS-BIOS command.".into()),
        }
    }
    pub fn bound_led(&self, request: &Value) -> bool {
        let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        settings.worker.as_ref().is_some_and(|w| !w.is_finished())
            && settings.config.as_ref().is_some_and(|c| {
                request["gatewayId"] == c.gateway_id
                    && request["moduleId"] == c.module_id
                    && request["actionId"] == c.led_action_id
            })
    }
    pub fn stop(&self) {
        self.stop_with(false);
    }
    pub fn shutdown(&self) {
        self.stop_with(true);
    }
    fn stop_with(&self, terminal: bool) {
        let mut settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        // Startup, profile replacement and terminal close share this guard.
        // A request admitted by the manager before close cannot restart later.
        settings.closed |= terminal;
        settings.cancel.store(true, Ordering::Release);
        if let Some(worker) = settings.worker.take() {
            let _ = worker.join();
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state["running"] = json!(false);
        state["feedbackFresh"] = json!(false);
        state["status"] = json!("stopped");
    }
}
impl Drop for DcsRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn write_led(panel: &Panel, config: &Config, led: &LedBinding, on: bool) -> Result<(), String> {
    // Read first: a lost setter reply must be reconciled before another setter.
    let state = call(panel, config, "action_describe", json!({}))?;
    connected_module(&state, config)?;
    let actions = &state["moduleActions"][&config.module_id];
    let current = crate::dcs_bios::validate_led(actions, &config.led_action_id)?;
    let values = current.values(on);
    if actions["states"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|s| s["id"] == config.led_action_id && s["values"] == values)
    {
        return Ok(());
    }
    // Use the freshly described descriptor; `led` is retained as route metadata.
    let _ = led;
    call(
        panel,
        config,
        "action_invoke",
        json!({"boot":actions["boot"],"catalogueRevision":actions["catalogueRevision"],"actionId":config.led_action_id,"values":values}),
    )?;
    Ok(())
}
fn intents(
    panel: &Panel,
    config: &Config,
    session: &mut Session,
    transport: &Transport,
    route: &Route,
    intents: Vec<Intent>,
    error: &mut Option<String>,
) {
    let mut queue = std::collections::VecDeque::from(intents);
    while let Some(intent) = queue.pop_front() {
        match intent {
            Intent::Command(command) => {
                let result = transport.send(&command);
                queue.extend(session.command_result(result.is_ok()));
                if let Err(e) = result {
                    *error = Some(e);
                }
            }
            Intent::Led(on) => {
                let result = write_led(panel, config, &route.led, on);
                session.led_result(on, result.is_ok());
                if let Err(e) = result {
                    *error = Some(e);
                }
            }
        }
    }
}
fn worker(
    panel: Panel,
    state: Arc<Mutex<Value>>,
    cancel: Arc<AtomicBool>,
    config: Config,
    mut session: Session,
    transport: Transport,
    mut route: Route,
) {
    let start = Instant::now();
    let mut next_poll = Instant::now();
    let mut error = None;
    let mut cursor = route.events["head"].as_u64().unwrap() as u32;
    let mut stream = route.events["stream"].as_u64().unwrap() as u32;
    let mut baselined = false;
    let mut overflow = 0u64;
    let mut gaps = 0u64;
    while !cancel.load(Ordering::Acquire) {
        let now = start.elapsed().as_millis() as u64;
        // Bound one drain so an abusive sender cannot starve freshness or panel I/O.
        for _ in 0..128 {
            match transport.receive() {
                Ok(Some(bytes)) => {
                    let pending = session.feed(&bytes, now);
                    intents(
                        &panel,
                        &config,
                        &mut session,
                        &transport,
                        &route,
                        pending,
                        &mut error,
                    );
                }
                Ok(None) => break,
                Err(e) => {
                    error = Some(e);
                    let pending = session.panel_unavailable();
                    intents(
                        &panel,
                        &config,
                        &mut session,
                        &transport,
                        &route,
                        pending,
                        &mut error,
                    );
                    break;
                }
            }
        }
        let pending = session.tick(start.elapsed().as_millis() as u64);
        intents(
            &panel,
            &config,
            &mut session,
            &transport,
            &route,
            pending,
            &mut error,
        );
        if Instant::now() >= next_poll {
            next_poll = Instant::now() + Duration::from_millis(50);
            let result: Result<(), String> = (|| {
                let snapshot = call(&panel, &config, "status", json!({}))?;
                let module = ready(&snapshot, &config)?;
                let current = json!([
                    snapshot["epoch"],
                    module["node"],
                    module["lease"],
                    module["boot"]
                ]);
                if current != route.value {
                    route = prepare(&panel, &config)?;
                    stream = route.events["stream"].as_u64().unwrap() as u32;
                    cursor = route.events["head"].as_u64().unwrap() as u32;
                    let pending = session.panel_unavailable();
                    intents(
                        &panel,
                        &config,
                        &mut session,
                        &transport,
                        &route,
                        pending,
                        &mut error,
                    );
                    baselined = false;
                    let pending = session.refresh_led(now);
                    intents(
                        &panel,
                        &config,
                        &mut session,
                        &transport,
                        &route,
                        pending,
                        &mut error,
                    );
                }
                let live = session.state(now);
                if !baselined
                    || session.needs_baseline()
                    || live["feedbackFresh"] != true
                    || live["missionActive"] != true
                {
                    let snapshot = call(
                        &panel,
                        &config,
                        "events_state",
                        json!({"boot":module["boot"]}),
                    )?;
                    ready(&snapshot, &config)?;
                    let events = &snapshot["moduleEvents"][&config.module_id];
                    stream = events["stream"]
                        .as_u64()
                        .ok_or(crate::panel_events::INVALID)? as u32;
                    cursor = events["head"]
                        .as_u64()
                        .ok_or(crate::panel_events::INVALID)? as u32;
                    overflow = events["overflow"].as_u64().unwrap_or(0);
                    let pending = session.baseline(
                        module["boot"].as_u64().unwrap(),
                        stream,
                        cursor,
                        events["held"][config.button as usize]
                            .as_bool()
                            .ok_or(crate::panel_events::INVALID)?,
                        now,
                    );
                    intents(
                        &panel,
                        &config,
                        &mut session,
                        &transport,
                        &route,
                        pending,
                        &mut error,
                    );
                    baselined = true;
                } else {
                    for _ in 0..4 {
                        let snapshot = call(
                            &panel,
                            &config,
                            "events_read",
                            json!({"boot":module["boot"],"stream":stream,"after":cursor}),
                        )?;
                        ready(&snapshot, &config)?;
                        let events = &snapshot["moduleEvents"][&config.module_id];
                        if events["route"] != route.value {
                            return Err(
                                "The panel event route changed; re-establishing its baseline."
                                    .into(),
                            );
                        }
                        overflow = events["overflow"].as_u64().unwrap_or(0);
                        for event in events["events"]
                            .as_array()
                            .ok_or(crate::panel_events::INVALID)?
                        {
                            if event["control"].as_u64() == Some(config.button as u64) {
                                let pending = session.event(
                                    event["sequence"].as_u64().unwrap() as u32,
                                    event["pressed"].as_bool().unwrap(),
                                    start.elapsed().as_millis() as u64,
                                );
                                intents(
                                    &panel,
                                    &config,
                                    &mut session,
                                    &transport,
                                    &route,
                                    pending,
                                    &mut error,
                                );
                            }
                        }
                        cursor = events["next"].as_u64().unwrap() as u32;
                        if events["more"] != true {
                            break;
                        }
                    }
                }
                Ok(())
            })();
            match result {
                Ok(()) => {
                    if session.state(now)["ledValue"].is_null() {
                        let pending = session.refresh_led(now);
                        intents(
                            &panel,
                            &config,
                            &mut session,
                            &transport,
                            &route,
                            pending,
                            &mut error,
                        );
                    }
                    if session.state(now)["commandUncertain"] != true
                        && session.state(now)["ledValue"].is_boolean()
                    {
                        error = None;
                    }
                }
                Err(e) => {
                    if e.contains("eventGap") {
                        gaps += 1;
                    }
                    error = Some(e);
                    let pending = session.panel_unavailable();
                    intents(
                        &panel,
                        &config,
                        &mut session,
                        &transport,
                        &route,
                        pending,
                        &mut error,
                    );
                    baselined = false;
                }
            }
        }
        let mut value = session.state(start.elapsed().as_millis() as u64);
        value["running"] = json!(true);
        value["eventOverflow"] = json!(overflow);
        value["eventGaps"] = json!(gaps);
        if let Some(error) = &error {
            value["error"] = json!(error);
            value["status"] = json!("error");
        }
        *state.lock().unwrap_or_else(|p| p.into_inner()) = value;
        thread::sleep(Duration::from_millis(5));
    }
    // An explicit stop requests the documented LED-off fallback through its setter.
    let pending = session.release();
    intents(
        &panel,
        &config,
        &mut session,
        &transport,
        &route,
        pending,
        &mut error,
    );
    let result = write_led(&panel, &config, &route.led, false);
    session.led_result(false, result.is_ok());
    let mut value = session.state(start.elapsed().as_millis() as u64);
    value["running"] = json!(false);
    value["status"] = json!("stopped");
    value["feedbackFresh"] = json!(false);
    value["eventOverflow"] = json!(overflow);
    value["eventGaps"] = json!(gaps);
    if let Err(error) = result {
        value["error"] = json!(error);
    }
    *state.lock().unwrap_or_else(|p| p.into_inner()) = value;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::UdpSocket;
    struct Mock {
        state: Value,
        events: Vec<Value>,
        head: u32,
        held: bool,
        led: bool,
        fail: bool,
    }
    fn config(command_port: u16, listen_port: u16) -> Config {
        Config::parse(&json!({"schemaVersion":1,"cataloguePath":PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../simulation/dcs-bios/catalogue"),"gatewayId":"stm32:00112233445566778899aabb","moduleId":"00112233445566778899aabbccddeeff","button":0,"ledActionId":"led.boolean","commandHost":"127.0.0.1","commandPort":command_port,"listenAddress":"127.0.0.1","listenPort":listen_port,"multicastGroup":"","multicastInterface":"127.0.0.1","freshnessMs":150})).unwrap()
    }
    fn mock(config: &Config) -> (Panel, Arc<Mutex<Mock>>) {
        let shared = Arc::new(Mutex::new(Mock {
            state: json!({"connected":true,"epoch":1,"modules":[{"id":config.module_id,"online":true,"enabled":false,"published":false,"reportId":null,"directUsb":false,"boot":1,"node":2,"lease":3}]}),
            events: vec![],
            head: 0,
            held: false,
            led: false,
            fail: false,
        }));
        let copy = shared.clone();
        let id = config.module_id.clone();
        let panel: Panel = Arc::new(move |request| {
            let mut m = copy.lock().unwrap();
            if m.fail {
                return json!({"ok":false,"error":"Disconnected panel"});
            }
            let mut state = m.state.clone();
            state["moduleActions"][&id] = json!({"boot":1,"catalogueRevision":1,"status":"ready","actions":[{"id":"led.boolean","label":"LED","endpoint":"led","semantics":"setter","readable":true,"parameters":[{"id":"enabled","label":"Enabled","type":"boolean"}]}],"states":[{"id":"led.boolean","values":[m.led]}]});
            if request["command"] == "action_invoke" {
                m.led = request["values"][0].as_bool().unwrap();
            }
            let mut events = json!({"ok":true,"schemaVersion":1,"stream":1,"head":m.head,"oldest":1,"overflow":0,"buttonCount":1,"held":[m.held],"encoderButtons":[],"boot":1,"route":[1,2,3,1]});
            if request["command"] == "events_read" {
                let after = request["after"].as_u64().unwrap();
                events["after"] = json!(after);
                events["next"] = json!(m.head);
                events["more"] = json!(false);
                events["events"] = json!(m
                    .events
                    .iter()
                    .filter(|e| e["sequence"].as_u64().unwrap() > after)
                    .collect::<Vec<_>>());
            }
            state["moduleEvents"][&id] = events;
            json!({"ok":true,"state":state})
        });
        (panel, shared)
    }
    fn path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.research/milestone2/runtime-tests")
            .join(uuid::Uuid::new_v4().to_string())
            .join("profile.json")
    }
    #[test]
    fn readiness_rejects_duplicate_hid_routes_and_accepts_only_online_disabled_panels() {
        let config = config(7778, 5010);
        let (panel, _) = mock(&config);
        let state = call(&panel, &config, "status", json!({})).unwrap();
        assert!(ready(&state, &config).is_ok());
        for (key, value) in [
            ("connected", json!(false)),
            ("reconnecting", json!(true)),
            ("layoutPending", json!(true)),
            ("modules", json!([])),
        ] {
            let mut s = state.clone();
            s[key] = value;
            assert!(ready(&s, &config).is_err());
        }
        for (key, value) in [
            ("online", json!(false)),
            ("enabled", json!(true)),
            ("published", json!(true)),
            ("reportId", json!(1)),
            ("directUsb", json!(true)),
        ] {
            let mut s = state.clone();
            s["modules"][0][key] = value;
            assert!(ready(&s, &config).is_err());
        }
        assert!(prepare(&panel, &config).is_ok());
        let mut invalid = config.clone();
        invalid.button = 1;
        assert!(prepare(&panel, &invalid).is_err());
        invalid = config.clone();
        invalid.led_action_id = "absent".into();
        assert!(prepare(&panel, &invalid).is_err());
    }
    #[test]
    fn settings_persist_but_never_autostart_and_invalid_requests_do_not_replace_them() {
        let config = config(7778, 5010);
        let (panel, shared) = mock(&config);
        let path = path();
        let service = DcsRuntime::new(panel.clone(), path.clone());
        assert_eq!(service.status()["configured"], false);
        assert!(service.request(&json!({"command":"dcs_start"})).is_err());
        assert!(service
            .request(&json!({"command":"dcs_configure","dcsConfig":config.value()}))
            .is_ok());
        assert!(service
            .request(&json!({"command":"dcs_configure","dcsConfig":null}))
            .is_err());
        assert!(service.request(&json!({"command":"unknown"})).is_err());
        assert!(service.request(&json!({"command":"dcs_status"})).is_ok());
        shared.lock().unwrap().state["modules"][0]["enabled"] = json!(true);
        assert!(service.request(&json!({"command":"dcs_start"})).is_err());
        let loaded = DcsRuntime::new(panel, path.clone());
        assert_eq!(loaded.status()["configured"], true);
        assert_eq!(loaded.status()["running"], false);
        loaded.request(&json!({"command":"dcs_stop"})).unwrap();
        std::fs::write(&path, b"invalid").unwrap();
        assert_eq!(
            DcsRuntime::new(Arc::new(|_| Value::Null), path).status()["status"],
            "error"
        );
    }
    #[test]
    fn terminal_shutdown_rejects_delayed_start_and_configure_but_stop_allows_restart() {
        let command = UdpSocket::bind("127.0.0.1:0").unwrap();
        let reserve = UdpSocket::bind("127.0.0.1:0").unwrap();
        let config = config(
            command.local_addr().unwrap().port(),
            reserve.local_addr().unwrap().port(),
        );
        drop(reserve);
        let (panel, _) = mock(&config);
        let path = path();
        let service = Arc::new(DcsRuntime::new(panel, path.clone()));
        service
            .request(&json!({"command":"dcs_configure","dcsConfig":config.value()}))
            .unwrap();
        for _ in 0..2 {
            service.request(&json!({"command":"dcs_start"})).unwrap();
            assert_eq!(service.status()["running"], true);
            service.stop();
            assert_eq!(service.status()["running"], false);
        }
        let saved = std::fs::read(&path).unwrap();
        let (resume, paused) = std::sync::mpsc::channel();
        let pending = service.clone();
        let request = std::thread::spawn(move || {
            // This request has already passed the manager's initial closed
            // check, but reaches the runtime only after terminal shutdown.
            paused.recv().unwrap();
            pending.request(&json!({"command":"dcs_start"}))
        });
        service.shutdown();
        resume.send(()).unwrap();
        let result = request.join().unwrap();
        // Keep the old failing implementation's worker from outliving the test.
        service.stop();
        assert!(
            result.is_err(),
            "A delayed start created a worker after shutdown"
        );
        let mut changed = config.clone();
        changed.button = 1;
        assert!(service
            .request(&json!({"command":"dcs_configure","dcsConfig":changed.value()}))
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        assert_eq!(service.status()["running"], false);
        service.shutdown();
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn output_reconciliation_reads_before_writing_and_does_not_repeat_confirmed_setters() {
        let config = config(7778, 5010);
        let (panel, shared) = mock(&config);
        write_led(&panel, &config, &LedBinding::Boolean, true).unwrap();
        assert!(shared.lock().unwrap().led);
        write_led(&panel, &config, &LedBinding::Boolean, true).unwrap();
        shared.lock().unwrap().state["modules"][0]["enabled"] = json!(true);
        write_led(&panel, &config, &LedBinding::Boolean, false).unwrap();
        assert!(
            !shared.lock().unwrap().led,
            "off fallback remains possible after HID enable"
        );
        shared.lock().unwrap().fail = true;
        assert!(write_led(&panel, &config, &LedBinding::Boolean, true).is_err());
    }
    #[test]
    fn background_service_sends_short_press_pair_tracks_feedback_and_stops_without_ui_polling() {
        let command = UdpSocket::bind("127.0.0.1:0").unwrap();
        command
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let reserve = UdpSocket::bind("127.0.0.1:0").unwrap();
        let export = reserve.local_addr().unwrap();
        drop(reserve);
        let config = config(command.local_addr().unwrap().port(), export.port());
        let (panel, shared) = mock(&config);
        let service = DcsRuntime::new(panel, path());
        service
            .request(&json!({"command":"dcs_configure","dcsConfig":config.value()}))
            .unwrap();
        service.request(&json!({"command":"dcs_start"})).unwrap();
        assert!(service.request(&json!({"command":"dcs_start"})).is_err());
        assert!(service
            .request(&json!({"command":"dcs_configure","dcsConfig":config.value()}))
            .is_err());
        assert!(service.bound_led(&json!({"gatewayId":config.gateway_id,"moduleId":config.module_id,"actionId":config.led_action_id})));
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let peer = thread::spawn(move || {
            let mut counter = 0u8;
            while !flag.load(Ordering::Acquire) {
                counter = counter.wrapping_add(1);
                let mut bytes = vec![0x55; 4];
                let name = b"A-10C\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
                bytes.extend([64, 0, 24, 0]);
                bytes.extend(name);
                bytes.extend([0x40, 0x12, 2, 0, 4, 0, 0xfe, 0xff, 2, 0, counter, 0]);
                sender.send_to(&bytes, export).unwrap();
                thread::sleep(Duration::from_millis(20));
            }
        });
        let until = Instant::now() + Duration::from_secs(3);
        while !shared.lock().unwrap().led && Instant::now() < until {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(shared.lock().unwrap().led, "fresh export turns LED on");
        thread::sleep(Duration::from_millis(100));
        {
            let mut m = shared.lock().unwrap();
            m.head = 2;
            m.events = vec![
                json!({"sequence":1,"control":0,"pressed":true}),
                json!({"sequence":2,"control":0,"pressed":false}),
            ];
        }
        let mut data = [0u8; 128];
        let first = command.recv(&mut data).unwrap();
        assert_eq!(&data[..first], b"UFC_MASTER_CAUTION 1\n");
        let second = command.recv(&mut data).unwrap();
        assert_eq!(&data[..second], b"UFC_MASTER_CAUTION 0\n");
        shared.lock().unwrap().fail = true;
        thread::sleep(Duration::from_millis(100));
        assert!(service.status()["error"]
            .as_str()
            .unwrap()
            .contains("Disconnected"));
        shared.lock().unwrap().fail = false;
        stop.store(true, Ordering::Release);
        peer.join().unwrap();
        thread::sleep(Duration::from_millis(300));
        assert!(
            !shared.lock().unwrap().led,
            "stale export uses off fallback"
        );
        service.stop();
        assert_eq!(service.status()["running"], false);
        assert!(!service.bound_led(&json!({"gatewayId":config.gateway_id,"moduleId":config.module_id,"actionId":config.led_action_id})));
    }
}
