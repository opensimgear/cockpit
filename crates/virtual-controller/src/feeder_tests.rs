use super::*;
use crate::vjoy::{self, Axis, Position, Slot};
use osg_mapping::engine::{ReportReason, TargetState};
use std::sync::{Arc, Mutex};
#[derive(Default)]
struct State {
    fail: String,
    calls: Vec<String>,
    busy: u8,
    slot: Option<Slot>,
    positions: Vec<Position>,
}
#[derive(Clone, Default)]
struct Fake(Arc<Mutex<State>>);
impl Driver for Fake {
    fn check(&mut self) -> Result<(), String> {
        if self.0.lock().unwrap().fail == "check" {
            Err("missing driver".into())
        } else {
            Ok(())
        }
    }
    fn inspect(&mut self, slot: u8) -> Result<Slot, String> {
        let state = self.0.lock().unwrap();
        if state.fail == "inspect" {
            return Err("inspect failed".into());
        }
        if let Some(actual) = state.slot.as_ref().filter(|actual| actual.id == slot) {
            return Ok(actual.clone());
        }
        let status = if slot == state.busy {
            Status::Busy
        } else if slot > 2 {
            Status::Missing
        } else {
            Status::Free
        };
        Ok(Slot {
            id: slot,
            status,
            owner_pid: None,
            buttons: 128,
            axes: vjoy::AXIS_USAGES
                .iter()
                .map(|u| Axis {
                    usage: *u,
                    min: 0,
                    max: 32767,
                })
                .collect(),
            continuous_hats: 4,
            discrete_hats: 0,
            force_feedback: false,
        })
    }
    fn acquire(&mut self, slot: u8) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        state.calls.push(format!("acquire:{slot}"));
        if state.fail == format!("acquire:{slot}") {
            Err("acquire failed".into())
        } else {
            Ok(())
        }
    }
    fn submit(&mut self, slot: u8, position: &Position) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        state.positions.push(position.clone());
        state
            .calls
            .push(format!("submit:{slot}:{}", position.words[18]));
        if state.fail == format!("submit:{slot}") {
            Err("submit failed".into())
        } else {
            Ok(())
        }
    }
    fn release(&mut self, slot: u8) {
        self.0.lock().unwrap().calls.push(format!("release:{slot}"));
    }
}

#[test]
fn typed_native_start_recording_reaches_the_strict_feeder_without_rewriting_reports() {
    use crate::session::{Channel, Factory, Session};
    use osg_mapping::{
        engine::{typed, Reason},
        profile::{Axis as LayoutAxis, AxisRange, Layout, OutputControl},
        program::{
            ControllerAxis, ControllerLayout, Endpoint, Program, Sink, SinkKind, SourcePort,
            VirtualController,
        },
        runtime::RecordingSink,
        signal::{PortContract, PortValue, ScalarSchema},
    };
    struct ActualFeeder {
        feeder: Feeder<Fake>,
        messages: Arc<Mutex<Vec<Message>>>,
        now: u64,
    }
    impl Channel for ActualFeeder {
        fn request(&mut self, message: &Message) -> Result<(), String> {
            self.messages.lock().unwrap().push(message.clone());
            self.now += 1;
            self.feeder.receive(message.clone(), self.now)
        }
        fn alive(&mut self) -> bool {
            true
        }
        fn terminate(&mut self) {
            self.feeder.stop();
        }
    }
    let driver = Fake::default();
    driver.0.lock().unwrap().slot = Some(Slot {
        id: 3,
        status: Status::Free,
        owner_pid: None,
        buttons: 16,
        axes: [48, 49, 50]
            .into_iter()
            .map(|usage| Axis {
                usage,
                min: 0,
                max: 32767,
            })
            .collect(),
        continuous_hats: 2,
        discrete_hats: 0,
        force_feedback: false,
    });
    let binding = Binding {
        target: "combined".into(),
        slot: 3,
        axes: vec![48, 49, 50],
        layout: Layout {
            buttons: 16,
            axes: [16384, 16384, 0]
                .into_iter()
                .map(|fallback| LayoutAxis {
                    range: AxisRange::Unipolar,
                    fallback,
                })
                .collect(),
            hats: 2,
        },
    };
    let endpoint = |node: &str, port: &str| Endpoint {
        node_id: node.into(),
        port_id: port.into(),
    };
    let contract = PortContract::State {
        schema: ScalarSchema::Integer {
            min: 0,
            max: 32767,
            unit: None,
        },
        freshness_ms: 1000,
        fallback: None,
    };
    let program = Program {
        sources: vec![SourcePort {
            endpoint: endpoint("stick", "axis"),
            contract: contract.clone(),
        }],
        nodes: vec![],
        sinks: vec![Sink {
            id: "roll".into(),
            endpoint: endpoint("combined", "roll"),
            from: endpoint("stick", "axis"),
            contract,
            kind: SinkKind::Virtual {
                controller_id: "combined".into(),
                control: OutputControl::Axis { index: 0 },
            },
        }],
        controllers: vec![VirtualController {
            id: "combined".into(),
            layout: ControllerLayout {
                buttons: (0..16).map(|i| format!("reserved-{i}")).collect(),
                axes: ["roll", "pitch", "collective"]
                    .into_iter()
                    .zip([16384, 16384, 0])
                    .map(|(port, fallback)| ControllerAxis {
                        port_id: port.into(),
                        range: AxisRange::Unipolar,
                        fallback,
                    })
                    .collect(),
                hats: vec!["right-view".into(), "left-view".into()],
            },
        }],
    };
    let run = token();
    let mut phase =
        typed::Phase::new_at(program, run.generation, run.session.clone(), Counter(10000)).unwrap();
    let mut effects = phase.drain();
    let mut recording = RecordingSink::default();
    recording.record(phase.drain_reports());
    let messages = Arc::new(Mutex::new(Vec::new()));
    let captured = messages.clone();
    let actual = driver.clone();
    let factory: Factory = Arc::new(move || {
        Ok(Box::new(ActualFeeder {
            feeder: Feeder::new(actual.clone()),
            messages: captured.clone(),
            now: 1,
        }))
    });
    let mut output =
        Session::start(factory, run.clone(), vec![binding.clone()], Counter(0)).unwrap();
    output
        .poll(&recording)
        .expect("the native typed constructor's first complete report must pass the actual feeder");
    phase
        .accept(typed::Frame {
            source: "stick".into(),
            generation: Counter(1),
            sequence: Counter(1),
            captured_us: Counter(9000),
            reason: Reason::Baseline,
            changes: vec![typed::Change {
                port_id: "axis".into(),
                sample: osg_mapping::program::SignalSample::scalar(PortValue::Number(23456.0)),
            }],
        })
        .unwrap();
    recording.record(phase.drain_reports());
    effects.extend(phase.drain());
    output.poll(&recording).unwrap();
    phase.advance(Counter(10500)).unwrap();
    recording.record(phase.drain_reports());
    assert!(
        phase.drain().is_empty(),
        "Unchanged time must not replay effects"
    );
    output.poll(&recording).unwrap();
    let reports = || {
        messages
            .lock()
            .unwrap()
            .iter()
            .filter_map(|message| match message {
                Message::Report { report, .. } => Some(report.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let delivered = reports();
    assert_eq!(delivered.len(), 2);
    assert_eq!(
        delivered.iter().map(|r| r.sequence.0).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(delivered.iter().all(|report| report.at_us.0 > 0
        && report.session == run.session
        && report.run_generation == run.generation));
    assert_eq!(delivered[0].reason, ReportReason::Start);
    assert_eq!(delivered[0].at_us, Counter(10000));
    assert_eq!(delivered[0].state, binding.fallback());
    assert_eq!(delivered[1].reason, ReportReason::Baseline);
    assert_eq!(delivered[1].at_us, Counter(10000));
    assert_eq!(delivered[1].state.axes, vec![23456, 16384, 0]);
    assert_eq!(delivered[1].state.buttons, vec![false; 16]);
    assert_eq!(delivered[1].state.hats, vec![None, None]);
    assert_eq!(
        phase
            .inspected_sample(&endpoint("stick", "axis"))
            .unwrap()
            .1,
        Some(Counter(9000))
    );
    phase.stop();
    effects.extend(phase.drain());
    recording.record(phase.drain_reports());
    output.poll(&recording).unwrap();
    output.stop().unwrap();
    assert_eq!(
        reports()
            .iter()
            .map(|report| report.sequence.0)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(reports()[2].state, binding.fallback());
    assert_eq!(
        effects
            .iter()
            .map(|effect| effect.sequence.0)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(
        effects
            .iter()
            .map(|effect| effect.reason)
            .collect::<Vec<_>>(),
        vec![
            ReportReason::Start,
            ReportReason::Baseline,
            ReportReason::Stop
        ]
    );
    assert!(matches!(
        effects[0].sample,
        osg_mapping::program::SignalSample::Unknown
    ));
    assert_eq!(
        effects[1].sample,
        osg_mapping::program::SignalSample::scalar(PortValue::Number(23456.0))
    );
    assert!(matches!(
        effects[2].sample,
        osg_mapping::program::SignalSample::Unavailable
    ));
    let state = driver.0.lock().unwrap();
    assert_eq!(
        state
            .calls
            .iter()
            .filter(|call| call.as_str() == "acquire:3")
            .count(),
        1
    );
    assert_eq!(
        state
            .calls
            .iter()
            .filter(|call| call.as_str() == "release:3")
            .count(),
        1
    );
    assert_eq!(state.positions.len(), 5); // acquire fallback, Start, Baseline, Stop report, release fallback
    assert_eq!(
        state.positions[2],
        binding.report(&delivered[1].state).unwrap()
    );
}
fn token() -> RunToken {
    RunToken {
        session: uuid::Uuid::new_v4().to_string(),
        generation: Counter(1),
    }
}
fn start(feeder: &mut Feeder<Fake>, token: &RunToken) {
    feeder
        .receive(
            Message::Start {
                token: token.clone(),
                bindings: vec![vjoy::tests::binding()],
            },
            1,
        )
        .unwrap();
}
fn report(token: &RunToken, sequence: u64, state: TargetState) -> Message {
    Message::Report {
        token: token.clone(),
        report: Report {
            session: token.session.clone(),
            run_generation: token.generation,
            sequence: Counter(sequence),
            at_us: Counter(sequence),
            target: "main".into(),
            reason: ReportReason::Change,
            state,
        },
    }
}
#[test]
fn initial_fallback_ordered_pulses_renewal_expiry_and_stale_tokens() {
    let driver = Fake::default();
    let mut feeder = Feeder::new(driver.clone());
    let run = token();
    start(&mut feeder, &run);
    assert_eq!(driver.0.lock().unwrap().calls, ["acquire:1", "submit:1:0"]);
    let mut held = vjoy::tests::binding().fallback();
    held.buttons[0] = true;
    feeder.receive(report(&run, 1, held), 2).unwrap();
    feeder
        .receive(report(&run, 2, vjoy::tests::binding().fallback()), 3)
        .unwrap();
    let other = token();
    assert!(feeder
        .receive(
            Message::Renew {
                token: other,
                sequence: Counter(1)
            },
            4
        )
        .is_err());
    feeder
        .receive(
            Message::Renew {
                token: run.clone(),
                sequence: Counter(1),
            },
            50_001,
        )
        .unwrap();
    assert!(feeder
        .receive(
            Message::Renew {
                token: run.clone(),
                sequence: Counter(1)
            },
            50_002
        )
        .is_err());
    feeder.tick(250_002).unwrap();
    assert!(feeder.tick(300_001).is_err());
    assert!(!feeder.active());
    assert!(feeder.fault().unwrap().contains("expired"));
    assert_eq!(
        driver.0.lock().unwrap().calls,
        [
            "acquire:1",
            "submit:1:0",
            "submit:1:1",
            "submit:1:0",
            "submit:1:0",
            "release:1"
        ]
    );
    assert!(feeder
        .receive(
            Message::Start {
                token: run.clone(),
                bindings: vec![vjoy::tests::binding()]
            },
            400_000
        )
        .is_err());
    assert!(feeder
        .receive(report(&run, 3, vjoy::tests::binding().fallback()), 400_001)
        .is_err());
    assert!(feeder
        .receive(Message::Stop { token: run }, 400_002)
        .is_err());
}
#[test]
fn preflight_partial_acquisition_and_submit_failure_release_only_owned_slots() {
    for failure in ["check", "inspect", "acquire:2", "submit:1"] {
        let driver = Fake::default();
        driver.0.lock().unwrap().fail = failure.into();
        let mut feeder = Feeder::new(driver.clone());
        let first = vjoy::tests::binding();
        let mut second = first.clone();
        second.target = "aux".into();
        second.slot = 2;
        assert!(feeder
            .receive(
                Message::Start {
                    token: token(),
                    bindings: vec![first, second]
                },
                1
            )
            .is_err());
        assert!(!feeder.active());
        let calls = &driver.0.lock().unwrap().calls;
        if failure == "acquire:2" || failure == "submit:1" {
            assert!(calls.contains(&"release:1".into()));
        }
        assert!(!calls.contains(&"release:2".into()));
    }
    let driver = Fake::default();
    driver.0.lock().unwrap().busy = 1;
    let mut feeder = Feeder::new(driver.clone());
    assert!(feeder
        .receive(
            Message::Start {
                token: token(),
                bindings: vec![vjoy::tests::binding()]
            },
            1
        )
        .is_err());
    assert!(driver.0.lock().unwrap().calls.is_empty());
}
#[test]
fn malformed_reports_rollback_stop_drop_and_explicit_recovery() {
    let driver = Fake::default();
    let mut feeder = Feeder::new(driver.clone());
    let run = token();
    assert_eq!(vjoy::inventory(&mut driver.clone()).unwrap().len(), 2);
    start(&mut feeder, &run);
    assert!(feeder
        .receive(
            Message::Start {
                token: token(),
                bindings: vec![vjoy::tests::binding()]
            },
            2
        )
        .is_err());
    for sequence in [0, 1] {
        let mut message = report(&run, sequence, vjoy::tests::binding().fallback());
        if let Message::Report { report, .. } = &mut message {
            if sequence == 1 {
                report.session = "old".into();
            }
        }
        assert!(feeder.receive(message, 2).is_err());
    }
    let mut bad = report(&run, 1, vjoy::tests::binding().fallback());
    if let Message::Report { report, .. } = &mut bad {
        report.target = "missing".into();
    }
    assert!(feeder.receive(bad, 3).is_err());
    let mut bad = vjoy::tests::binding().fallback();
    bad.hats[0] = Some(9);
    assert!(feeder.receive(report(&run, 1, bad), 4).is_err());
    assert!(feeder.recover(&[vjoy::tests::binding()]).is_err());
    driver.0.lock().unwrap().fail = "submit:1".into();
    assert!(feeder
        .receive(report(&run, 1, vjoy::tests::binding().fallback()), 5)
        .is_err());
    assert!(!feeder.active());
    driver.0.lock().unwrap().fail.clear();
    feeder.recover(&[vjoy::tests::binding()]).unwrap();
    driver.0.lock().unwrap().busy = 1;
    assert!(feeder.recover(&[vjoy::tests::binding()]).is_err());
    driver.0.lock().unwrap().busy = 0;
    let run = token();
    start(&mut feeder, &run);
    feeder.receive(Message::Stop { token: run }, 2).unwrap();
    assert!(!feeder.active());
    let run = token();
    start(&mut feeder, &run);
    drop(feeder);
    assert_eq!(driver.0.lock().unwrap().calls.last().unwrap(), "release:1");
}
#[test]
fn start_bounds_and_duplicate_slots_do_not_touch_driver() {
    let mut feeder = Feeder::new(Fake::default());
    let mut invalid = token();
    invalid.generation = Counter(0);
    assert!(feeder
        .receive(
            Message::Start {
                token: invalid,
                bindings: vec![vjoy::tests::binding()]
            },
            1
        )
        .is_err());
    assert!(feeder
        .receive(
            Message::Start {
                token: token(),
                bindings: vec![]
            },
            1
        )
        .is_err());
    assert!(feeder
        .receive(
            Message::Start {
                token: token(),
                bindings: vec![vjoy::tests::binding(); 9]
            },
            1
        )
        .is_err());
    assert!(feeder
        .receive(
            Message::Start {
                token: token(),
                bindings: vec![vjoy::tests::binding(); 2]
            },
            1
        )
        .is_err());
    assert!(feeder
        .receive(
            Message::Start {
                token: token(),
                bindings: vec![vjoy::tests::binding()]
            },
            u64::MAX
        )
        .is_err());
    feeder.retired = (0..128).map(|n| n.to_string()).collect();
    assert!(feeder
        .receive(
            Message::Start {
                token: token(),
                bindings: vec![vjoy::tests::binding()]
            },
            1
        )
        .is_err());
}

#[test]
fn failed_fallback_is_reported_instead_of_acknowledging_a_clean_stop() {
    let driver = Fake::default();
    let mut feeder = Feeder::new(driver.clone());
    let run = token();
    start(&mut feeder, &run);
    driver.0.lock().unwrap().fail = "submit:1".into();
    let error = feeder.receive(Message::Stop { token: run }, 2).unwrap_err();
    assert!(error.contains("Fallback could not be confirmed"));
    assert!(!feeder.active());
    assert_eq!(driver.0.lock().unwrap().calls.last().unwrap(), "release:1");
    let run = token();
    driver.0.lock().unwrap().fail.clear();
    start(&mut feeder, &run);
    driver.0.lock().unwrap().fail = "submit:1".into();
    let error = feeder
        .receive(report(&run, 1, vjoy::tests::binding().fallback()), 2)
        .unwrap_err();
    assert!(error.contains("submit failed"));
    assert!(error.contains("Fallback could not be confirmed"));
}

#[test]
fn recovery_protocol_rejects_stale_or_ambiguous_intent_before_touching_driver() {
    let driver = Fake::default();
    let mut feeder = Feeder::new(driver.clone());
    let run = token();
    let binding = vjoy::tests::binding();
    let request = Message::Recover {
        token: run.clone(),
        bindings: vec![binding.clone()],
    };
    feeder.receive(request.clone(), 1).unwrap();
    assert_eq!(
        driver.0.lock().unwrap().calls,
        ["acquire:1", "submit:1:0", "release:1"]
    );
    assert!(feeder.receive(request, 2).is_err());
    for bindings in [vec![], vec![binding.clone(); 2], vec![binding.clone(); 9]] {
        assert!(feeder
            .receive(
                Message::Recover {
                    token: token(),
                    bindings
                },
                3
            )
            .is_err());
    }
    let mut bad = token();
    bad.generation = Counter(0);
    assert!(feeder
        .receive(
            Message::Recover {
                token: bad,
                bindings: vec![binding.clone()]
            },
            4
        )
        .is_err());
    let mut bad = binding.clone();
    bad.slot = 0;
    assert!(feeder.recover(&[bad]).is_err());
    assert_eq!(driver.0.lock().unwrap().calls.len(), 3);
    start(&mut feeder, &token());
    assert!(feeder
        .receive(
            Message::Recover {
                token: token(),
                bindings: vec![binding]
            },
            5
        )
        .is_err());
}
