//! Opt-in evidence for one applied native telemetry endpoint.
use crate::profile_session::RunOwner;
use osg_mapping::{
    engine::typed::{
        native::{Admission, NativeChangeKind, NativeInput, NativeValidity},
        Phase,
    },
    program::Endpoint,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};

const MAX_ROWS: usize = 512;
const TRACE_WINDOW: Duration = Duration::from_secs(30);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static TRACKER: Mutex<Option<Tracker>> = Mutex::new(None);

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Start {
        owner: RunOwner,
        source: String,
        port: String,
    },
    Finish {
        owner: RunOwner,
    },
}

struct Tracker {
    owner: RunOwner,
    expires: Instant,
    endpoint: Endpoint,
    rows: Vec<Value>,
    last: Option<(String, u64, u64)>,
    error: Option<String>,
}

pub(super) fn active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

pub(crate) fn request(
    mut request: Value,
    current_owner: Option<&RunOwner>,
) -> Result<Value, String> {
    if std::env::var("OSG_REACTIVE_QUALIFICATION").as_deref() != Ok("1") {
        return Err("Desktop qualification is disabled.".into());
    }
    request
        .as_object_mut()
        .ok_or("Invalid native timing request.")?
        .remove("command");
    let request = serde_json::from_value(request).map_err(|error| error.to_string())?;
    let mut slot = TRACKER.lock().unwrap_or_else(|p| p.into_inner());
    match request {
        Request::Start {
            owner,
            source,
            port,
        } => {
            if current_owner != Some(&owner)
                || owner.game_id.as_deref() != Some("dcs")
                || [&source, &port]
                    .into_iter()
                    .any(|value| value.is_empty() || value.len() > 512)
            {
                return Err("Select one exact DCS profile endpoint.".into());
            }
            let now = Instant::now();
            if slot.as_ref().is_some_and(|tracker| {
                tracker.owner == owner && tracker.error.is_none() && now < tracker.expires
            }) {
                return Err("A native timing trace is already active.".into());
            }
            *slot = Some(Tracker {
                owner,
                expires: now + TRACE_WINDOW,
                endpoint: Endpoint {
                    node_id: source,
                    port_id: port,
                },
                rows: Vec::new(),
                last: None,
                error: None,
            });
            ACTIVE.store(true, Ordering::Release);
            Ok(json!({}))
        }
        Request::Finish { owner } => {
            if !slot.as_ref().is_some_and(|tracker| tracker.owner == owner) {
                return Err(
                    "This native timing trace belongs to a different profile owner.".into(),
                );
            }
            let tracker = slot.take().unwrap();
            ACTIVE.store(false, Ordering::Release);
            if let Some(error) = tracker.error {
                return Err(error);
            }
            if Instant::now() >= tracker.expires {
                return Err("The native timing trace expired.".into());
            }
            Ok(json!({ "rows": tracker.rows }))
        }
    }
}

pub(super) fn accepted(
    owner: &RunOwner,
    admissions: &[Admission],
    diagnostics: &[String],
    reducer: &Phase,
    applied_us: u64,
) {
    let mut slot = TRACKER.lock().unwrap_or_else(|p| p.into_inner());
    let Some(tracker) = slot
        .as_mut()
        .filter(|tracker| &tracker.owner == owner && tracker.error.is_none())
    else {
        return;
    };
    if Instant::now() >= tracker.expires {
        tracker.error = Some("The native timing trace expired.".into());
        ACTIVE.store(false, Ordering::Release);
        return;
    }
    observe(tracker, admissions, diagnostics, reducer, applied_us);
}

fn observe(
    tracker: &mut Tracker,
    admissions: &[Admission],
    diagnostics: &[String],
    reducer: &Phase,
    applied_us: u64,
) {
    for admission in admissions {
        let Admission::Native(frame) = admission else {
            continue;
        };
        if frame.source != tracker.endpoint.node_id {
            continue;
        }
        let Some(change) = frame.changes.iter().find(|change| {
            change.port_id == tracker.endpoint.port_id && change.kind == NativeChangeKind::Update
        }) else {
            continue;
        };
        let [contributor] = change.contributors.as_slice() else {
            continue;
        };
        if !matches!(&contributor.input, NativeInput::Channel(channel) if channel == &change.port_id)
            || !matches!(contributor.validity, NativeValidity::Sampled { .. })
        {
            continue;
        }
        let Some(capture) = frame.captures.get(contributor.capture as usize) else {
            tracker.error = Some("The native timing capture reference is missing.".into());
            return;
        };
        let applied = reducer.inspected_observation(&tracker.endpoint);
        if !diagnostics.is_empty()
            || !applied.is_some_and(|sample| {
                sample.sample == &change.sample
                    && sample.captured_at == Some(capture.captured_us)
                    && sample.captured_sequence == capture.sequence
            })
        {
            tracker.error = Some("The reducer did not retain the exact native capture.".into());
            return;
        }
        let same_producer = tracker
            .last
            .as_ref()
            .is_none_or(|(producer, generation, sequence)| {
                producer == &capture.producer_id
                    && *generation == capture.generation.0
                    && *sequence < capture.sequence.0
            });
        if !same_producer || applied_us < capture.captured_us.0 || tracker.rows.len() == MAX_ROWS {
            tracker.error =
                Some("Native timing lost its producer, order, clock or trace budget.".into());
            return;
        }
        tracker.last = Some((
            capture.producer_id.clone(),
            capture.generation.0,
            capture.sequence.0,
        ));
        tracker.rows.push(json!({
            "producer": capture.producer_id, "generation": capture.generation, "sequence": capture.sequence,
            "capturedUs": capture.captured_us, "appliedUs": applied_us.to_string(), "clockId": frame.clock_id,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use osg_mapping::{
        engine::{
            typed::native::{
                NativeCapture, NativeChange, NativeContributor, NativeFrame, NativePolicy,
                NativePortRegistration, NativeSourceRegistration,
            },
            Counter,
        },
        program::{Program, SignalSample, SourcePort},
        signal::{PortContract, PortValue, ScalarSchema},
    };

    #[test]
    fn trace_command_requires_opt_in_and_fences_the_exact_owner() {
        if std::env::var("OSG_REACTIVE_QUALIFICATION").as_deref() != Ok("1") {
            assert_eq!(
                request(Value::Null, None),
                Err("Desktop qualification is disabled.".into())
            );
            return;
        }
        let owner = json!({
            "session": "fixture", "generation": 1, "profileId": "fixture",
            "profileRevision": 1, "gameId": "dcs",
        });
        let start = json!({
            "command": "qualification_native_timing", "action": "start", "owner": owner,
            "source": "telemetry", "port": "airspeed",
        });
        let current = serde_json::from_value(owner.clone()).unwrap();
        assert!(request(start.clone(), None).is_err());
        let mut invalid = start.clone();
        invalid["port"] = json!("");
        assert!(request(invalid, Some(&current)).is_err());
        assert!(!active());
        request(start.clone(), Some(&current)).unwrap();
        assert!(active());
        assert!(request(start.clone(), Some(&current)).is_err());
        let finish = json!({"action": "finish", "owner": owner});
        let mut stale = finish.clone();
        stale["owner"]["generation"] = json!(2);
        assert!(request(stale, None).is_err());
        assert!(active());
        TRACKER.lock().unwrap().as_mut().unwrap().expires = Instant::now();
        request(start, Some(&current)).unwrap();
        assert_eq!(request(finish, None).unwrap(), json!({"rows": []}));
        assert!(!active());
    }

    #[test]
    fn trace_requires_the_exact_retained_native_capture_and_order() {
        let endpoint = Endpoint {
            node_id: "telemetry".into(),
            port_id: "airspeed".into(),
        };
        let mut reducer = Phase::new(
            Program {
                sources: vec![SourcePort {
                    endpoint: endpoint.clone(),
                    contract: PortContract::State {
                        schema: ScalarSchema::Boolean {},
                        freshness_ms: 100,
                        fallback: None,
                    },
                }],
                nodes: vec![],
                sinks: vec![],
                controllers: vec![],
            },
            Counter(1),
            "b3738156-6794-42dc-978b-2376dce0c6c6".into(),
        )
        .unwrap();
        let clock_id = "ed88a2b1-9774-467e-9c69-5b06ba9048eb";
        reducer.pin_native_clock(clock_id).unwrap();
        reducer
            .register_native_source(NativeSourceRegistration {
                source: endpoint.node_id.clone(),
                clock_id: clock_id.into(),
                ports: vec![NativePortRegistration {
                    port_id: endpoint.port_id.clone(),
                    family: "ownship".into(),
                    policy: NativePolicy::Sampled,
                }],
            })
            .unwrap();
        let frame = |sequence, kind| {
            Admission::Native(NativeFrame {
                source: "telemetry".into(),
                generation: Counter(1),
                sequence: Counter(sequence),
                clock_id: clock_id.into(),
                captures: vec![NativeCapture {
                    producer_id: "receiver".into(),
                    generation: Counter(1),
                    sequence: Counter(sequence),
                    byte_offset: None,
                    captured_us: Counter(sequence * 10),
                }],
                leases: vec![],
                changes: vec![NativeChange {
                    port_id: "airspeed".into(),
                    continuity: Counter(1),
                    kind,
                    sample: SignalSample::scalar(PortValue::Boolean(true)),
                    contributors: vec![NativeContributor {
                        input: NativeInput::Channel("airspeed".into()),
                        capture: 0,
                        validity: NativeValidity::Sampled {
                            valid_until_us: Counter(100_000),
                        },
                    }],
                }],
            })
        };
        let new_tracker = || Tracker {
            owner: RunOwner {
                session: "fixture".into(),
                generation: 1,
                profile_id: "fixture".into(),
                profile_revision: 1,
                game_id: Some("dcs".into()),
            },
            expires: Instant::now() + TRACE_WINDOW,
            endpoint: endpoint.clone(),
            rows: Vec::new(),
            last: None,
            error: None,
        };
        let baseline = frame(1, NativeChangeKind::Baseline);
        let update = frame(2, NativeChangeKind::Update);
        let mut tracker = new_tracker();
        reducer
            .accept_admissions([baseline.clone()], Counter(11))
            .unwrap();
        observe(&mut tracker, &[baseline], &[], &reducer, 12);
        assert!(tracker.rows.is_empty());
        observe(
            &mut tracker,
            std::slice::from_ref(&update),
            &[],
            &reducer,
            21,
        );
        assert!(tracker.error.is_some());

        let mut tracker = new_tracker();
        reducer
            .accept_admissions([update.clone()], Counter(22))
            .unwrap();
        observe(
            &mut tracker,
            std::slice::from_ref(&update),
            &[],
            &reducer,
            23,
        );
        assert_eq!(tracker.rows[0]["capturedUs"], "20");
        assert_eq!(tracker.rows[0]["appliedUs"], "23");
        observe(&mut tracker, &[update], &[], &reducer, 25);
        assert!(tracker.error.is_some());
        assert_eq!(tracker.rows.len(), 1);
    }
}
