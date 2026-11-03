use super::*;
use crate::wire::Identity;
use serde_json::json;

fn encoded(value: Value) -> EncodedPayload {
    Identity {topic:"state".into(),source:"source".into(),session:"session".into()}
        .encode(&value,7,"clock",false).unwrap()
}
fn stream() -> Stream<Value,Value> {
    Stream::with_limits(1,json!(0),0,"clock",Limits {max_pending_frames:1,max_pending_bytes:16,..DEFAULT_LIMITS}).unwrap()
}
fn latest(stream: &Stream<Value,Value>, generation: u64, value: u64) -> u64 {
    stream.update_latest_encoded(generation,encoded(json!(value)),value,"clock").unwrap()
}

#[test]
fn a_thousand_unread_states_remain_one_shared_current_snapshot_without_loss() {
    let stream = stream();
    let mut unread = stream.subscribe().unwrap();
    let mut awaiting_ack = stream.subscribe().unwrap();
    let mut fast = stream.subscribe().unwrap();
    let in_flight = awaiting_ack.try_recv().unwrap().unwrap();
    fast.try_recv().unwrap();
    for value in 1..=1000 {
        let revision = latest(&stream,1,value);
        assert_eq!(revision,value+1);
        assert!(matches!(fast.try_recv().unwrap(),Some(Delivery::Snapshot(snapshot))
            if snapshot.value==json!(value)&&snapshot.revision==revision&&snapshot.captured_us==value));
        let state = stream.shared.lock();
        for subscriber in state.subscribers.values() {
            assert!(subscriber.events.is_empty());assert_eq!(subscriber.pending_bytes,0);
            assert!(subscriber.loss.is_none());assert!(!subscriber.blocked);
            if let Some(snapshot) = &subscriber.snapshot {assert!(Arc::ptr_eq(&snapshot.bytes,&state.snapshot.bytes));}
        }
    }
    assert!(matches!(in_flight,Delivery::Snapshot(snapshot) if snapshot.value==json!(0)));
    for reader in [&mut unread,&mut awaiting_ack] {
        assert!(matches!(reader.try_recv().unwrap(),Some(Delivery::Snapshot(snapshot)) if snapshot.value==json!(1000)));
        assert!(reader.try_recv().unwrap().is_none());
    }
    assert!(fast.try_recv().unwrap().is_none());
    assert_eq!(stream.shared.lock().sequence,0,"absolute state produces no event sequence");
}

#[test]
fn pending_generation_boundary_precedes_the_coalesced_current_baseline() {
    let stream = stream();let mut reader = stream.subscribe().unwrap();
    stream.replace_generation(2,json!(10),10,"clock").unwrap();
    latest(&stream,2,11);latest(&stream,2,12);
    assert!(matches!(reader.try_recv().unwrap(),Some(Delivery::Loss(loss))
        if loss.reason==LossReason::GenerationReplaced&&loss.generation==2));
    assert!(matches!(reader.try_recv().unwrap(),Some(Delivery::Snapshot(snapshot))
        if snapshot.value==json!(12)&&snapshot.generation==2&&snapshot.revision==4));
    assert!(reader.try_recv().unwrap().is_none());
}

#[test]
fn actual_overflow_is_preserved_before_recovery_even_with_unread_initial_baseline() {
    for consume_initial in [false,true] {
        let stream = stream();let mut reader = stream.subscribe().unwrap();
        if consume_initial {reader.try_recv().unwrap();}
        stream.publish(1,json!(1),json!(1),1,"clock").unwrap();
        stream.publish(1,json!(2),json!(2),2,"clock").unwrap();
        latest(&stream,1,3);latest(&stream,1,4);
        assert!(matches!(reader.try_recv().unwrap(),Some(Delivery::Loss(loss)) if loss.reason==LossReason::Overflow));
        assert!(stream.shared.lock().subscribers[&reader.id].blocked);
        assert!(matches!(reader.try_recv().unwrap(),Some(Delivery::Snapshot(snapshot)) if snapshot.value==json!(4)));
        assert!(!stream.shared.lock().subscribers[&reader.id].blocked);
        assert!(reader.try_recv().unwrap().is_none());
    }
}

#[test]
fn a_previously_delivered_loss_is_not_fabricated_again_before_the_fresh_baseline() {
    let stream = stream();let mut reader = stream.subscribe().unwrap();reader.try_recv().unwrap();
    stream.publish(1,json!(1),json!(1),1,"clock").unwrap();
    stream.publish(1,json!(2),json!(2),2,"clock").unwrap();
    assert!(matches!(reader.try_recv().unwrap(),Some(Delivery::Loss(_))));
    latest(&stream,1,3);
    assert!(matches!(reader.try_recv().unwrap(),Some(Delivery::Snapshot(snapshot)) if snapshot.value==json!(3)));
    assert!(!stream.shared.lock().subscribers[&reader.id].blocked);
}

#[test]
fn invalid_admission_is_atomic_and_cannot_replace_a_pending_state_or_closed_boundary() {
    let stream = stream();let mut reader = stream.subscribe().unwrap();
    for (generation,clock,revision,expected) in [
        (0,"clock",1,PublishError::StaleGeneration),(2,"clock",1,PublishError::StaleGeneration),
        (1,"bad clock",1,PublishError::InvalidClockId),(1,"clock",u64::MAX,PublishError::CounterExhausted),
    ] {
        stream.shared.lock().revision=revision;
        assert_eq!(stream.update_latest_encoded(generation,encoded(json!(9)),9,clock),Err(expected));
        let state = stream.shared.lock();assert_eq!(state.revision,revision);assert_eq!(state.sequence,0);
        assert_eq!(&*state.snapshot.bytes,b"0");
        assert_eq!(state.subscribers[&reader.id].snapshot.as_ref().unwrap().revision,1);
    }
    stream.close();
    assert_eq!(stream.update_latest_encoded(1,encoded(json!(9)),9,"clock"),Err(PublishError::Closed));
    assert!(matches!(reader.try_recv().unwrap(),Some(Delivery::Loss(loss)) if loss.reason==LossReason::Closed));
    assert!(reader.try_recv().unwrap().is_none());
}

#[test]
fn encoded_state_rechecks_the_journals_snapshot_budget_before_mutating() {
    let stream = Stream::<Value,Value>::with_limits(1,json!(0),0,"clock",Limits {max_snapshot_bytes:1,..DEFAULT_LIMITS}).unwrap();
    let mut reader = stream.subscribe().unwrap();reader.try_recv().unwrap();
    assert_eq!(stream.update_latest_encoded(1,encoded(json!(42)),7,"clock"),Err(PublishError::SnapshotTooLarge));
    assert_eq!(stream.revision(),1);assert_eq!(&*stream.shared.lock().snapshot.bytes,b"0");
    assert!(reader.try_recv().unwrap().is_none());
}
