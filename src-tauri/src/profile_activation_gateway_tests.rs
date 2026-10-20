#[test]
fn cleanup_progress_is_available_before_admission_and_cannot_cancel_output_release() {
    for (command,phase,origin,message) in [("profile_stop","deactivating","deactivation","Profile deactivated."),("profile_recover","recovering","recovery","Outputs recovered.")] {
        let(manager,_)=setup();let operation=uuid::Uuid::new_v4().to_string();
        let admission=manager.profile_admission.write().unwrap();
        let coordinator=manager.profile_session.lock().unwrap();
        let pending=manager.clone();let pending_id=operation.clone();
        let worker=std::thread::spawn(move||request(&pending,command,None,json!({"activationId":pending_id})));
        let deadline=std::time::Instant::now()+Duration::from_secs(3);
        let current=loop {
            let current=request(&manager,"profile_activation_current",None,json!({}));
            if current["state"]["profileActivation"]["activationId"]==operation {break current;}
            assert!(std::time::Instant::now()<deadline,"Cleanup progress waited for the output locks: {current}");
            std::thread::sleep(Duration::from_millis(5));
        };
        let current=&current["state"]["profileActivation"];
        assert_eq!(current["phase"],phase);assert_eq!(current["origin"],origin);assert_eq!(current["canCancel"],false);
        let epoch=manager.profile_admission_epoch.load(Ordering::Acquire);
        let cancel=request(&manager,"profile_activation_cancel",None,json!({"activationId":operation}));
        assert_eq!(cancel["ok"],false);assert_eq!(manager.profile_admission_epoch.load(Ordering::Acquire),epoch);
        drop(coordinator);drop(admission);
        let result=worker.join().unwrap();assert_eq!(result["ok"],true,"{result}");
        let completed=request(&manager,"profile_activation_status",None,json!({"activationId":operation}));
        let completed=&completed["state"]["profileActivation"];
        assert_eq!(completed["phase"],"succeeded");assert_eq!(completed["message"],message);assert_eq!(completed["outcome"],"inactive");
        assert!(!completed["steps"].as_array().unwrap().is_empty());
        assert!(request(&manager,"profile_activation_current",None,json!({}))["state"]["profileActivation"].is_null());
    }
}

#[test]
fn cancelling_tracked_preparation_keeps_the_previous_exact_owner_without_a_warning() {
    let(mut manager,_)=setup();
    let initial=request(&manager,"profile_apply",None,json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1}));
    assert_eq!(initial["ok"],true,"{initial}");
    let owner=initial["state"]["profiles"]["profileSession"]["owner"].clone();
    let prepare=manager.profile_prepare.clone();
    let(entered_tx,entered_rx)=mpsc::channel();let(release_tx,release_rx)=mpsc::channel();let release=Mutex::new(release_rx);
    Arc::get_mut(&mut manager).unwrap().profile_prepare=Arc::new(move|candidate| {
        entered_tx.send(()).unwrap();release.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();prepare(candidate)
    });
    let operation=uuid::Uuid::new_v4().to_string();let apply_id=operation.clone();let pending=manager.clone();
    let apply=std::thread::spawn(move||request(&pending,"profile_apply",None,json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1,"activationId":apply_id,"expectedActiveOwner":owner})));
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let progress=request(&manager,"profile_activation_status",None,json!({"activationId":operation}));
    assert_eq!(progress["ok"],true,"{progress}");assert_eq!(progress["state"]["profileActivation"]["phase"],"preparing");assert_eq!(progress["state"]["profileActivation"]["canCancel"],true);
    let cancelled=request(&manager,"profile_activation_cancel",None,json!({"activationId":operation}));
    assert_eq!(cancelled["ok"],true,"{cancelled}");assert_eq!(cancelled["state"]["profileActivation"]["phase"],"cancelling");
    // The cancellation command acknowledges a request, never a completed undo.
    assert!(cancelled["state"]["profileActivation"]["outcome"].is_null());
    release_tx.send(()).unwrap();let result=apply.join().unwrap();assert_eq!(result["ok"],false,"{result}");
    let terminal=request(&manager,"profile_activation_status",None,json!({"activationId":operation}));
    assert_eq!(terminal["state"]["profileActivation"]["phase"],"cancelled");assert_eq!(terminal["state"]["profileActivation"]["outcome"],"previous-active");assert!(terminal["state"]["profileActivation"]["error"].is_null());
    let status=request(&manager,"profile_status",None,json!({}));
    assert_eq!(status["state"]["profiles"]["profileSession"]["owner"],initial["state"]["profiles"]["profileSession"]["owner"]);
    assert!(status["state"]["profiles"]["profileSession"]["fault"].is_null());
}

#[test]
fn tracked_progress_and_cancel_do_not_wait_for_takeover_admission_or_coordinator_locks() {
    let(manager,_)=setup();let operation=uuid::Uuid::new_v4().to_string();
    manager.begin_profile_activation(&json!({"command":"profile_apply","clientId":CLIENT,"id":osg_profiles::DEFAULT_PROFILE_ID,"activationId":operation})).unwrap();
    manager.profile_activations.activating(&operation);
    let admission=manager.profile_admission.write().unwrap();let coordinator=manager.profile_session.lock().unwrap();
    let(copy_tx,copy_rx)=mpsc::channel();let pending=manager.clone();let pending_id=operation.clone();
    let worker=std::thread::spawn(move|| {
        let progress=request(&pending,"profile_activation_status",None,json!({"activationId":pending_id}));
        let cancelled=request(&pending,"profile_activation_cancel",None,json!({"activationId":pending_id}));
        copy_tx.send((progress,cancelled)).unwrap();
    });
    let result=copy_rx.recv_timeout(Duration::from_secs(1));drop(coordinator);drop(admission);worker.join().unwrap();
    let(progress,cancelled)=result.expect("Progress and exact cancellation must complete while both takeover locks are held");
    assert_eq!(progress["state"]["profileActivation"]["phase"],"activating");assert_eq!(cancelled["state"]["profileActivation"]["phase"],"cancelling");
    manager.finish_profile_activation(&operation,&json!({"ok":false,"error":"cancelled"}));
}

#[test]
fn malformed_or_replayed_tracked_activation_never_cancels_current_owner() {
    let(manager,_)=setup();let operation=uuid::Uuid::new_v4().to_string();
    let fields=json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1,"activationId":operation});
    let first=request(&manager,"profile_apply",None,fields.clone());assert_eq!(first["ok"],true,"{first}");
    let epoch=manager.profile_admission_epoch.load(Ordering::Acquire);
    assert_eq!(request(&manager,"profile_apply",None,fields)["ok"],false);
    assert_eq!(request(&manager,"profile_apply",None,json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1,"activationId":"invalid"}))["ok"],false);
    let late=request(&manager,"profile_activation_cancel",None,json!({"activationId":operation}));assert_eq!(late["ok"],true,"{late}");
    assert_eq!(manager.profile_admission_epoch.load(Ordering::Acquire),epoch);
    assert_eq!(request(&manager,"profile_status",None,json!({}))["state"]["profiles"]["profileSession"]["owner"],first["state"]["profiles"]["profileSession"]["owner"]);
}
