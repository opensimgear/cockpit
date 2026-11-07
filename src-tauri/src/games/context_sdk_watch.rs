//! Watch exact SDK catalogue paths on the native game context owner.
use super::*;
use crate::game_read_models::signals::Domain;
use osg_streams::NativeWake;

const XPLANE: [&str; 2] = ["x-plane-11", "x-plane-12"];
const WATCH_RETRY: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(super) struct SdkWatches([SourceWatch; 2]);

#[derive(Default)]
pub(super) struct SourceWatch {
    paths: Vec<PathBuf>,
    child_root: Option<PathBuf>,
    watch: Option<prepared_sources::watch::Watch>,
    dirty: Arc<AtomicBool>,
    retry_at: Option<Instant>,
}

impl SdkWatches {
    pub(super) fn retry_at(&self) -> Option<Instant> {
        self.0.iter().filter_map(SourceWatch::retry_at).min()
    }

    pub(super) fn refresh(&mut self, inner: &Inner) -> bool {
        let paths = {
            let settings = inner.settings.lock().unwrap_or_else(|error| error.into_inner());
            XPLANE.map(|id| settings.catalogue.iter()
                .find(|game| crate::game_discovery::integration_key(game) == Some(id))
                .and_then(|game| crate::game_integration_setup::sdk_source_path(&game["executable"]))
                .filter(|path| path.is_absolute()))
        };
        let mut changed = false;
        for ((watch, path), id) in self.0.iter_mut().zip(paths).zip(XPLANE) {
            if watch.refresh(path.into_iter().collect(), &inner.changes.context_worker_wake) {
                inner.setup.set_setup_watch(id, watch.active());
                inner.setup.invalidate_xplane_catalogue(id);
                changed = true;
            }
        }
        changed
    }
}

impl SourceWatch {
    pub(super) fn active(&self) -> bool { self.watch.is_some() }

    pub(super) fn retry_at(&self) -> Option<Instant> { self.retry_at }

    pub(super) fn refresh(&mut self, paths: Vec<PathBuf>, wake: &Arc<NativeWake>) -> bool {
        self.refresh_with_children(paths, None, wake)
    }

    pub(super) fn refresh_with_children(
        &mut self, paths: Vec<PathBuf>,
        children: Option<(&Path, prepared_sources::watch::ChildFilter)>,
        wake: &Arc<NativeWake>,
    ) -> bool {
        let child_root = children.map(|(root, _)| root.to_path_buf());
        let invalidated = self.dirty.swap(false, Ordering::AcqRel)
            || self.paths != paths || self.child_root != child_root;
        if !invalidated && !self.retry_at.is_some_and(|at| at <= Instant::now()) {
            return false;
        }
        self.paths = paths;
        self.child_root = child_root;
        if self.paths.is_empty() {
            self.watch = None;
            self.retry_at = None;
            return invalidated;
        }
        if self.watch.is_none() {
            let wake = Arc::clone(wake);
            let dirty = self.dirty.clone();
            self.watch = prepared_sources::watch::Watch::new(Arc::new(move |_| {
                dirty.store(true, Ordering::Release);
                wake.notify();
            })).ok();
        }
        let registered = self.watch.as_mut().is_some_and(|watch|
            watch.replace_with_children(&self.paths, children).is_ok());
        if !registered {
            self.watch = None;
        }
        self.retry_at = (!registered).then(|| Instant::now() + WATCH_RETRY);
        // A successful retry needs one new baseline. A failed retry on an
        // unchanged scope does not invalidate the last committed read model.
        invalidated || registered
    }
}

pub(super) fn publish_change(inner: &Inner) {
    inner.changes.mark(Domain::Runtime);
    inner.changes.mark_resources();
}

pub(super) fn maintenance_delay(inner: &Inner, process_live: bool, serviced_at: Instant) -> Option<Duration> {
    let settings = inner.settings.lock().unwrap_or_else(|error| error.into_inner());
    let mut configured = Vec::new();
    let mut needs_setup_maintenance = false;
    for game in &settings.catalogue {
        let Some(key) = crate::game_discovery::integration_key(game) else { continue; };
        let Some(id) = game["id"].as_str() else { continue; };
        if game["installed"] == true || game["executable"].is_string() || game["installPath"].is_string() {
            configured.push(id.to_owned());
            needs_setup_maintenance |= inner.setup.needs_external_setup(key);
        }
    }
    drop(settings);
    if configured.is_empty() { return None; }
    if !process_live { return Some(Duration::from_millis(500)); }
    let now = Instant::now();
    let setup = needs_setup_maintenance.then(|| {
        inner.context_setup_check.lock().unwrap_or_else(|p| p.into_inner())
            .unwrap_or(now) + Duration::from_secs(2)
    });
    let vehicle = inner.vehicles.lock().unwrap_or_else(|p| p.into_inner()).context_deadline(&configured, serviced_at);
    setup.into_iter().chain(vehicle).min().map(|at| at.saturating_duration_since(now))
}

#[cfg(all(test, any(target_os = "windows", target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn xplane_sdk_watch_wakes_only_for_its_selected_file() {
        let directory = crate::test_directory();
        let selected = directory.path().join("DataRefs.txt");
        let sibling = directory.path().join("unrelated.txt");
        std::fs::write(&selected, "old catalogue").unwrap();
        let changes = crate::game_read_models::signals::Signal::default();
        let wake = &changes.context_worker_wake;
        wake.bind_thread(thread::current()).unwrap();
        let mut watch = SourceWatch::default();
        assert!(watch.refresh(vec![selected.clone()], wake));
        assert!(watch.watch.is_some());
        assert!(!watch.refresh(vec![selected.clone()], wake));

        std::fs::write(sibling, "unrelated").unwrap();
        let quiet_until = Instant::now() + Duration::from_millis(150);
        while let Some(remaining) = quiet_until.checked_duration_since(Instant::now()) {
            assert!(!watch.dirty.load(Ordering::Acquire));
            thread::park_timeout(remaining);
        }

        let observed = wake.observed();
        std::fs::write(&selected, "new catalogue").unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while wake.observed() == observed {
            let remaining = deadline.checked_duration_since(Instant::now())
                .expect("SDK file write did not wake the native watcher");
            thread::park_timeout(remaining);
        }
        // Even if an unrelated wait consumes the token, the worker must drain again.
        thread::park_timeout(Duration::ZERO);
        assert!(!wake.wait_scheduled(observed, Some(Duration::ZERO)));
        assert_eq!(changes.context_wake.observed(), 0, "A file write does not replace the process scope");
        assert_eq!(changes.context_process_wake.observed(), 0);
        assert!(watch.refresh(vec![selected], wake));
        assert!(watch.refresh(Vec::new(), wake));
        assert!(watch.watch.is_none());
    }

    #[test]
    fn failed_registration_rearms_same_paths_after_parent_recovers() {
        let directory = crate::test_directory();
        let parent = directory.path().join("sdk");
        let selected = parent.join("DataRefs.txt");
        std::fs::write(&parent, "blocked directory").unwrap();
        let wake = Arc::default();
        let mut watch = SourceWatch::default();
        assert!(watch.refresh(vec![selected.clone()], &wake));
        assert!(!watch.active());
        assert!(watch.retry_at().is_some());
        assert!(!watch.refresh(vec![selected.clone()], &wake), "An early wake does not retry");

        let expired = Instant::now();
        watch.retry_at = Some(expired);
        assert!(!watch.refresh(vec![selected.clone()], &wake), "A failed retry keeps the same baseline");
        assert!(watch.retry_at().unwrap() > expired);

        std::fs::remove_file(&parent).unwrap();
        std::fs::create_dir(&parent).unwrap();
        std::fs::write(&selected, "first catalogue").unwrap();
        watch.retry_at = Some(Instant::now());
        assert!(watch.refresh(vec![selected.clone()], &wake));
        assert!(watch.active());
        assert!(watch.retry_at().is_none());
        assert!(!watch.refresh(vec![selected], &wake));
    }

    #[test]
    fn removing_the_scope_cancels_watch_retry() {
        let wake = Arc::default();
        let mut watch = SourceWatch::default();
        assert!(watch.refresh(vec![PathBuf::from("relative")], &wake));
        assert!(watch.retry_at().is_some());
        assert!(watch.refresh(Vec::new(), &wake));
        assert!(!watch.active());
        assert!(watch.retry_at().is_none());
    }
}
