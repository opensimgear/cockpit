use super::*;
use std::sync::mpsc;
use std::time::Duration;

fn xplane_game(scratch: &Scratch) -> Value {
    json!({"id":"x-plane-12","integrationKey":"x-plane-12","provider":"x-plane",
        "integrationCompatible":true,"installed":true,"executable":scratch.0.join("X-Plane.exe")})
}

fn configured(runtime: &GameIntegrationSetup, game: &Value) -> Value {
    let base = settings_snapshot(&runtime.inner);
    let mut draft = base.clone();
    draft["games"]["x-plane-12"] =
        json!({"connection":crate::xplane::defaults(12),"executable":game["executable"]});
    commit_settings(&runtime.inner, &base, draft.clone()).unwrap();
    runtime.refresh(std::slice::from_ref(game));
    draft
}

#[test]
fn blocked_commit_keeps_passive_readers_on_old_snapshot_and_rejects_second_stale_draft() {
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    let runtime = Arc::new(GameIntegrationSetup::new(path.clone(), true));
    let game = xplane_game(&scratch);
    let base = configured(&runtime, &game);
    let previous_revision = runtime.provider_setup_revision("x-plane-12");
    let mut first_draft = base.clone();
    first_draft["games"]["x-plane-12"]["connection"]["port"] = json!(49001);
    let mut second_draft = base.clone();
    second_draft["games"]["x-plane-11"] =
        json!({"connection":crate::xplane::defaults(11),"executable":"another.exe"});
    let (entered, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    *runtime.inner.settings_commit_pause.lock().unwrap() = Some((entered, blocked));
    let first = runtime.clone();
    let first_base = base.clone();
    let expected = first_draft.clone();
    let first_writer = thread::spawn(move || commit_settings(&first.inner, &first_base, first_draft));
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
    let second = runtime.clone();
    let second_writer = thread::spawn(move || commit_settings(&second.inner, &base, second_draft));
    let reader = runtime.clone();
    let (completed, received) = mpsc::channel();
    let getter = thread::spawn(move || {
        let mut catalogue = vec![game];
        reader.decorate(&mut catalogue);
        let display = catalogue[0]["integrationSetup"].clone();
        reader.decorate_source(&mut catalogue);
        completed.send((display, catalogue[0]["integrationSetup"].clone(),
            reader.provider_setup_revision("x-plane-12"),
            reader.provider_preferences_snapshot("x-plane-12"))).unwrap();
    });
    let captured = received.recv_timeout(Duration::from_secs(1));
    release.send(()).unwrap();
    first_writer.join().unwrap().unwrap();
    let rejected = second_writer.join().unwrap().unwrap_err();
    getter.join().unwrap();
    let (display, source, revision, (preferences, preferences_revision)) =
        captured.expect("Passive readers must not wait for durable settings persistence");
    assert_eq!(display["state"], "ready");
    assert_eq!(display["connection"]["port"], 49000);
    assert_eq!(source["state"], "ready");
    assert_eq!(source["providerSetupRevision"], previous_revision);
    assert_eq!(revision, previous_revision);
    assert_eq!(preferences_revision, previous_revision);
    assert_eq!(preferences["connection"]["port"], 49000);
    assert!(rejected.contains("changed while preparing"));
    assert_eq!(settings_snapshot(&runtime.inner), expected);
    assert_eq!(read_settings(&path).unwrap(), expected);
}

#[test]
fn failed_validation_or_external_revision_conflict_never_publishes_a_draft() {
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    let runtime = GameIntegrationSetup::new(path.clone(), true);
    let game = xplane_game(&scratch);
    let base = configured(&runtime, &game);
    let previous_revision = runtime.provider_setup_revision("x-plane-12");
    let mut invalid = base.clone();
    invalid["schemaVersion"] = json!(2);
    assert!(commit_settings(&runtime.inner, &base, invalid).is_err());
    assert_eq!(settings_snapshot(&runtime.inner), base);
    assert_eq!(read_settings(&path).unwrap(), base);
    let mut external = base.clone();
    external["games"]["x-plane-11"] =
        json!({"connection":crate::xplane::defaults(11),"executable":"external.exe"});
    fixture_write_settings(&path, &external);
    let mut attempted = base.clone();
    attempted["games"]["x-plane-12"]["connection"]["port"] = json!(49002);
    assert!(commit_settings(&runtime.inner, &base, attempted).is_err());
    assert_eq!(settings_snapshot(&runtime.inner), base);
    assert_eq!(read_settings(&path).unwrap(), external);
    let mut catalogue = vec![game];
    runtime.decorate(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "ready");
    assert_eq!(catalogue[0]["integrationSetup"]["providerSetupRevision"], previous_revision);
}

#[test]
fn blocked_sdk_stamp_does_not_hold_the_passive_catalogue_metadata_mutex() {
    let scratch = Scratch::new();
    let runtime = Arc::new(GameIntegrationSetup::new(scratch.0.join("setup.json"), true));
    let game = xplane_game(&scratch);
    configured(&runtime, &game);
    runtime.resource_integration_catalogue("x-plane-12").unwrap();
    let expected = runtime.cached_integration_catalogue_metadata_for_vehicle("x-plane-12", None).unwrap();
    let (entered, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    *runtime.inner.catalogue_stamp_pause.lock().unwrap() = Some((entered, blocked));
    let source = runtime.clone();
    let producer = thread::spawn(move || source.resource_integration_catalogue("x-plane-12"));
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
    let reader = runtime.clone();
    let (completed, received) = mpsc::channel();
    let getter = thread::spawn(move || {
        let metadata = reader.cached_integration_catalogue_metadata_for_vehicle("x-plane-12", None);
        let mut catalogue = vec![game];
        reader.decorate(&mut catalogue);
        completed.send((metadata, catalogue[0]["integrationSetup"].clone())).unwrap();
    });
    let captured = received.recv_timeout(Duration::from_secs(1));
    release.send(()).unwrap();
    producer.join().unwrap().unwrap();
    getter.join().unwrap();
    let (metadata, status) = captured.expect("SDK filesystem metadata must not block cached metadata getters");
    assert_eq!(metadata, Some(expected));
    assert_eq!(status["state"], "ready");
    assert!(status["catalogueRevision"].is_string());
}
