use super::*;
#[test]
#[ignore = "Read-only installed SDK benchmark; set OSG_XPLANE_TEST_EXECUTABLE."]
fn installed_xplane_provider_catalogue_benchmark() {
    let executable =
        std::env::var("OSG_XPLANE_TEST_EXECUTABLE").expect("Choose installed X-Plane executable");
    let scratch = Scratch::new();
    let setup = GameIntegrationSetup::new(scratch.0.join("setup.json"), false);
    let game = json!({"id":"x-plane-12","installed":true,"executable":executable});
    setup.save_provider_settings(&json!({"command":"game_integration_settings_save","expectedSetupRevision":setup.provider_setup_revision("x-plane-12"),"connection":crate::xplane::defaults(12)}), &game).unwrap();
    let start = Instant::now();
    let metadata = setup
        .integration_catalogue_metadata_for_vehicle("x-plane-12", None)
        .unwrap();
    println!(
        "XP cold metadata: {:?}; {}",
        start.elapsed(),
        metadata["catalogueRevision"]
    );
    let start = Instant::now();
    for _ in 0..10 {
        setup
            .integration_catalogue_metadata_for_vehicle("x-plane-12", None)
            .unwrap();
    }
    println!("XP 10 warm metadata: {:?}", start.elapsed());
    let start = Instant::now();
    let full = setup
        .resource_integration_catalogue_for_vehicle("x-plane-12", None)
        .unwrap();
    println!(
        "XP warm full catalogue: {:?}; {} channels",
        start.elapsed(),
        full["channels"].as_array().unwrap().len()
    );
    drop(full);
    setup.inner.catalogues.lock().unwrap().clear();
    let start = Instant::now();
    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            setup
                .resource_integration_catalogue_for_vehicle("x-plane-12", None)
                .unwrap()["channels"]
                .as_array()
                .unwrap()
                .len()
        });
        let second = scope.spawn(|| {
            setup
                .resource_integration_catalogue_for_vehicle("x-plane-12", None)
                .unwrap()["channels"]
                .as_array()
                .unwrap()
                .len()
        });
        assert_eq!(first.join().unwrap(), second.join().unwrap());
    });
    println!("XP 2 concurrent cold catalogues: {:?}", start.elapsed());
}
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("osg-provider-settings-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn game(id: &str) -> Value {
    json!({"id":id,"integrationKey":id,"integrationCompatible":false,"provider":if id=="dcs"{"dcs-bios"}else if id.starts_with("x-plane-"){"x-plane"}else{id},"installed":false,"executable":null,"launchSupported":false})
}
fn save(setup: &GameIntegrationSetup, id: &str, connection: Value) -> Result<(), String> {
    setup.save_provider_settings(&json!({"command":"game_integration_settings_save","gameId":id,"expectedSetupRevision":setup.provider_setup_revision(id),"connection":connection}),&game(id))
}
#[test]
fn settings_save_is_game_owned_optimistic_and_never_creates_binding_documents() {
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    let setup = GameIntegrationSetup::new(path.clone(), false);
    let revision = setup.provider_setup_revision("x-plane-11");
    let mut connection = crate::xplane::defaults(11);
    connection["port"] = json!(49005);
    save(&setup, "x-plane-11", connection.clone()).unwrap();
    assert_ne!(revision, setup.provider_setup_revision("x-plane-11"));
    let saved = read_settings(&path).unwrap();
    assert_eq!(saved["games"]["x-plane-11"]["connection"], connection);
    assert!(saved["games"]["x-plane-11"].get("document").is_none());
    assert!(!scratch.0.join("integration.json").exists());
    assert!(setup.save_provider_settings(&json!({"command":"game_integration_settings_save","expectedSetupRevision":revision,"connection":crate::xplane::defaults(11)}),&game("x-plane-11")).is_err());
    assert_eq!(read_settings(&path).unwrap(), saved);
    let reopened = GameIntegrationSetup::new(path, false);
    assert_eq!(
        reopened.provider_setup_revision("x-plane-11"),
        setup.provider_setup_revision("x-plane-11")
    );
}
#[test]
fn closed_games_keep_settings_and_catalogues_without_starting_integrations() {
    let scratch = Scratch::new();
    let setup = GameIntegrationSetup::new(scratch.0.join("setup.json"), false);
    save(&setup, "x-plane-11", crate::xplane::defaults(11)).unwrap();
    let first = setup.resource_integration_catalogue("x-plane-11").unwrap();
    let second = setup.resource_integration_catalogue("x-plane-12").unwrap();
    assert_ne!(first["catalogueRevision"], second["catalogueRevision"]);
    let mut games = vec![game("x-plane-11")];
    setup.decorate(&mut games);
    assert_eq!(games[0]["integrationSetup"]["connection"]["port"], 49000);
    assert!(games[0]["integrationSetup"]["providerSetupRevision"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert_eq!(
        games[0]["integrationSetup"]["catalogueRevision"],
        first["catalogueRevision"]
    );
    let old = first["providerSetupRevision"].clone();
    let mut connection = crate::xplane::defaults(11);
    connection["freshnessMs"] = json!(2000);
    save(&setup, "x-plane-11", connection).unwrap();
    let fresh = setup.resource_integration_catalogue("x-plane-11").unwrap();
    assert_ne!(old, fresh["providerSetupRevision"]);
}
#[test]
fn runtime_identity_bindings_wrong_game_and_external_file_edits_are_rejected() {
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    let setup = GameIntegrationSetup::new(path.clone(), false);
    let mut connection = crate::xplane::defaults(11);
    connection["processIdentity"] = json!({"pid":42,"epoch":1,"path":"X-Plane.exe"});
    assert!(save(&setup, "x-plane-11", connection).is_err());
    assert!(save(&setup, "x-plane-12", crate::xplane::defaults(11)).is_err());
    assert!(setup.save_provider_settings(&json!({"command":"game_integration_settings_save","expectedSetupRevision":setup.provider_setup_revision("x-plane-11"),"connection":crate::xplane::defaults(11),"aircraftModule":"Aircraft"}),&game("x-plane-11")).is_err());
    assert!(setup.save_provider_settings(&json!({"command":"game_integration_settings_save","expectedSetupRevision":setup.provider_setup_revision("x-plane-11"),"connection":crate::xplane::defaults(11),"document":{}}),&game("x-plane-11")).is_err());
    save(&setup, "x-plane-11", crate::xplane::defaults(11)).unwrap();
    let mut external = read_settings(&path).unwrap();
    external["games"]["x-plane-11"]["connection"]["port"] = json!(49001);
    fixture_write_settings(&path, &external);
    assert!(save(&setup, "x-plane-11", crate::xplane::defaults(11)).is_err());
    assert_eq!(read_settings(&path).unwrap(), external);
}
#[test]
fn dcs_aircraft_catalogue_and_folder_live_only_in_game_settings() {
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    let setup = GameIntegrationSetup::new(path.clone(), false);
    let connection = dcs_connection(&scratch.0.join("catalogue"));
    let request = json!({"command":"game_integration_settings_save","expectedSetupRevision":setup.provider_setup_revision("dcs"),"connection":connection,"userFolder":scratch.0,"aircraftModule":"FA-18C_hornet"});
    setup
        .save_provider_settings(&request, &game("dcs"))
        .unwrap();
    let saved = read_settings(&path).unwrap();
    assert_eq!(saved["games"]["dcs"]["aircraftModule"], "FA-18C_hornet");
    assert!(setup.resource_integration_catalogue("dcs").is_err());
    let mut bad = request.clone();
    bad["expectedSetupRevision"] = json!(setup.provider_setup_revision("dcs"));
    bad["aircraftModule"] = json!("../outside");
    assert!(setup.save_provider_settings(&bad, &game("dcs")).is_err());
    assert_eq!(read_settings(&path).unwrap(), saved);
}
#[test]
fn dcs_vehicle_catalogues_are_independent_and_browsing_never_mutates_settings_or_current_revision()
{
    let scratch = Scratch::new();
    let catalogue = scratch.0.join("catalogue");
    fs::create_dir_all(&catalogue).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dcs-bios");
    for name in [
        "A-10C.json",
        "AircraftAliases.json",
        "MetadataStart.json",
        "MetadataEnd.json",
    ] {
        fs::copy(fixtures.join(name), catalogue.join(name)).unwrap();
    }
    fs::copy(
        fixtures.join("CommonData-altitude.json"),
        catalogue.join("CommonData.json"),
    )
    .unwrap();
    fs::write(catalogue.join("OtherJet.json"), "{}").unwrap();
    fs::write(catalogue.join("F-14.json"), "{}").unwrap();
    fs::write(catalogue.join("AircraftAliases.json"), json!({"A-10C":["A-10C","CommonData"],"A-10C_2":["A-10C","CommonData"],"OtherJet":["OtherJet","CommonData"],"F-14B":["F-14","CommonData"],"F-14A-135-GR":["F-14","CommonData"]}).to_string()).unwrap();
    let path = scratch.0.join("setup.json");
    let setup = GameIntegrationSetup::new(path.clone(), false);
    setup.save_provider_settings(&json!({"command":"game_integration_settings_save","expectedSetupRevision":setup.provider_setup_revision("dcs"),"connection":dcs_connection(&catalogue),"userFolder":scratch.0,"aircraftModule":"A-10C"}), &game("dcs")).unwrap();
    let saved = read_settings(&path).unwrap();
    let base = setup.resource_integration_catalogue("dcs").unwrap();
    let first = setup
        .resource_integration_catalogue_for_vehicle("dcs", Some("A-10C_2"))
        .unwrap();
    let second = setup
        .resource_integration_catalogue_for_vehicle("dcs", Some("OtherJet"))
        .unwrap();
    let tomcat = setup
        .resource_integration_catalogue_for_vehicle("dcs", Some("F-14B"))
        .unwrap();
    setup
        .inner
        .catalogues
        .lock()
        .unwrap()
        .get_mut("dcs:F-14")
        .unwrap()
        .0 -= Duration::from_secs(3);
    let refreshed_tomcat = setup
        .resource_integration_catalogue_for_vehicle("dcs", Some("F-14"))
        .unwrap();
    assert_eq!(tomcat, refreshed_tomcat);
    assert_eq!(
        refreshed_tomcat["vehicleAliases"],
        json!(["F-14A-135-GR", "F-14B"])
    );
    assert_eq!(first["aircraftModule"], "A-10C");
    assert_eq!(second["aircraftModule"], "OtherJet");
    assert!(first["controls"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["inputs"]
            .as_array()
            .is_some_and(|inputs| !inputs.is_empty())));
    assert!(second["controls"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["sourceModule"] == "CommonData"));
    assert_ne!(first["catalogueRevision"], second["catalogueRevision"]);
    assert_ne!(base["catalogueRevision"], first["catalogueRevision"]);
    assert_eq!(
        setup.current_catalogue_revision_for_vehicle("dcs", Some("A-10C_2")),
        first["catalogueRevision"].as_str().map(str::to_owned)
    );
    assert_eq!(
        setup.current_catalogue_revision("dcs"),
        base["catalogueRevision"].as_str().map(str::to_owned)
    );
    assert_eq!(read_settings(&path).unwrap(), saved);
    setup
        .inner
        .catalogues
        .lock()
        .unwrap()
        .get_mut("dcs:A-10C")
        .unwrap()
        .0 -= Duration::from_secs(3);
    fs::write(
        catalogue.join("AircraftAliases.json"),
        "{changed and invalid}",
    )
    .unwrap();
    // Passive observation keeps reading the small prior capture without
    // touching source files. Full preparation still validates aliases.
    let metadata = setup
        .integration_catalogue_metadata_for_vehicle("dcs", Some("A-10C_2"))
        .unwrap();
    assert_eq!(metadata["catalogueRevision"], first["catalogueRevision"]);
    assert_eq!(metadata["aircraftModule"], "A-10C");
    assert!(metadata.get("controls").is_none());
    assert!(metadata.to_string().len() < 512);
    assert!(setup
        .resource_integration_catalogue_for_vehicle("dcs", Some("A-10C_2"))
        .is_err());
}
#[test]
fn corrupt_or_oversized_preferences_remain_visible_and_read_only() {
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    fs::write(&path, b"{broken").unwrap();
    crate::persistence::test_support::record(&path, "games", "setup", b"{broken");
    let setup = GameIntegrationSetup::new(path.clone(), false);
    assert!(save(&setup, "x-plane-11", crate::xplane::defaults(11)).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"{broken");
    crate::persistence::test_support::record(
        &path,
        "games",
        "setup",
        &vec![b' '; MAX_SETTINGS + 1],
    );
    assert!(read_settings(&path).is_err());
    let mut invalid = json!({"schemaVersion":1,"games":{"x-plane-11":{"connection":crate::xplane::defaults(11),"document":{}}}});
    fixture_write_settings(&path, &invalid);
    assert!(read_settings(&path).is_err());
    invalid["games"]["x-plane-11"]
        .as_object_mut()
        .unwrap()
        .remove("document");
    invalid["games"]["x-plane-11"]["connection"]["processIdentity"] = json!({"pid":1});
    fixture_write_settings(&path, &invalid);
    assert!(read_settings(&path).is_err());
}
#[test]
fn assetto_provider_settings_and_read_only_catalogues_remain_distinct() {
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    let setup = GameIntegrationSetup::new(path.clone(), false);
    for id in [
        "assetto-corsa",
        "assetto-corsa-competizione",
        "assetto-corsa-evo",
    ] {
        let connection = json!({"source":"shared-memory","freshnessMs":2100});
        save(&setup, id, connection.clone()).unwrap();
        let catalogue = setup.resource_integration_catalogue(id).unwrap();
        assert_eq!(catalogue["provider"], id);
        assert_eq!(catalogue["commands"], json!([]));
        assert!(catalogue["channels"].as_array().unwrap().len() > 5);
        for id in [
            "rpm",
            "speed_kmh",
            "gear",
            "max_rpm",
            "rev_ratio",
            "engine.rpm",
        ] {
            assert!(catalogue["channels"]
                .as_array()
                .unwrap()
                .iter()
                .any(|channel| channel["id"] == id));
        }
        assert_eq!(
            setup.provider_connection_snapshot(id).unwrap().0,
            connection
        );
        let mut unsupported = connection;
        unsupported["commandPort"] = json!(7778);
        assert!(save(&setup, id, unsupported).is_err());
    }
    let reopened = GameIntegrationSetup::new(path, false);
    for id in [
        "assetto-corsa",
        "assetto-corsa-competizione",
        "assetto-corsa-evo",
    ] {
        assert_eq!(
            setup.provider_setup_revision(id),
            reopened.provider_setup_revision(id)
        );
    }
}
