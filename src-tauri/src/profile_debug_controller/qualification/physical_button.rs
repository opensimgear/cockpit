//! Bounded, passive read of the same ordered HID reports as the applied route.
use super::{serialize_timestamp, Clock};
use osg_controller_input::{
    registry::Kind,
    stream::{self, Event, Reason},
    ControllerService,
};
use osg_streams::NativeWake;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{sync::{Arc, Mutex}, time::Duration};

const MAX_DURATION_MS: u64 = 10_000;
const MAX_EDGES: usize = 256;
static TRACE: Mutex<()> = Mutex::new(());

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    command: String,
    source_id: String,
    control_id: String,
    duration_ms: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClockSample {
    #[serde(serialize_with = "serialize_timestamp")]
    before_ns: u64,
    #[serde(serialize_with = "serialize_timestamp")]
    now_us: u64,
    #[serde(serialize_with = "serialize_timestamp")]
    after_ns: u64,
}

fn clock_sample(qpc: &Clock, capture: stream::Clock) -> Result<ClockSample, String> {
    let before_ns = qpc.now()?;
    let now_us = capture.now_us();
    let after_ns = qpc.now()?;
    if after_ns < before_ns {
        return Err("The Windows performance counter moved backwards.".into());
    }
    Ok(ClockSample { before_ns, now_us, after_ns })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Edge {
    #[serde(serialize_with = "serialize_timestamp")]
    sequence: u64,
    #[serde(serialize_with = "serialize_timestamp")]
    native_read_us: u64,
    #[serde(serialize_with = "serialize_timestamp")]
    captured_us: u64,
    pressed: bool,
}

fn pressed(event: &Event, control: &str) -> Result<bool, String> {
    match event.values.get(control).map(|value| value.native) {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err("The selected physical button has no valid native level.".into()),
    }
}

struct Trace {
    generation: u64,
    sequence: u64,
    last_read_us: u64,
    pressed: bool,
    edges: Vec<Edge>,
}

impl Trace {
    fn advance(&mut self, event: Event, control: &str, start_us: u64) -> Result<(), String> {
        if event.reason != Reason::Change
            || event.generation != self.generation
            || self.sequence.checked_add(1) != Some(event.sequence)
        {
            return Err("Physical capture lost continuity during qualification.".into());
        }
        let read = event.native_read_us
            .filter(|read| *read >= self.last_read_us && *read <= event.captured_us)
            .ok_or("A physical report lacks ordered Windows Raw Input timing.")?;
        self.sequence = event.sequence;
        self.last_read_us = read;
        let level = pressed(&event, control)?;
        if self.pressed != level {
            if self.edges.len() == MAX_EDGES || read < start_us {
                return Err("A physical edge exceeded the trace bound or predates the baseline.".into());
            }
            self.pressed = level;
            self.edges.push(Edge {
                sequence: event.sequence,
                native_read_us: read,
                captured_us: event.captured_us,
                pressed: level,
            });
        }
        Ok(())
    }
}

pub(crate) fn run(controllers: &ControllerService, request: Value) -> Result<Value, String> {
    if std::env::var("OSG_REACTIVE_QUALIFICATION").as_deref() != Ok("1") {
        return Err("Physical controller qualification is disabled.".into());
    }
    let _guard = TRACE.try_lock().map_err(|_| "A physical button trace is already running.")?;
    let request: Request = serde_json::from_value(request).map_err(|error| error.to_string())?;
    if request.command != "qualification_physical_button"
        || uuid::Uuid::parse_str(&request.source_id).is_err()
        || request.control_id.len() > 128
        || !(1..=MAX_DURATION_MS).contains(&request.duration_ms)
    {
        return Err("Choose one physical controller button and a bounded trace duration.".into());
    }

    let wake = Arc::new(NativeWake::default());
    wake.bind_thread(std::thread::current()).map_err(|_| "Cannot bind the trace reader.")?;
    let notify = wake.clone();
    let ids = [request.source_id];
    let watch = controllers.watch_reports(&ids, Arc::new(move || notify.notify()))?;
    let snapshot = controllers.stream_snapshot(&ids)?;
    watch.advance(snapshot.cursor);
    let source = &snapshot.sources[0];
    let control = source.controls.iter().find(|control| {
        control.id == request.control_id && control.kind == Kind::Button
    }).ok_or("The selected native control must be a physical button.")?;
    if pressed(&source.baseline, &control.id)? {
        return Err("Release the selected physical button before tracing.".into());
    }
    let qpc = Clock::new()?;
    let clock = controllers.capture_clock();
    let before = clock_sample(&qpc, clock)?;
    let end_us = snapshot.captured_us.checked_add(request.duration_ms * 1000)
        .ok_or("The trace deadline exceeds the native clock range.")?;
    let mut cursor = snapshot.cursor;
    let mut trace = Trace {
        generation: source.baseline.generation,
        sequence: source.baseline.sequence,
        last_read_us: 0,
        pressed: false,
        edges: Vec::new(),
    };

    loop {
        let observed = wake.observed();
        let page = controllers.stream_page_for_watch(&snapshot.session, cursor, &watch);
        if page.gap {
            return Err("Physical trace lost its native journal cursor.".into());
        }
        watch.advance(page.cursor);
        cursor = page.cursor;
        for event in page.events {
            if event.captured_us > end_us {
                break;
            }
            trace.advance(event, &control.id, snapshot.captured_us)?;
        }
        // The page's own timestamp proves its boundary was captured after
        // the deadline. A clock read after an earlier page would miss a tail.
        if page.captured_us >= end_us {
            break;
        }
        let remaining = end_us.saturating_sub(clock.now_us());
        wake.wait(observed, Duration::from_micros(remaining));
    }
    let after = clock_sample(&qpc, clock)?;
    Ok(json!({
        "sourceId": ids[0],
        "control": control,
        "session": snapshot.session,
        "generation": trace.generation.to_string(),
        "firstSequence": source.baseline.sequence.to_string(),
        "lastSequence": trace.sequence.to_string(),
        "startUs": snapshot.captured_us.to_string(),
        "endUs": end_us.to_string(),
        "finalPressed": trace.pressed,
        "clockBefore": before,
        "clockAfter": after,
        "edges": trace.edges
    }))
}

#[cfg(test)]
mod tests;
