use super::*;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Waker},
    time::Duration,
};

fn assert_pending(future: &mut Pin<Box<impl Future>>) {
    assert!(future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
}

struct Cached {
    identity: String,
    operation: String,
    game: String,
    failure: watch::Sender<Option<String>>,
    cancel: watch::Sender<bool>,
    monitor: Weak<Monitor>,
}
impl Cached {
    fn ready() -> Self {
        let identity = uuid::Uuid::new_v4().to_string();
        let operation = uuid::Uuid::new_v4().to_string();
        let game = format!("permission-changes-{identity}");
        let (failure, receiver) = watch::channel(None);
        let monitor = Arc::new(super::super::process_events::test_monitor_receiver(
            receiver,
        ));
        let weak = Arc::downgrade(&monitor);
        let (cancel, _) = watch::channel(false);
        entries().lock().unwrap().insert(
            identity.clone(),
            Entry {
                game: game.clone(),
                operation: operation.clone(),
                began: Instant::now(),
                state: "ready",
                error: None,
                monitor: Some(monitor),
                cancel: cancel.clone(),
            },
        );
        Self {
            identity,
            operation,
            game,
            failure,
            cancel,
            monitor: weak,
        }
    }
    fn observer(&self) -> impl std::future::Future<Output = ()> + Send + 'static {
        observe_failure(
            self.identity.clone(),
            self.operation.clone(),
            self.failure.subscribe(),
            self.cancel.subscribe(),
        )
    }
}
impl Drop for Cached {
    fn drop(&mut self) {
        entries().lock().unwrap().remove(&self.identity);
    }
}

#[test]
fn change_watchers_are_weak_deduplicated_pruned_and_bounded() {
    let watchers = ChangeWatchers::default();
    let first = Arc::new(Signal::default());
    watchers.register(&first);
    watchers.register(&first);
    assert_eq!(watchers.0.lock().unwrap().len(), 1);
    let epoch = first.wake.observed();
    watchers.notify();
    assert_eq!(first.wake.observed(), epoch + 1);
    assert_eq!(Arc::strong_count(&first), 1);
    drop(first);
    watchers.notify();
    assert!(watchers.0.lock().unwrap().is_empty());
    let signals = (0..65)
        .map(|_| Arc::new(Signal::default()))
        .collect::<Vec<_>>();
    for signal in &signals {
        watchers.register(signal);
    }
    assert_eq!(watchers.0.lock().unwrap().len(), 64);
    watchers.notify();
    assert_eq!(signals[63].wake.observed(), 1);
    assert_eq!(signals[64].wake.observed(), 0);
    drop(signals);
    watchers.register(&Arc::new(Signal::default()));
    assert_eq!(watchers.0.lock().unwrap().len(), 1);
}

#[test]
fn subscribing_and_passive_status_never_add_native_receivers_or_work() {
    let cached = Cached::ready();
    let signal = Arc::new(Signal::default());
    let receivers = cached.failure.receiver_count();
    let strong = cached.monitor.strong_count();
    watch_changes(signal.clone());
    assert!(signal.wake.observed() > 0);
    for _ in 0..20 {
        assert_eq!(status(&cached.game)["state"], "ready");
    }
    assert_eq!(cached.failure.receiver_count(), receivers);
    assert_eq!(cached.monitor.strong_count(), strong);
    assert!(!*cached.cancel.borrow());
    assert!(cached.failure.borrow().is_none());
}

#[tokio::test]
async fn injected_failure_updates_cached_permission_state_without_a_status_read() {
    let cached = Cached::ready();
    let mut observer = Box::pin(cached.observer());
    assert_pending(&mut observer);
    cached
        .failure
        .send_replace(Some("selected process exited".into()));
    observer.await;
    let registry = entries().lock().unwrap();
    let entry = &registry[&cached.identity];
    assert_eq!(entry.state, "failed");
    assert_eq!(entry.error.as_deref(), Some("selected process exited"));
}

#[tokio::test]
async fn closed_failure_channel_updates_state_and_matches_the_cached_native_failure() {
    let cached = Cached::ready();
    let (sender, receiver) = watch::channel(None);
    let monitor = Arc::new(super::super::process_events::test_monitor_receiver(
        receiver.clone(),
    ));
    entries()
        .lock()
        .unwrap()
        .get_mut(&cached.identity)
        .unwrap()
        .monitor = Some(monitor.clone());
    drop(sender);
    observe_failure(
        cached.identity.clone(),
        cached.operation.clone(),
        receiver,
        cached.cancel.subscribe(),
    )
    .await;
    assert_eq!(status(&cached.game)["state"], "failed");
    assert_eq!(status(&cached.game)["details"], monitor.error().unwrap());
    assert!(monitor.error().unwrap().contains("unexpectedly"));
}

#[tokio::test]
async fn cancellation_and_owner_drop_end_a_pending_observer_without_retaining_a_monitor() {
    let cached = Cached::ready();
    let mut observer = Box::pin(cached.observer());
    assert_pending(&mut observer);
    cancel(&cached.game);
    tokio::time::timeout(Duration::from_secs(1), observer)
        .await
        .unwrap();
    assert_eq!(status(&cached.game)["state"], "disabled");

    let cached = Cached::ready();
    let mut observer = Box::pin(cached.observer());
    assert_pending(&mut observer);
    let weak = cached.monitor.clone();
    drop(cached);
    assert!(weak.upgrade().is_none());
    tokio::time::timeout(Duration::from_secs(1), observer)
        .await
        .unwrap();
}

#[tokio::test]
async fn an_old_monitor_failure_cannot_replace_a_new_operation() {
    let cached = Cached::ready();
    let observer = cached.observer();
    {
        let mut registry = entries().lock().unwrap();
        let entry = registry.get_mut(&cached.identity).unwrap();
        entry.operation = "replacement".into();
        entry.state = "starting";
    }
    cached
        .failure
        .send_replace(Some("old monitor failed".into()));
    observer.await;
    assert_eq!(status(&cached.game)["state"], "starting");
    let registry = entries().lock().unwrap();
    assert!(registry[&cached.identity].error.is_none());
}
