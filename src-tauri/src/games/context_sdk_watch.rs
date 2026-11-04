//! Watch exact SDK catalogue paths on the native game context owner.
use super::*;
use crate::game_read_models::signals::Domain;

const XPLANE: [&str; 2] = ["x-plane-11", "x-plane-12"];

#[derive(Default)]
pub(super) struct SdkWatches([SdkWatch; 2]);

#[derive(Default)]
struct SdkWatch {
    path: Option<PathBuf>,
    watch: Option<prepared_sources::watch::Watch>,
    dirty: Arc<AtomicBool>,
}

impl SdkWatches {
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
            if watch.refresh(path) {
                inner.setup.invalidate_xplane_catalogue(id);
                changed = true;
            }
        }
        changed
    }
}

impl SdkWatch {
    fn refresh(&mut self, path: Option<PathBuf>) -> bool {
        if !self.dirty.swap(false, Ordering::AcqRel) && self.path == path {
            return false;
        }
        self.path = path;
        if let Some(path) = &self.path {
            if self.watch.is_none() {
                let worker = thread::current();
                let dirty = self.dirty.clone();
                self.watch = prepared_sources::watch::Watch::new(Arc::new(move |_| {
                    dirty.store(true, Ordering::Release);
                    worker.unpark();
                })).ok();
            }
            if let Some(watch) = &mut self.watch {
                if watch.replace(std::slice::from_ref(path)).is_err() {
                    self.watch = None;
                }
            }
        } else {
            self.watch = None;
        }
        true
    }
}

pub(super) fn publish_change(inner: &Inner) {
    inner.changes.mark(Domain::Runtime);
}

pub(super) fn maintenance_delay(inner: &Inner) -> Option<Duration> {
    inner.settings.lock().unwrap_or_else(|error| error.into_inner())
        .catalogue.iter().any(|game| {
            crate::game_discovery::integration_key(game).is_some()
                && (game["installed"] == true || game["executable"].is_string()
                    || game["installPath"].is_string())
        }).then_some(Duration::from_millis(500))
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
        let mut watch = SdkWatch::default();
        assert!(watch.refresh(Some(selected.clone())));
        assert!(watch.watch.is_some());
        assert!(!watch.refresh(Some(selected.clone())));

        std::fs::write(sibling, "unrelated").unwrap();
        let quiet_until = Instant::now() + Duration::from_millis(150);
        while let Some(remaining) = quiet_until.checked_duration_since(Instant::now()) {
            assert!(!watch.dirty.load(Ordering::Acquire));
            thread::park_timeout(remaining);
        }

        std::fs::write(&selected, "new catalogue").unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !watch.dirty.load(Ordering::Acquire) {
            let remaining = deadline.checked_duration_since(Instant::now())
                .expect("SDK file write did not wake the native watcher");
            thread::park_timeout(remaining);
        }
        assert!(watch.refresh(Some(selected)));
        assert!(watch.refresh(None));
        assert!(watch.watch.is_none());
    }
}
