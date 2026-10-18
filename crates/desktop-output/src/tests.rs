use crate::{
    coordinator::{Channel, Coordinator},
    protocol::*,
    reducer::Reducer,
    worker::Worker,
};
use osg_mapping::{
    desktop::{Control, Delivery, Intent, Key},
    engine::{typed::Token, Counter, RunToken},
};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
#[derive(Clone)]
struct Recording {
    events: Arc<Mutex<Vec<Vec<NativeEvent>>>>,
    eligible: Arc<AtomicBool>,
    short: Arc<AtomicUsize>,
}
impl Recording {
    fn new() -> Self {
        Self {
            events: Arc::default(),
            eligible: Arc::new(AtomicBool::new(true)),
            short: Arc::new(AtomicUsize::new(usize::MAX)),
        }
    }
}
impl Adapter for Recording {
    fn eligible(&mut self, _: &Scope) -> Result<bool, String> {
        Ok(self.eligible.load(Ordering::Relaxed))
    }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
        self.events.lock().unwrap().push(events.to_vec());
        let count = self.short.swap(usize::MAX, Ordering::Relaxed);
        Ok(if count == usize::MAX {
            native_count(events)
        } else {
            count
        })
    }
}
fn run(n: u64) -> RunToken {
    RunToken {
        session: format!("run-{n}"),
        generation: Counter(1),
    }
}
fn token(n: u64) -> Token {
    Token {
        run: run(n),
        node_id: "node".into(),
        dependencies: vec![],
    }
}
fn control(key: u16) -> Control {
    Control::Key { key: Key(key) }
}
fn delivery(n: u64, op: u64, intent: Intent) -> Delivery {
    Delivery {
        producer: token(n),
        action_id: "action".into(),
        operation_id: Counter(op),
        sequence: Counter(op),
        at_us: Counter(op),
        deadline_us: None,
        intent,
    }
}
fn request(w: &mut Worker<Recording>, seq: &mut u64, command: Command, now: u64) -> Ack {
    *seq += 1;
    w.receive(
        Request {
            sequence: Counter(*seq),
            deadline_us: None,
            command,
        },
        now,
    )
}
fn activate(w: &mut Worker<Recording>, seq: &mut u64, n: u64) {
    assert!(
        request(
            w,
            seq,
            Command::Activate {
                run: run(n),
                scope: Scope::DesktopWide
            },
            0
        )
        .ok
    );
    assert!(
        request(
            w,
            seq,
            Command::Arm {
                producer: token(n),
                action_id: "action".into()
            },
            0
        )
        .ok
    );
}
#[test]
fn owners_across_runs_preserve_shared_modifiers_and_stable_chord_order() {
    let mut r = Reducer::default();
    r.arm(token(1), "action".into()).unwrap();
    r.arm(token(2), "action".into()).unwrap();
    let first = r
        .delivery(
            &delivery(
                1,
                1,
                Intent::Held {
                    controls: vec![control(0x0a), control(0xe0)],
                },
            ),
            false,
        )
        .unwrap();
    assert_eq!(
        first.events,
        vec![
            NativeEvent::Key {
                usage: 0xe0,
                down: true,
                repeat: false
            },
            NativeEvent::Key {
                usage: 0x0a,
                down: true,
                repeat: false
            }
        ]
    );
    let second = r
        .delivery(
            &delivery(
                2,
                1,
                Intent::Held {
                    controls: vec![control(0xe0), control(0x0b)],
                },
            ),
            false,
        )
        .unwrap();
    assert!(second.collision);
    assert_eq!(second.events.len(), 1);
    let released = r.pause(&run(1));
    assert_eq!(
        released,
        vec![NativeEvent::Key {
            usage: 0x0a,
            down: false,
            repeat: false
        }]
    );
    assert_eq!(r.held().len(), 2);
    let released = r.stop();
    assert_eq!(
        released.last(),
        Some(&NativeEvent::Key {
            usage: 0xe0,
            down: false,
            repeat: false
        })
    );
}
#[test]
fn native_aliases_cannot_release_another_owner() {
    let mut r = Reducer::default();
    r.arm(token(1), "action".into()).unwrap();
    r.arm(token(2), "action".into()).unwrap();
    r.delivery(
        &delivery(
            1,
            1,
            Intent::Held {
                controls: vec![control(0x31)],
            },
        ),
        false,
    )
    .unwrap();
    assert!(r
        .delivery(
            &delivery(
                2,
                1,
                Intent::Held {
                    controls: vec![control(0x32)]
                }
            ),
            false
        )
        .unwrap()
        .events
        .is_empty());
    assert!(r.pause(&run(1)).is_empty());
    assert_eq!(r.stop().len(), 1);
}
#[test]
fn leases_and_focus_release_without_input_and_require_fresh_arm() {
    let rec = Recording::new();
    let mut w = Worker::new(rec.clone());
    let mut seq = 0;
    activate(&mut w, &mut seq, 1);
    request(
        &mut w,
        &mut seq,
        Command::Deliver {
            delivery: delivery(
                1,
                1,
                Intent::Held {
                    controls: vec![control(4)],
                },
            ),
        },
        0,
    );
    rec.eligible.store(false, Ordering::Relaxed);
    w.tick(1).unwrap();
    assert!(w.held().is_empty());
    rec.eligible.store(true, Ordering::Relaxed);
    assert!(
        !request(
            &mut w,
            &mut seq,
            Command::Deliver {
                delivery: delivery(
                    1,
                    2,
                    Intent::Held {
                        controls: vec![control(4)]
                    }
                )
            },
            2
        )
        .ok
    );
    assert!(
        request(
            &mut w,
            &mut seq,
            Command::Arm {
                producer: token(1),
                action_id: "action".into()
            },
            2
        )
        .ok
    );
    assert!(
        request(
            &mut w,
            &mut seq,
            Command::Deliver {
                delivery: delivery(
                    1,
                    3,
                    Intent::Held {
                        controls: vec![control(4)]
                    }
                )
            },
            3
        )
        .ok
    );
    w.tick(LEASE_US).unwrap();
    assert!(w.held().is_empty());
    assert!(!request(&mut w, &mut seq, Command::Renew { run: run(1) }, LEASE_US).ok);
}
#[test]
fn partial_native_count_is_uncertain_fault_with_cleanup_and_no_replay() {
    let rec = Recording::new();
    let mut w = Worker::new(rec.clone());
    let mut seq = 0;
    activate(&mut w, &mut seq, 1);
    rec.short.store(1, Ordering::Relaxed);
    let ack = request(
        &mut w,
        &mut seq,
        Command::Deliver {
            delivery: delivery(
                1,
                1,
                Intent::Held {
                    controls: vec![control(4), control(0xe0)],
                },
            ),
        },
        0,
    );
    assert!(!ack.ok && ack.uncertain && ack.cleanup_complete);
    assert_eq!(ack.attempted, 2);
    assert_eq!(ack.confirmed, 1);
    assert!(w.fault().is_some());
    assert!(w.held().is_empty());
    let count = rec.events.lock().unwrap().len();
    assert!(
        !request(
            &mut w,
            &mut seq,
            Command::Deliver {
                delivery: delivery(
                    1,
                    2,
                    Intent::Held {
                        controls: vec![control(4)]
                    }
                )
            },
            1
        )
        .ok
    );
    assert_eq!(rec.events.lock().unwrap().len(), count);
}
#[test]
fn stale_deadlines_and_channels_never_replay_and_cleanup_survives_scope_loss() {
    let rec = Recording::new();
    let mut w = Worker::new(rec.clone());
    let mut seq = 0;
    activate(&mut w, &mut seq, 1);
    seq += 1;
    let ack = w.receive(
        Request {
            sequence: Counter(seq),
            deadline_us: Some(Counter(1)),
            command: Command::Deliver {
                delivery: delivery(1, 1, Intent::Pointer { dx: 20, dy: 0 }),
            },
        },
        2,
    );
    assert!(ack.ok && ack.skipped);
    assert_eq!(ack.attempted, 0);
    let stale = w.receive(
        Request {
            sequence: Counter(seq),
            deadline_us: None,
            command: Command::Deliver {
                delivery: delivery(1, 1, Intent::Pointer { dx: 20, dy: 0 }),
            },
        },
        2,
    );
    assert!(!stale.ok);
    assert!(rec.events.lock().unwrap().is_empty());
    rec.eligible.store(false, Ordering::Relaxed);
    assert!(
        request(
            &mut w,
            &mut seq,
            Command::Deliver {
                delivery: delivery(1, 2, Intent::Cleanup)
            },
            3
        )
        .ok
    );
}
struct DeadAfterSubmission {
    worker: Worker<Recording>,
    die: bool,
}
impl Channel for DeadAfterSubmission {
    fn request(&mut self, r: &Request) -> Result<Ack, String> {
        let ack = self.worker.receive(r.clone(), 0);
        if self.die && matches!(r.command, Command::Deliver { .. }) {
            Err("Worker died after SendInput before ACK.".into())
        } else {
            Ok(ack)
        }
    }
}
fn clock() -> u64 {
    1000
}
#[test]
fn parent_retains_inflight_possible_holds_after_worker_death_before_ack() {
    let recording = Recording::new();
    let mut c = Coordinator::with_channel(
        Box::new(DeadAfterSubmission {
            worker: Worker::new(recording.clone()),
            die: true,
        }),
        clock,
    );
    c.activate(run(1), Scope::DesktopWide).unwrap();
    c.arm(token(1), "action".into()).unwrap();
    assert!(c
        .submit(
            delivery(
                1,
                1,
                Intent::Held {
                    controls: vec![control(4)]
                }
            ),
            1
        )
        .is_err());
    let snapshot = c.snapshot();
    assert!(snapshot.uncertain && !snapshot.cleanup_complete && snapshot.in_flight.is_some());
    assert!(snapshot.possible_held.contains(&control(4)));
    assert!(c
        .submit(delivery(1, 2, Intent::Pointer { dx: 2, dy: 0 }), 2)
        .is_err());
    assert_eq!(recording.events.lock().unwrap().len(), 1);
}
#[test]
fn oversized_and_truncated_worker_transport_is_rejected() {
    use std::io::Cursor;
    assert!(crate::worker::read_frame(&mut Cursor::new(vec![b'x'; FRAME_LIMIT + 1])).is_err());
    assert!(crate::worker::read_frame(&mut Cursor::new(b"{}".to_vec())).is_err());
}
#[test]
fn readiness_and_ordinary_startup_are_inert() {
    assert!(crate::entry::helper_main().is_none());
    assert!(Coordinator::spawn(false, true).is_err());
    assert!(Coordinator::spawn(true, false).is_err());
}
#[cfg(windows)]
#[test]
fn session_exclusion_spans_native_workers() {
    let first = crate::windows::SessionLock::acquire().unwrap();
    assert!(crate::windows::SessionLock::acquire().is_err());
    drop(first);
    assert!(crate::windows::SessionLock::acquire().is_ok());
}

#[test]
fn worker_handles_renew_pause_stop_invalid_scopes_tokens_and_source_epochs() {
    let rec = Recording::new();
    let mut w = Worker::new(rec.clone());
    let mut seq = 0;
    let invalid = RunToken {
        session: String::new(),
        generation: Counter(0),
    };
    assert!(
        !request(
            &mut w,
            &mut seq,
            Command::Activate {
                run: invalid,
                scope: Scope::DesktopWide
            },
            0
        )
        .ok
    );
    let bad_scope = Scope::Application {
        identity: ProcessIdentity {
            executable: "x".into(),
            pid: 0,
            created: "0".into(),
        },
    };
    assert!(
        !request(
            &mut w,
            &mut seq,
            Command::Activate {
                run: run(1),
                scope: bad_scope
            },
            0
        )
        .ok
    );
    activate(&mut w, &mut seq, 1);
    assert!(
        !request(
            &mut w,
            &mut seq,
            Command::Activate {
                run: run(1),
                scope: Scope::DesktopWide
            },
            0
        )
        .ok
    );
    assert!(request(&mut w, &mut seq, Command::Renew { run: run(1) }, 1).ok);
    assert!(
        request(
            &mut w,
            &mut seq,
            Command::Deliver {
                delivery: delivery(
                    1,
                    1,
                    Intent::Held {
                        controls: vec![control(4)]
                    }
                )
            },
            1
        )
        .ok
    );
    let mut stale = delivery(
        1,
        2,
        Intent::Held {
            controls: vec![control(5)],
        },
    );
    stale.producer.run.generation = Counter(2);
    assert!(!request(&mut w, &mut seq, Command::Deliver { delivery: stale }, 2).ok);
    assert!(
        !request(
            &mut w,
            &mut seq,
            Command::Recover {
                controls: vec![control(4)]
            },
            2
        )
        .ok
    );
    assert!(request(&mut w, &mut seq, Command::Pause { run: run(1) }, 2).ok);
    assert!(w.held().is_empty());
    assert!(request(&mut w, &mut seq, Command::Stop, 2).ok);
    assert!(
        request(
            &mut w,
            &mut seq,
            Command::Recover {
                controls: vec![control(4)]
            },
            3
        )
        .ok
    );
}
#[derive(Clone)]
struct Bytes(Arc<Mutex<Vec<u8>>>);
impl std::io::Write for Bytes {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[test]
fn worker_transport_serves_handshake_stop_and_malformed_frames_without_native_input() {
    use std::io::Cursor;
    fn clock() -> u64 {
        0
    }
    let output = Bytes(Arc::default());
    let buffer = output.0.clone();
    let request = Request {
        sequence: Counter(1),
        deadline_us: None,
        command: Command::Stop,
    };
    let mut bytes = serde_json::to_vec(&request).unwrap();
    bytes.push(b'\n');
    crate::worker::serve(Recording::new(), Cursor::new(bytes), output, clock).unwrap();
    let values: Vec<serde_json::Value> = String::from_utf8(buffer.lock().unwrap().clone())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(values.len(), 2);
    assert_eq!(values[0]["backend"], "desktop-input");
    assert_eq!(values[1]["ok"], true);
    assert!(crate::worker::serve(
        Recording::new(),
        Cursor::new(b"{}\n".to_vec()),
        std::io::sink(),
        clock
    )
    .is_err());
    assert!(crate::worker::serve(
        Recording::new(),
        Cursor::new(vec![b'x'; FRAME_LIMIT + 1]),
        std::io::sink(),
        clock
    )
    .is_err());
    struct Fail;
    impl std::io::Write for Fail {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "fixture pipe closed",
            ))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert!(
        crate::worker::serve(Recording::new(), Cursor::new(Vec::<u8>::new()), Fail, clock).is_err()
    );
}

#[test]
fn source_generation_and_publication_epoch_changes_cannot_inherit_armed_ownership() {
    use osg_mapping::{engine::typed::Dependency, program::Endpoint};
    let mut source = token(1);
    source.dependencies = vec![Dependency {
        endpoint: Endpoint {
            node_id: "companion".into(),
            port_id: "button".into(),
        },
        generation: Counter(1),
        epoch: Counter(1),
    }];
    let mut r = Reducer::default();
    r.arm(source.clone(), "action".into()).unwrap();
    let mut d = delivery(
        1,
        1,
        Intent::Held {
            controls: vec![control(4)],
        },
    );
    d.producer = source.clone();
    r.delivery(&d, false).unwrap();
    d.operation_id = Counter(2);
    d.producer.dependencies[0].generation = Counter(2);
    assert!(r.delivery(&d, false).is_err());
    assert_eq!(r.held(), vec![control(4)]);
    d.producer = source;
    d.producer.dependencies[0].epoch = Counter(2);
    assert!(r.delivery(&d, false).is_err());
    assert_eq!(r.held(), vec![control(4)]);
    assert_eq!(r.pause(&run(1)).len(), 1);
    assert!(r.delivery(&d, false).is_err());
}
#[test]
fn overloaded_or_unsupported_hold_is_transactional_and_repeat_keeps_ownership() {
    let mut r = Reducer::default();
    r.arm(token(1), "action".into()).unwrap();
    assert!(r
        .delivery(
            &delivery(
                1,
                1,
                Intent::Held {
                    controls: vec![control(4); 17]
                }
            ),
            false
        )
        .is_err());
    assert!(r.held().is_empty());
    assert!(r
        .delivery(
            &delivery(
                1,
                1,
                Intent::Held {
                    controls: vec![control(0xffff)]
                }
            ),
            false
        )
        .is_err());
    assert!(r
        .delivery(&delivery(1, 1, Intent::Repeat { key: Key(4) }), false)
        .is_err());
    r.delivery(
        &delivery(
            1,
            1,
            Intent::Held {
                controls: vec![control(4)],
            },
        ),
        false,
    )
    .unwrap();
    let repeat = r
        .delivery(&delivery(1, 2, Intent::Repeat { key: Key(4) }), false)
        .unwrap();
    assert_eq!(
        repeat.events,
        vec![NativeEvent::Key {
            usage: 4,
            down: true,
            repeat: true
        }]
    );
    let skipped = r
        .delivery(&delivery(1, 3, Intent::Repeat { key: Key(4) }), true)
        .unwrap();
    assert!(skipped.skipped && skipped.events.is_empty());
    assert_eq!(r.held(), vec![control(4)]);
    let mut r = Reducer::default();
    let distinct: Vec<_> = (4..=0x44)
        .filter(|usage| *usage != 0x32)
        .map(control)
        .collect();
    for n in 0..4 {
        let identity = token(n + 1);
        r.arm(identity, "action".into()).unwrap();
        let controls = distinct[n as usize * 16..(n as usize + 1) * 16].to_vec();
        r.delivery(&delivery(n + 1, 1, Intent::Held { controls }), false)
            .unwrap();
    }
    assert_eq!(r.held().len(), 64);
    r.arm(token(5), "action".into()).unwrap();
    assert!(r
        .delivery(
            &delivery(
                5,
                1,
                Intent::Held {
                    controls: vec![control(0xe0)]
                }
            ),
            false
        )
        .is_err());
    assert_eq!(r.held().len(), 64);
}
