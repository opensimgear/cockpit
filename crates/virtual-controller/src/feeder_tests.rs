use super::*;
use crate::vjoy::{self, Axis, Position, Slot};
use osg_mapping::engine::{ReportReason, TargetState};
use std::sync::{Arc, Mutex};
#[derive(Default)]
struct State {
    fail: String,
    calls: Vec<String>,
    busy: u8,
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
