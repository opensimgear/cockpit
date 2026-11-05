use super::*;
use osg_mapping::{desktop::Key, engine::{Counter, typed::Dependency}, program::Endpoint};

fn down(node: &str, operation: u64) -> Delivery {
    Delivery {
        producer: Token { run: RunToken { session: "guard-run".into(), generation: Counter(1) },
            node_id: node.into(), dependencies: vec![] },
        action_id: "tap".into(), operation_id: Counter(operation), sequence: Counter(operation),
        at_us: Counter(0), deadline_us: None,
        intent: Intent::Held { controls: vec![Control::Key { key: Key(4) }] },
    }
}
fn arm_down(reducer: &mut Reducer, delivery: &Delivery) {
    reducer.arm(delivery.producer.clone(), delivery.action_id.clone()).unwrap();
    reducer.delivery(delivery, false).unwrap();
}

#[test]
fn exact_release_is_idempotent_and_does_not_consume_the_up_operation() {
    let mut reducer = Reducer::default();
    let down = down("a", 1);
    arm_down(&mut reducer, &down);
    assert_eq!(reducer.release_exact(&down).unwrap().unwrap().len(), 1);
    assert_eq!(reducer.release_exact(&down).unwrap(), Some(vec![]));
    let mut up = down.clone();
    up.operation_id = Counter(2);
    up.intent = Intent::Held { controls: vec![] };
    assert!(reducer.delivery(&up, false).unwrap().events.is_empty());
    assert_eq!(reducer.release_exact(&down).unwrap(), None);
}

#[test]
fn exact_release_preserves_another_owners_shared_control() {
    let mut reducer = Reducer::default();
    let first = down("a", 1);
    let other = down("b", 1);
    arm_down(&mut reducer, &first);
    arm_down(&mut reducer, &other);
    assert_eq!(reducer.release_exact(&first).unwrap(), Some(vec![]));
    assert_eq!(reducer.held(), vec![Control::Key { key: Key(4) }]);
    assert_eq!(reducer.release_exact(&other).unwrap().unwrap().len(), 1);
}

#[test]
fn operation_and_source_generation_replacement_fence_old_guards() {
    let mut reducer = Reducer::default();
    let old = down("a", 1);
    arm_down(&mut reducer, &old);
    let newer = down("a", 2);
    reducer.delivery(&newer, false).unwrap();
    assert_eq!(reducer.release_exact(&old).unwrap(), None);
    let mut generation = old.clone();
    generation.producer.dependencies = vec![Dependency {
        endpoint: Endpoint { node_id: "physical".into(), port_id: "button".into() },
        generation: Counter(2), epoch: Counter(3),
    }];
    arm_down(&mut reducer, &generation);
    assert_eq!(reducer.release_exact(&newer).unwrap(), None);
    assert_eq!(reducer.held().len(), 1);
    assert_eq!(reducer.release_exact(&generation).unwrap().unwrap().len(), 1);
}

#[test]
fn rearming_same_owner_and_malformed_release_cannot_clear_new_hold() {
    let mut reducer = Reducer::default();
    let old = down("a", 5);
    arm_down(&mut reducer, &old);
    let fresh = down("a", 1);
    arm_down(&mut reducer, &fresh);
    assert_eq!(reducer.release_exact(&old).unwrap(), None);
    let mut malformed = fresh.clone();
    malformed.intent = Intent::Cleanup;
    assert!(reducer.release_exact(&malformed).is_err());
    assert_eq!(reducer.held().len(), 1);
}
