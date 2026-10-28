use super::*;
use osg_mapping::{
    engine::{
        typed::{Dependency, Token},
        Counter, RunToken,
    },
    program::Endpoint,
};

fn delivery(run: u64, at: u64, deadline: Option<u64>, intent: Intent) -> Delivery {
    Delivery {
        producer: Token {
            run: RunToken {
                session: "test-session".into(),
                generation: Counter(run),
            },
            node_id: "source".into(),
            dependencies: vec![Dependency {
                endpoint: Endpoint {
                    node_id: "source".into(),
                    port_id: "x".into(),
                },
                generation: Counter(1),
                epoch: Counter(1),
            }],
        },
        action_id: "pointer".into(),
        operation_id: Counter(at + 1),
        sequence: Counter(at + 1),
        at_us: Counter(at),
        deadline_us: deadline.map(Counter),
        intent,
    }
}

fn pointer() -> Delivery {
    delivery(1, 10, Some(30), Intent::Pointer { dx: 2, dy: -1 })
}

#[test]
fn pointer_grouping_accepts_only_adjacent_same_tick_same_run_motion() {
    let first = pointer();
    let same = pointer();
    let different_time = delivery(1, 11, Some(30), Intent::Pointer { dx: 3, dy: 0 });
    let different_run = delivery(2, 10, Some(30), Intent::Pointer { dx: 3, dy: 0 });
    let different_deadline = delivery(1, 10, Some(31), Intent::Pointer { dx: 3, dy: 0 });

    assert!(same_pointer_batch(&first, &same));
    assert!(!same_pointer_batch(&first, &different_time));
    assert!(!same_pointer_batch(&first, &different_run));
    assert!(same_pointer_batch(&first, &different_deadline));
}

#[test]
fn source_deadline_clock_includes_dispatch_elapsed_time() {
    let started = Instant::now()
        .checked_sub(std::time::Duration::from_millis(2))
        .unwrap();
    assert!(effective_source_now(100, started) >= 2_100);
}

#[test]
fn pointer_grouping_stops_at_discrete_barriers_and_resumes_after_them() {
    let first = pointer();
    let barrier = delivery(
        1,
        10,
        Some(30),
        Intent::Repeat {
            key: osg_mapping::desktop::Key(4),
        },
    );
    let after_barrier = pointer();

    assert!(!same_pointer_batch(&first, &barrier));
    // The dispatch loop never searches past the first mismatch, so this later
    // compatible motion starts a new group after the discrete operation.
    assert!(!same_pointer_batch(&barrier, &after_barrier));
    assert!(same_pointer_batch(&after_barrier, &pointer()));
}
