use super::installed;
use notify::{event::{MetadataKind, ModifyKind}, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use osg_streams::NativeWake;
use std::{path::Path, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}};

#[derive(Default)]
pub(super) struct Changes {
    dirty: AtomicBool,
    lost: AtomicBool,
    repair_attempted: AtomicBool,
    issue: Mutex<Option<String>>,
}

impl Changes {
    pub(super) fn arm(self: &Arc<Self>, directory: &Path, wake: &Arc<NativeWake>) -> Option<RecommendedWatcher> {
        self.dirty.store(true, Ordering::Release);
        *self.issue.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        let changes = self.clone();
        let wake = wake.clone();
        let result = notify::recommended_watcher(move |event| changes.received(event, &wake))
            .and_then(|mut watcher| {
                watcher.watch(directory, RecursiveMode::NonRecursive)?;
                Ok(watcher)
            });
        match result {
            Ok(watcher) => Some(watcher),
            Err(error) => {
                self.report(&format!("Cannot watch optional Windows drivers: {error}"));
                None
            }
        }
    }

    fn received(&self, result: notify::Result<Event>, wake: &NativeWake) {
        match result {
            Ok(event) if event.need_rescan() => {}
            Ok(event) if driver_change(&event) => {
                self.repair_attempted.store(false, Ordering::Release);
                *self.issue.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            }
            Ok(_) => return,
            Err(error) => {
                self.report(&format!("Optional Windows driver notifications were interrupted: {error}"));
                self.lost.store(true, Ordering::Release);
            }
        }
        self.dirty.store(true, Ordering::Release);
        wake.notify();
    }

    fn report(&self, message: &str) {
        *self.issue.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(message.chars().take(512).collect());
    }

    pub(super) fn needs_repair(&self) -> bool {
        self.lost.swap(false, Ordering::AcqRel) && !self.repair_attempted.swap(true, Ordering::AcqRel)
    }

    pub(super) fn invalidate_versions(&self, directory: &Path) {
        if self.dirty.swap(false, Ordering::AcqRel) { installed::invalidate_versions(directory); }
    }

    pub(super) fn issue(&self) -> Option<String> {
        self.issue.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
}

fn driver_change(event: &Event) -> bool {
    if matches!(event.kind, EventKind::Access(_) | EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime))) {
        return false;
    }
    event.paths.iter().filter_map(|path| path.file_name()?.to_str()).any(|name| {
        name.eq_ignore_ascii_case(installed::VJOY_FILE) || name.eq_ignore_ascii_case(installed::HIDHIDE_FILE)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{Runtime, DRIVERS_TOPIC, TOPIC};
    use notify::event::Flag;
    use serde_json::json;

    #[test]
    fn driver_only_commit_leaves_general_inventory_readers_quiet() {
        let runtime = Runtime::new(true);
        let observations = crate::observations::Runtime::new();
        runtime.observe(&observations).unwrap();
        let broker = observations.broker();
        let mut inventory = broker.subscribe("devices", "ui", TOPIC).unwrap();
        let mut drivers = broker.subscribe("driver-settings", "mcp", DRIVERS_TOPIC).unwrap();
        inventory.try_recv().unwrap().unwrap();
        drivers.try_recv().unwrap().unwrap();
        let mut value = json!({"observedAt":1,"drivers":{"supported":true,"vjoy":{"installed":false}},"virtualDevices":[]});
        runtime.commit(value.clone());
        inventory.try_recv().unwrap().unwrap();
        drivers.try_recv().unwrap().unwrap();
        value["drivers"]["vjoy"] = json!({"installed":true,"version":"1.0"});
        runtime.commit(value.clone());
        assert!(inventory.try_recv().unwrap().is_none());
        assert_eq!(drivers.try_recv().unwrap().unwrap().payload, Some(value["drivers"].clone()));
        assert_eq!(runtime.snapshot.lock().unwrap().as_ref().unwrap().value["drivers"], value["drivers"]);
        runtime.commit(value.clone());
        assert!(drivers.try_recv().unwrap().is_none());
        value["virtualDevices"] = json!([{"id":"vjoy:1"}]);
        runtime.commit(value);
        assert!(inventory.try_recv().unwrap().is_some());
        assert!(drivers.try_recv().unwrap().is_none());
    }

    #[test]
    fn exact_changes_ignore_reads_and_allow_one_rearm_per_loss() {
        let changes = Changes::default();
        let wake = NativeWake::default();
        for name in ["other.sys", "vjoy.sys.backup", "drivers"] {
            changes.received(Ok(Event::new(EventKind::Any).add_path(name.into())), &wake);
        }
        changes.received(Ok(Event::new(EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime)))
            .add_path(installed::VJOY_FILE.into())), &wake);
        assert_eq!(wake.observed(), 0);
        changes.received(Ok(Event::new(EventKind::Other).set_flag(Flag::Rescan)), &wake);
        assert_eq!(wake.observed(), 1);
        assert!(!changes.needs_repair());
        changes.received(Err(notify::Error::generic("watch lost")), &wake);
        assert!(changes.needs_repair());
        changes.received(Err(notify::Error::generic("still unavailable")), &wake);
        assert!(!changes.needs_repair());
        assert!(changes.issue().unwrap().contains("still unavailable"));
        changes.received(Ok(Event::new(EventKind::Any).add_path("HIDHIDE.SYS".into())), &wake);
        assert!(changes.issue().is_none());
        changes.received(Err(notify::Error::generic("new loss")), &wake);
        assert!(changes.needs_repair());
    }

    #[cfg(windows)]
    #[test]
    fn windows_reports_driver_file_creation_in_a_private_directory() {
        let root = crate::test_directory();
        let changes = Arc::new(Changes::default());
        let wake = Arc::new(NativeWake::default());
        wake.bind_thread(std::thread::current()).unwrap();
        assert!(changes.arm(&root.path().join("absent"), &wake).is_none());
        assert!(changes.issue().is_some());
        let _watch = changes.arm(root.path(), &wake).unwrap();
        changes.invalidate_versions(root.path());
        let observed = wake.observed();
        std::fs::write(root.path().join(installed::VJOY_FILE), b"fixture").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while wake.observed() == observed {
            let remaining = deadline.checked_duration_since(std::time::Instant::now())
                .expect("Optional driver creation must wake the owner");
            wake.wait(observed, remaining);
        }
        assert!(changes.dirty.load(Ordering::Acquire));
        assert!(changes.issue().is_none());
    }
}
