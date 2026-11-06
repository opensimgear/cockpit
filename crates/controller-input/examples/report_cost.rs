//! Synthetic admission-cost diagnostic, never a physical latency qualification.
use osg_controller_input::{
    registry::{Control, Definition, Identity, Registry},
    stream::{Reason, REPORT_LIMIT},
};
use osg_streams::{Broker, Reader};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    thread,
    time::{Duration, Instant},
};

const AXIS: &str = "hid:0001:0030:1:0:128";
const BUTTON: &str = "hid:0009:0001:1:0:0";
const REVERSE_BUTTON: &str = "hid:0009:0002:1:0:1";

fn encoder_buttons(report: u32) -> (u32, u32) {
    match report % 4 {
        1 => (1, 0),
        3 => (0, 1),
        _ => (0, 0),
    }
}

fn preview_axis(payload: &Value) -> Option<i64> {
    payload["controls"]
        .as_array()?
        .iter()
        .find(|row| row["capability"]["id"] == AXIS)?["value"]["native"]
        .as_i64()
}

fn paced(
    registry: &mut Registry,
    broker: &Broker,
    id: &str,
    reader: &mut Reader,
    data: &mut BTreeMap<u16, u32>,
    hz: u32,
) -> Value {
    const REPORTS: u32 = 300;
    let baseline = registry.stream_snapshot(&[id.into()]).unwrap();
    let period = Duration::from_secs(1) / hz;
    let start = Instant::now();
    let mut lateness = Vec::new();
    for index in 1..=REPORTS {
        let deadline = start + period * index;
        thread::sleep(deadline.saturating_duration_since(Instant::now()));
        lateness.push(deadline.elapsed().as_micros());
        let (forward, reverse) = encoder_buttons(index);
        data.insert(0, forward);
        data.insert(1, reverse);
        data.insert(128, index);
        registry.report(id, 1, data).unwrap();
    }
    let elapsed = start.elapsed();
    let page = registry.stream_page(&baseline.session, baseline.cursor);
    assert!(!page.gap);
    assert_eq!(page.events.len(), REPORTS as usize);
    for (index, event) in (1..=REPORTS).zip(&page.events) {
        assert_eq!(
            event.sequence,
            baseline.sources[0].baseline.sequence + u64::from(index)
        );
        let (forward, reverse) = encoder_buttons(index);
        assert_eq!(event.values[BUTTON].native, i64::from(forward));
        assert_eq!(event.values[REVERSE_BUTTON].native, i64::from(reverse));
        assert_eq!(event.values[AXIS].native, i64::from(index));
    }
    registry
        .prepare_observation(broker, id)
        .unwrap()
        .with_subscription(|| Ok(()))
        .unwrap();
    let mut preview_deliveries = 0;
    let mut preview_losses = 0;
    let mut latest_axis = None;
    while let Some(frame) = reader.try_recv().unwrap() {
        preview_deliveries += 1;
        if frame.kind == "loss" {
            preview_losses += 1;
            latest_axis = None;
        }
        if frame.kind == "snapshot" {
            latest_axis = preview_axis(&frame.payload.unwrap());
        }
    }
    assert_eq!(latest_axis, Some(i64::from(REPORTS)));
    for report in 1..=REPORT_LIMIT as u32 + 1 {
        let (forward, reverse) = encoder_buttons(report);
        data.insert(0, forward);
        data.insert(1, reverse);
        registry.report(id, 1, data).unwrap();
    }
    let overrun = registry.stream_page(&page.session, page.cursor);
    assert!(overrun.gap);
    assert!(
        overrun.events.is_empty(),
        "Overrun must not return partial edges"
    );
    let held = registry.stream_snapshot(&[id.into()]).unwrap();
    assert_eq!(held.sources[0].baseline.reason, Reason::Baseline);
    assert_eq!(held.sources[0].baseline.values[BUTTON].native, 1);
    registry.fault(id, "Synthetic report loss");
    assert!(registry.stream_snapshot(&[id.into()]).is_err());
    data.insert(0, 0);
    registry.report(id, 1, data).unwrap();
    let recovered = registry.stream_page(&held.session, held.cursor);
    assert!(!recovered.gap);
    assert_eq!(
        recovered
            .events
            .iter()
            .map(|event| &event.reason)
            .collect::<Vec<_>>(),
        vec![&Reason::Gap, &Reason::Baseline]
    );
    assert_eq!(recovered.events[1].values[BUTTON].native, 0);
    assert_eq!(recovered.events[1].values[REVERSE_BUTTON].native, 0);
    lateness.sort_unstable();
    json!({"targetHz":hz,"observedHz":f64::from(REPORTS)/elapsed.as_secs_f64(),
        "reports":REPORTS,"orderedEncoderButtonEdges":page.events.len(),"previewDeliveries":preview_deliveries,
        "overrunReports":REPORT_LIMIT+1,"overrunReturnedEdges":overrun.events.len(),"recoveredNeutralBaseline":true,
        "previewLosses":preview_losses,"schedulingLatenessP95Us":lateness[lateness.len()*95/100]})
}

fn main() {
    let mut results = Vec::new();
    let paced_mode = std::env::args().any(|argument| argument == "--paced");
    for (scenario, hz) in if paced_mode {
        vec![("open", 125), ("open", 1000)]
    } else {
        vec![("never-opened", 0), ("open", 0), ("opened-then-closed", 0)]
    } {
        let observed = scenario != "never-opened";
        let mut registry = Registry::default();
        let mut controls = Vec::new();
        for index in 0..128 {
            controls.push(Control::new(9, index + 1, 1, 0, index));
        }
        for index in 0..6 {
            let mut axis = Control::new(1, 0x30 + index, 1, 0, 128 + index);
            axis.bits = 12;
            axis.max = 4095;
            controls.push(axis);
        }
        let mut hat = Control::new(1, 0x39, 1, 0, 134);
        hat.bits = 4;
        hat.max = 7;
        hat.has_null = true;
        controls.push(hat);
        for index in 135..137 {
            let mut raw = Control::new(0, 0, 1, 0, index);
            raw.bits = 12;
            raw.max = 4095;
            controls.push(raw);
        }
        let id = registry
            .connect(Definition {
                identity: Identity {
                    backend: "diagnostic".into(),
                    vendor_id: 1,
                    product_id: 2,
                    serial: None,
                    collection: "capture-cost".into(),
                    fingerprint: String::new(),
                    path: "synthetic".into(),
                },
                name: "Synthetic 137-control report".into(),
                controls,
                diagnostics: vec![],
            })
            .unwrap();
        let mut data = BTreeMap::from([
            (128, 0),
            (129, 2048),
            (130, 2048),
            (131, 2048),
            (132, 2048),
            (133, 2048),
            (134, 8),
            (135, 0),
            (136, 0),
        ]);
        registry.report(&id, 1, &data).unwrap();
        let broker = Broker::new("report-cost".into()).unwrap();
        let mut reader = if observed {
            Some(
                registry
                    .prepare_observation(&broker, &id)
                    .unwrap()
                    .with_subscription(|| {
                        broker
                            .subscribe("diagnostic", "ui", &format!("controllers.input.{id}"))
                            .map_err(|e| e.to_string())
                    })
                    .unwrap(),
            )
        } else {
            None
        };
        if let Some(reader) = &mut reader {
            reader.recv().unwrap();
        }
        if paced_mode {
            results.push(paced(
                &mut registry,
                &broker,
                &id,
                reader.as_mut().unwrap(),
                &mut data,
                hz,
            ));
            continue;
        }
        if scenario == "opened-then-closed" {
            drop(reader.take());
        }
        let mut times = Vec::new();
        for index in 0..600 {
            data.insert(128, (index * 29) % 4096);
            let started = Instant::now();
            registry.report(&id, 1, &data).unwrap();
            let elapsed = started.elapsed().as_micros() as u64;
            if index >= 100 {
                times.push(elapsed);
            }
            if let Some(reader) = &mut reader {
                let frame = reader.recv().unwrap();
                assert_eq!(frame.kind, "snapshot");
                let payload = frame.payload.unwrap();
                assert_eq!(payload["source"]["sequence"], (index + 2).to_string());
                assert_eq!(preview_axis(&payload), Some(i64::from(data[&128])));
            }
        }
        times.sort_unstable();
        results.push(json!({"scenario":scenario,"previewRegistered":reader.is_some(),"controls":137,"samples":times.len(),
            "reportCostUs":{"p50":times[times.len()/2],"p95":times[times.len()*95/100],"p99":times[times.len()*99/100],"max":times.last()}}));
    }
    println!("{}", serde_json::to_string_pretty(&results).unwrap());
}
