use super::*;
use serde_json::json;

#[test]
fn completion_and_refresh_only_invalidate_owners_of_the_changed_root() {
    let pool = Mutex::new(Service::default());
    let (first, a) = register(&pool, Path::new("one")).unwrap();
    let (second, b) = register(&pool, Path::new("two")).unwrap();
    let first_signal = Arc::new(crate::game_read_models::signals::Signal::default());
    let second_signal = Arc::new(crate::game_read_models::signals::Signal::default());
    a.changes.set(Arc::downgrade(&first_signal)).unwrap();
    b.changes.set(Arc::downgrade(&second_signal)).unwrap();
    let source = json!({"catalogue":[{"installations":[{"sources":[{"client":"steam","appId":"42"}]}]}],"discovery":{"status":"ready"}});
    replace(&pool, first, &source).unwrap(); replace(&pool, second, &source).unwrap();
    a.notify(); b.notify();
    let before_a = first_signal.wake.observed(); let before_b = second_signal.wake.observed();
    let wakes = {
        let mut service = pool.lock().unwrap();
        service.cache_ready(Key { root: "one".into(), app_id: 42 }, "image".into());
        super::wakes(&service)
    };
    for inner in wakes { inner.notify(); }
    assert_ne!(first_signal.wake.observed(), before_a);
    assert_eq!(second_signal.wake.observed(), before_b);
    let unchanged = first_signal.wake.observed();
    let wakes = {
        let mut service = pool.lock().unwrap();
        service.cache_ready(Key { root: "one".into(), app_id: 42 }, "image".into());
        super::wakes(&service)
    };
    for inner in wakes { inner.notify(); }
    assert_eq!(first_signal.wake.observed(), unchanged);
}
