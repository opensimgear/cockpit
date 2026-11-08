use super::*;
use crate::game_vehicles::CatalogueSource;

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
#[test]
fn scoped_catalogue_retains_siblings_and_discards_a_retired_read() {
    let scratch = Scratch::new();
    let (runtime, _, _) = runtime(&scratch);
    runtime.stop_test_context_observer();
    let game = |id: &str, name: &str| {
        let root = scratch.path.join(name);
        let aircraft = root.join("Aircraft");
        std::fs::create_dir_all(&aircraft).unwrap();
        std::fs::write(aircraft.join(format!("{name}.acf")), "Fixture").unwrap();
        json!({"id":id,"integrationKey":id,"integrationCompatible":true,
            "provider":"x-plane","installPath":root})
    };
    let old = game("x-plane-11", "old");
    let replacement = game("x-plane-11", "replacement");
    let sibling = game("x-plane-12", "sibling");
    runtime.seed_test_catalogue(vec![old.clone(), sibling.clone()]);
    let inner = &runtime.inner;
    let mut first = context_vehicle_catalogues::Catalogues::default();
    let mut other = context_vehicle_catalogues::Catalogues::default();
    let sibling_wake = inner.changes.setup_watch_wakes.get("x-plane-12").unwrap();
    other.refresh_scope(
        inner,
        Some("x-plane-12"),
        sibling_wake,
        || true,
        CatalogueSource::read,
    );
    let library = inner.changes.context_wake.observed();
    let current = || inner.changes.context_wake.observed() == library;
    let first_wake = inner.changes.setup_watch_wakes.get("x-plane-11").unwrap();
    first.refresh_scope(inner, Some("x-plane-11"), first_wake, current, |source| {
        let rows = source.read();
        runtime.seed_test_catalogue(vec![replacement.clone(), sibling.clone()]);
        context_vehicle_catalogues::retain_current(inner);
        rows
    });
    let rows = |game: &Value| {
        let mut game = game.clone();
        inner.vehicles.lock().unwrap().decorate(&mut game);
        game["vehicleSupport"]["vehicles"].clone()
    };
    assert_eq!(rows(&old), json!([]));
    assert_eq!(
        rows(&sibling),
        json!([{"id":"Aircraft/sibling.acf","name":"sibling"}])
    );
    first.refresh_scope(
        inner,
        Some("x-plane-11"),
        first_wake,
        || true,
        CatalogueSource::read,
    );
    assert_eq!(
        rows(&replacement),
        json!([{"id":"Aircraft/replacement.acf","name":"replacement"}])
    );
    assert_eq!(
        rows(&sibling),
        json!([{"id":"Aircraft/sibling.acf","name":"sibling"}])
    );
    runtime.shutdown();
}
