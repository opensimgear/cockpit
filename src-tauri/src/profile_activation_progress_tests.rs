use super::*;

const OP:&str="11111111-1111-4111-8111-111111111111";
const NEXT:&str="22222222-2222-4222-8222-222222222222";
const PROFILE:&str="33333333-3333-4333-8333-333333333333";

#[test]
fn native_progress_observation_follows_committed_attempt_changes() {
    let manager=GatewayManager::mcp_test_manager();
    let broker=manager.observations.broker();
    let mut reader=broker.subscribe("activation-test","mcp","profiles.activation").unwrap();
    assert!(reader.try_recv().unwrap().unwrap().payload.unwrap().is_null());
    let tracker=&manager.profile_activations;
    manager.profile_admission_epoch.store(7,Ordering::Release);
    tracker.begin(OP,PROFILE,||7).unwrap();

    let mut next=|| {
        let deadline=Instant::now()+Duration::from_secs(2);
        loop {
            if let Some(frame)=reader.try_recv().unwrap(){return frame.payload.unwrap();}
            assert!(Instant::now()<deadline,"Activation observation did not wake.");
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    assert_eq!(next()["phase"],"preparing");
    {
        let mut entries = tracker.entries.lock().unwrap();
        entries.get_mut(OP).unwrap().message = "Committed while authority remains locked".into();
        tracker.commit(&entries);
        assert_eq!(next()["message"], "Committed while authority remains locked");
    }
    tracker.note(7,"launching","Launching simulator");
    assert_eq!(next()["step"],"launching");
    tracker.cancel(OP,&manager.profile_admission_epoch).unwrap();
    assert_eq!(next()["cancelRequested"],true);
    manager.finish_profile_activation(OP,&json!({"ok":false}));
    assert_eq!(next()["phase"],"cancelled");

    let mut cleanup=json!({"command":"profile_stop","activationId":NEXT});
    assert_eq!(manager.begin_profile_cleanup(&mut cleanup,9).as_deref(),Some(NEXT));
    assert_eq!(next()["phase"],"deactivating");
    manager.finish_profile_activation(NEXT,&json!({"ok":true}));
    assert_eq!(next()["phase"],"succeeded");

    let failed=uuid::Uuid::new_v4().to_string();
    tracker.begin(&failed,PROFILE,||10).unwrap();
    assert_eq!(next()["phase"],"preparing");
    manager.finish_profile_activation(&failed,&json!({"ok":false,"error":"Authored activation failure"}));
    let failure=next();
    assert_eq!(failure["phase"],"failed");
    assert_eq!(failure["error"],"Authored activation failure");
    assert_eq!(tracker.current(),None);

    let switch=uuid::Uuid::new_v4().to_string();
    tracker.begin_with(&switch,PROFILE,||11,|entry| {
        entry.origin="vehicle-switch".into();
        entry.cancellable=false;
    }).unwrap();
    assert_eq!(next()["origin"],"vehicle-switch");
    manager.finish_profile_activation(&switch,&json!({"ok":true}));
    assert_eq!(next()["phase"],"succeeded");
    assert!(next().is_null(),"The deadline withdraws the confirmation without another request.");
}

#[test]
fn committed_progress_wakes_consumers_without_replaying_ignored_updates() {
    let tracker=Tracker::default();
    let epoch=AtomicU64::new(7);
    let before=tracker.changes.observed();
    tracker.begin(OP,PROFILE,||7).unwrap();
    let started=tracker.changes.observed();
    assert!(started>before);
    tracker.note(6,"preparing","Late old attempt");
    assert_eq!(tracker.changes.observed(),started);
    tracker.note(7,"preparing","Preparing outputs");
    let noted=tracker.changes.observed();
    assert!(noted>started);
    tracker.note(7,"preparing","Preparing outputs");
    assert_eq!(tracker.changes.observed(),noted);
    tracker.cancel(OP,&epoch).unwrap();
    assert!(tracker.changes.observed()>noted);
    assert_eq!(tracker.current_entry(Instant::now()).unwrap().0.phase,"cancelling");
}

#[test]
fn vehicle_confirmation_ends_at_its_exact_deadline() {
    let tracker=Tracker::default();
    tracker.begin_with(OP,PROFILE,||7,|entry|entry.origin="vehicle-switch".into()).unwrap();
    let finished=Instant::now();
    tracker.entries.lock().unwrap().get_mut(OP).unwrap().completed=Some(finished);
    let end=finished+VEHICLE_SWITCH_CONFIRMATION;
    assert_eq!(tracker.current_entry(finished).unwrap().1,Some(end));
    assert!(tracker.current_entry(end).is_none());
}

#[test]
fn quick_vehicle_completion_is_visible_but_never_resurrected_after_a_newer_operation() {
    let tracker=Tracker::default();tracker.begin(OP,PROFILE,||7).unwrap();
    {
        let mut entries=tracker.entries.lock().unwrap();let entry=entries.get_mut(OP).unwrap();
        entry.origin="vehicle-switch".into();entry.cancellable=false;entry.phase="succeeded";entry.completed=Some(Instant::now());
    }
    assert_eq!(tracker.current().as_deref(),Some(OP));
    {
        tracker.entries.lock().unwrap().get_mut(OP).unwrap().completed=Some(Instant::now()-VEHICLE_SWITCH_CONFIRMATION);
    }
    assert!(tracker.current().is_none(),"A completed switch must eventually dismiss");
    tracker.entries.lock().unwrap().get_mut(OP).unwrap().completed=Some(Instant::now());
    tracker.begin(NEXT,PROFILE,||8).unwrap();assert_eq!(tracker.current().as_deref(),Some(NEXT));
    {
        let mut entries=tracker.entries.lock().unwrap();let entry=entries.get_mut(NEXT).unwrap();
        entry.phase="succeeded";entry.completed=Some(Instant::now());
    }
    assert!(tracker.current().is_none(),"An older switch must not reappear after newer manual work");
}

#[test]
fn noncancellable_switch_never_revokes_the_native_control_epoch() {
    let tracker=Tracker::default();let epoch=AtomicU64::new(7);
    tracker.begin(OP,PROFILE,||7).unwrap();
    tracker.entries.lock().unwrap().get_mut(OP).unwrap().cancellable=false;
    assert!(tracker.cancel(OP,&epoch).is_err());assert_eq!(epoch.load(Ordering::Acquire),7);
    assert_eq!(tracker.entries.lock().unwrap()[OP].view(7,vec![])["canCancel"],false);
}

#[test]
fn a_late_old_epoch_switch_cannot_mask_or_reopen_after_newer_manual_progress() {
    let tracker=Tracker::default();tracker.begin(NEXT,PROFILE,||8).unwrap();
    tracker.begin_with(OP,PROFILE,||7,|entry|{entry.origin="vehicle-switch".into();entry.cancellable=false;}).unwrap();
    assert_eq!(tracker.current().as_deref(),Some(NEXT));
    {
        let mut entries=tracker.entries.lock().unwrap();let entry=entries.get_mut(NEXT).unwrap();
        entry.phase="succeeded";entry.completed=Some(Instant::now());
    }
    assert!(tracker.current().is_none());
    tracker.entries.lock().unwrap().get_mut(OP).unwrap().completed=Some(Instant::now());
    assert!(tracker.current().is_none());
}

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
