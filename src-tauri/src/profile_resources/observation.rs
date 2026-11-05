use super::*;

const TOPIC: &str = "profiles.resources";
const CLOCK: &str = "profile-resources";

fn capture(state: &live_validation::StateMutex) -> Value {
    let state = state.lock().unwrap_or_else(|error| error.into_inner());
    let diagnostics: Vec<String> = state.diagnostics.iter().take(64)
        .map(|text| text.chars().take(512).collect()).collect();
    json!({
        "session": state.registry.session(),
        "revision": state.registry.revision(),
        "inventoryDiagnostics": diagnostics,
        "readOnly": state.read_only,
    })
}

impl Runtime {
    // The serialized profile transition owns the only activation waiter.
    pub(crate) fn watch_activation_catalogue(&self, wake: &Arc<osg_streams::NativeWake>) {
        *self.state.activation_wake.lock().unwrap_or_else(|p| p.into_inner()) = Arc::downgrade(wake);
    }

    pub(crate) fn watch_reconnect_catalogue(&self, wake: &Arc<osg_streams::NativeWake>) {
        *self.state.reconnect_wake.lock().unwrap_or_else(|p| p.into_inner()) = Arc::downgrade(wake);
    }

    pub(crate) fn watch_learning_catalogue(&self, wake: &Arc<osg_streams::NativeWake>) {
        *self.state.learning_wake.lock().unwrap_or_else(|p| p.into_inner()) = Arc::downgrade(wake);
    }

    pub(crate) fn observe_catalogue(&self, broker: &osg_streams::Broker) -> Result<(), String> {
        let mut running = self.observation_worker.lock().unwrap_or_else(|error| error.into_inner());
        if running.is_some() || self.stop.load(Ordering::Acquire) {
            return Err("Resource catalogue observation cannot start.".into());
        }
        let initial = capture(&self.state);
        let publisher = broker.register(
            osg_streams::TopicSpec {
                topic: TOPIC.into(),
                source: CLOCK.into(),
                audiences: vec!["ui".into(), "browser".into(), "mcp".into()],
            },
            initial.clone(), 0, CLOCK.into(),
        ).map_err(|error| error.to_string())?;
        let wake = self.state.catalogue_wake.clone();
        let state = self.state.clone();
        let closed = self.stop.clone();
        let active = publisher.clone();
        let worker = thread::Builder::new().name("profile-resource-observation".into()).spawn(move || {
            let started = std::time::Instant::now();
            let mut previous = initial;
            while !closed.load(Ordering::Acquire) {
                let observed = wake.observed();
                let next = capture(&state);
                if next != previous {
                    // Readiness uses a nonblocking registry snapshot. Signal only
                    // after capture releases the writer's registry lock.
                    state.wake_reconnect();
                    let at = started.elapsed().as_micros().try_into().unwrap_or(u64::MAX);
                    if let Err(error) = active.publish_latest_state(active.generation(), next.clone(), at, CLOCK) {
                        eprintln!("Profile resource observation failed: {error}");
                        break;
                    }
                    previous = next;
                }
                if !closed.load(Ordering::Acquire) { wake.wait_scheduled(observed, None); }
            }
            active.close();
        }).map_err(|error| {
            broker.retire(&publisher);
            error.to_string()
        })?;
        self.state.catalogue_wake.bind_thread(worker.thread().clone())
            .map_err(|_| "Resource catalogue observer already owns this wake.".to_owned())?;
        *running = Some(worker);
        Ok(())
    }

    pub(super) fn stop_observing_catalogue(&self) {
        self.state.catalogue_wake.notify();
        let worker = self.observation_worker.lock().unwrap_or_else(|error| error.into_inner()).take();
        if let Some(worker) = worker { let _ = worker.join(); }
    }
}
