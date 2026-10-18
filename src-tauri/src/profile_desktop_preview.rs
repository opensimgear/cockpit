//! Recording-only replay of the current unified compilation. This module has
//! no native adapter, OS handles, profile storage or activation authority.
use osg_mapping::{
    desktop::{Delivery, Scheduler},
    engine::{
        typed::{Frame, Phase},
        Counter,
    },
    program::{Program, SinkKind},
};
use serde_json::{json, Value};
const MAX_FRAMES: usize = 256;
const MAX_CHANGES: usize = 4096;
const MAX_ADVANCES: usize = 2048;
const MAX_DURATION_US: u64 = 60_000_000;
const MAX_REPLY_BYTES: usize = 1_900_000;

pub(crate) fn record(program: &Program, request: &Value) -> Result<Value, String> {
    let frames: Vec<Frame> =
        serde_json::from_value(request.get("frames").cloned().unwrap_or_else(|| json!([])))
            .map_err(|error| format!("Invalid desktop recording frames: {error}"))?;
    let advances: Vec<Counter> = serde_json::from_value(
        request
            .get("advances")
            .cloned()
            .unwrap_or_else(|| json!([])),
    )
    .map_err(|error| format!("Invalid desktop recording clock advances: {error}"))?;
    if frames.len() > MAX_FRAMES
        || frames
            .iter()
            .map(|frame| frame.changes.len())
            .sum::<usize>()
            > MAX_CHANGES
        || advances.len() > MAX_ADVANCES
        || frames
            .iter()
            .any(|frame| frame.captured_us.0 > MAX_DURATION_US)
        || advances.iter().any(|at| at.0 > MAX_DURATION_US)
        || frames
            .windows(2)
            .any(|pair| pair[0].captured_us > pair[1].captured_us)
        || advances.windows(2).any(|pair| pair[0] > pair[1])
    {
        return Err("Desktop recording requires ordered bounded input: 256 frames, 4096 changes, 2048 advances and 60 seconds.".into());
    }
    program.compile()?;
    if !program
        .sinks
        .iter()
        .any(|sink| matches!(sink.kind, SinkKind::Desktop { .. }))
    {
        return Err(
            "Connect an exact desktop action in the current profile graph before recording it."
                .into(),
        );
    }
    let mut phase = Phase::new(
        program.clone(),
        Counter(1),
        "a78d48b8-3958-4de5-a8b3-d3b52b1e24e7".into(),
    )?;
    let mut scheduler = Scheduler::from_program(phase.run_token(), program)?;
    let mut deliveries = vec![];
    pump(&mut phase, &mut scheduler, &mut deliveries)?;
    let mut frames = frames.into_iter().peekable();
    let mut advances = advances.into_iter().peekable();
    while frames.peek().is_some() || advances.peek().is_some() {
        let at = frames
            .peek()
            .map(|frame| frame.captured_us)
            .into_iter()
            .chain(advances.peek().copied())
            .min()
            .unwrap();
        let mut batch = vec![];
        while frames.peek().is_some_and(|frame| frame.captured_us == at) {
            batch.push(frames.next().unwrap());
        }
        if !batch.is_empty() {
            let diagnostics = phase.accept_batch(batch)?;
            if !diagnostics.is_empty() {
                return Err(format!(
                    "Desktop recording input was rejected: {}",
                    diagnostics.join(" ")
                ));
            }
        }
        while advances.peek().is_some_and(|advance| *advance == at) {
            advances.next();
        }
        phase.advance(at)?;
        pump(&mut phase, &mut scheduler, &mut deliveries)?;
    }
    phase.stop();
    scheduler.stop();
    deliveries.extend(scheduler.drain());
    if deliveries.len() > osg_mapping::desktop::MAX_DELIVERIES {
        return Err("Desktop recording exceeds 4096 ordered operations.".into());
    }
    let mut reducer = osg_desktop_output::reducer::Reducer::default();
    let mut owners = std::collections::BTreeMap::new();
    let mut operations = vec![];
    for delivery in &deliveries {
        let cleanup = matches!(&delivery.intent, osg_mapping::desktop::Intent::Cleanup)
            || matches!(&delivery.intent,osg_mapping::desktop::Intent::Held{controls}if controls.is_empty());
        let mut events = vec![];
        let mut collision = false;
        if !cleanup || owners.get(&delivery.action_id) == Some(&delivery.producer) {
            if !cleanup && owners.get(&delivery.action_id) != Some(&delivery.producer) {
                events.extend(reducer.arm(delivery.producer.clone(), delivery.action_id.clone())?);
                owners.insert(delivery.action_id.clone(), delivery.producer.clone());
            }
            let reduction = reducer.delivery(delivery, false)?;
            events.extend(reduction.events);
            collision = reduction.collision;
        }
        operations.push(json!({"sequence":delivery.sequence,"operationId":delivery.operation_id,"actionId":delivery.action_id,
            "atUs":delivery.at_us,"events":events,"collision":collision,"held":reducer.held()}));
    }
    let result = json!({"mode":"recording","nativeSubmission":false,"deliveries":deliveries,"operations":operations,"decisions":scheduler.decisions(),
        "trace":scheduler.trace(),"fault":scheduler.fault(),"droppedDecisions":scheduler.dropped_decisions(),
        "limitations":"Synthetic recording proves scheduling and ownership intents; native submission and application behavior require separate qualification."});
    if serde_json::to_vec(&result)
        .map_err(|error| error.to_string())?
        .len()
        > MAX_REPLY_BYTES
    {
        return Err(
            "Desktop recording exceeds its inspection reply budget; record a shorter trace.".into(),
        );
    }
    Ok(result)
}
fn pump(
    phase: &mut Phase,
    scheduler: &mut Scheduler,
    deliveries: &mut Vec<Delivery>,
) -> Result<(), String> {
    for sink in &phase.program().sinks {
        if matches!(sink.kind, SinkKind::Desktop { .. }) {
            scheduler.reconcile_producer(&sink.id, phase.endpoint_token(&sink.from)?)?;
        }
    }
    for effect in phase.drain() {
        if let Some(producer) = &effect.producer {
            let sink = phase
                .program()
                .sinks
                .iter()
                .find(|sink| sink.id == effect.sink_id)
                .unwrap();
            if *producer != phase.endpoint_token(&sink.from)? {
                continue;
            }
        }
        scheduler.accept_effect(&effect)?;
    }
    scheduler.advance(phase.at_us())?;
    deliveries.extend(scheduler.drain());
    if deliveries.len() > osg_mapping::desktop::MAX_DELIVERIES {
        return Err("Desktop recording exceeds 4096 ordered operations.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use osg_mapping::{
        desktop::{Action, Intent, Key, Trace},
        engine::{typed::Change, Reason},
        program::*,
        signal::{PortContract, PortValue, ScalarSchema},
    };
    fn program() -> Program {
        let action = Action::KeyHold { key: Key(4) };
        let source = Endpoint {
            node_id: "source".into(),
            port_id: "button".into(),
        };
        Program {
            sources: vec![SourcePort {
                endpoint: source.clone(),
                contract: action.contract(),
            }],
            nodes: vec![],
            sinks: vec![Sink {
                id: "action".into(),
                endpoint: Endpoint {
                    node_id: "desktop".into(),
                    port_id: "key-a".into(),
                },
                from: source,
                contract: action.contract(),
                kind: SinkKind::Desktop { action },
            }],
            controllers: vec![],
        }
    }
    fn frame(sequence: u64, at: u64, reason: Reason, value: bool) -> Frame {
        Frame {
            source: "source".into(),
            generation: Counter(1),
            sequence: Counter(sequence),
            captured_us: Counter(at),
            reason,
            changes: vec![Change {
                port_id: "button".into(),
                sample: SignalSample::scalar(PortValue::Boolean(value)),
            }],
        }
    }
    #[test]
    fn current_graph_records_exact_held_actions_and_replays_without_native_submission() {
        let request = json!({"frames":[frame(1,0,Reason::Baseline,false),frame(2,1,Reason::Change,true),frame(3,100_000,Reason::Change,false)],"advances":["50000","200000"]});
        let result = record(&program(), &request).unwrap();
        assert_eq!(result["nativeSubmission"], false);
        let deliveries: Vec<Delivery> =
            serde_json::from_value(result["deliveries"].clone()).unwrap();
        assert!(deliveries.iter().any(
            |delivery| matches!(&delivery.intent,Intent::Held{controls} if !controls.is_empty())
        ));
        assert!(deliveries.iter().any(
            |delivery| matches!(&delivery.intent,Intent::Held{controls} if controls.is_empty())
        ));
        let trace: Trace = serde_json::from_value(result["trace"].clone()).unwrap();
        assert_eq!(trace.replay().unwrap().deliveries, deliveries);
        assert_eq!(result, record(&program(), &request).unwrap());
    }
    #[test]
    fn preview_rejects_missing_baselines_stale_generations_backwards_time_and_excessive_work() {
        let no_baseline = json!({"frames":[frame(2,1,Reason::Change,true)]});
        assert!(record(&program(), &no_baseline).is_err());
        for request in [
            json!({"frames":[frame(1,2,Reason::Baseline,false),frame(2,1,Reason::Change,true)]}),
            json!({"advances":["2","1"]}),
            json!({"advances":["60000001"]}),
            json!({"advances":[1]}),
            json!({"frames":vec![frame(1,0,Reason::Baseline,false);257]}),
        ] {
            assert!(record(&program(), &request).is_err());
        }
    }
    #[test]
    fn wheel_preview_keeps_distinct_occurrences_and_ids() {
        let mut program = program();
        let contract = PortContract::Event {
            schema: ScalarSchema::Boolean {},
        };
        program.sources[0].contract = contract.clone();
        program.sinks[0].contract = contract;
        program.sinks[0].kind = SinkKind::Desktop {
            action: Action::Wheel {
                horizontal: 0,
                vertical: 1,
            },
        };
        let mut frames = vec![frame(1, 0, Reason::Baseline, false)];
        for sequence in 2..=11 {
            frames.push(frame(sequence, sequence, Reason::Change, true));
        }
        let result = record(&program, &json!({"frames":frames})).unwrap();
        let deliveries: Vec<Delivery> =
            serde_json::from_value(result["deliveries"].clone()).unwrap();
        assert_eq!(
            deliveries
                .iter()
                .filter(|delivery| matches!(delivery.intent, Intent::Wheel { .. }))
                .count(),
            10
        );
    }
}
