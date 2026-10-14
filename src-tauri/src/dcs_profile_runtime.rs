//! One bounded I/O worker per gateway; simulator exports never wait for panel I/O.
use crate::dcs_bios::Transport;
use crate::dcs_profile::{
    format_output, CompiledInput, CompiledProfile, ExportSession, ExportSnapshot, InputKind,
    Output, Target,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

type Panel = Arc<dyn Fn(Value) -> Value + Send + Sync>;
const CLIENT: &str = "cockpit-dcs-bios-integration";
const POLL: Duration = Duration::from_millis(50);

fn call(panel: &Panel, target: &Target, command: &str, extra: Value) -> Result<Value, String> {
    let mut request = json!({"command":command,"clientId":CLIENT,"gatewayId":target.gateway_id,"moduleId":target.module_id});
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
fn module(state: &Value, target: &Target, input: bool) -> Result<Value, String> {
    let module = state["modules"]
        .as_array()
        .and_then(|modules| modules.iter().find(|m| m["id"] == target.module_id))
        .ok_or("Reconnect the bound panel.")?;
    if state["connected"] != true
        || state["reconnecting"] == true
        || state["layoutPending"] == true
        || module["online"] != true
    {
        return Err("Reconnect the bound panel and wait for its route to settle.".into());
    }
    if input
        && (module["enabled"] != false
            || module["published"] != false
            || !module["reportId"].is_null()
            || module["directUsb"] != false)
    {
        return Err("Disable bound input panels' HID controllers and disconnect their direct USB input before starting DCS-BIOS.".into());
    }
    Ok(module.clone())
}
fn token(state: &Value, module: &Value) -> Value {
    json!([
        state["epoch"],
        module["node"],
        module["lease"],
        module["boot"]
    ])
}
#[derive(Clone)]
struct Prepared {
    target: Target,
    route: Value,
    actions: BTreeMap<String, Value>,
    revision: u64,
    input: bool,
}
fn source(input: &CompiledInput, events: &Value) -> Result<(), String> {
    crate::panel_events::validate(events, None)?;
    let b = &input.binding;
    let contacts = events["contactButtonCount"]
        .as_u64()
        .ok_or("Update the panel firmware to advertise typed input capabilities.")?
        as usize;
    let maintained = events["maintainedButtons"].as_array().unwrap();
    let valid = match b.kind {
        InputKind::Button => b.control < contacts && !maintained.contains(&json!(b.control)),
        InputKind::Switch => maintained.contains(&json!(b.control)),
        InputKind::Encoder => b.control < events["encoderButtons"].as_array().unwrap().len() / 2,
        InputKind::Axis => b.control < events["axisCount"].as_u64().unwrap() as usize,
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "Binding {} does not match the advertised physical input kind/index.",
            b.id
        ))
    }
}
fn prepare_one(
    profile: &CompiledProfile,
    panel: &Panel,
    target: &Target,
) -> Result<Prepared, String> {
    let input = profile.inputs.iter().any(|i| i.binding.target == *target);
    let snapshot = call(panel, target, "status", json!({}))?;
    let m = module(&snapshot, target, input)?;
    let route = token(&snapshot, &m);
    let mut actions = BTreeMap::new();
    let mut revision = 0;
    let outputs: Vec<_> = profile
        .outputs
        .iter()
        .filter(|o| o.binding.target == *target)
        .collect();
    if !outputs.is_empty() {
        let snapshot = call(panel, target, "action_describe", json!({}))?;
        let m = module(&snapshot, target, input)?;
        let catalogue = &snapshot["moduleActions"][&target.module_id];
        if token(&snapshot, &m) != route
            || catalogue["route"] != route
            || catalogue["status"] != "ready"
        {
            return Err("The panel action route changed while starting; retry Start.".into());
        }
        revision = catalogue["catalogueRevision"]
            .as_u64()
            .filter(|n| *n > 0 && *n <= u32::MAX as u64)
            .ok_or(crate::actions::INVALID)?;
        let mut endpoints = BTreeSet::new();
        for output in outputs {
            let action = catalogue["actions"]
                .as_array()
                .and_then(|a| a.iter().find(|a| a["id"] == output.binding.action_id))
                .ok_or("The bound output action is no longer advertised.")?
                .clone();
            crate::actions::descriptor(&action)?;
            if !endpoints.insert(action["endpoint"].as_str().unwrap().to_owned()) {
                return Err("Bind each physical output endpoint only once.".into());
            }
            format_output(&output.binding, None, true, &action)?;
            format_output(&output.binding, None, false, &action)?;
            actions.insert(output.binding.id.clone(), action);
        }
    }
    if input {
        let snapshot = call(panel, target, "events_state", json!({"boot":m["boot"]}))?;
        let m = module(&snapshot, target, true)?;
        let events = snapshot["moduleEvents"][&target.module_id].clone();
        if token(&snapshot, &m) != route || events["route"] != route {
            return Err("The panel input route changed while starting; retry Start.".into());
        }
        for i in profile
            .inputs
            .iter()
            .filter(|i| i.binding.target == *target)
        {
            source(i, &events)?;
        }
    }
    Ok(Prepared {
        target: target.clone(),
        route,
        actions,
        revision,
        input,
    })
}

#[derive(Default)]
struct Meter {
    writes: u64,
    reconciled: u64,
    coalesced: u64,
    events: u64,
    commands: u64,
    peak: usize,
    latency: VecDeque<f64>,
    poll_max: f64,
    last_poll: Option<Instant>,
}
impl Meter {
    fn latency(&mut self, milliseconds: f64) {
        if self.latency.len() == 128 {
            self.latency.pop_front();
        }
        self.latency.push_back(milliseconds);
    }
    fn value(&self, id: &str, depth: usize, elapsed: f64) -> Value {
        let mut latency: Vec<_> = self.latency.iter().copied().collect();
        latency.sort_by(f64::total_cmp);
        let mean = if latency.is_empty() {
            0.0
        } else {
            latency.iter().sum::<f64>() / latency.len() as f64
        };
        let p95 = latency
            .get(((latency.len() as f64 * 0.95).ceil() as usize).saturating_sub(1))
            .copied()
            .unwrap_or(0.0);
        json!({"gatewayId":id,"queueDepth":depth,"queueHighWater":self.peak,"actionWrites":self.writes,
            "reconciled":self.reconciled,"coalesced":self.coalesced,"latencyMeanMs":mean,"latencyP95Ms":p95,
            "latencyMaxMs":latency.last().copied().unwrap_or(0.0),"throughputPerSecond":self.writes as f64/elapsed.max(0.001),
            "inputPollMaxIntervalMs":self.poll_max,"eventsProcessed":self.events,"commandsSent":self.commands})
    }
}
struct OutputState {
    binding: Output,
    action: Value,
    desired: Value,
    applied: Option<Value>,
    pending: bool,
    status: String,
    error: Option<String>,
    format_error: Option<String>,
}
struct Shared {
    export: ExportSnapshot,
    updated: Instant,
    inputs: BTreeMap<String, Value>,
    outputs: BTreeMap<String, OutputState>,
    meters: BTreeMap<String, Meter>,
    gaps: u64,
    overflow: u64,
}
fn sync_desired(shared: &mut Shared, snapshot: ExportSnapshot, now: Instant) {
    shared.export = snapshot;
    shared.updated = now;
    for output in shared.outputs.values_mut() {
        let known = shared.export.values.get(&output.binding.id);
        let result = format_output(&output.binding, known, shared.export.fresh, &output.action);
        let (desired, error) = match result {
            Ok(value) => (value, None),
            Err(error) => (
                format_output(&output.binding, None, false, &output.action).unwrap_or(json!([])),
                Some(error),
            ),
        };
        output.status = if error.is_some() {
            "error"
        } else if !shared.export.fresh {
            "stale"
        } else if known.is_none() {
            "unknown"
        } else {
            "ready"
        }
        .into();
        // A setter has exactly one pending value. New export state replaces it;
        // relative commands and ordered button edges never enter this queue.
        if desired != output.desired {
            if output.pending {
                shared
                    .meters
                    .get_mut(&output.binding.target.gateway_id)
                    .unwrap()
                    .coalesced += 1;
            }
            output.desired = desired;
            output.pending = output.applied.as_ref() != Some(&output.desired);
        }
        output.format_error = error;
    }
    for (gateway, meter) in &mut shared.meters {
        meter.peak = meter.peak.max(
            shared
                .outputs
                .values()
                .filter(|o| o.binding.target.gateway_id == *gateway && o.pending)
                .count(),
        );
    }
}
fn snapshot(shared: &Shared, elapsed: Duration, running: bool) -> Value {
    let export = &shared.export;
    let outputs:Vec<_>=shared.outputs.values().map(|o|json!({"id":o.binding.id,"gatewayId":o.binding.target.gateway_id,"moduleId":o.binding.target.module_id,
        "actionId":o.binding.action_id,"status":if o.error.is_some()||o.format_error.is_some(){"error"}else{&o.status},"ready":o.status=="ready"&&!o.pending&&o.error.is_none()&&o.format_error.is_none(),
        "desired":o.desired,"applied":o.applied,"pending":o.pending,"error":o.format_error.as_ref().or(o.error.as_ref())})).collect();
    let gateways: Vec<_> = shared
        .meters
        .iter()
        .map(|(id, m)| {
            m.value(
                id,
                shared
                    .outputs
                    .values()
                    .filter(|o| o.binding.target.gateway_id == *id && o.pending)
                    .count(),
                elapsed.as_secs_f64(),
            )
        })
        .collect();
    let inputs: Vec<_> = shared
        .inputs
        .values()
        .map(|input| {
            let mut input = input.clone();
            if !running || !export.fresh {
                input["ready"] = json!(false);
                input["armed"] = json!(false);
                input["needsBaseline"] = json!(true);
                if input["status"] != "error" {
                    input["status"] = json!("waiting");
                }
            }
            input
        })
        .collect();
    json!({"running":running,"aircraft":export.aircraft,"missionActive":export.mission_active,"feedbackFresh":export.fresh,
        "feedbackAgeMs":export.age_ms,"status":if !running{"stopped"}else if export.fresh{"active"}else if export.age_ms.is_some(){"stale"}else{"waiting"},
        "error":null,"updateCounter":export.counter,"eventOverflow":shared.overflow,"eventGaps":shared.gaps,"commandsSent":shared.meters.values().map(|m|m.commands).sum::<u64>(),
        "ledValue":null,"inputs":inputs,"outputs":outputs,"gateways":gateways,"malformedFrames":export.malformed_frames})
}

struct InputState {
    binding: CompiledInput,
    held: bool,
    armed: bool,
    pressed_sent: bool,
    last_value: Option<u16>,
    commands: u64,
    uncertain: bool,
    error: Option<String>,
}
impl InputState {
    fn new(binding: CompiledInput) -> Self {
        Self {
            binding,
            held: false,
            armed: false,
            pressed_sent: false,
            last_value: None,
            commands: 0,
            uncertain: false,
            error: None,
        }
    }
    fn report(&self, ready: bool) -> Value {
        let b = &self.binding.binding;
        json!({"id":b.id,"gatewayId":b.target.gateway_id,"moduleId":b.target.module_id,
        "kind":b.kind.name(),"control":b.control,"command":b.command,"ready":ready&&self.armed&&!self.uncertain,"armed":self.armed,"needsBaseline":!ready,
        "status":if self.uncertain||self.error.is_some(){"error"}else if ready&&self.armed{"ready"}else if ready&&b.kind==InputKind::Button{"waitingRelease"}else if ready&&b.kind==InputKind::Axis{"unavailable"}else{"waiting"},
        "lastValue":if matches!(b.kind,InputKind::Button|InputKind::Switch){json!(self.held)}else{json!(self.last_value)},"commandsSent":self.commands,"error":self.error})
    }
    fn send(
        &mut self,
        transport: &Transport,
        argument: String,
        meter: &mut Meter,
    ) -> Result<(), String> {
        match transport.send_control(&self.binding.binding.command, &argument) {
            Ok(()) => {
                self.commands += 1;
                meter.commands += 1;
                Ok(())
            }
            Err(error) => {
                self.uncertain = true;
                self.error=Some(format!("Command delivery is uncertain and was not replayed; Stop and Start this profile. {error}"));
                Err(error)
            }
        }
    }
    fn release(&mut self, transport: &Transport, meter: &mut Meter) {
        self.armed = false;
        if self.pressed_sent {
            self.pressed_sent = false;
            let _ = self.send(
                transport,
                self.binding.binding.released_value.to_string(),
                meter,
            );
        }
    }
    fn absolute(
        &mut self,
        value: u16,
        transport: &Transport,
        meter: &mut Meter,
    ) -> Result<(), String> {
        if self.uncertain {
            return Ok(());
        }
        let b = &self.binding.binding;
        let minimum = if b.kind == InputKind::Axis {
            b.deadband.max(1)
        } else {
            1
        };
        if self
            .last_value
            .is_none_or(|last| last.abs_diff(value) >= minimum)
        {
            self.send(transport, value.to_string(), meter)?;
            self.last_value = Some(value);
        }
        self.armed = true;
        Ok(())
    }
    fn event(
        &mut self,
        control: usize,
        pressed: bool,
        events: &Value,
        transport: &Transport,
        meter: &mut Meter,
    ) -> Result<(), String> {
        if self.uncertain {
            return Ok(());
        }
        let b = self.binding.binding.clone();
        match b.kind {
            InputKind::Encoder => {
                if !pressed {
                    return Ok(());
                }
                let pair =
                    &events["encoderButtons"].as_array().unwrap()[b.control * 2..b.control * 2 + 2];
                let positive = pair[0].as_u64() == Some(control as u64);
                if positive || pair[1].as_u64() == Some(control as u64) {
                    let argument = if b.interface == "fixed_step" {
                        if positive {
                            "INC".into()
                        } else {
                            "DEC".into()
                        }
                    } else {
                        format!("{}{}", if positive { "+" } else { "-" }, b.step)
                    };
                    self.send(transport, argument, meter)?;
                }
            }
            InputKind::Button if control == b.control => {
                if pressed == self.held {
                    return Ok(());
                }
                self.held = pressed;
                if !pressed {
                    self.armed = true;
                    if self.pressed_sent {
                        self.pressed_sent = false;
                        self.send(transport, b.released_value.to_string(), meter)?;
                    }
                } else if self.armed {
                    self.pressed_sent = true;
                    self.send(transport, b.pressed_value.to_string(), meter)?;
                }
            }
            InputKind::Switch if control == b.control => {
                self.held = pressed;
                self.absolute(
                    if pressed {
                        b.pressed_value
                    } else {
                        b.released_value
                    },
                    transport,
                    meter,
                )?;
            }
            _ => {}
        }
        Ok(())
    }
    fn baseline(
        &mut self,
        events: &Value,
        fresh: bool,
        transport: &Transport,
        meter: &mut Meter,
    ) -> Result<(), String> {
        self.release(transport, meter);
        self.last_value = None;
        let b = self.binding.binding.clone();
        if matches!(b.kind, InputKind::Button | InputKind::Switch) {
            self.held = events["held"][b.control].as_bool().unwrap();
        }
        if !fresh || self.uncertain {
            return Ok(());
        }
        match b.kind {
            InputKind::Button => self.armed = !self.held,
            InputKind::Switch => self.absolute(
                if self.held {
                    b.pressed_value
                } else {
                    b.released_value
                },
                transport,
                meter,
            )?,
            InputKind::Encoder => self.armed = true,
            InputKind::Axis => self.axis(events, transport, meter)?,
        }
        Ok(())
    }
    fn axis(
        &mut self,
        events: &Value,
        transport: &Transport,
        meter: &mut Meter,
    ) -> Result<(), String> {
        let b = &self.binding.binding;
        if b.kind != InputKind::Axis {
            return Ok(());
        }
        if events["axisAvailable"][b.control] != true {
            self.armed = false;
            self.last_value = None;
            return Ok(());
        }
        let raw = events["axes"][b.control].as_u64().unwrap();
        let value = ((raw * self.binding.maximum as u64 + 32767) / 65535) as u16;
        self.absolute(value, transport, meter)
    }
}
struct Device {
    prepared: Prepared,
    inputs: Vec<InputState>,
    cursor: Option<(u32, u32)>,
    epoch: u64,
    was_fresh: bool,
    available: bool,
}
impl Device {
    fn release(&mut self, transport: &Transport, meter: &mut Meter) {
        for input in &mut self.inputs {
            input.release(transport, meter);
        }
        self.cursor = None;
        self.available = false;
        self.was_fresh = false;
    }
    fn poll(
        &mut self,
        profile: &CompiledProfile,
        panel: &Panel,
        transport: &Transport,
        export: &ExportSnapshot,
        fresh: bool,
        meter: &mut Meter,
        shared: &Arc<Mutex<Shared>>,
    ) -> Result<(bool, u64), String> {
        let target = &self.prepared.target;
        let state = call(panel, target, "status", json!({}))?;
        let m = module(&state, target, self.prepared.input)?;
        if token(&state, &m) != self.prepared.route {
            self.release(transport, meter);
            self.prepared = prepare_one(profile, panel, &self.prepared.target)?;
        }
        if !self.prepared.input {
            self.available = true;
            return Ok((false, 0));
        }
        if self.cursor.is_none() || self.epoch != export.epoch || fresh != self.was_fresh || !fresh
        {
            let state = call(
                panel,
                &self.prepared.target,
                "events_state",
                json!({"boot":self.prepared.route[3]}),
            )?;
            module(&state, &self.prepared.target, true)?;
            let events = &state["moduleEvents"][&self.prepared.target.module_id];
            if events["route"] != self.prepared.route {
                return Err("The panel event route changed while baselining.".into());
            }
            for input in &self.inputs {
                source(&input.binding, events)?;
            }
            if fresh && !still_fresh(shared, export.epoch, profile.profile.network.freshness_ms) {
                self.release(transport, meter);
                return Ok((false, events["overflow"].as_u64().unwrap()));
            }
            for input in &mut self.inputs {
                input.baseline(events, fresh, transport, meter)?;
            }
            self.cursor = Some((
                events["stream"].as_u64().unwrap() as u32,
                events["head"].as_u64().unwrap() as u32,
            ));
            self.epoch = export.epoch;
            self.was_fresh = fresh;
            self.available = true;
            return Ok((false, events["overflow"].as_u64().unwrap()));
        }
        let mut more = false;
        let mut overflow = 0;
        for _ in 0..4 {
            let (stream, after) = self.cursor.unwrap();
            let state = call(
                panel,
                &self.prepared.target,
                "events_read",
                json!({"boot":self.prepared.route[3],"stream":stream,"after":after}),
            )?;
            module(&state, &self.prepared.target, true)?;
            let events = &state["moduleEvents"][&self.prepared.target.module_id];
            crate::panel_events::validate(events, Some((stream, after)))?;
            if events["route"] != self.prepared.route {
                return Err("The panel event route changed.".into());
            }
            for input in &self.inputs {
                source(&input.binding, events)?;
            }
            for event in events["events"].as_array().unwrap() {
                if !still_fresh(shared, export.epoch, profile.profile.network.freshness_ms) {
                    self.release(transport, meter);
                    return Ok((false, events["overflow"].as_u64().unwrap()));
                }
                meter.events += 1;
                for input in &mut self.inputs {
                    input.event(
                        event["control"].as_u64().unwrap() as usize,
                        event["pressed"].as_bool().unwrap(),
                        events,
                        transport,
                        meter,
                    )?;
                }
            }
            if !still_fresh(shared, export.epoch, profile.profile.network.freshness_ms) {
                self.release(transport, meter);
                return Ok((false, events["overflow"].as_u64().unwrap()));
            }
            for input in &mut self.inputs {
                input.axis(events, transport, meter)?;
            }
            self.cursor = Some((stream, events["next"].as_u64().unwrap() as u32));
            overflow = events["overflow"].as_u64().unwrap();
            more = events["more"] == true;
            if !more {
                break;
            }
        }
        self.available = true;
        Ok((more, overflow))
    }
}
fn still_fresh(shared: &Arc<Mutex<Shared>>, epoch: u64, freshness: u64) -> bool {
    let shared = shared.lock().unwrap_or_else(|p| p.into_inner());
    shared.export.epoch == epoch
        && shared.export.fresh
        && shared
            .export
            .age_ms
            .unwrap_or(u64::MAX)
            .saturating_add(shared.updated.elapsed().as_millis() as u64)
            <= freshness
}
fn apply_output(
    panel: &Panel,
    prepared: &Prepared,
    binding: &Output,
    desired: &Value,
    latest: Option<&dyn Fn() -> Value>,
) -> Result<(bool, Value), String> {
    let state = call(panel, &prepared.target, "action_describe", json!({}))?;
    let m = module(&state, &prepared.target, prepared.input)?;
    let catalogue = &state["moduleActions"][&prepared.target.module_id];
    if token(&state, &m) != prepared.route
        || catalogue["route"] != prepared.route
        || catalogue["status"] != "ready"
        || catalogue["catalogueRevision"].as_u64() != Some(prepared.revision)
    {
        return Err("The output route/catalogue changed; refreshing current state.".into());
    }
    let action = catalogue["actions"]
        .as_array()
        .and_then(|a| a.iter().find(|a| a["id"] == binding.action_id))
        .ok_or(crate::actions::INVALID)?;
    if action != prepared.actions.get(&binding.id).unwrap() {
        return Err("The output descriptor changed; refresh the profile.".into());
    }
    // A slow read can outlive many export updates. Choose the newest desired
    // setter value after reconciliation, before starting its one invocation.
    let desired = latest.map_or_else(|| desired.clone(), |latest| latest());
    crate::actions::arguments(action, &desired)?;
    if catalogue["states"].as_array().is_some_and(|s| {
        s.iter()
            .any(|s| s["id"] == binding.action_id && s["values"] == desired)
    }) {
        return Ok((false, desired));
    }
    let state = call(
        panel,
        &prepared.target,
        "action_invoke",
        json!({"boot":prepared.route[3],"catalogueRevision":prepared.revision,"actionId":binding.action_id,"values":desired}),
    )?;
    let m = module(&state, &prepared.target, prepared.input)?;
    let actions = &state["moduleActions"][&prepared.target.module_id];
    if token(&state, &m) != prepared.route
        || actions["route"] != prepared.route
        || actions["catalogueRevision"].as_u64() != Some(prepared.revision)
        || actions["status"] != "ready"
        || !actions["states"].as_array().is_some_and(|s| {
            s.iter()
                .any(|s| s["id"] == binding.action_id && s["values"] == desired)
        })
    {
        return Err("Output application is uncertain; readable state will be reconciled before another setter.".into());
    }
    Ok((true, desired))
}

fn gateway_worker(
    profile: Arc<CompiledProfile>,
    panel: Panel,
    transport: Arc<Transport>,
    shared: Arc<Mutex<Shared>>,
    cancel: Arc<AtomicBool>,
    mut devices: Vec<Device>,
    gateway: String,
) {
    let mut meter = Meter::default();
    let mut next_poll = Instant::now();
    let mut round_robin = 0;
    let output_ids: Vec<_> = profile
        .outputs
        .iter()
        .filter(|o| o.binding.target.gateway_id == gateway)
        .map(|o| o.binding.id.clone())
        .collect();
    while !cancel.load(Ordering::Acquire) {
        let mut backlog = false;
        if Instant::now() >= next_poll {
            let now = Instant::now();
            if let Some(last) = meter.last_poll.replace(now) {
                meter.poll_max = meter
                    .poll_max
                    .max(now.duration_since(last).as_secs_f64() * 1000.0);
            }
            next_poll = now + POLL;
            for device in &mut devices {
                let (export, fresh) = {
                    let shared = shared.lock().unwrap_or_else(|p| p.into_inner());
                    let e = shared.export.clone();
                    let fresh = e.fresh
                        && e.age_ms
                            .unwrap_or(u64::MAX)
                            .saturating_add(shared.updated.elapsed().as_millis() as u64)
                            <= profile.profile.network.freshness_ms;
                    (e, fresh)
                };
                let previous = device.prepared.route.clone();
                let result = device.poll(
                    &profile, &panel, &transport, &export, fresh, &mut meter, &shared,
                );
                let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
                if previous != device.prepared.route {
                    for output in state
                        .outputs
                        .values_mut()
                        .filter(|o| o.binding.target == device.prepared.target)
                    {
                        output.action = device.prepared.actions[&output.binding.id].clone();
                        output.applied = None;
                        output.pending = true;
                        output.error = None;
                    }
                }
                match result {
                    Ok((more, overflow)) => {
                        backlog |= more;
                        state.overflow = state.overflow.max(overflow);
                        for input in &mut device.inputs {
                            if !input.uncertain {
                                input.error = None;
                            }
                        }
                    }
                    Err(error) => {
                        if error.contains("eventGap") {
                            state.gaps += 1;
                        }
                        drop(state);
                        device.release(&transport, &mut meter);
                        for input in &mut device.inputs {
                            if !input.uncertain {
                                input.error = Some(error.clone());
                            }
                        }
                        state = shared.lock().unwrap_or_else(|p| p.into_inner());
                        for output in state
                            .outputs
                            .values_mut()
                            .filter(|o| o.binding.target == device.prepared.target)
                        {
                            output.applied = None;
                            output.pending = true;
                            output.error = Some(error.clone());
                        }
                    }
                }
                for input in &device.inputs {
                    state.inputs.insert(
                        input.binding.binding.id.clone(),
                        input.report(device.available && fresh),
                    );
                }
            }
        }
        // Drain ordered inputs first. At most one blocking output is in flight
        // on this gateway; every other gateway has its own independent worker.
        if backlog {
            next_poll = Instant::now();
        } else if !output_ids.is_empty() {
            let mut job = None;
            {
                let state = shared.lock().unwrap_or_else(|p| p.into_inner());
                for offset in 0..output_ids.len() {
                    let index = (round_robin + offset) % output_ids.len();
                    let o = &state.outputs[&output_ids[index]];
                    if o.pending {
                        job = Some((index, o.binding.clone(), o.desired.clone()));
                        break;
                    }
                }
            }
            if let Some((index, binding, desired)) = job {
                round_robin = (index + 1) % output_ids.len();
                let prepared = &devices
                    .iter()
                    .find(|d| d.prepared.target == binding.target)
                    .unwrap()
                    .prepared;
                let latest = || {
                    shared.lock().unwrap_or_else(|p| p.into_inner()).outputs[&binding.id]
                        .desired
                        .clone()
                };
                let started = Instant::now();
                let result = apply_output(&panel, prepared, &binding, &desired, Some(&latest));
                meter.latency(started.elapsed().as_secs_f64() * 1000.0);
                let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
                let output = state.outputs.get_mut(&binding.id).unwrap();
                match result {
                    Ok((written, applied)) => {
                        if written {
                            meter.writes += 1;
                        } else {
                            meter.reconciled += 1;
                        }
                        output.applied = Some(applied);
                        output.pending = output.applied.as_ref() != Some(&output.desired);
                        output.error = None;
                    }
                    Err(error) => {
                        output.applied = None;
                        output.pending = true;
                        output.error = Some(error);
                    }
                }
            }
        }
        {
            let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
            let existing = state.meters.get_mut(&gateway).unwrap();
            meter.coalesced = existing.coalesced;
            meter.peak = meter.peak.max(existing.peak);
            *existing = Meter {
                writes: meter.writes,
                reconciled: meter.reconciled,
                coalesced: meter.coalesced,
                events: meter.events,
                commands: meter.commands,
                peak: meter.peak,
                latency: meter.latency.clone(),
                poll_max: meter.poll_max,
                last_poll: meter.last_poll,
            };
        }
        thread::sleep(Duration::from_millis(5));
    }
    for device in &mut devices {
        device.release(&transport, &mut meter);
    }
    for binding in profile
        .outputs
        .iter()
        .filter(|o| o.binding.target.gateway_id == gateway)
        .map(|o| &o.binding)
    {
        let prepared = &devices
            .iter()
            .find(|d| d.prepared.target == binding.target)
            .unwrap()
            .prepared;
        let action = prepared.actions.get(&binding.id).unwrap();
        let result = format_output(binding, None, false, action).and_then(|value| {
            apply_output(&panel, prepared, binding, &value, None).map(|(_, applied)| applied)
        });
        let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
        let output = state.outputs.get_mut(&binding.id).unwrap();
        match result {
            Ok(value) => {
                output.applied = Some(value.clone());
                output.desired = value;
                output.pending = false;
                output.error = None;
                output.status = "stale".into();
            }
            Err(error) => {
                output.applied = None;
                output.error = Some(error);
            }
        }
    }
    let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
    for device in &devices {
        for input in &device.inputs {
            state
                .inputs
                .insert(input.binding.binding.id.clone(), input.report(false));
        }
    }
    let existing = state.meters.get_mut(&gateway).unwrap();
    existing.commands = meter.commands;
}

pub(crate) fn start(
    profile: CompiledProfile,
    panel: Panel,
    state: Arc<Mutex<Value>>,
    cancel: Arc<AtomicBool>,
) -> Result<JoinHandle<()>, String> {
    let targets: BTreeSet<_> = profile
        .inputs
        .iter()
        .map(|i| i.binding.target.clone())
        .chain(profile.outputs.iter().map(|o| o.binding.target.clone()))
        .collect();
    let mut prepared = Vec::new();
    for target in &targets {
        prepared.push(prepare_one(&profile, &panel, target)?);
    }
    let transport = Arc::new(Transport::open_network(&profile.profile.network)?);
    let mut session = ExportSession::new(&profile);
    let mut shared = Shared {
        export: session.snapshot(0),
        updated: Instant::now(),
        inputs: BTreeMap::new(),
        outputs: BTreeMap::new(),
        meters: BTreeMap::new(),
        gaps: 0,
        overflow: 0,
    };
    for target in &targets {
        shared.meters.entry(target.gateway_id.clone()).or_default();
    }
    for output in &profile.outputs {
        let action = prepared
            .iter()
            .find(|p| p.target == output.binding.target)
            .unwrap()
            .actions[&output.binding.id]
            .clone();
        let desired = format_output(&output.binding, None, false, &action)?;
        shared.outputs.insert(
            output.binding.id.clone(),
            OutputState {
                binding: output.binding.clone(),
                action,
                desired,
                applied: None,
                pending: true,
                status: "unknown".into(),
                error: None,
                format_error: None,
            },
        );
    }
    let shared = Arc::new(Mutex::new(shared));
    let profile = Arc::new(profile);
    *state.lock().unwrap_or_else(|p| p.into_inner()) =
        snapshot(&shared.lock().unwrap(), Duration::ZERO, true);
    Ok(thread::spawn(move || {
        let started = Instant::now();
        let mut groups: BTreeMap<String, Vec<Device>> = BTreeMap::new();
        for prepared in prepared {
            let inputs = profile
                .inputs
                .iter()
                .filter(|i| i.binding.target == prepared.target)
                .cloned()
                .map(InputState::new)
                .collect();
            groups
                .entry(prepared.target.gateway_id.clone())
                .or_default()
                .push(Device {
                    prepared,
                    inputs,
                    cursor: None,
                    epoch: 0,
                    was_fresh: false,
                    available: false,
                });
        }
        let mut workers = Vec::new();
        for (gateway, devices) in groups {
            let (p, t, s, c, profile) = (
                panel.clone(),
                transport.clone(),
                shared.clone(),
                cancel.clone(),
                profile.clone(),
            );
            workers.push(thread::spawn(move || {
                gateway_worker(profile, p, t, s, c, devices, gateway)
            }));
        }
        while !cancel.load(Ordering::Acquire) {
            for _ in 0..128 {
                match transport.receive() {
                    Ok(Some(bytes)) => session.feed(&bytes, started.elapsed().as_millis() as u64),
                    Ok(None) => break,
                    Err(_) => {
                        session.reset_stream(started.elapsed().as_millis() as u64);
                        break;
                    }
                }
            }
            let now = started.elapsed().as_millis() as u64;
            session.tick(now);
            let mut shared = shared.lock().unwrap_or_else(|p| p.into_inner());
            sync_desired(&mut shared, session.snapshot(now), Instant::now());
            *state.lock().unwrap_or_else(|p| p.into_inner()) =
                snapshot(&shared, started.elapsed(), true);
            drop(shared);
            thread::sleep(Duration::from_millis(5));
        }
        for worker in workers {
            let _ = worker.join();
        }
        *state.lock().unwrap_or_else(|p| p.into_inner()) = snapshot(
            &shared.lock().unwrap_or_else(|p| p.into_inner()),
            started.elapsed(),
            false,
        );
    }))
}

#[cfg(test)]
#[path = "dcs_profile_runtime_tests.rs"]
mod tests;
