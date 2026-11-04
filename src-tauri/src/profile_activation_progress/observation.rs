use super::*;

pub(super) const TOPIC: &str = "profiles.activation";
const CLOCK: &str = "profile-activation";

impl Tracker {
    pub(in crate::gateways) fn stop_observation(&self) {
        self.closed.store(true, Ordering::Release);
        self.changes.notify();
        let worker = self.worker.lock().unwrap_or_else(|error| error.into_inner()).take();
        if let Some(worker) = worker {
            if worker.thread().id() != std::thread::current().id() { let _ = worker.join(); }
        }
    }

    /// Capture and serialize committed tracker and companion state off the
    /// activation and routing threads. Wake on source commits and exact
    /// confirmation or companion freshness deadlines.
    pub(in crate::gateways) fn observe(
        self: &Arc<Self>,
        broker: &osg_streams::Broker,
        companion_host: Arc<crate::companion_host::Runtime>,
        admission: Arc<AtomicU64>,
    ) -> Result<(), String> {
        let mut running = self.worker.lock().unwrap_or_else(|error| error.into_inner());
        if running.is_some() || self.closed.load(Ordering::Acquire) {
            return Err("Profile activation observation cannot start again.".into());
        }
        let publisher = broker.register(
            osg_streams::TopicSpec {
                topic: TOPIC.into(),
                source: CLOCK.into(),
                audiences: vec!["ui".into(), "browser".into(), "mcp".into()],
            },
            Value::Null, 0, CLOCK.into(),
        ).map_err(|error| error.to_string())?;
        let wake = self.changes.clone();
        let worker_wake = wake.clone();
        let tracker = Arc::downgrade(self);
        let companion_observers = companion_host.clone();
        let worker = std::thread::Builder::new().name("profile-activation-observation".into())
            .spawn(move || {
                let origin = Instant::now();
                let mut published = Value::Null;
                loop {
                    let observed = worker_wake.observed();
                    let Some(tracker) = tracker.upgrade() else { break; };
                    if tracker.closed.load(Ordering::Acquire) { break; }
                    let current = tracker.current_entry(Instant::now());
                    drop(tracker);
                    let (value, deadline) = current.map_or((Value::Null, None), |(entry, deadline)| {
                        let epoch = admission.load(Ordering::Acquire);
                        let (value, expiry) = project(&entry, &companion_host, epoch);
                        (value, deadline.into_iter().chain(expiry).min())
                    });
                    if worker_wake.observed() != observed {
                        continue;
                    }
                    if value != published {
                        let captured = origin.elapsed().as_micros().try_into().unwrap_or(u64::MAX);
                        if let Err(error) = publisher.publish_latest_state(publisher.generation(), value.clone(), captured, CLOCK) {
                            eprintln!("Profile activation observation failed: {error}");
                            break;
                        }
                        published = value;
                    }
                    let wait = deadline.map(|at| at.saturating_duration_since(Instant::now()));
                    worker_wake.wait_scheduled(observed, wait);
                }
                publisher.close();
            }).map_err(|error| error.to_string())?;
        wake.bind_thread(worker.thread().clone()).map_err(|_| "Profile activation observation is already running.".to_owned())?;
        companion_observers.observe_panel_operations(wake.clone());
        *running = Some(worker);
        Ok(())
    }
}
