#[test]
fn editor_save_updates_only_its_active_profile_and_keeps_manual_selection() {
    let (manager, provider) = setup();
    let id = osg_profiles::DEFAULT_PROFILE_ID;
    let applied = request(&manager,"profile_apply",None,json!({"id":id,"expectedRevision":1}));
    assert_eq!(applied["ok"],true,"{applied}");
    let original = applied["state"]["profiles"]["profileSession"]["owner"].clone();
    let created = request(&manager,"profile_create",None,json!({"name":"Inactive"}));
    let mut inactive = created["state"]["profiles"]["document"].clone();
    inactive["name"] = json!("Saved inactive");
    let saved = request(&manager,"profile_editor_save",None,json!({"document":inactive,"expectedRevision":1,"assignments":[]}));
    assert_eq!(saved["ok"],true,"{saved}");
    assert_eq!(saved["state"]["profiles"]["saveApplication"],json!({"state":"inactive","error":null}));
    assert_eq!(saved["state"]["profiles"]["profileSession"]["owner"],original);
    assert!(request(&manager,"profile_activation_current",None,json!({}))["state"]["profileActivation"].is_null(),
        "Saving an inactive profile must not create an activation operation");
    let document = request(&manager,"profile_get",None,json!({"id":id}))["state"]["profiles"]["document"].clone();
    let saved = request(&manager,"profile_editor_save",None,json!({"document":document,"expectedRevision":1,"assignments":[]}));
    assert_eq!(saved["ok"],true,"{saved}");
    let profiles = &saved["state"]["profiles"];
    assert_eq!(profiles["saveApplication"],json!({"state":"applied","error":null}));
    assert_eq!(profiles["document"]["revision"],2);
    assert_eq!(profiles["profileSession"]["owner"]["profileRevision"],2);
    assert_ne!(profiles["profileSession"]["owner"],original);
    assert_eq!(profiles["automatic"]["mode"],"manual");
    assert_eq!(profiles["automatic"]["manualProfileId"],id);
    assert!(provider.adapters.lock().unwrap().is_empty());
}

#[test]
fn editor_save_reports_preparation_failure_after_persisting_and_keeps_previous_run() {
    let (mut manager, _) = setup();
    let id = osg_profiles::DEFAULT_PROFILE_ID;
    let applied = request(&manager,"profile_apply",None,json!({"id":id,"expectedRevision":1}));
    assert_eq!(applied["ok"],true);
    let previous = applied["state"]["profiles"]["profileSession"].clone();
    Arc::get_mut(&mut manager).unwrap().profile_prepare = Arc::new(|_| Err("Required device is unavailable.".into()));
    let document = request(&manager,"profile_get",None,json!({"id":id}))["state"]["profiles"]["document"].clone();
    let saved = request(&manager,"profile_editor_save",None,json!({"document":document,"expectedRevision":1,"assignments":[]}));
    assert_eq!(saved["ok"],true,"{saved}");
    let profiles = &saved["state"]["profiles"];
    assert_eq!(profiles["document"]["revision"],2);
    assert_eq!(profiles["saveApplication"]["state"],"failed");
    assert!(profiles["saveApplication"]["error"].as_str().unwrap().starts_with("Profile saved, but"));
    assert!(profiles["saveApplication"]["error"].as_str().unwrap().contains("Required device"));
    assert_eq!(profiles["profileSession"],previous);
    assert_eq!(request(&manager,"profile_get",None,json!({"id":id}))["state"]["profiles"]["document"]["revision"],2);
}

#[test]
fn editor_save_reports_takeover_failure_as_saved_and_exposes_recovery_status() {
    let (manager, _) = setup();
    let id = osg_profiles::DEFAULT_PROFILE_ID;
    assert_eq!(request(&manager,"profile_apply",None,json!({"id":id,"expectedRevision":1}))["ok"],true);
    manager.profile_session.lock().unwrap().fail_save_when_attempting(crate::profile_session::ApplyPhase::Acquire);
    let document = request(&manager,"profile_get",None,json!({"id":id}))["state"]["profiles"]["document"].clone();
    let saved = request(&manager,"profile_editor_save",None,json!({"document":document,"expectedRevision":1,"assignments":[]}));
    assert_eq!(saved["ok"],true,"{saved}");
    assert_eq!(saved["state"]["profiles"]["saveApplication"]["state"],"failed");
    assert_eq!(saved["state"]["profiles"]["document"]["revision"],2);
    assert_eq!(saved["state"]["profiles"]["profileSession"]["state"],"recovery-needed");
    assert!(saved["state"]["profiles"]["profileSession"]["owner"].is_null());
    assert_eq!(request(&manager,"profile_get",None,json!({"id":id}))["state"]["profiles"]["document"]["revision"],2);
}

#[test]
fn inactive_editor_save_and_draft_application_capture_do_not_cancel_input_learning() {
    let gate=Arc::new(std::sync::Barrier::new(2));
    let (manager,source)=learning_manager(Some(gate.clone()));
    let manager=Arc::new(manager);
    let learning=manager.clone();
    let pending=std::thread::spawn(move||request(&learning,"profile_input_learn_start",None,source));
    gate.wait();
    let document=request(&manager,"profile_get",None,json!({"id":osg_profiles::DEFAULT_PROFILE_ID}))["state"]["profiles"]["document"].clone();
    let saved=request(&manager,"profile_editor_save",None,json!({"document":document,"expectedRevision":1,"assignments":[]}));
    assert_eq!(saved["ok"],true,"{saved}");
    assert_eq!(saved["state"]["profiles"]["saveApplication"]["state"],"inactive");
    // This fixture has no native application, but even a failed passive capture
    // must not invalidate the learner or a staged profile transition.
    assert_eq!(request(&manager,"profile_desktop_capture_application",None,json!({}))["ok"],false);
    gate.wait();
    let learned=pending.join().unwrap();
    assert_eq!(learned["ok"],true,"{learned}");
}

#[test]
fn slow_editor_save_keeps_stop_and_recovery_responsive_without_late_rearm() {
    for operation in ["profile_stop","profile_recover"] {
        let (mut manager, _) = setup();
        let id = osg_profiles::DEFAULT_PROFILE_ID;
        assert_eq!(request(&manager,"profile_apply",None,json!({"id":id,"expectedRevision":1}))["ok"],true);
        let document = request(&manager,"profile_get",None,json!({"id":id}))["state"]["profiles"]["document"].clone();
        let prepare = manager.profile_prepare.clone();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release = Mutex::new(release_rx);
        Arc::get_mut(&mut manager).unwrap().profile_prepare = Arc::new(move |candidate| {
            entered_tx.send(()).unwrap();
            release.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();
            prepare(candidate)
        });
        let saving = manager.clone();
        let worker = std::thread::spawn(move || request(&saving,"profile_editor_save",None,json!({"document":document,"expectedRevision":1,"assignments":[]})));
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let progress = request(&manager,"profile_activation_current",None,json!({}));
        assert_eq!(progress["ok"],true,"{progress}");
        let progress = &progress["state"]["profileActivation"];
        assert_eq!(progress["origin"],"saved-changes");
        assert_eq!(progress["profileId"],id);
        let activation_id = progress["activationId"].as_str().unwrap().to_owned();
        assert_eq!(request(&manager,"profile_get",None,json!({"id":id}))["state"]["profiles"]["document"]["revision"],2);
        let (done_tx, done_rx) = mpsc::channel();
        let control = manager.clone();
        let control = std::thread::spawn(move || {
            let fields = json!({});
            done_tx.send(request(&control,operation,None,fields)).unwrap();
        });
        let completed = done_rx.recv_timeout(Duration::from_secs(2));
        release_tx.send(()).unwrap();
        let saved = worker.join().unwrap();
        control.join().unwrap();
        let completed = completed.expect("Emergency controls must complete before Save preparation resumes.");
        assert_eq!(completed["ok"],true,"{completed}");
        assert_eq!(saved["ok"],true,"{saved}");
        assert_eq!(saved["state"]["profiles"]["saveApplication"]["state"],"failed");
        assert!(saved["state"]["profiles"]["saveApplication"]["error"].as_str().unwrap().contains("cancelled"));
        assert!(request(&manager,"profile_status",None,json!({}))["state"]["profiles"]["profileSession"]["owner"].is_null());
        let completed = request(&manager,"profile_activation_status",None,json!({"activationId":activation_id}));
        assert_eq!(completed["ok"],true,"{completed}");
        assert_eq!(completed["state"]["profileActivation"]["phase"],"failed",
            "The successful save response must not hide a failed application");
        assert!(completed["state"]["profileActivation"]["error"].as_str().unwrap().contains("cancelled"));
    }
}

#[test]
fn tracked_cancel_of_saved_changes_preserves_the_saved_document_and_previous_active_run() {
    let (mut manager, _) = setup();
    let id = osg_profiles::DEFAULT_PROFILE_ID;
    let applied = request(&manager,"profile_apply",None,json!({"id":id,"expectedRevision":1}));
    assert_eq!(applied["ok"],true,"{applied}");
    let previous = applied["state"]["profiles"]["profileSession"]["owner"].clone();
    let document = request(&manager,"profile_get",None,json!({"id":id}))["state"]["profiles"]["document"].clone();
    let prepare = manager.profile_prepare.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release = Mutex::new(release_rx);
    Arc::get_mut(&mut manager).unwrap().profile_prepare = Arc::new(move |candidate| {
        entered_tx.send(()).unwrap();
        release.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();
        prepare(candidate)
    });
    let saving = manager.clone();
    let worker = std::thread::spawn(move || request(&saving,"profile_editor_save",None,json!({"document":document,"expectedRevision":1,"assignments":[]})));
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let progress = request(&manager,"profile_activation_current",None,json!({}));
    let activation_id = progress["state"]["profileActivation"]["activationId"].as_str().unwrap().to_owned();
    let cancelled = request(&manager,"profile_activation_cancel",None,json!({"activationId":activation_id}));
    release_tx.send(()).unwrap();
    let saved = worker.join().unwrap();
    assert_eq!(cancelled["ok"],true,"{cancelled}");
    assert_eq!(saved["ok"],true,"{saved}");
    assert_eq!(saved["state"]["profiles"]["document"]["revision"],2);
    assert_eq!(saved["state"]["profiles"]["saveApplication"]["state"],"failed");
    assert_eq!(saved["state"]["profiles"]["profileSession"]["owner"],previous);
    let completed = request(&manager,"profile_activation_status",None,json!({"activationId":activation_id}));
    let completed = &completed["state"]["profileActivation"];
    assert_eq!(completed["origin"],"saved-changes");
    assert_eq!(completed["phase"],"cancelled");
    assert_eq!(completed["outcome"],"previous-active");
}

#[test]
fn a_newer_editor_save_replaces_the_run_and_cancels_older_saved_preparation() {
    let (mut manager, _) = setup();
    let id = osg_profiles::DEFAULT_PROFILE_ID;
    assert_eq!(request(&manager,"profile_apply",None,json!({"id":id,"expectedRevision":1}))["ok"],true);
    let document = request(&manager,"profile_get",None,json!({"id":id}))["state"]["profiles"]["document"].clone();
    let prepare = manager.profile_prepare.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release = Mutex::new(release_rx);
    let first = AtomicBool::new(true);
    Arc::get_mut(&mut manager).unwrap().profile_prepare = Arc::new(move |candidate| {
        if first.swap(false,Ordering::AcqRel) {
            entered_tx.send(()).unwrap();
            release.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();
        }
        prepare(candidate)
    });
    let saving = manager.clone();
    let worker = std::thread::spawn(move || request(&saving,"profile_editor_save",None,json!({"document":document,"expectedRevision":1,"assignments":[]})));
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut latest = request(&manager,"profile_get",None,json!({"id":id}))["state"]["profiles"]["document"].clone();
    latest["eligibility"]["gameIds"] = json!(["dcs"]);
    let latest = request(&manager,"profile_editor_save",None,json!({"document":latest,"expectedRevision":2,"assignments":[]}));
    release_tx.send(()).unwrap();
    let older = worker.join().unwrap();
    assert_eq!(latest["ok"],true,"{latest}");
    assert_eq!(latest["state"]["profiles"]["saveApplication"]["state"],"applied");
    assert_eq!(older["ok"],true,"{older}");
    assert_eq!(older["state"]["profiles"]["saveApplication"]["state"],"failed");
    let status = request(&manager,"profile_status",None,json!({}));
    assert_eq!(status["state"]["profiles"]["profileSession"]["owner"],latest["state"]["profiles"]["profileSession"]["owner"]);
    assert_eq!(status["state"]["profiles"]["profileSession"]["owner"]["profileRevision"],3);
}
