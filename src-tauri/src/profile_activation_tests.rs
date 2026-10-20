#[test]
fn explicit_activation_only_replaces_the_confirmed_active_owner() {
    let (manager, _) = setup();
    let first = request(&manager,"profile_apply",None,json!({
        "id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1,"expectedActiveOwner":null
    }));
    assert_eq!(first["ok"],true,"{first}");
    let first_owner = first["state"]["profiles"]["profileSession"]["owner"].clone();
    let created = request(&manager,"profile_create",None,json!({"name":"Confirmed replacement"}));
    let next_id = created["state"]["profiles"]["document"]["id"].clone();
    let policy = manager.profile_automatic.view();
    let unconfirmed = request(&manager,"profile_apply",None,json!({
        "id":next_id,"expectedRevision":1,"expectedActiveOwner":null
    }));
    assert_eq!(unconfirmed["ok"],false,"{unconfirmed}");
    assert!(unconfirmed["error"].as_str().unwrap().contains("active profile changed"));
    assert_eq!(manager.profile_automatic.view(),policy,"Rejected activation must not change the manual pin.");
    assert_eq!(request(&manager,"profile_status",None,json!({}))["state"]["profiles"]["profileSession"]["owner"],first_owner);

    let confirmed = request(&manager,"profile_apply",None,json!({
        "id":next_id,"expectedRevision":1,"expectedActiveOwner":first_owner
    }));
    assert_eq!(confirmed["ok"],true,"{confirmed}");
    let next_owner = confirmed["state"]["profiles"]["profileSession"]["owner"].clone();
    assert_eq!(next_owner["profileId"],next_id);
    assert_ne!(next_owner,first_owner);

    for stale in [first_owner,Value::Null] {
        let rejected = request(&manager,"profile_apply",None,json!({
            "id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1,"expectedActiveOwner":stale
        }));
        assert_eq!(rejected["ok"],false,"{rejected}");
        assert_eq!(request(&manager,"profile_status",None,json!({}))["state"]["profiles"]["profileSession"]["owner"],next_owner);
    }
}

#[test]
fn activation_confirmation_requires_the_complete_native_owner() {
    let (manager, _) = setup();
    let first = request(&manager,"profile_apply",None,json!({
        "id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1,"expectedActiveOwner":null
    }));
    let owner = first["state"]["profiles"]["profileSession"]["owner"].clone();
    let mut newer_generation = owner.clone();
    newer_generation["generation"] = json!(owner["generation"].as_u64().unwrap()+1);
    let mut unknown_field = owner.clone();
    unknown_field["confirmed"] = json!(true);
    let mut missing_context = owner.clone();
    missing_context.as_object_mut().unwrap().remove("gameId");
    for invalid in [json!(true),json!({"profileId":osg_profiles::DEFAULT_PROFILE_ID}),newer_generation,unknown_field,missing_context] {
        let rejected = request(&manager,"profile_apply",None,json!({
            "id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1,"expectedActiveOwner":invalid
        }));
        assert_eq!(rejected["ok"],false,"{rejected}");
        assert_eq!(request(&manager,"profile_status",None,json!({}))["state"]["profiles"]["profileSession"]["owner"],owner);
    }
}

#[test]
fn activation_rechecks_confirmed_owner_after_slow_preparation_without_an_epoch_change() {
    let (mut manager, _) = setup();
    let fields = json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1,"expectedActiveOwner":null});
    let first = request(&manager,"profile_apply",None,fields.clone());
    assert_eq!(first["ok"],true,"{first}");
    let owner = first["state"]["profiles"]["profileSession"]["owner"].clone();
    let prepare = manager.profile_prepare.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release = Mutex::new(release_rx);
    Arc::get_mut(&mut manager).unwrap().profile_prepare = Arc::new(move |candidate| {
        let prepared = prepare(candidate);
        entered_tx.send(()).unwrap();
        release.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();
        prepared
    });
    let pending = manager.clone();
    let worker = std::thread::spawn(move || request(&pending,"profile_apply",None,json!({
        "id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1,"expectedActiveOwner":owner
    })));
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let epoch = manager.profile_admission_epoch.load(Ordering::Acquire);
    // Health can retire an owner without going through an operator control.
    manager.profile_session.lock().unwrap().stop().unwrap();
    assert_eq!(manager.profile_admission_epoch.load(Ordering::Acquire),epoch);
    release_tx.send(()).unwrap();
    let rejected = worker.join().unwrap();
    assert_eq!(rejected["ok"],false,"{rejected}");
    assert!(rejected["error"].as_str().unwrap().contains("active profile changed"),"{rejected}");
    assert!(manager.profile_session.lock().unwrap().status().owner.is_none());
}
