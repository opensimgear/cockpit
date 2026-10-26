#[test]
fn mcp_authorization_revoked_queued_settings_and_draft_writes_fail_before_dispatch() {
    let manager = Arc::new(GatewayManager::mcp_test_manager());
    let client = crate::mcp_tools::native_client("mcp-queued-settings-fixture");
    manager.mcp_authorize_client(&client, true);
    let current = request(
        &manager,
        "profile_local_settings_get",
        None,
        json!({"scope":"selection-mode"}),
    )["state"]["localSettings"]
        .clone();
    let admission = manager.profile_admission.write().unwrap();
    let saving = manager.clone();
    let save_client = client.clone();
    let pending = std::thread::spawn(move || {
        request(
            &saving,
            "profile_local_settings_save",
            None,
            json!({"scope":"selection-mode","expectedSettingsRevision":current["revision"],"expectedActiveOwner":null,"settings":{"mode":"automatic"},"clientId":save_client}),
        )
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !manager.profile_automatic.pending() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        manager.profile_automatic.pending(),
        "The settings request must capture its authorization and queue for native admission first"
    );
    let epoch = manager.profile_admission_epoch.load(Ordering::Acquire);
    // This is the same policy update used by mcp_authorize_client while its
    // native admission guard is held. Revoke wins before the queued writer.
    manager.set_mcp_authorization_admitted(&client, false);
    drop(admission);
    let denied = pending.join().unwrap();
    assert_eq!(denied["ok"], false, "{denied}");
    assert!(
        denied["error"].as_str().unwrap().contains("authorization"),
        "{denied}"
    );
    assert_eq!(
        request(
            &manager,
            "profile_local_settings_get",
            None,
            json!({"scope":"selection-mode"})
        )["state"]["localSettings"]["settings"]["mode"],
        "manual"
    );
    let document = osg_profiles::new_document("Revoked draft").unwrap();
    let denied = request(
        &manager,
        "profile_draft_save",
        None,
        json!({"document":document,"assignments":[],"expectedRevision":null,"clientId":client}),
    );
    assert_eq!(denied["ok"], false, "{denied}");
    assert!(denied["error"].as_str().unwrap().contains("authorization"));
    assert_eq!(manager.profiles.lock().unwrap().saved_documents().len(), 1);
    assert_eq!(
        manager.profile_admission_epoch.load(Ordering::Acquire),
        epoch
    );
}

#[test]
fn mcp_authorization_revocation_and_reenable_cannot_commit_staged_apply() {
    for reenable in [false, true] {
        let (mut manager, _) = setup();
        let id = osg_profiles::DEFAULT_PROFILE_ID;
        let active = request(
            &manager,
            "profile_apply",
            None,
            json!({"id":id,"expectedRevision":1}),
        );
        assert_eq!(active["ok"], true, "{active}");
        let owner = active["state"]["profiles"]["profileSession"]["owner"].clone();
        let created = request(
            &manager,
            "profile_create",
            None,
            json!({"name":"Pending MCP takeover"}),
        );
        let next = created["state"]["profiles"]["document"]["id"].clone();
        let client = crate::mcp_tools::native_client("mcp-revocation-fixture");
        manager.mcp_authorize_client(&client, true);
        let prepare = manager.profile_prepare.clone();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release = Mutex::new(release_rx);
        Arc::get_mut(&mut manager).unwrap().profile_prepare = Arc::new(move |candidate| {
            entered_tx.send(()).unwrap();
            release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            prepare(candidate)
        });
        let applying = manager.clone();
        let apply_client = client.clone();
        let pending = std::thread::spawn(move || {
            request(
                &applying,
                "profile_apply",
                None,
                json!({"id":next,"expectedRevision":1,"expectedActiveOwner":owner,"clientId":apply_client}),
            )
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let epoch = manager.profile_admission_epoch.load(Ordering::Acquire);
        manager.mcp_authorize_client(&client, false);
        let denied = request(
            &manager,
            "profile_apply",
            None,
            json!({"id":id,"expectedRevision":1,"clientId":client}),
        );
        assert_eq!(denied["ok"], false, "{denied}");
        if reenable {
            manager.mcp_authorize_client(&client, true);
        }
        assert_eq!(manager.profile_admission_epoch.load(Ordering::Acquire),epoch,
            "Credential changes must not cancel unrelated clients through the global admission epoch");
        release_tx.send(()).unwrap();
        let denied = pending.join().unwrap();
        assert_eq!(denied["ok"], false, "reenable={reenable}: {denied}");
        assert!(
            denied["error"].as_str().unwrap().contains("authorization"),
            "{denied}"
        );
        let retained = request(&manager, "profile_status", None, json!({}));
        assert_eq!(
            retained["state"]["profiles"]["profileSession"],
            active["state"]["profiles"]["profileSession"]
        );
    }
}

#[test]
fn mcp_authorization_revoking_another_client_preserves_staged_apply() {
    let (mut manager, _) = setup();
    let client = crate::mcp_tools::native_client("mcp-authorized-fixture");
    let other = crate::mcp_tools::native_client("mcp-unrelated-fixture");
    manager.mcp_authorize_client(&client, true);
    manager.mcp_authorize_client(&other, true);
    let prepare = manager.profile_prepare.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release = Mutex::new(release_rx);
    Arc::get_mut(&mut manager).unwrap().profile_prepare = Arc::new(move |candidate| {
        entered_tx.send(()).unwrap();
        release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        prepare(candidate)
    });
    let applying = manager.clone();
    let pending = std::thread::spawn(move || {
        request(
            &applying,
            "profile_apply",
            None,
            json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1,"expectedActiveOwner":null,"clientId":client}),
        )
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let epoch = manager.profile_admission_epoch.load(Ordering::Acquire);
    manager.mcp_authorize_client(&other, false);
    assert_eq!(
        manager.profile_admission_epoch.load(Ordering::Acquire),
        epoch
    );
    release_tx.send(()).unwrap();
    let applied = pending.join().unwrap();
    assert_eq!(applied["ok"], true, "{applied}");
    assert_eq!(
        applied["state"]["profiles"]["profileSession"]["owner"]["profileId"],
        osg_profiles::DEFAULT_PROFILE_ID
    );
}

#[test]
fn local_settings_revision_detects_native_ui_edits_and_owner_changes() {
    let (manager, _) = setup();
    let observed = request(
        &manager,
        "profile_local_settings_get",
        None,
        json!({"scope":"selection-mode"}),
    );
    assert_eq!(observed["ok"], true, "{observed}");
    let revision = observed["state"]["localSettings"]["revision"].clone();
    assert_eq!(
        request(
            &manager,
            "profile_selection_mode",
            None,
            json!({"mode":"automatic"})
        )["ok"],
        true
    );
    let epoch = manager.profile_admission_epoch.load(Ordering::Acquire);
    let stale = request(
        &manager,
        "profile_local_settings_save",
        None,
        json!({"scope":"selection-mode","expectedSettingsRevision":revision,"expectedActiveOwner":null,"settings":{"mode":"manual"}}),
    );
    assert_eq!(stale["ok"], false, "{stale}");
    assert!(stale["error"]
        .as_str()
        .unwrap()
        .contains("revision conflict"));
    assert_eq!(
        manager.profile_admission_epoch.load(Ordering::Acquire),
        epoch,
        "A stale MCP settings write cannot cancel newer native preparation"
    );
    assert_eq!(
        request(&manager, "profile_automatic_status", None, json!({}))["state"]["profiles"]
            ["automatic"]["mode"],
        "automatic"
    );
    let id = osg_profiles::DEFAULT_PROFILE_ID;
    let applied = request(
        &manager,
        "profile_apply",
        None,
        json!({"id":id,"expectedRevision":1}),
    );
    assert_eq!(applied["ok"], true, "{applied}");
    let current = request(
        &manager,
        "profile_local_settings_get",
        None,
        json!({"scope":"selection-mode"}),
    )["state"]["localSettings"]
        .clone();
    let rejected = request(
        &manager,
        "profile_local_settings_save",
        None,
        json!({"scope":"selection-mode","expectedSettingsRevision":current["revision"],"expectedActiveOwner":null,"settings":{"mode":"automatic"}}),
    );
    assert_eq!(rejected["ok"], false, "{rejected}");
    assert_eq!(
        request(&manager, "profile_automatic_status", None, json!({}))["state"]["profiles"]
            ["automatic"]["mode"],
        "manual"
    );
    let saved = request(
        &manager,
        "profile_local_settings_save",
        None,
        json!({"scope":"selection-mode","expectedSettingsRevision":current["revision"],"expectedActiveOwner":current["profileSession"]["owner"],"settings":{"mode":"automatic"}}),
    );
    assert_eq!(saved["ok"], true, "{saved}");
    assert_eq!(
        saved["state"]["localSettings"]["settings"]["mode"],
        "automatic"
    );
    assert_eq!(
        saved["state"]["localSettings"]["profileSession"]["owner"],
        current["profileSession"]["owner"]
    );
}

#[test]
fn local_settings_reject_unsupported_fields_and_guard_device_dispatch() {
    let (manager, provider) = setup();
    let current = request(
        &manager,
        "profile_local_settings_get",
        None,
        json!({"scope":"selection-mode"}),
    )["state"]["localSettings"]
        .clone();
    for settings in [
        json!({"mode":"automatic","processIdentity":{"pid":7}}),
        json!({"mode":"automatic","profileOwner":null}),
    ] {
        let rejected = request(
            &manager,
            "profile_local_settings_save",
            None,
            json!({"scope":"selection-mode","expectedSettingsRevision":current["revision"],"expectedActiveOwner":null,"settings":settings}),
        );
        assert_eq!(rejected["ok"], false, "{rejected}");
        assert!(rejected["error"].as_str().unwrap().contains("unsupported"));
    }
    let active = request(
        &manager,
        "profile_apply",
        None,
        json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1}),
    );
    assert_eq!(active["ok"], true, "{active}");
    let before = provider.adapters.lock().unwrap().len();
    let rejected = manager.mcp_guarded_request(
        json!({"command":"ports","clientId":"mcp-native-test-client"}),
        Value::Null,
    );
    assert_eq!(rejected["ok"], false, "{rejected}");
    assert_eq!(provider.adapters.lock().unwrap().len(), before);
    assert_eq!(
        request(&manager, "profile_status", None, json!({}))["state"]["profiles"]["profileSession"]
            ["owner"],
        active["state"]["profiles"]["profileSession"]["owner"]
    );
}

#[test]
fn draft_save_preserves_the_exact_active_owner_until_explicit_apply() {
    let (mut manager, _) = setup();
    let id = osg_profiles::DEFAULT_PROFILE_ID;
    let applied = request(
        &manager,
        "profile_apply",
        None,
        json!({"id":id,"expectedRevision":1}),
    );
    assert_eq!(applied["ok"], true, "{applied}");
    let owner = applied["state"]["profiles"]["profileSession"]["owner"].clone();
    let mut document = request(&manager, "profile_get", None, json!({"id":id}))["state"]
        ["profiles"]["document"]
        .clone();
    document["editor"]["viewport"]["zoom"] = json!(1.5);
    Arc::get_mut(&mut manager).unwrap().profile_prepare =
        Arc::new(|_| panic!("Saving an MCP draft cannot prepare activation"));
    let saved = request(
        &manager,
        "profile_draft_save",
        None,
        json!({"document":document,"expectedRevision":1,"assignments":[]}),
    );
    assert_eq!(saved["ok"], true, "{saved}");
    assert_eq!(saved["state"]["profiles"]["document"]["revision"], 2);
    assert_eq!(
        saved["state"]["profiles"]["saveApplication"]["state"],
        "saved"
    );
    assert_eq!(saved["state"]["profiles"]["profileSession"]["owner"], owner);
    assert!(
        request(&manager, "profile_activation_current", None, json!({}))["state"]
            ["profileActivation"]
            .is_null()
    );
}

#[test]
fn editor_save_applies_the_new_saved_context_and_none_to_its_active_profile() {
    let (manager, _) = setup();
    let directory = crate::test_directory();
    let mut game_ids = Vec::new();
    for name in ["First Registered Game", "Second Registered Game"] {
        let install_root = directory.path().join(name.replace(' ', "-"));
        std::fs::create_dir_all(&install_root).unwrap();
        let executable = install_root.join(format!("{name}.exe"));
        std::fs::write(&executable, b"fixture executable").unwrap();
        game_ids.push(
            manager
                .games
                .register_test_game(name, executable.to_str().unwrap()),
        );
    }
    let id = osg_profiles::DEFAULT_PROFILE_ID;
    assert_eq!(
        request(
            &manager,
            "profile_apply",
            None,
            json!({"id":id,"expectedRevision":1})
        )["ok"],
        true
    );
    for (revision, context) in [
        (1, Some(game_ids[0].as_str())),
        (2, Some(game_ids[1].as_str())),
        (3, None),
    ] {
        let mut document = request(&manager, "profile_get", None, json!({"id":id}))["state"]
            ["profiles"]["document"]
            .clone();
        document["eligibility"]["gameIds"] = context.map_or_else(|| json!([]), |id| json!([id]));
        let saved = request(
            &manager,
            "profile_editor_save",
            None,
            json!({"document":document,"expectedRevision":revision,"assignments":[]}),
        );
        assert_eq!(saved["ok"], true, "{saved}");
        assert_eq!(
            saved["state"]["profiles"]["saveApplication"]["state"], "applied",
            "{saved}"
        );
        assert_eq!(
            saved["state"]["profiles"]["profileSession"]["owner"]["gameId"],
            json!(context)
        );
    }
}

#[test]
fn editor_save_updates_only_its_active_profile_and_keeps_manual_selection() {
    let (manager, provider) = setup();
    let id = osg_profiles::DEFAULT_PROFILE_ID;
    let applied = request(
        &manager,
        "profile_apply",
        None,
        json!({"id":id,"expectedRevision":1}),
    );
    assert_eq!(applied["ok"], true, "{applied}");
    let original = applied["state"]["profiles"]["profileSession"]["owner"].clone();
    let created = request(&manager, "profile_create", None, json!({"name":"Inactive"}));
    let mut inactive = created["state"]["profiles"]["document"].clone();
    inactive["name"] = json!("Saved inactive");
    let saved = request(
        &manager,
        "profile_editor_save",
        None,
        json!({"document":inactive,"expectedRevision":1,"assignments":[]}),
    );
    assert_eq!(saved["ok"], true, "{saved}");
    assert_eq!(
        saved["state"]["profiles"]["saveApplication"],
        json!({"state":"inactive","error":null})
    );
    assert_eq!(
        saved["state"]["profiles"]["profileSession"]["owner"],
        original
    );
    assert!(
        request(&manager, "profile_activation_current", None, json!({}))["state"]
            ["profileActivation"]
            .is_null(),
        "Saving an inactive profile must not create an activation operation"
    );
    let document = request(&manager, "profile_get", None, json!({"id":id}))["state"]["profiles"]
        ["document"]
        .clone();
    let saved = request(
        &manager,
        "profile_editor_save",
        None,
        json!({"document":document,"expectedRevision":1,"assignments":[]}),
    );
    assert_eq!(saved["ok"], true, "{saved}");
    let profiles = &saved["state"]["profiles"];
    assert_eq!(
        profiles["saveApplication"],
        json!({"state":"applied","error":null})
    );
    assert_eq!(profiles["document"]["revision"], 2);
    assert_eq!(profiles["profileSession"]["owner"]["profileRevision"], 2);
    assert_ne!(profiles["profileSession"]["owner"], original);
    assert_eq!(profiles["automatic"]["mode"], "manual");
    assert_eq!(profiles["automatic"]["manualProfileId"], id);
    assert!(provider.adapters.lock().unwrap().is_empty());
}

#[test]
fn editor_save_reports_preparation_failure_after_persisting_and_keeps_previous_run() {
    let (mut manager, _) = setup();
    let id = osg_profiles::DEFAULT_PROFILE_ID;
    let applied = request(
        &manager,
        "profile_apply",
        None,
        json!({"id":id,"expectedRevision":1}),
    );
    assert_eq!(applied["ok"], true);
    let previous = applied["state"]["profiles"]["profileSession"].clone();
    Arc::get_mut(&mut manager).unwrap().profile_prepare =
        Arc::new(|_| Err("Required device is unavailable.".into()));
    let document = request(&manager, "profile_get", None, json!({"id":id}))["state"]["profiles"]
        ["document"]
        .clone();
    let saved = request(
        &manager,
        "profile_editor_save",
        None,
        json!({"document":document,"expectedRevision":1,"assignments":[]}),
    );
    assert_eq!(saved["ok"], true, "{saved}");
    let profiles = &saved["state"]["profiles"];
    assert_eq!(profiles["document"]["revision"], 2);
    assert_eq!(profiles["saveApplication"]["state"], "failed");
    assert!(profiles["saveApplication"]["error"]
        .as_str()
        .unwrap()
        .starts_with("Profile saved, but"));
    assert!(profiles["saveApplication"]["error"]
        .as_str()
        .unwrap()
        .contains("Required device"));
    assert_eq!(profiles["profileSession"], previous);
    assert_eq!(
        request(&manager, "profile_get", None, json!({"id":id}))["state"]["profiles"]["document"]
            ["revision"],
        2
    );
}

#[test]
fn rejected_background_connections_do_not_cancel_active_editor_save_preparation() {
    let (mut manager, provider) = setup();
    let id = osg_profiles::DEFAULT_PROFILE_ID;
    assert_eq!(
        request(
            &manager,
            "profile_apply",
            None,
            json!({"id":id,"expectedRevision":1})
        )["ok"],
        true
    );
    let document = request(&manager, "profile_get", None, json!({"id":id}))["state"]["profiles"]
        ["document"]
        .clone();
    let prepare = manager.profile_prepare.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release = Mutex::new(release_rx);
    Arc::get_mut(&mut manager).unwrap().profile_prepare = Arc::new(move |candidate| {
        entered_tx.send(()).unwrap();
        release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        prepare(candidate)
    });
    let saving = manager.clone();
    let worker = std::thread::spawn(move || {
        request(
            &saving,
            "profile_editor_save",
            None,
            json!({"document":document,"expectedRevision":1,"assignments":[]}),
        )
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let epoch = manager.profile_admission_epoch.load(Ordering::Acquire);
    // GatewayWorkspace periodically retries present disconnected gateways. A
    // denied connection must not masquerade as a newer native profile control.
    let blocked = (0..3)
        .map(|_| {
            request(
                &manager,
                "connect",
                Some(FIRST),
                json!({"backend":"pico","port":"COM7"}),
            )
        })
        .collect::<Vec<_>>();
    let observed_epoch = manager.profile_admission_epoch.load(Ordering::Acquire);
    release_tx.send(()).unwrap();
    let saved = worker.join().unwrap();
    for blocked in blocked {
        assert_eq!(blocked["ok"], false, "{blocked}");
        assert!(
            blocked["error"]
                .as_str()
                .unwrap()
                .contains("active profile owns"),
            "{blocked}"
        );
    }
    assert_eq!(
        observed_epoch, epoch,
        "Rejected independent connections cannot cancel profile preparation"
    );
    assert!(
        provider.adapters.lock().unwrap().is_empty(),
        "Rejected retries cannot open a transport"
    );
    assert_eq!(saved["ok"], true, "{saved}");
    assert_eq!(
        saved["state"]["profiles"]["saveApplication"],
        json!({"state":"applied","error":null})
    );
    assert_eq!(
        saved["state"]["profiles"]["profileSession"]["owner"]["profileRevision"],
        2
    );
    assert_eq!(saved["state"]["profiles"]["automatic"]["mode"], "manual");
}

#[test]
fn editor_save_reports_takeover_failure_as_saved_and_exposes_recovery_status() {
    let (manager, _) = setup();
    let id = osg_profiles::DEFAULT_PROFILE_ID;
    assert_eq!(
        request(
            &manager,
            "profile_apply",
            None,
            json!({"id":id,"expectedRevision":1})
        )["ok"],
        true
    );
    manager
        .profile_session
        .lock()
        .unwrap()
        .fail_save_when_attempting(crate::profile_session::ApplyPhase::Acquire);
    let document = request(&manager, "profile_get", None, json!({"id":id}))["state"]["profiles"]
        ["document"]
        .clone();
    let saved = request(
        &manager,
        "profile_editor_save",
        None,
        json!({"document":document,"expectedRevision":1,"assignments":[]}),
    );
    assert_eq!(saved["ok"], true, "{saved}");
    assert_eq!(
        saved["state"]["profiles"]["saveApplication"]["state"],
        "failed"
    );
    assert_eq!(saved["state"]["profiles"]["document"]["revision"], 2);
    assert_eq!(
        saved["state"]["profiles"]["profileSession"]["state"],
        "recovery-needed"
    );
    assert!(saved["state"]["profiles"]["profileSession"]["owner"].is_null());
    assert_eq!(
        request(&manager, "profile_get", None, json!({"id":id}))["state"]["profiles"]["document"]
            ["revision"],
        2
    );
}

#[test]
fn inactive_editor_save_and_draft_application_capture_do_not_cancel_input_learning() {
    let gate = Arc::new(std::sync::Barrier::new(2));
    let (manager, source) = learning_manager(Some(gate.clone()));
    let manager = Arc::new(manager);
    let learning = manager.clone();
    let pending =
        std::thread::spawn(move || request(&learning, "profile_input_learn_start", None, source));
    gate.wait();
    let document = request(
        &manager,
        "profile_get",
        None,
        json!({"id":osg_profiles::DEFAULT_PROFILE_ID}),
    )["state"]["profiles"]["document"]
        .clone();
    let saved = request(
        &manager,
        "profile_editor_save",
        None,
        json!({"document":document,"expectedRevision":1,"assignments":[]}),
    );
    assert_eq!(saved["ok"], true, "{saved}");
    assert_eq!(
        saved["state"]["profiles"]["saveApplication"]["state"],
        "inactive"
    );
    // This fixture has no native application, but even a failed passive capture
    // must not invalidate the learner or a staged profile transition.
    assert_eq!(
        request(
            &manager,
            "profile_desktop_capture_application",
            None,
            json!({})
        )["ok"],
        false
    );
    gate.wait();
    let learned = pending.join().unwrap();
    assert_eq!(learned["ok"], true, "{learned}");
}

#[test]
fn slow_editor_save_keeps_stop_and_recovery_responsive_without_late_rearm() {
    for operation in ["profile_stop", "profile_recover"] {
        let (mut manager, _) = setup();
        let id = osg_profiles::DEFAULT_PROFILE_ID;
        assert_eq!(
            request(
                &manager,
                "profile_apply",
                None,
                json!({"id":id,"expectedRevision":1})
            )["ok"],
            true
        );
        let document = request(&manager, "profile_get", None, json!({"id":id}))["state"]
            ["profiles"]["document"]
            .clone();
        let prepare = manager.profile_prepare.clone();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release = Mutex::new(release_rx);
        Arc::get_mut(&mut manager).unwrap().profile_prepare = Arc::new(move |candidate| {
            entered_tx.send(()).unwrap();
            release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            prepare(candidate)
        });
        let saving = manager.clone();
        let worker = std::thread::spawn(move || {
            request(
                &saving,
                "profile_editor_save",
                None,
                json!({"document":document,"expectedRevision":1,"assignments":[]}),
            )
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let progress = request(&manager, "profile_activation_current", None, json!({}));
        assert_eq!(progress["ok"], true, "{progress}");
        let progress = &progress["state"]["profileActivation"];
        assert_eq!(progress["origin"], "saved-changes");
        assert_eq!(progress["profileId"], id);
        let activation_id = progress["activationId"].as_str().unwrap().to_owned();
        assert_eq!(
            request(&manager, "profile_get", None, json!({"id":id}))["state"]["profiles"]
                ["document"]["revision"],
            2
        );
        let (done_tx, done_rx) = mpsc::channel();
        let control = manager.clone();
        let control = std::thread::spawn(move || {
            let fields = json!({});
            done_tx
                .send(request(&control, operation, None, fields))
                .unwrap();
        });
        let completed = done_rx.recv_timeout(Duration::from_secs(2));
        release_tx.send(()).unwrap();
        let saved = worker.join().unwrap();
        control.join().unwrap();
        let completed =
            completed.expect("Emergency controls must complete before Save preparation resumes.");
        assert_eq!(completed["ok"], true, "{completed}");
        assert_eq!(saved["ok"], true, "{saved}");
        assert_eq!(
            saved["state"]["profiles"]["saveApplication"]["state"],
            "failed"
        );
        assert!(saved["state"]["profiles"]["saveApplication"]["error"]
            .as_str()
            .unwrap()
            .contains("cancelled"));
        assert!(
            request(&manager, "profile_status", None, json!({}))["state"]["profiles"]
                ["profileSession"]["owner"]
                .is_null()
        );
        let completed = request(
            &manager,
            "profile_activation_status",
            None,
            json!({"activationId":activation_id}),
        );
        assert_eq!(completed["ok"], true, "{completed}");
        assert_eq!(
            completed["state"]["profileActivation"]["phase"], "failed",
            "The successful save response must not hide a failed application"
        );
        assert!(completed["state"]["profileActivation"]["error"]
            .as_str()
            .unwrap()
            .contains("cancelled"));
    }
}

#[test]
fn tracked_cancel_of_saved_changes_preserves_the_saved_document_and_previous_active_run() {
    let (mut manager, _) = setup();
    let id = osg_profiles::DEFAULT_PROFILE_ID;
    let applied = request(
        &manager,
        "profile_apply",
        None,
        json!({"id":id,"expectedRevision":1}),
    );
    assert_eq!(applied["ok"], true, "{applied}");
    let previous = applied["state"]["profiles"]["profileSession"]["owner"].clone();
    let document = request(&manager, "profile_get", None, json!({"id":id}))["state"]["profiles"]
        ["document"]
        .clone();
    let prepare = manager.profile_prepare.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release = Mutex::new(release_rx);
    Arc::get_mut(&mut manager).unwrap().profile_prepare = Arc::new(move |candidate| {
        entered_tx.send(()).unwrap();
        release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        prepare(candidate)
    });
    let saving = manager.clone();
    let worker = std::thread::spawn(move || {
        request(
            &saving,
            "profile_editor_save",
            None,
            json!({"document":document,"expectedRevision":1,"assignments":[]}),
        )
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let progress = request(&manager, "profile_activation_current", None, json!({}));
    let activation_id = progress["state"]["profileActivation"]["activationId"]
        .as_str()
        .unwrap()
        .to_owned();
    let cancelled = request(
        &manager,
        "profile_activation_cancel",
        None,
        json!({"activationId":activation_id}),
    );
    release_tx.send(()).unwrap();
    let saved = worker.join().unwrap();
    assert_eq!(cancelled["ok"], true, "{cancelled}");
    assert_eq!(saved["ok"], true, "{saved}");
    assert_eq!(saved["state"]["profiles"]["document"]["revision"], 2);
    assert_eq!(
        saved["state"]["profiles"]["saveApplication"]["state"],
        "failed"
    );
    assert_eq!(
        saved["state"]["profiles"]["profileSession"]["owner"],
        previous
    );
    let completed = request(
        &manager,
        "profile_activation_status",
        None,
        json!({"activationId":activation_id}),
    );
    let completed = &completed["state"]["profileActivation"];
    assert_eq!(completed["origin"], "saved-changes");
    assert_eq!(completed["phase"], "cancelled");
    assert_eq!(completed["outcome"], "previous-active");
}

#[test]
fn a_newer_editor_save_replaces_the_run_and_cancels_older_saved_preparation() {
    let (mut manager, _) = setup();
    let id = osg_profiles::DEFAULT_PROFILE_ID;
    assert_eq!(
        request(
            &manager,
            "profile_apply",
            None,
            json!({"id":id,"expectedRevision":1})
        )["ok"],
        true
    );
    let document = request(&manager, "profile_get", None, json!({"id":id}))["state"]["profiles"]
        ["document"]
        .clone();
    let prepare = manager.profile_prepare.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release = Mutex::new(release_rx);
    let first = AtomicBool::new(true);
    Arc::get_mut(&mut manager).unwrap().profile_prepare = Arc::new(move |candidate| {
        if first.swap(false, Ordering::AcqRel) {
            entered_tx.send(()).unwrap();
            release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        }
        prepare(candidate)
    });
    let saving = manager.clone();
    let worker = std::thread::spawn(move || {
        request(
            &saving,
            "profile_editor_save",
            None,
            json!({"document":document,"expectedRevision":1,"assignments":[]}),
        )
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut latest = request(&manager, "profile_get", None, json!({"id":id}))["state"]["profiles"]
        ["document"]
        .clone();
    latest["editor"]["viewport"]["x"] = json!(120);
    let latest = request(
        &manager,
        "profile_editor_save",
        None,
        json!({"document":latest,"expectedRevision":2,"assignments":[]}),
    );
    release_tx.send(()).unwrap();
    let older = worker.join().unwrap();
    assert_eq!(latest["ok"], true, "{latest}");
    assert_eq!(
        latest["state"]["profiles"]["saveApplication"]["state"],
        "applied"
    );
    assert_eq!(older["ok"], true, "{older}");
    assert_eq!(
        older["state"]["profiles"]["saveApplication"]["state"],
        "failed"
    );
    let status = request(&manager, "profile_status", None, json!({}));
    assert_eq!(
        status["state"]["profiles"]["profileSession"]["owner"],
        latest["state"]["profiles"]["profileSession"]["owner"]
    );
    assert_eq!(
        status["state"]["profiles"]["profileSession"]["owner"]["profileRevision"],
        3
    );
}
