use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher, WatcherKind};
use osg_streams::NativeWake;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Component, Path, PathBuf},
    sync::{atomic::{AtomicBool, Ordering}, Arc},
};

#[derive(Clone)]
pub(super) struct Source(PathBuf);

#[derive(Default)]
pub(super) struct Stop {
    closed: AtomicBool,
    wake: Arc<NativeWake>,
}

impl Stop {
    pub(super) fn new() -> io::Result<Self> { Ok(Self::default()) }

    pub(super) fn signal(&self) {
        self.closed.store(true, Ordering::Release);
        self.wake.notify();
    }
}

#[derive(Default, PartialEq, Eq)]
struct Scope {
    entries: BTreeSet<PathBuf>,
    directories: BTreeMap<PathBuf, DirectoryIdentity>,
}

#[cfg(unix)]
type DirectoryIdentity = (u64, u64);
#[cfg(not(unix))]
type DirectoryIdentity = ();

fn directory_identity(_metadata: &fs::Metadata) -> DirectoryIdentity {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (_metadata.dev(), _metadata.ino())
    }
}

impl Scope {
    fn resolve(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() { return Err(io::Error::other("Auto-start path is not absolute.")); }
        let mut scope = Self::default();
        scope.collect(path, 0)?;
        Ok(scope)
    }

    fn collect(&mut self, path: &Path, links: usize) -> io::Result<()> {
        if links >= 40 || path.components().count() > 64 {
            return Err(io::Error::other("Auto-start path is too deep or has a symlink loop."));
        }
        let mut components = path.components();
        let mut current = PathBuf::new();
        while let Some(part) = components.next() {
            match part {
                Component::Prefix(prefix) => { current.push(prefix.as_os_str()); continue; }
                Component::CurDir => continue,
                Component::ParentDir => { current.pop(); continue; }
                _ => {}
            }
            let next = current.join(part.as_os_str());
            self.entries.insert(next.clone());
            match fs::symlink_metadata(&next) {
                Ok(metadata) if metadata.is_symlink() => {
                    match fs::read_link(&next) {
                        Ok(link) => return self.collect(&current.join(link).join(components.as_path()), links + 1),
                        Err(error) if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::InvalidInput) => break,
                        Err(error) => return Err(error),
                    }
                }
                Ok(metadata) if metadata.is_dir() => { self.directories.insert(next.clone(), directory_identity(&metadata)); }
                Ok(_) => break,
                Err(error) if error.kind() == io::ErrorKind::NotFound => break,
                Err(error) => return Err(error),
            }
            current = next;
        }
        Ok(())
    }

    fn changed(&self, event: notify::Result<notify::Event>) -> bool {
        match event {
            Err(_) => true,
            Ok(event) if event.need_rescan() => true,
            Ok(event) => !matches!(event.kind, EventKind::Access(_))
                && event.paths.iter().any(|path| self.entries.contains(path)),
        }
    }
}

impl Source {
    pub(super) fn new(path: PathBuf) -> Self { Self(path) }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(super) fn registration(app_name: &str) -> Result<Self, String> {
        let home = dirs::home_dir().ok_or("Cannot locate the auto-start home directory.")?;
        // Match auto-launch's pinned XDG and LaunchAgent paths, including its
        // deliberate use of the home directory for the Linux XDG location.
        #[cfg(target_os = "linux")]
        let entry = home.join(".config").join("autostart").join(format!("{app_name}.desktop"));
        #[cfg(target_os = "macos")]
        let entry = home.join("Library").join("LaunchAgents").join(format!("{app_name}.plist"));
        Ok(Self::new(entry))
    }

    fn arm(&self, dirty: &Arc<AtomicBool>, wake: &Arc<NativeWake>) -> io::Result<RecommendedWatcher> {
        for _ in 0..8 {
            let scope = Arc::new(Scope::resolve(&self.0)?);
            let filter = scope.clone();
            let dirty = dirty.clone();
            let wake = wake.clone();
            let mut watcher = notify::recommended_watcher(move |event| {
                if filter.changed(event) && !dirty.swap(true, Ordering::AcqRel) { wake.notify(); }
            }).map_err(io::Error::other)?;
            let registration = scope.directories.keys().try_for_each(|dir| watcher.watch(dir, RecursiveMode::NonRecursive));
            // Changes between discovery and registration are covered by rebuilding
            // the watch set before the baseline. An identical set can read now.
            if *scope != Scope::resolve(&self.0)? { continue; }
            registration.map_err(io::Error::other)?;
            return Ok(watcher);
        }
        Err(io::Error::other("Auto-start directories changed during watch registration."))
    }

    pub(super) fn run(&self, stop: &Stop, mut capture: impl FnMut()) -> io::Result<()> {
        if matches!(RecommendedWatcher::kind(), WatcherKind::PollWatcher | WatcherKind::NullWatcher) {
            return Err(io::Error::other("Native auto-start file notifications are unavailable."));
        }
        stop.wake.bind_thread(std::thread::current()).map_err(|_| io::Error::other("Auto-start worker is already bound."))?;
        let dirty = Arc::new(AtomicBool::new(false));
        let mut watcher = self.arm(&dirty, &stop.wake)?;
        capture();
        loop {
            let observed = stop.wake.observed();
            if stop.closed.load(Ordering::Acquire) { break; }
            if dirty.swap(false, Ordering::AcqRel) {
                watcher = self.arm(&dirty, &stop.wake)?;
                capture();
            }
            stop.wake.wait_scheduled(observed, None);
        }
        drop(watcher);
        Ok(())
    }
}

#[cfg(test)]
#[path = "autostart_files_tests.rs"]
mod tests;
