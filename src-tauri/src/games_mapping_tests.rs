// Exercises the real mapping service through the game coordinator and simulated peripherals.
fn mapping_fixture() -> (
    GamesRuntime,
    Arc<osg_mapping_host::service::Service>,
    Arc<Mutex<Mock>>,
    Arc<FakePlatform>,
    PathBuf,
    Value,
) {
    let (panel, mock) = mock();
    let platform = Arc::new(FakePlatform::new());
    let path = std::env::temp_dir()
        .join(format!("osg-game-mapping-{}", uuid::Uuid::new_v4()))
        .join("games.json");
    let root = path.parent().unwrap().join("mappings");
    let profile = osg_mapping::profile::Profile::parse(include_bytes!(
        "../../crates/mapping/fixtures/direct-profile.json"
    ))
    .unwrap();
    let assignments = profile
        .roles
        .iter()
        .map(|r| osg_mapping::engine::Assignment {
            role: r.id.clone(),
            source: osg_mapping::engine::Source {
                origin: r.origin,
                id: r.id.clone(),
            },
            controls: r
                .controls
                .iter()
                .map(|c| osg_mapping::engine::ControlAssignment {
                    control: c.id.clone(),
                    source_control: c.id.clone(),
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    let store = osg_mapping::store::Store::new(root.clone());
    store.save(&profile).unwrap();
    store.save_assignments(&profile.id, &assignments).unwrap();
    let service = Arc::new(osg_mapping_host::service::Service::new(
        Arc::new(osg_controller_input::ControllerService::new(None, false)),
        Some(root),
        Arc::new(|| Err("No driver in fixture".into())),
    ));
    let captured = service.capture_saved(&profile.id, None).unwrap();
    let mapping = service.clone();
    let log = mock.clone();
    let wrapper: Panel = Arc::new(move |r| {
        let command = r["command"].as_str().unwrap_or("");
        if !command.starts_with("_game_mapping_") {
            return panel(r);
        }
        log.lock().unwrap().calls.push(command.into());
        let j = &r["mapping"];
        let result = match command {
            "_game_mapping_snapshot" => mapping.game_snapshot(&r["capture"]),
            "_game_mapping_take" => mapping.game_take(j).map(|_| json!({})),
            "_game_mapping_arm" => mapping.game_arm(j).map(|_| json!({})),
            "_game_mapping_stop" => mapping.game_stop(j).map(|_| json!({})),
            "_game_mapping_abandon" => {
                mapping.game_abandon(j);
                Ok(json!({}))
            }
            "_game_mapping_resume" => mapping
                .game_resume(j, r["recovery"] == true)
                .map(|_| json!({})),
            _ => Err("Unexpected mapping request".into()),
        };
        match result {
            Ok(state) => json!({"ok":true,"state":state}),
            Err(error) => json!({"ok":false,"error":error}),
        }
    });
    (
        GamesRuntime::new(wrapper, path.clone(), platform.clone()),
        service,
        mock,
        platform,
        path,
        captured,
    )
}
#[test]
fn captured_mapping_launch_failure_neutralizes_before_hardware_restore_and_resumes_fresh() {
    let (runtime, mapping, mock, platform, path, captured) = mapping_fixture();
    let old = mapping
        .request(&json!({"command":"mapping_preview_start","profileId":captured["profile"]["id"]}))
        .unwrap()["token"]
        .clone();
    let mut p = peripheral_profile("dcs");
    p["schemaVersion"] = json!(2);
    p["mapping"] = captured;
    save(&runtime, p);
    platform.fail.store(true, Ordering::Release);
    launch(&runtime, "dcs");
    wait(&runtime, "failed");
    let status = mapping
        .request(&json!({"command":"mapping_status"}))
        .unwrap();
    assert_eq!(status["running"], true);
    assert_ne!(status["token"], old);
    let m = mock.lock().unwrap();
    assert_eq!(m.config, config("Baseline"));
    assert!(m.enabled);
    let at = |name: &str| m.calls.iter().position(|s| s == name).unwrap();
    assert!(at("_game_mapping_snapshot") < at("config_apply"));
    assert!(at("_game_mapping_take") < at("config_apply"));
    assert!(at("disable") < at("_game_mapping_arm"));
    assert!(at("_game_mapping_stop") < m.calls.iter().rposition(|s| s == "config_apply").unwrap());
    assert!(at("_game_mapping_resume") > m.calls.iter().rposition(|s| s == "enable").unwrap());
    drop(m);
    drop(runtime);
    drop(mapping);
    clean(&path);
}
#[test]
fn optional_mapping_dependency_failure_precedes_hardware_and_launcher() {
    let (runtime, mapping, mock, platform, path, mut captured) = mapping_fixture();
    captured["profile"]["requirements"]["deviceHiding"] = json!(true);
    captured["visibility"] = json!({"mode":"hidhide-session","sourceIds":["controller"]});
    let mut p = peripheral_profile("dcs");
    p["schemaVersion"] = json!(2);
    p["mapping"] = captured;
    save(&runtime, p);
    launch(&runtime, "dcs");
    wait(&runtime, "failed");
    assert!(!platform.launched.load(Ordering::Acquire));
    assert!(!mock.lock().unwrap().calls.iter().any(|c| matches!(
        c.as_str(),
        "config_apply" | "disable" | "integration_stop" | "_game_mapping_take"
    )));
    assert!(!path.with_extension("recovery.json").exists());
    drop(runtime);
    drop(mapping);
    clean(&path);
}
#[test]
fn captured_game_mutation_guards_and_external_mapping_conflict_preserve_new_run() {
    let (runtime, mapping, _mock, _platform, path, captured) = mapping_fixture();
    let mut p = peripheral_profile("dcs");
    p["schemaVersion"] = json!(2);
    p["mapping"] = captured.clone();
    save(&runtime, p);
    launch(&runtime, "dcs");
    wait(&runtime, "running");
    for command in [
        "mapping_assignments_save",
        "mapping_virtual_start",
        "controller_visibility_start",
        "mapping_preview_start",
    ] {
        assert!(runtime.guard_request(&json!({"command":command})).is_err());
    }
    for command in [
        "mapping_stop",
        "controller_visibility_restore",
        "mapping_panel_sources",
        "mapping_panel_inspect",
        "controller_list",
        "mapping_status",
    ] {
        assert!(runtime.guard_request(&json!({"command":command})).is_ok());
    }
    let newer = mapping
        .request(&json!({"command":"mapping_preview_start","profileId":captured["profile"]["id"]}))
        .unwrap()["token"]
        .clone();
    runtime
        .request(&json!({"command":"game_deactivate"}))
        .unwrap();
    wait(&runtime, "recovery-needed");
    assert_eq!(
        mapping
            .request(&json!({"command":"mapping_status"}))
            .unwrap()["token"],
        newer
    );
    runtime
        .request(&json!({"command":"game_abandon","preserveCurrentSetup":true}))
        .unwrap();
    assert_eq!(runtime.status()["session"]["phase"], "idle");
    assert_eq!(
        mapping
            .request(&json!({"command":"mapping_status"}))
            .unwrap()["token"],
        newer
    );
    drop(runtime);
    drop(mapping);
    clean(&path);
}

#[test]
fn schema_two_restart_offers_recovery_without_reactivating_captured_or_prior_mapping() {
    let (runtime, mapping, _mock, platform, path, captured) = mapping_fixture();
    mapping
        .request(&json!({"command":"mapping_preview_start","profileId":captured["profile"]["id"]}))
        .unwrap();
    let mut p = peripheral_profile("dcs");
    p["schemaVersion"] = json!(2);
    p["mapping"] = captured;
    save(&runtime, p);
    launch(&runtime, "dcs");
    wait(&runtime, "running");
    let interrupted = fs::read(path.with_extension("recovery.json")).unwrap();
    let panel = runtime.inner.panel.clone();
    runtime.shutdown();
    drop(runtime);
    mapping.request(&json!({"command":"mapping_stop"})).unwrap();
    fs::write(path.with_extension("recovery.json"), interrupted).unwrap();
    let restarted = GamesRuntime::new(panel, path.clone(), platform);
    assert_eq!(restarted.status()["session"]["phase"], "recovery-needed");
    assert_eq!(
        mapping
            .request(&json!({"command":"mapping_status"}))
            .unwrap()["running"],
        false
    );
    restarted
        .request(&json!({"command":"game_recover"}))
        .unwrap();
    wait(&restarted, "idle");
    assert_eq!(
        mapping
            .request(&json!({"command":"mapping_status"}))
            .unwrap()["running"],
        false
    );
    drop(restarted);
    drop(mapping);
    clean(&path);
}
