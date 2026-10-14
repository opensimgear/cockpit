use super::*;
use crate::feeder::{Feeder, Message, LEASE_US};
use osg_mapping::{
    engine::{Counter, Report, ReportReason, RunToken},
    profile::{Axis as ProfileAxis, AxisRange, Layout},
};
use std::sync::{Arc, Mutex};
pub fn binding(slot: u8) -> Binding {
    Binding {
        target: format!("output-{slot}"),
        slot,
        axes: vec![0x30, 0x37],
        layout: Layout {
            buttons: 32,
            axes: vec![
                ProfileAxis {
                    range: AxisRange::Unipolar,
                    fallback: 0
                };
                2
            ],
            hats: 4,
        },
    }
}
#[derive(Default)]
struct State {
    created: Vec<u8>,
    closed: Vec<u8>,
    events: Vec<(u8, Vec<Event>)>,
    fail_create: u8,
    fail_write: bool,
    missing: bool,
}
#[derive(Clone, Default)]
struct Fake(Arc<Mutex<State>>);
struct Handle {
    state: Fake,
    slot: u8,
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.state.0.lock().unwrap().closed.push(self.slot);
    }
}
impl Device for Handle {
    fn emit(&mut self, e: &[Event]) -> Result<(), String> {
        let mut s = self.state.0.lock().unwrap();
        if s.fail_write {
            return Err("write failed".into());
        }
        s.events.push((self.slot, e.to_vec()));
        Ok(())
    }
}
impl Backend for Fake {
    type Device = Handle;
    fn ready(&mut self) -> Result<(), String> {
        if self.0.lock().unwrap().missing {
            Err("permissions".into())
        } else {
            Ok(())
        }
    }
    fn create(&mut self, t: &Template) -> Result<Handle, String> {
        let mut s = self.0.lock().unwrap();
        if s.fail_create == t.binding.slot {
            return Err("create failed".into());
        }
        s.created.push(t.binding.slot);
        Ok(Handle {
            state: self.clone(),
            slot: t.binding.slot,
        })
    }
}
fn token() -> RunToken {
    RunToken {
        session: uuid::Uuid::new_v4().to_string(),
        generation: Counter(1),
    }
}
#[test]
fn templates_reject_truncation_and_preserve_local_identity() {
    let b = binding(1);
    let t = Template::new(b.clone()).unwrap();
    assert!(t.physical.starts_with("opensimgear-virtual:"));
    assert_eq!(t.name, "OpenSimGear Cockpit 1");
    assert_eq!(t.slot(false).buttons, 32);
    assert_eq!(t.slot(true).status, Status::Owned);
    let mut bad = b.clone();
    bad.layout.buttons = 33;
    assert!(Template::new(bad).is_err());
    let mut bad = b.clone();
    bad.axes[1] = 0x38;
    assert!(Template::new(bad).is_err());
    let mut bad = b;
    bad.slot = 0;
    assert!(Template::new(bad).is_err());
}
#[test]
fn complete_reports_encode_buttons_axes_and_all_hat_directions() {
    let b = binding(2);
    let t = Template::new(b.clone()).unwrap();
    let mut state = b.fallback();
    state.buttons[31] = true;
    state.axes = vec![32767, 12];
    for h in (0..8).map(Some).chain([None]) {
        state.hats = vec![h; 4];
        let p = b.report(&state).unwrap();
        let e = t.events(&p).unwrap();
        assert_eq!(e.len(), 43);
        assert_eq!(
            e.last(),
            Some(&Event {
                kind: 0,
                code: 0,
                value: 0
            })
        );
        assert!(e.contains(&Event {
            kind: 1,
            code: 0x13f,
            value: 1
        }));
        assert!(e.contains(&Event {
            kind: 3,
            code: 0,
            value: 32767
        }));
    }
    let mut p = b.report(&state).unwrap();
    p.device = 1;
    assert!(t.events(&p).is_err());
    p.device = 2;
    p.words[3] = -1;
    assert!(t.events(&p).is_err());
    p.words[3] = 0;
    p.words[19] = 12;
    assert!(t.events(&p).is_err());
}
#[test]
fn two_output_feeder_stop_lease_failure_and_recovery_do_not_recreate() {
    for failure in ["stop", "lease", "write", "create", "permission"] {
        let fake = Fake::default();
        if failure == "create" {
            fake.0.lock().unwrap().fail_create = 2;
        }
        if failure == "permission" {
            fake.0.lock().unwrap().missing = true;
        }
        let mut feeder = Feeder::new(Output::new(fake.clone()));
        let token = token();
        let bs = vec![binding(1), binding(2)];
        let result = feeder.receive(
            Message::Start {
                token: token.clone(),
                bindings: bs.clone(),
            },
            1,
        );
        if ["create", "permission"].contains(&failure) {
            assert!(result.is_err());
        } else {
            result.unwrap();
            assert_eq!(fake.0.lock().unwrap().created, vec![1, 2]);
            if failure == "write" {
                fake.0.lock().unwrap().fail_write = true;
                assert!(feeder
                    .receive(
                        Message::Report {
                            token: token.clone(),
                            report: Report {
                                session: token.session.clone(),
                                run_generation: token.generation,
                                sequence: Counter(1),
                                at_us: Counter(1),
                                target: bs[0].target.clone(),
                                state: bs[0].fallback(),
                                reason: ReportReason::Change
                            }
                        },
                        2
                    )
                    .is_err());
            } else if failure == "lease" {
                assert!(feeder.tick(LEASE_US + 1).is_err());
            } else {
                feeder.receive(Message::Stop { token }, 2).unwrap();
            }
        }
        drop(feeder);
        let s = fake.0.lock().unwrap();
        assert_eq!(s.created, s.closed);
        drop(s);
        let before = fake.0.lock().unwrap().created.clone();
        let mut recovery = Feeder::new(Output::new(fake.clone()));
        recovery.recover(&bs).unwrap();
        assert_eq!(fake.0.lock().unwrap().created, before);
    }
}
#[test]
fn creator_drop_and_invalid_operations_are_bounded() {
    let fake = Fake::default();
    let mut o = Output::new(fake.clone());
    assert!(o.prepare(&[]).is_err());
    assert!(o.prepare(&[binding(1), binding(1)]).is_err());
    assert!(o.inspect(1).is_err());
    assert!(o.acquire(1).is_err());
    assert!(o
        .submit(1, &binding(1).report(&binding(1).fallback()).unwrap())
        .is_err());
    o.prepare(&[binding(1)]).unwrap();
    assert!(o
        .submit(1, &binding(1).report(&binding(1).fallback()).unwrap())
        .is_err());
    o.acquire(1).unwrap();
    assert!(o.acquire(1).is_err());
    assert!(o.prepare(&[binding(2)]).is_err());
    assert_eq!(o.inspect(1).unwrap().status, Status::Owned);
    drop(o);
    assert_eq!(fake.0.lock().unwrap().closed, vec![1]);
}
