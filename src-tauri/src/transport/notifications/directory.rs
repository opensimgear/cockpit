//! Event-driven rearming for a replaceable/missing advertisement directory.
//! Native callbacks only coalesce work; watch changes and I/O run on this owner.
use super::*;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher, WatcherKind};
use std::{path::{Path, PathBuf}, sync::{atomic::{AtomicBool, Ordering}, mpsc, Mutex}, thread::{self, JoinHandle}};

const MAX_ANCESTORS: usize = 64;
#[derive(Default)]
struct Pending { dirty: AtomicBool, lost: AtomicBool, closed: AtomicBool, wake: NativeWake }
struct FailureGuard { pending: Arc<Pending>, error: Arc<Mutex<Option<String>>>, wake: Arc<NativeWake> }
impl Drop for FailureGuard {
    fn drop(&mut self) {
        if !self.pending.closed.swap(true, Ordering::AcqRel) {
            *self.error.lock().unwrap_or_else(|p|p.into_inner()) = Some("Local gateway notification owner stopped unexpectedly.".into());
            self.wake.notify();
        }
    }
}
impl Pending {
    fn event(&self, event: notify::Result<Event>, target: &Path) {
        if self.closed.load(Ordering::Acquire) { return; }
        let changed = match event {
            Err(_) => { self.lost.store(true, Ordering::Release); true },
            Ok(event) if event.need_rescan() => { self.lost.store(true, Ordering::Release); true },
            Ok(event) => !matches!(event.kind, EventKind::Access(_)) && event.paths.iter().any(|path|
                target.starts_with(path) || path.parent() == Some(target)),
        };
        if changed && !self.dirty.swap(true, Ordering::AcqRel) { self.wake.notify(); }
    }
}
pub(super) struct DirectoryNotifications {
    pending: Arc<Pending>, error: Arc<Mutex<Option<String>>>, worker: Option<JoinHandle<()>>,
}
impl DirectoryNotifications {
    pub(super) fn start(target: PathBuf, wake: Arc<NativeWake>) -> Result<Self, String> {
        if !target.is_absolute() || target.components().any(|part| matches!(part, std::path::Component::ParentDir))
            || target.ancestors().count() > MAX_ANCESTORS {
            return Err("Local gateway advertisement notification path is invalid or too deep.".into());
        }
        if matches!(RecommendedWatcher::kind(), WatcherKind::PollWatcher | WatcherKind::NullWatcher) {
            return Err("Native local gateway advertisement notifications are unsupported.".into());
        }
        // macOS /tmp is a system symlink. Resolve the existing directory prefix
        // once, preserving missing suffixes so callback paths match native
        // physical paths. This supplies notification scope, never read authority.
        let target = resolve_target(&target)?;
        let pending = Arc::new(Pending::default());
        let error = Arc::new(Mutex::new(None));
        let shared = pending.clone(); let failure = error.clone();
        let (ready, started) = mpsc::sync_channel(1);
        let worker = thread::Builder::new().name("gateway-advertisements".into()).spawn(move || {
            let _failure_guard = FailureGuard { pending: shared.clone(), error: failure.clone(), wake: wake.clone() };
            let _ = shared.wake.bind_thread(thread::current());
            let mut watcher = match arm(&target, &shared) {
                Ok(watcher) => { let _ = ready.send(Ok(())); watcher },
                Err(error) => { shared.closed.store(true, Ordering::Release); let _ = ready.send(Err(error)); return; },
            };
            loop {
                let observed = shared.wake.observed();
                if shared.closed.load(Ordering::Acquire) { break; }
                if shared.dirty.swap(false, Ordering::AcqRel) {
                    if shared.lost.swap(false, Ordering::AcqRel) {
                        *failure.lock().unwrap_or_else(|p|p.into_inner()) = Some("Local gateway advertisement notifications were lost. Restart the workspace observer.".into());
                        wake.notify(); break;
                    }
                    match arm(&target, &shared) {
                        Ok(replacement) => watcher = replacement,
                        Err(error) => { *failure.lock().unwrap_or_else(|p|p.into_inner()) = Some(error); wake.notify(); break; },
                    }
                    wake.notify();
                }
                shared.wake.wait_scheduled(observed, None);
            }
            shared.closed.store(true, Ordering::Release);
            drop(watcher);
        }).map_err(|error| format!("Cannot start local gateway notification owner: {error}"))?;
        match started.recv() {
            Ok(Ok(())) => Ok(Self { pending, error, worker: Some(worker) }),
            Ok(Err(error)) => { let _ = worker.join(); Err(error) },
            Err(_) => { let _ = worker.join(); Err("Local gateway notification owner stopped during startup.".into()) },
        }
    }
    pub(super) fn error(&self) -> Option<String> { self.error.lock().unwrap_or_else(|p|p.into_inner()).clone() }
    pub(super) fn shutdown(&mut self) {
        self.pending.closed.store(true, Ordering::Release); self.pending.wake.notify();
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}
impl Drop for DirectoryNotifications { fn drop(&mut self) { self.shutdown(); } }

fn arm(target: &Path, pending: &Arc<Pending>) -> Result<RecommendedWatcher, String> {
    let shared = pending.clone(); let filter = target.to_owned();
    let mut watcher = notify::recommended_watcher(move |event| shared.event(event, &filter))
        .map_err(|_| "Cannot create local gateway advertisement notifications.".to_owned())?;
    // Register every existing ancestor nonrecursively. Parent replacement and
    // creation of missing intermediate directories therefore trigger rearming.
    // The replacement is fully registered before its predecessor is dropped.
    for ancestor in target.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.is_dir() => {
                if let Err(error) = watcher.watch(ancestor, RecursiveMode::NonRecursive) {
                    // A deleted directory can disappear between inspection and
                    // registration. Its higher existing ancestors still cover
                    // replacement; this is not a timed registration retry.
                    let missing = matches!(&error.kind, notify::ErrorKind::PathNotFound)
                        || matches!(&error.kind, notify::ErrorKind::Io(error) if error.kind() == std::io::ErrorKind::NotFound);
                    if !missing { return Err("Cannot watch a local gateway advertisement directory.".into()); }
                }
            },
            Ok(_) => return Err("Local gateway advertisement ancestor is not a directory.".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(_) => return Err("Cannot inspect a local gateway advertisement notification directory.".into()),
        }
    }
    Ok(watcher)
}

fn resolve_target(target: &Path) -> Result<PathBuf, String> {
    for ancestor in target.ancestors() {
        match std::fs::canonicalize(ancestor) {
            Ok(existing) => return Ok(existing.join(target.strip_prefix(ancestor)
                .map_err(|_| "Invalid local gateway advertisement scope.")?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(_) => return Err("Cannot resolve the local gateway advertisement notification scope.".into()),
        }
    }
    Err("Local gateway advertisement notifications have no existing ancestor.".into())
}

#[cfg(test)]
mod tests;
