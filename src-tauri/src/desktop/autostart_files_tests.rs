use super::*;
use notify::Event;
use std::{sync::mpsc, thread, time::{Duration, Instant}};

fn with_source(entry: &Path, checks: impl FnOnce(&dyn Fn(bool))) {
    let source = Source::new(entry.to_path_buf());
    let stop = Arc::new(Stop::new().unwrap());
    let worker_stop = stop.clone();
    let entry = entry.to_path_buf();
    let (changes, captures) = mpsc::channel();
    let worker = thread::spawn(move || {
        source.run(&worker_stop, || { let _ = changes.send(entry.exists()); })
    });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| checks(&|expected| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while captures.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap() != expected {}
    })));
    stop.signal();
    worker.join().unwrap().unwrap();
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}

#[test]
fn exact_entry_follows_nested_creation_parent_rename_and_recreation() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("config").join("autostart");
    let entry = parent.join("OpenSimGear Cockpit.desktop");
    let moved = root.path().join("old");
    with_source(&entry, |captured| {
        captured(false);
        fs::create_dir_all(&parent).unwrap();
        fs::write(&entry, "fixture").unwrap();
        captured(true);
        fs::rename(&parent, moved).unwrap();
        captured(false);
        fs::create_dir(&parent).unwrap();
        fs::write(&entry, "replacement").unwrap();
        captured(true);
        fs::remove_file(&entry).unwrap();
        captured(false);
    });
}

#[cfg(unix)]
#[test]
fn dangling_relative_link_tracks_referent_and_link_retargeting() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let links = root.path().join("config");
    let real = root.path().join("registration");
    fs::create_dir(&links).unwrap();
    let entry = links.join("OpenSimGear Cockpit.desktop");
    symlink("../registration", &entry).unwrap();
    with_source(&entry, |captured| {
        captured(false);
        fs::write(&real, "fixture").unwrap();
        captured(true);
        fs::remove_file(&real).unwrap();
        captured(false);
        fs::write(root.path().join("new"), "replacement").unwrap();
        let replacement = links.join("replacement");
        symlink("../new", &replacement).unwrap();
        fs::rename(replacement, &entry).unwrap();
        captured(true);
    });
}

#[test]
fn scope_ignores_siblings_and_reads_and_recaptures_on_notification_loss() {
    let root = tempfile::tempdir().unwrap();
    let entry = root.path().join("autostart.desktop");
    let scope = Scope::resolve(&entry).unwrap();
    assert!(scope.changed(Ok(Event::new(EventKind::Any).add_path(entry.clone()))));
    assert!(!scope.changed(Ok(Event::new(EventKind::Any).add_path(root.path().join("other.desktop")))));
    assert!(!scope.changed(Ok(Event::new(EventKind::Access(notify::event::AccessKind::Read)).add_path(entry))));
    assert!(scope.changed(Ok(Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan))));
    assert!(scope.changed(Err(notify::Error::generic("watch lost"))));
    assert!(Scope::resolve(Path::new("relative")).is_err());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn registration_keeps_the_plugin_package_name_and_platform_suffix() {
    let entry = Source::registration("OpenSimGear Cockpit").unwrap().0;
    #[cfg(target_os = "linux")]
    assert!(entry.ends_with(".config/autostart/OpenSimGear Cockpit.desktop"));
    #[cfg(target_os = "macos")]
    assert!(entry.ends_with("Library/LaunchAgents/OpenSimGear Cockpit.plist"));
}
