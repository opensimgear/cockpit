use super::*;
use osg_mapping::{
    desktop::{Control, Delivery, Key},
    engine::{typed::Token, Counter},
};
use std::sync::Mutex;

struct ShortThenCleanup {
    contexts: Arc<Mutex<Vec<(u64, u64, u64)>>>,
    calls: usize,
}
impl Adapter for ShortThenCleanup {
    fn eligible(&mut self, _: &Scope) -> Result<bool, String> {
        Ok(true)
    }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
        let context = submission_context().expect("each adapter submission has its own context");
        self.contexts
            .lock()
            .unwrap()
            .push((context.request, context.operation, context.category));
        self.calls += 1;
        Ok(if self.calls == 1 {
            0
        } else {
            native_count(events)
        })
    }
}

#[test]
fn short_direct_submission_cleanup_has_no_inherited_request_or_operation() {
    assert!(submission_context().is_none());
    let contexts = Arc::new(Mutex::new(vec![]));
    let mut worker = Worker::new(ShortThenCleanup {
        contexts: contexts.clone(),
        calls: 0,
    });
    let run = RunToken {
        session: "context-test-run".into(),
        generation: Counter(1),
    };
    let producer = Token {
        run: run.clone(),
        node_id: "key-source".into(),
        dependencies: vec![],
    };
    for (sequence, command) in [
        (
            1,
            Command::Activate {
                run,
                scope: Scope::DesktopWide,
            },
        ),
        (
            2,
            Command::Arm {
                producer: producer.clone(),
                action_id: "hold".into(),
            },
        ),
    ] {
        assert!(
            worker
                .receive(
                    Request {
                        sequence: Counter(sequence),
                        deadline_us: None,
                        command
                    },
                    0
                )
                .ok
        );
    }
    let ack = worker.receive(
        Request {
            sequence: Counter(3),
            deadline_us: None,
            command: Command::Deliver {
                delivery: Delivery {
                    producer,
                    action_id: "hold".into(),
                    operation_id: Counter(1),
                    sequence: Counter(1),
                    at_us: Counter(1),
                    deadline_us: None,
                    intent: Intent::Held {
                        controls: vec![Control::Key { key: Key(4) }],
                    },
                },
            },
        },
        1,
    );
    assert!(!ack.ok && ack.uncertain && ack.cleanup_complete);
    assert_eq!((ack.attempted, ack.confirmed), (1, 0));
    assert!(ack.held.is_empty() && worker.held().is_empty());
    assert_eq!(*contexts.lock().unwrap(), [(3, 1, 1), (0, 0, 5)]);
    assert!(
        submission_context().is_none(),
        "request scope must be restored after error cleanup"
    );
}
