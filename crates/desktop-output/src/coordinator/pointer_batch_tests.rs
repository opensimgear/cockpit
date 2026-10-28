use super::*;
use crate::worker::Worker;
use osg_mapping::desktop::Key;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Recording {
    events: Arc<Mutex<Vec<Vec<NativeEvent>>>>,
    short: Arc<Mutex<bool>>,
    eligible: Arc<Mutex<Option<bool>>>,
}
impl Adapter for Recording {
    fn eligible(&mut self, _: &Scope) -> Result<bool, String> {
        Ok(self.eligible.lock().unwrap().unwrap_or(true))
    }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
        self.events.lock().unwrap().push(events.to_vec());
        Ok(if std::mem::take(&mut *self.short.lock().unwrap()) {
            0
        } else {
            native_count(events)
        })
    }
}
struct Pipe {
    worker: Worker<Recording>,
    fault: Arc<Mutex<u8>>,
}
impl Channel for Pipe {
    fn request(&mut self, request: &Request) -> Result<Ack, String> {
        let mut ack = self.worker.receive(request.clone(), 1000);
        if matches!(request.command, Command::DeliverPointerBatch { .. }) {
            match *self.fault.lock().unwrap() {
                1 => {
                    ack.pointer_batch.as_mut().unwrap().operation_ids.pop();
                }
                2 => ack.pointer_batch.as_mut().unwrap().dx += 1,
                3 => ack.pointer_batch = None,
                4 => ack.operation_id = Some(Counter(1)),
                5 => ack.held.push(Control::Key { key: Key(4) }),
                6 => ack.attempted = 0,
                7 => ack.collision = true,
                8 => return Err("lost batch acknowledgement".into()),
                9 => ack.pointer_batch.as_mut().unwrap().operation_ids[1] = Counter(999),
                _ => {}
            }
        }
        Ok(ack)
    }
}
fn run() -> RunToken {
    RunToken {
        session: "pointer-vector".into(),
        generation: Counter(1),
    }
}
fn delivery(node: &str, operation: u64, dx: i32, dy: i32) -> Delivery {
    Delivery {
        producer: Token {
            run: run(),
            node_id: node.into(),
            dependencies: vec![],
        },
        action_id: "pointer".into(),
        operation_id: Counter(operation),
        sequence: Counter(30),
        at_us: Counter(50),
        deadline_us: Some(Counter(100)),
        intent: Intent::Pointer { dx, dy },
    }
}
fn batch(operation: u64) -> Vec<Delivery> {
    vec![
        delivery("horizontal", operation, 7, 0),
        delivery("vertical", operation, 0, -9),
    ]
}
fn clock() -> u64 {
    1000
}
fn setup() -> (Coordinator, Recording, Arc<Mutex<u8>>) {
    let record = Recording::default();
    let fault = Arc::new(Mutex::new(0));
    let mut parent = Coordinator::with_channel(
        Box::new(Pipe {
            worker: Worker::new(record.clone()),
            fault: fault.clone(),
        }),
        clock,
    );
    parent.activate(run(), Scope::DesktopWide).unwrap();
    for value in batch(1) {
        parent.arm(value.producer, value.action_id).unwrap();
    }
    (parent, record, fault)
}
fn worker_setup() -> (Worker<Recording>, Recording, u64) {
    let record = Recording::default();
    let mut worker = Worker::new(record.clone());
    let mut seq = 0;
    let commands = std::iter::once(Command::Activate {
        run: run(),
        scope: Scope::DesktopWide,
    })
    .chain(batch(1).into_iter().map(|d| Command::Arm {
        producer: d.producer,
        action_id: d.action_id,
    }));
    for command in commands {
        seq += 1;
        assert!(
            worker
                .receive(
                    Request {
                        sequence: Counter(seq),
                        deadline_us: None,
                        command
                    },
                    1000
                )
                .ok
        );
    }
    (worker, record, seq)
}
fn send(
    worker: &mut Worker<Recording>,
    seq: &mut u64,
    values: Vec<Delivery>,
    deadline: Option<Counter>,
) -> Ack {
    *seq += 1;
    worker.receive(
        Request {
            sequence: Counter(*seq),
            deadline_us: deadline,
            command: Command::DeliverPointerBatch { deliveries: values },
        },
        1000,
    )
}

#[test]
fn vector_has_one_native_request_and_one_physical_counter_owner() {
    let (mut parent, record, _) = setup();
    let acks = parent.submit_pointer_batch(batch(1), 50).unwrap();
    assert_eq!(
        *record.events.lock().unwrap(),
        [vec![NativeEvent::Pointer { dx: 7, dy: -9 }]]
    );
    assert_eq!((acks[0].attempted, acks[0].confirmed), (1, 1));
    assert_eq!((acks[1].attempted, acks[1].confirmed), (0, 0));
    for ack in acks {
        assert_eq!(ack.operation_id, Some(Counter(1)));
        assert_eq!(
            ack.pointer_batch.unwrap(),
            crate::pointer_batch::describe(&batch(1)).unwrap()
        );
    }
    assert!(parent.submit_pointer_batch(batch(1), 50).is_err());
}

#[test]
fn invalid_batch_is_transactional_in_parent_and_worker() {
    for invalid in 0..10 {
        let mut values = batch(1);
        match invalid {
            0 => {
                values.pop();
            }
            1 => {
                values[1].intent = Intent::Wheel {
                    horizontal: 0,
                    vertical: 1,
                }
            }
            2 => values[1].producer.run.generation = Counter(2),
            3 => values[1].at_us = Counter(51),
            4 => values[1].producer.node_id = "unarmed".into(),
            5 => values[1].operation_id = Counter(0),
            6 => {
                values[0].intent = Intent::Pointer {
                    dx: i32::MAX,
                    dy: 0,
                };
                values[1].intent = Intent::Pointer { dx: 1, dy: 0 };
            }
            7 => values = vec![values[0].clone(); MAX_BATCH + 1],
            8 => values[1] = values[0].clone(),
            9 => {
                values[0].intent = Intent::Pointer {
                    dx: 0,
                    dy: i32::MIN,
                };
                values[1].intent = Intent::Pointer { dx: 0, dy: -1 };
            }
            _ => unreachable!(),
        }
        let (mut parent, record, _) = setup();
        assert!(parent.submit_pointer_batch(values.clone(), 50).is_err());
        assert!(record.events.lock().unwrap().is_empty());
        parent.submit_pointer_batch(batch(1), 50).unwrap();
        let (mut worker, record, mut seq) = worker_setup();
        assert!(!send(&mut worker, &mut seq, values, None).ok);
        assert!(record.events.lock().unwrap().is_empty());
        assert!(send(&mut worker, &mut seq, batch(1), None).ok);
    }
}

#[test]
fn minimum_deadline_expires_entire_vector_and_consumes_all_fences() {
    let (mut parent, record, _) = setup();
    let mut values = batch(1);
    values[0].deadline_us = None;
    let acks = parent.submit_pointer_batch(values, 100).unwrap();
    assert!(acks.iter().all(|ack| ack.skipped && ack.attempted == 0));
    assert!(record.events.lock().unwrap().is_empty());
    assert!(parent.submit_pointer_batch(batch(1), 50).is_err());
    parent.submit_pointer_batch(batch(2), 50).unwrap();
    let (mut worker, record, mut seq) = worker_setup();
    let ack = send(&mut worker, &mut seq, batch(1), Some(Counter(1000)));
    assert!(ack.ok && ack.skipped && ack.pointer_batch.is_some());
    assert!(record.events.lock().unwrap().is_empty());
    assert!(!send(&mut worker, &mut seq, batch(1), None).ok);
}

#[test]
fn cancelling_vector_consumes_operations_without_native_submission() {
    let (mut parent, record, _) = setup();
    let mut values = batch(1);
    values[1].intent = Intent::Pointer { dx: -7, dy: 0 };
    let acks = parent.submit_pointer_batch(values, 50).unwrap();
    assert!(acks
        .iter()
        .all(|ack| ack.ok && !ack.skipped && ack.attempted == 0));
    assert!(record.events.lock().unwrap().is_empty());
    assert!(parent.submit_pointer_batch(batch(1), 50).is_err());
}

#[test]
fn tampered_batch_acknowledgements_and_transport_loss_block_replay() {
    for failure in 1..=9 {
        let (mut parent, record, fault) = setup();
        *fault.lock().unwrap() = failure;
        assert!(parent.submit_pointer_batch(batch(1), 50).is_err());
        assert!(parent.snapshot().uncertain);
        assert!(parent.submit_pointer_batch(batch(2), 50).is_err());
        assert_eq!(record.events.lock().unwrap().len(), 1);
    }
}

#[test]
fn partial_batch_submission_faults_and_releases_existing_holds() {
    let (mut parent, record, _) = setup();
    let mut hold = batch(1).remove(0);
    hold.intent = Intent::Held {
        controls: vec![Control::Key { key: Key(4) }],
    };
    parent.submit(hold, 50).unwrap();
    *record.short.lock().unwrap() = true;
    assert!(parent.submit_pointer_batch(batch(2), 50).is_err());
    assert!(parent.snapshot().uncertain);
    assert_eq!(parent.snapshot().held, vec![Control::Key { key: Key(4) }]);
    let events = record.events.lock().unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[1], [NativeEvent::Pointer { dx: 7, dy: -9 }]);
    assert_eq!(
        events[2],
        [NativeEvent::Key {
            usage: 4,
            down: false,
            repeat: false
        }]
    );
}

#[test]
fn scope_rejection_is_definitive_and_has_no_pointer_submission() {
    let (mut parent, record, _) = setup();
    *record.eligible.lock().unwrap() = Some(false);
    assert!(parent.submit_pointer_batch(batch(1), 50).is_err());
    assert!(!parent.snapshot().uncertain);
    assert_eq!(parent.snapshot().phase, "paused");
    assert!(record.events.lock().unwrap().is_empty());
}

#[test]
fn batch_preserves_hold_recovery_evidence_and_checks_it_before_dispatch() {
    let (mut parent, record, _) = setup();
    let dir = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
        .unwrap();
    let path = dir.path().join("batch-journal.json");
    parent.journal = Some(path.clone());
    let mut hold = batch(1).remove(0);
    hold.intent = Intent::Held {
        controls: vec![Control::Key { key: Key(4) }],
    };
    parent.submit(hold, 50).unwrap();
    let before = osg_persistence::test_support::read_evidence(&path, "recovery-desktop").unwrap();
    let writes = parent.persist_attempts.get();
    parent.submit_pointer_batch(batch(2), 50).unwrap();
    assert_eq!(parent.persist_attempts.get(), writes);
    assert_eq!(
        osg_persistence::test_support::read_evidence(&path, "recovery-desktop").unwrap(),
        before
    );
    assert_eq!(parent.snapshot().held, vec![Control::Key { key: Key(4) }]);
    osg_persistence::test_support::write_evidence(&path, "recovery-desktop", Some(b"tampered"))
        .unwrap();
    assert!(parent.submit_pointer_batch(batch(3), 50).is_err());
    assert_eq!(record.events.lock().unwrap().len(), 2);
}
