//! Explicit ordered panel-event sources. HID snapshots never feed this adapter.
use crate::dcs_profile_runtime::Panel;
use osg_controller_input::{
    registry::{Control, Kind, Sample},
    stream::{Event, Reason as CaptureReason, SourceSnapshot},
    ControllerService,
};
use osg_mapping::{
    engine::{Assignment, Change, Counter, Frame, Reason, Source},
    profile::{Origin, Profile, Value as InputValue},
};
use osg_mapping_host::panels::{Feed, Prepared, Provider as PanelProvider};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
pub struct Provider {
    pub panel: Panel,
    pub gateways: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
    pub clock: Arc<ControllerService>,
}
fn source_id(gateway: &str, module: &str) -> String {
    format!("panel-events:{gateway}/{module}")
}
fn target(id: &str) -> Result<(String, String), String> {
    let (gateway, module) = id
        .strip_prefix("panel-events:")
        .and_then(|s| s.rsplit_once('/'))
        .ok_or("Choose an explicit panel-event source.")?;
    let (backend, serial) = gateway.split_once(':').ok_or("Invalid panel gateway.")?;
    if crate::gateways::canonical_id(backend, serial).as_deref() != Some(gateway)
        || module.len() != 32
        || !module
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || module.bytes().all(|b| b == b'0')
    {
        return Err("Invalid panel-event identity.".into());
    }
    Ok((gateway.into(), module.into()))
}
fn call(panel: &Panel, id: &str, command: &str, extra: Value) -> Result<Value, String> {
    let (gateway, module) = target(id)?;
    let mut request = extra;
    request["command"] = json!(command);
    request["gatewayId"] = json!(gateway);
    request["moduleId"] = json!(module);
    request["clientId"] = json!("cockpit-mapping-panel-events");
    let response = panel(request);
    if response["ok"] != true {
        return Err(response["error"]
            .as_str()
            .unwrap_or("Panel event request failed.")
            .into());
    }
    Ok(response["state"].clone())
}
fn route(state: &Value, module: &str) -> Result<Value, String> {
    let m = state["modules"]
        .as_array()
        .and_then(|rows| rows.iter().find(|m| m["id"] == module))
        .ok_or("Reconnect the assigned panel.")?;
    if state["connected"] != true
        || state["reconnecting"] == true
        || state["layoutPending"] == true
        || m["online"] != true
        || m["boot"].as_u64().is_none_or(|v| v == 0)
        || state["epoch"].as_u64().is_none()
        || m["node"].as_u64().is_none()
        || m["lease"].as_u64().is_none()
    {
        return Err("Wait for a stable panel route and boot.".into());
    }
    // Virtual mapping eligibility does not inherit DCS direct-command HID restrictions.
    Ok(json!([state["epoch"], m["node"], m["lease"], m["boot"]]))
}
fn controls(events: &Value) -> Result<Vec<Control>, String> {
    crate::panel_events::validate(events, None)?;
    let mut result = Vec::new();
    for index in 0..events["buttonCount"].as_u64().unwrap() {
        let mut c = Control::new(9, index as u16 + 1, 0, 0, index as u16);
        c.id = format!("button:{index}");
        result.push(c);
    }
    for index in 0..events["axisCount"].as_u64().unwrap_or(0) {
        let mut c = Control::new(
            1,
            events["axisUsage"][index as usize].as_u64().unwrap() as u16,
            0,
            0,
            index as u16,
        );
        c.id = format!("axis:{index}");
        c.min = 0;
        c.max = 65535;
        c.bits = 16;
        result.push(c);
    }
    Ok(result)
}
fn values(events: &Value, controls: &[Control]) -> Result<BTreeMap<String, Sample>, String> {
    controls
        .iter()
        .map(|control| {
            let index = control.data_index as usize;
            let raw = if control.kind == Kind::Button {
                u32::from(
                    events["held"][index]
                        .as_bool()
                        .ok_or("Missing panel button baseline.")?,
                )
            } else {
                if events["axisAvailable"][index] != true {
                    return Err(
                        "Assigned panel axis is unavailable; no stale baseline is accepted.".into(),
                    );
                }
                events["axes"][index]
                    .as_u64()
                    .ok_or("Missing panel axis baseline.")? as u32
            };
            Ok((control.id.clone(), control.decode(raw)?))
        })
        .collect()
}
#[derive(Clone)]
struct Cursor {
    id: String,
    route: Value,
    stream: u32,
    after: u32,
    generation: u64,
    sequence: u64,
    controls: Vec<Control>,
    values: BTreeMap<String, Sample>,
}
impl Cursor {
    fn prepare(
        panel: &Panel,
        id: &str,
        selected: &BTreeSet<String>,
        now: u64,
        generation: u64,
    ) -> Result<(Self, SourceSnapshot), String> {
        let (_, module) = target(id)?;
        let state = call(panel, id, "status", json!({}))?;
        let token = route(&state, &module)?;
        let state = call(panel, id, "events_state", json!({"boot":token[3]}))?;
        let events = &state["moduleEvents"][&module];
        if route(&state, &module)? != token || events["route"] != token {
            return Err("Panel route changed during baseline.".into());
        }
        let available = controls(events)?;
        let controls = available
            .into_iter()
            .filter(|c| selected.contains(&c.id))
            .collect::<Vec<_>>();
        if controls.len() != selected.len() {
            return Err("Panel controls changed; edit the mapping assignment. Panel events do not include hats.".into());
        }
        let values = values(events, &controls)?;
        let cursor = Self {
            id: id.into(),
            route: token,
            stream: events["stream"].as_u64().unwrap() as u32,
            after: events["head"].as_u64().unwrap() as u32,
            generation,
            sequence: 1,
            controls: controls.clone(),
            values: values.clone(),
        };
        let snapshot = SourceSnapshot {
            id: id.into(),
            controls,
            baseline: Event {
                cursor: 0,
                source: id.into(),
                generation,
                sequence: 1,
                captured_us: now,
                reason: CaptureReason::Baseline,
                values,
            },
        };
        Ok((cursor, snapshot))
    }
    fn frame(&mut self, reason: Reason, changes: Vec<Change>, now: u64) -> Frame {
        self.sequence += 1;
        Frame {
            source: Source {
                origin: Origin::OsgPanelEvents,
                id: self.id.clone(),
            },
            generation: Counter(self.generation),
            sequence: Counter(self.sequence),
            captured_us: Counter(now),
            reason,
            changes,
        }
    }
    fn read(&mut self, panel: &Panel, now: u64) -> Result<Vec<Frame>, String> {
        let (_, module) = target(&self.id)?;
        let state = call(
            panel,
            &self.id,
            "events_read",
            json!({"boot":self.route[3],"stream":self.stream,"after":self.after}),
        )?;
        let events = &state["moduleEvents"][&module];
        if route(&state, &module)? != self.route || events["route"] != self.route {
            return Err("Panel route changed.".into());
        }
        crate::panel_events::validate(events, Some((self.stream, self.after)))?;
        let mut frames = Vec::new();
        for event in events["events"].as_array().unwrap() {
            let id = format!("button:{}", event["control"].as_u64().unwrap());
            if self.values.contains_key(&id) {
                let value = event["pressed"].as_bool().unwrap();
                self.values.insert(
                    id.clone(),
                    Sample {
                        native: i64::from(value),
                        normalized: Some(i64::from(value)),
                    },
                );
                frames.push(self.frame(
                    Reason::Change,
                    vec![Change {
                        control: id,
                        value: InputValue::Button(value),
                    }],
                    now,
                ));
            }
        }
        self.after = events["next"].as_u64().unwrap() as u32;
        // Axis snapshots are current state, not fabricated button-history events.
        let axes = self
            .controls
            .iter()
            .filter(|c| c.kind == Kind::Axis)
            .cloned()
            .collect::<Vec<_>>();
        let latest = values(events, &axes)?;
        let mut changed = Vec::new();
        for (id, value) in latest {
            if self.values.get(&id) != Some(&value) {
                changed.push(Change {
                    control: id.clone(),
                    value: InputValue::Axis(
                        value.normalized.ok_or("Unavailable panel axis.")? as i32
                    ),
                });
                self.values.insert(id, value);
            }
        }
        if !changed.is_empty() {
            frames.push(self.frame(Reason::Change, changed, now));
        }
        Ok(frames)
    }
}
impl PanelProvider for Provider {
    fn list(&self) -> Result<Value, String> {
        let mut sources = Vec::new();
        for gateway in (self.gateways)().into_iter().take(32) {
            let state = (self.panel)(
                json!({"command":"status","gatewayId":gateway,"clientId":"cockpit-mapping-panel-list"}),
            );
            if state["ok"] != true {
                continue;
            }
            for module in state["state"]["modules"].as_array().into_iter().flatten() {
                if module["online"] == true {
                    sources.push(json!({"id":source_id(&gateway,module["id"].as_str().unwrap_or("")),"gatewayId":gateway,"moduleId":module["id"],"name":module["name"],"origin":"osg-panel-events"}));
                }
            }
            if sources.len() > 256 {
                return Err("Panel source inventory exceeds its bound.".into());
            }
        }
        Ok(json!({"panelSources":sources}))
    }
    fn inspect(&self, id: &str) -> Result<Value, String> {
        let (_, module) = target(id)?;
        let state = call(&self.panel, id, "status", json!({}))?;
        let token = route(&state, &module)?;
        let state = call(&self.panel, id, "events_state", json!({"boot":token[3]}))?;
        let events = &state["moduleEvents"][&module];
        if route(&state, &module)? != token || events["route"] != token {
            return Err("Panel route changed during inspection.".into());
        }
        Ok(
            json!({"panelControls":controls(events)?.iter().map(|c|json!({"capability":c,"value":null})).collect::<Vec<_>>(),"hatsSupported":false,"sourceId":id}),
        )
    }
    fn prepare(&self, _profile: &Profile, assignments: &[Assignment]) -> Result<Prepared, String> {
        let mut selected = BTreeMap::<String, BTreeSet<String>>::new();
        let mut routes = BTreeMap::new();
        for assignment in assignments
            .iter()
            .filter(|a| a.source.origin == Origin::OsgPanelEvents)
        {
            let (gateway, module) = target(&assignment.source.id)?;
            if routes
                .insert(module, gateway.clone())
                .is_some_and(|previous| previous != gateway)
            {
                return Err(
                    "The same panel cannot enter a mapping through two gateway routes.".into(),
                );
            }
            selected
                .entry(assignment.source.id.clone())
                .or_default()
                .extend(assignment.controls.iter().map(|c| c.source_control.clone()));
        }
        for assignment in assignments
            .iter()
            .filter(|a| a.source.origin == Origin::HostController)
        {
            let source = self.clock.request(
                &json!({"command":"controller_inspect","sourceId":assignment.source.id,"offset":0}),
            )?;
            let serial = source["identity"]["serial"]
                .as_str()
                .unwrap_or("")
                .to_ascii_lowercase();
            if routes.keys().any(|module| serial == *module)
                || source["source"]["name"]
                    .as_str()
                    .is_some_and(|name| name.to_ascii_lowercase().contains("opensimgear"))
            {
                return Err("Do not combine a panel's HID route and panel-event route in one mapping. Choose one explicit source path.".into());
            }
        }
        let mut snapshots = Vec::new();
        let mut by_gateway = BTreeMap::<String, Vec<Cursor>>::new();
        for (id, controls) in &selected {
            let now = self.clock.stream_page("", 0).captured_us;
            let (cursor, snapshot) = Cursor::prepare(&self.panel, id, controls, now, 1)?;
            snapshots.push(snapshot);
            by_gateway.entry(target(id)?.0).or_default().push(cursor);
        }
        let (feed, sender) = Feed::channel();
        for cursors in by_gateway.into_values() {
            let sender = sender.clone();
            let stop = feed.stop.clone();
            let overflow = feed.overflow.clone();
            let panel = self.panel.clone();
            let clock = self.clock.clone();
            std::thread::Builder::new()
                .name("mapping-panel-events".into())
                .spawn(move || {
                    let mut cursors = cursors.into_iter().map(|c| (c, false)).collect::<Vec<_>>();
                    while !stop.load(Ordering::Acquire) {
                        for (cursor, lost) in &mut cursors {
                            if stop.load(Ordering::Acquire) {
                                return;
                            }
                            let now = clock.stream_page("", 0).captured_us;
                            let frames = if *lost {
                                let selected =
                                    cursor.controls.iter().map(|c| c.id.clone()).collect();
                                match Cursor::prepare(
                                    &panel,
                                    &cursor.id,
                                    &selected,
                                    now,
                                    cursor.generation + 1,
                                ) {
                                    Ok((next, snapshot)) => {
                                        *cursor = next;
                                        *lost = false;
                                        vec![Frame {
                                            source: Source {
                                                origin: Origin::OsgPanelEvents,
                                                id: cursor.id.clone(),
                                            },
                                            generation: Counter(cursor.generation),
                                            sequence: Counter(1),
                                            captured_us: Counter(now),
                                            reason: Reason::Baseline,
                                            changes: snapshot
                                                .baseline
                                                .values
                                                .into_iter()
                                                .map(|(control, sample)| Change {
                                                    value: if control.starts_with("button:") {
                                                        InputValue::Button(
                                                            sample.normalized == Some(1),
                                                        )
                                                    } else {
                                                        InputValue::Axis(
                                                            sample.normalized.unwrap_or(0) as i32,
                                                        )
                                                    },
                                                    control,
                                                })
                                                .collect(),
                                        }]
                                    }
                                    Err(_) => Vec::new(),
                                }
                            } else {
                                match cursor.read(&panel, now) {
                                    Ok(frames) => frames,
                                    Err(_) => {
                                        *lost = true;
                                        vec![cursor.frame(Reason::Gap, Vec::new(), now)]
                                    }
                                }
                            };
                            for mut frame in frames {
                                frame.captured_us = Counter(clock.stream_page("", 0).captured_us);
                                if stop.load(Ordering::Acquire) {
                                    return;
                                }
                                if sender.try_send(frame).is_err() {
                                    overflow.store(true, Ordering::Release);
                                    return;
                                }
                            }
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                })
                .map_err(|e| format!("Cannot start panel capture: {e}"))?;
        }
        drop(sender);
        Ok(Prepared { snapshots, feed })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    const G: &str = "stm32:00112233445566778899aabb";
    const M: &str = "00112233445566778899aabbccddeeff";
    fn events() -> Value {
        json!({"ok":true,"schemaVersion":1,"stream":7,"head":0,"oldest":1,"overflow":0,"buttonCount":2,"held":[false,false],"contactButtonCount":2,"maintainedButtons":[1],"encoderButtons":[],"selectorButtons":[],"axisCount":1,"axes":[32767],"axisNeutral":[0],"axisUsage":[48],"axisAvailable":[true],"sampledAtMs":1,"route":[1,2,3,4]})
    }
    fn fixture() -> (Provider, Arc<Mutex<Value>>) {
        let shared = Arc::new(Mutex::new(
            json!({"connected":true,"reconnecting":false,"layoutPending":false,"epoch":1,"modules":[{"id":M,"name":"Panel","online":true,"node":2,"lease":3,"boot":4,"enabled":true,"published":true,"reportId":1,"directUsb":true}],"moduleEvents":{M:events()}}),
        ));
        let read = shared.clone();
        let panel: Panel = Arc::new(move |request| {
            let mut state = read.lock().unwrap().clone();
            if request["command"] == "events_read" {
                let e = &mut state["moduleEvents"][M];
                e["after"] = request["after"].clone();
                e["next"] = e["head"].clone();
                e["more"] = json!(false);
                let after = request["after"].as_u64().unwrap();
                let items = e["events"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|v| v["sequence"].as_u64().unwrap() > after)
                    .collect::<Vec<_>>();
                e["events"] = json!(items);
            }
            json!({"ok":true,"state":state})
        });
        (
            Provider {
                panel,
                gateways: Arc::new(|| vec![G.into()]),
                clock: Arc::new(ControllerService::new(None, false)),
            },
            shared,
        )
    }
    fn assignments() -> Vec<Assignment> {
        vec![Assignment {
            role: "panel".into(),
            source: Source {
                origin: Origin::OsgPanelEvents,
                id: source_id(G, M),
            },
            controls: vec![
                osg_mapping::engine::ControlAssignment {
                    control: "button".into(),
                    source_control: "button:0".into(),
                },
                osg_mapping::engine::ControlAssignment {
                    control: "axis".into(),
                    source_control: "axis:0".into(),
                },
            ],
        }]
    }
    #[test]
    fn consumers_have_independent_cursors_and_use_events_with_hid_enabled() {
        let (provider, state) = fixture();
        assert_eq!(
            provider.list().unwrap()["panelSources"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            provider.inspect(&source_id(G, M)).unwrap()["hatsSupported"],
            false
        );
        let selected = BTreeSet::from(["button:0".into(), "axis:0".into()]);
        let (mut first, baseline) =
            Cursor::prepare(&provider.panel, &source_id(G, M), &selected, 1, 1).unwrap();
        let (mut second, _) =
            Cursor::prepare(&provider.panel, &source_id(G, M), &selected, 1, 1).unwrap();
        assert_eq!(baseline.controls.len(), 2);
        {
            let mut s = state.lock().unwrap();
            let e = &mut s["moduleEvents"][M];
            e["head"] = json!(2);
            e["events"] = json!([{"sequence":1,"control":0,"pressed":true},{"sequence":2,"control":0,"pressed":false}]);
            e["axes"] = json!([65535]);
        }
        let one = first.read(&provider.panel, 10).unwrap();
        let two = second.read(&provider.panel, 10).unwrap();
        assert_eq!(
            serde_json::to_value(&one).unwrap(),
            serde_json::to_value(&two).unwrap()
        );
        assert_eq!(one.len(), 3);
        assert!(first.read(&provider.panel, 11).unwrap().is_empty());
        state.lock().unwrap()["modules"][0]["boot"] = json!(5);
        assert!(first.read(&provider.panel, 12).is_err());
        assert!(Cursor::prepare(&provider.panel, &source_id(G, M), &selected, 13, 2).is_err());
        state.lock().unwrap()["moduleEvents"][M]["route"] = json!([1, 2, 3, 5]);
        let (next, _) =
            Cursor::prepare(&provider.panel, &source_id(G, M), &selected, 14, 2).unwrap();
        assert_eq!(next.generation, 2);
        assert_eq!(next.after, 2);
    }
    #[test]
    fn unavailable_axes_invalid_routes_hats_and_duplicate_routes_fail_closed() {
        let (provider, state) = fixture();
        for invalid in [
            "hid:123",
            "panel-events:bad/123",
            "panel-events:stm32:00112233445566778899aabb/00000000000000000000000000000000",
        ] {
            assert!(target(invalid).is_err());
        }
        let selected = BTreeSet::from(["hat:0".into()]);
        assert!(Cursor::prepare(&provider.panel, &source_id(G, M), &selected, 1, 1).is_err());
        let selected = BTreeSet::from(["axis:0".into()]);
        state.lock().unwrap()["moduleEvents"][M]["axisAvailable"] = json!([false]);
        assert!(Cursor::prepare(&provider.panel, &source_id(G, M), &selected, 1, 1).is_err());
        let p = Profile::parse(include_bytes!(
            "../../crates/mapping/fixtures/direct-profile.json"
        ))
        .unwrap();
        let mut a = assignments();
        let mut duplicate = a[0].clone();
        duplicate.source.id = source_id("stm32:11112233445566778899aabb", M);
        a.push(duplicate);
        assert!(provider.prepare(&p, &a).is_err());
        state.lock().unwrap()["layoutPending"] = json!(true);
        assert!(provider.inspect(&source_id(G, M)).is_err());
    }
    #[test]
    fn worker_detects_gap_rebaselines_on_reboot_and_drop_cancels() {
        let (provider, state) = fixture();
        let p = Profile::parse(include_bytes!(
            "../../crates/mapping/fixtures/direct-profile.json"
        ))
        .unwrap();
        let mut prepared = provider.prepare(&p, &assignments()).unwrap();
        let stop = prepared.feed.stop.clone();
        {
            let mut s = state.lock().unwrap();
            s["modules"][0]["boot"] = json!(5);
            s["moduleEvents"][M]["route"] = json!([1, 2, 3, 5]);
            s["moduleEvents"][M]["stream"] = json!(8);
        }
        let until = std::time::Instant::now() + Duration::from_secs(2);
        let mut reasons = Vec::new();
        while std::time::Instant::now() < until {
            reasons.extend(prepared.feed.drain().unwrap().into_iter().map(|f| f.reason));
            if reasons.contains(&Reason::Baseline) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(reasons.contains(&Reason::Gap));
        assert!(reasons.contains(&Reason::Baseline));
        drop(prepared);
        assert!(stop.load(Ordering::Acquire));
    }
}
