use super::*;
use std::sync::mpsc;
use std::time::Duration;

fn prepared() -> (Scratch, Arc<GameIntegrationSetup>, Value, PathBuf) {
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    let game = ac_catalogue_game(&scratch.0.join("AC"));
    ac_catalogue_car(
        Path::new(game["installPath"].as_str().unwrap()),
        "car",
        "Original",
    );
    let runtime = Arc::new(GameIntegrationSetup::new(path.clone(), true));
    runtime
        .request(
            &json!({"command":"game_integration_setup"}),
            &game,
            Arc::new(FakePlatform(false)),
        )
        .unwrap();
    wait_setup(&runtime);
    (
        scratch,
        runtime,
        game,
        path.with_file_name("assetto-corsa-cars-v1.json"),
    )
}

#[test]
fn blocked_persistence_keeps_readers_available_and_cancel_reports_finalizing_promptly() {
    let (_scratch, runtime, game, path) = prepared();
    let initial = ac_catalogue_choices(&runtime, &game);
    ac_catalogue_car(
        Path::new(game["installPath"].as_str().unwrap()),
        "new_car",
        "New",
    );
    let db = osg_persistence::Database::for_namespace(&path).unwrap();
    let persisted = db.read("games", "ac-cars").unwrap().unwrap();
    let (entered, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    *runtime.inner.ac_cars.commit_pause.lock().unwrap() = Some((entered, blocked));
    runtime
        .request(
            &json!({"command":"game_integration_setup"}),
            &game,
            Arc::new(FakePlatform(false)),
        )
        .unwrap();
    observed.recv_timeout(Duration::from_secs(5)).unwrap();

    let (sent, received) = mpsc::channel();
    let reading = runtime.clone();
    let selected = game.clone();
    let reader = thread::spawn(move || {
        sent.send(ac_catalogue_choices(&reading, &selected))
            .unwrap()
    });
    let choices = received.recv_timeout(Duration::from_secs(2));
    let (sent, received) = mpsc::channel();
    let cancelling = runtime.clone();
    let selected = game.clone();
    let cancel = thread::spawn(move || {
        sent.send(cancelling.request(
            &cancel_request(&cancelling),
            &selected,
            Arc::new(FakePlatform(false)),
        ))
        .unwrap()
    });
    let cancellation = received.recv_timeout(Duration::from_secs(2));
    let during = db.read("games", "ac-cars").unwrap().unwrap();
    // Release before assertions so a failed latency assertion still cleans up.
    release.send(()).unwrap();
    reader.join().unwrap();
    cancel.join().unwrap();
    wait_setup(&runtime);
    assert_eq!(choices.unwrap(), initial);
    assert!(cancellation
        .unwrap()
        .unwrap_err()
        .contains("already finalizing"));
    assert_eq!(during.payload, persisted.payload);
    assert_eq!(during.revision, persisted.revision);
    assert_eq!(
        ac_catalogue_choices(&runtime, &game)
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let restarted = GameIntegrationSetup::new(path.with_file_name("setup.json"), true);
    assert_eq!(
        ac_catalogue_choices(&restarted, &game),
        ac_catalogue_choices(&runtime, &game)
    );
    runtime.shutdown();
    restarted.shutdown();
}

#[test]
fn accepted_cancel_before_reservation_returns_promptly_and_retains_durable_revision() {
    let (_scratch, runtime, game, path) = prepared();
    let initial = ac_catalogue_choices(&runtime, &game);
    let db = osg_persistence::Database::for_namespace(&path).unwrap();
    let persisted = db.read("games", "ac-cars").unwrap().unwrap();
    ac_catalogue_car(
        Path::new(game["installPath"].as_str().unwrap()),
        "new_car",
        "New",
    );
    let (entered, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    *runtime.inner.ac_cars_publication_pause.lock().unwrap() = Some((entered, blocked));
    runtime
        .request(
            &json!({"command":"game_integration_setup"}),
            &game,
            Arc::new(FakePlatform(false)),
        )
        .unwrap();
    observed.recv_timeout(Duration::from_secs(5)).unwrap();
    let (sent, received) = mpsc::channel();
    let cancelling = runtime.clone();
    let selected = game.clone();
    let cancel = thread::spawn(move || {
        sent.send(cancelling.request(
            &cancel_request(&cancelling),
            &selected,
            Arc::new(FakePlatform(false)),
        ))
        .unwrap()
    });
    let cancellation = received.recv_timeout(Duration::from_secs(2));
    release.send(()).unwrap();
    cancel.join().unwrap();
    wait_setup(&runtime);
    cancellation.unwrap().unwrap();
    assert_eq!(ac_catalogue_choices(&runtime, &game), initial);
    let retained = db.read("games", "ac-cars").unwrap().unwrap();
    assert_eq!(retained.payload, persisted.payload);
    assert_eq!(retained.revision, persisted.revision);
    runtime
        .request(
            &json!({"command":"game_integration_setup"}),
            &game,
            Arc::new(FakePlatform(false)),
        )
        .unwrap();
    wait_setup(&runtime);
    assert_eq!(
        ac_catalogue_choices(&runtime, &game)
            .as_array()
            .unwrap()
            .len(),
        2
    );
    runtime.shutdown();
}

#[test]
fn a_retired_operation_cannot_be_revived_by_a_new_publication_token() {
    let retired = super::super::car_publication::Publication::default();
    retired.cancel().unwrap();
    let next = super::super::car_publication::Publication::default();
    next.begin_commit().unwrap();
    assert!(retired.begin_commit().is_err());
    assert!(next.cancel().is_err());
    next.finish();
    next.cancel().unwrap();
    assert!(next.begin_commit().is_err());
}
