use super::*;

const OP:&str="11111111-1111-4111-8111-111111111111";
const NEXT:&str="22222222-2222-4222-8222-222222222222";
const PROFILE:&str="33333333-3333-4333-8333-333333333333";

#[test]
fn actual_steps_follow_exact_epoch_and_rebind_without_replaying_after_cancel() {
    let tracker=Tracker::default();let epoch=AtomicU64::new(7);
    tracker.begin(OP,PROFILE,||7).unwrap();
    tracker.note(6,"hiding-peripherals","Old work");
    tracker.note(7,"creating-virtual-device","Creating virtual controller output");
    tracker.rebind(OP,7,8).unwrap();epoch.store(8,Ordering::Release);
    tracker.note(7,"hiding-peripherals","Old work");
    tracker.note(8,"launching","Launching DCS World…");
    {let entries=tracker.entries.lock().unwrap();let current=&entries[OP];assert_eq!(current.phase,"launching");assert_eq!(current.steps.len(),2);assert_eq!(current.message,"Launching DCS World…");}
    tracker.cancel(OP,&epoch).unwrap();tracker.note(8,"starting-routes","Must not appear");
    let entries=tracker.entries.lock().unwrap();assert_eq!(entries[OP].phase,"cancelling");assert_eq!(entries[OP].steps.len(),2);
}

#[test]
fn exact_cancel_is_idempotent_and_only_revokes_its_captured_epoch() {
    let tracker=Tracker::default();let epoch=AtomicU64::new(7);
    tracker.begin(OP,PROFILE,||epoch.fetch_add(1,Ordering::AcqRel)+1).unwrap();
    assert_eq!(epoch.load(Ordering::Acquire),8);
    tracker.cancel(OP,&epoch).unwrap();
    assert_eq!(epoch.load(Ordering::Acquire),9);
    tracker.cancel(OP,&epoch).unwrap();
    assert_eq!(epoch.load(Ordering::Acquire),9);
    let entry=tracker.entries.lock().unwrap().get(OP).unwrap().clone();
    assert_eq!(entry.phase,"cancelling");assert!(entry.cancel_requested);
    assert_eq!(entry.view(9,vec![])["canCancel"],false);
}

#[test]
fn stale_or_unknown_cancel_cannot_invalidate_a_newer_activation_or_owner() {
    let tracker=Tracker::default();let epoch=AtomicU64::new(1);
    tracker.begin(OP,PROFILE,||epoch.fetch_add(1,Ordering::AcqRel)+1).unwrap();
    tracker.begin(NEXT,PROFILE,||epoch.fetch_add(1,Ordering::AcqRel)+1).unwrap();
    assert!(tracker.cancel(OP,&epoch).is_err());
    assert!(tracker.cancel(PROFILE,&epoch).is_err());
    assert_eq!(epoch.load(Ordering::Acquire),3);
    let entries=tracker.entries.lock().unwrap();
    assert!(!entries[OP].cancel_requested);assert!(!entries[NEXT].cancel_requested);
    assert_eq!(entries[OP].view(3,vec![])["canCancel"],false);
    assert_eq!(entries[NEXT].view(3,vec![])["canCancel"],true);
}

#[test]
fn reused_or_malformed_operation_identity_is_rejected_before_epoch_changes() {
    let tracker=Tracker::default();let epoch=AtomicU64::new(1);
    tracker.begin(OP,PROFILE,||epoch.fetch_add(1,Ordering::AcqRel)+1).unwrap();
    assert!(tracker.begin(OP,PROFILE,||panic!("duplicate must not revoke another operation")).is_err());
    assert!(tracker.begin("invalid",PROFILE,||panic!("malformed must not revoke another operation")).is_err());
    assert!(tracker.begin(NEXT,"invalid",||panic!("malformed must not revoke another operation")).is_err());
    assert_eq!(epoch.load(Ordering::Acquire),2);
}

#[test]
fn completed_cancel_does_not_touch_epoch_and_pending_admission_is_bounded() {
    let tracker=Tracker::default();let epoch=AtomicU64::new(1);
    tracker.begin(OP,PROFILE,||2).unwrap();
    {let mut entries=tracker.entries.lock().unwrap();let entry=entries.get_mut(OP).unwrap();entry.phase="succeeded";entry.completed=Some(Instant::now());}
    tracker.cancel(OP,&epoch).unwrap();assert_eq!(epoch.load(Ordering::Acquire),1);
    for _ in 0..MAX_RETAINED {
        let id=uuid::Uuid::new_v4().to_string();tracker.begin(&id,PROFILE,||epoch.fetch_add(1,Ordering::AcqRel)+1).unwrap();
    }
    assert_eq!(tracker.entries.lock().unwrap().len(),MAX_RETAINED);
    assert!(tracker.begin(&uuid::Uuid::new_v4().to_string(),PROFILE,||panic!("full tracker must not revoke outputs")).is_err());
}

#[test]
fn cancellation_at_the_success_boundary_reports_the_committed_profile_without_stopping_it() {
    let manager=super::super::tests::manager_fixture();let operation=uuid::Uuid::new_v4().to_string();
    let applied=manager.request(json!({"command":"profile_apply","clientId":"activation-outcome-test-client","id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1,"activationId":operation}));
    assert_eq!(applied["ok"],true,"{applied}");
    let owner=applied["state"]["profiles"]["profileSession"]["owner"].clone();
    let epoch=manager.profile_admission_epoch.load(Ordering::Acquire);
    // Model a cancel admitted after the final host cancellation check but
    // before successful takeover reaches the volatile tracker completion.
    {
        let mut entries=manager.profile_activations.entries.lock().unwrap();
        let activation=entries.get_mut(&operation).unwrap();
        activation.cancel_requested=true;
        activation.completed=None;
        activation.phase="cancelling";
    }
    manager.finish_profile_activation(&operation,&applied);
    let progress=manager.request(json!({"command":"profile_activation_status","clientId":"activation-outcome-test-client","activationId":operation}));
    assert_eq!(progress["state"]["profileActivation"]["phase"],"succeeded");
    assert_eq!(progress["state"]["profileActivation"]["cancelRequested"],true);
    assert_eq!(progress["state"]["profileActivation"]["canCancel"],false);
    assert_eq!(progress["state"]["profileActivation"]["message"],"The profile became active before cancellation could take effect.");
    assert!(progress["state"]["profileActivation"]["error"].is_null());
    assert_eq!(manager.profile_admission_epoch.load(Ordering::Acquire),epoch);
    assert_eq!(manager.request(json!({"command":"profile_status","clientId":"activation-outcome-test-client"}))["state"]["profiles"]["profileSession"]["owner"],owner);
}
