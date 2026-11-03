//! Source-owned artwork projection. Reads never schedule preparation or I/O.
use super::*;
use crate::game_artwork::PreparedLocalArtwork;
use std::sync::Condvar;
mod watch;

#[derive(Default)]
struct State {
    requested: u64,
    handled: u64,
    closed: bool,
    artwork: PreparedLocalArtwork,
    error: Option<String>,
    paths: Option<Arc<Vec<PathBuf>>>,
    invalidated: bool,
    watch_error: Option<String>,
}
#[derive(Default)]
pub(super) struct Source {
    state: Mutex<State>,
    completed: Condvar,
    wake: osg_streams::NativeWake,
    changes: std::sync::OnceLock<std::sync::Weak<crate::game_read_models::signals::Signal>>,
    #[cfg(test)]
    pause: Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
}
impl Source {
    fn changed(&self) {
        if let Some(changes) = self.changes.get().and_then(std::sync::Weak::upgrade) {
            changes.mark(crate::game_read_models::signals::Domain::Artwork);
        }
    }
    pub(super) fn request(&self, invalidate: bool, loss: Option<String>) -> Option<u64> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.closed { return None; }
        let Some(next) = state.requested.checked_add(1) else {
            state.closed = true;
            state.artwork = PreparedLocalArtwork::default();
            state.error = Some("Artwork source generation exhausted. Reopen Cockpit.".into());
            self.completed.notify_all();
            self.wake.notify();
            self.changed();
            return None;
        };
        state.requested = next;
        state.invalidated |= invalidate;
        let changed = loss.is_some() && state.error != loss;
        if loss.is_some() { state.error = loss; }
        drop(state);
        if changed { self.changed(); }
        self.wake.notify();
        Some(next)
    }
    pub(super) fn refresh(&self, invalidate: bool) {
        let Some(requested) = self.request(invalidate, None) else { return; };
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        while !state.closed && state.handled < requested {
            state = self.completed.wait(state).unwrap_or_else(|p| p.into_inner());
        }
    }
    pub(super) fn snapshot(&self) -> PreparedLocalArtwork {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).artwork.clone()
    }
    pub(super) fn error(&self) -> Option<String> {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).error.clone()
    }
    fn pending(&self) -> Option<u64> {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        (!state.closed && state.handled < state.requested).then_some(state.requested)
    }
    pub(super) fn close(&self, error: Option<String>) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let changed = !state.artwork.same_images(&PreparedLocalArtwork::default()) || state.error != error;
        state.closed = true;
        state.artwork = PreparedLocalArtwork::default();
        state.error = error;
        drop(state);
        if changed { self.changed(); }
        self.completed.notify_all();
        self.wake.notify();
    }
}

impl GamesRuntime {
    pub(super) fn start_prepared_sources(&self) {
        let _ = self.inner.prepared_sources.changes.set(Arc::downgrade(&self.inner.changes));
        let inner = self.inner.clone();
        let failed = inner.clone();
        let worker = thread::Builder::new().name("game-artwork-source".into()).spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(inner)));
            if result.is_err() {
                failed.prepared_sources.close(Some("Artwork source preparation failed. Reopen Cockpit.".into()));
            }
        });
        match worker {
            Ok(worker) => *self.prepared_sources_worker.lock().unwrap_or_else(|p| p.into_inner()) = Some(worker),
            Err(error) => self.inner.prepared_sources.close(Some(format!("Cannot start artwork source: {error}"))),
        }
    }
}

fn run(inner: Arc<Inner>) {
    let source = &inner.prepared_sources;
    let _ = source.wake.bind_thread(thread::current());
    let weak = Arc::downgrade(&inner);
    let changed = Arc::new(move |event: Result<(), String>| {
        if let Some(inner) = weak.upgrade() {
            inner.prepared_sources.request(true, event.err());
        }
    });
    let mut watch = watch::Watch::new(changed.clone());
    loop {
        let observed = source.wake.observed();
        if inner.closed.load(Ordering::Acquire) { break; }
        if let Some(generation) = source.pending() {
            // A failed native watch is retried only on another real source or
            // committed change, never by a state-refresh timer.
            if watch.is_err() { watch = watch::Watch::new(changed.clone()); }
            prepare(&inner, generation, &mut watch);
        }
        if source.state.lock().unwrap_or_else(|p| p.into_inner()).closed { break; }
        source.wake.wait_scheduled(observed, None);
    }
}

fn prepare(inner: &Inner, generation: u64, watch: &mut Result<watch::Watch, String>) {
    let (catalogue, revision, metadata_error) = {
        let settings = inner.settings.lock().unwrap_or_else(|p| p.into_inner());
        (settings.catalogue.clone(), settings.saved.revision, settings.metadata_error.clone())
    };
    let paths = crate::game_artwork::local_source_paths(inner.installations.root(), &catalogue);
    let (needs_files, previous, previous_watch_error) = {
        let state = inner.prepared_sources.state.lock().unwrap_or_else(|p| p.into_inner());
        (state.invalidated || state.watch_error.is_some() || paths.as_ref().ok() != state.paths.as_deref(), state.artwork.clone(), state.watch_error.clone())
    };
    let watch_error = if needs_files {
        paths.as_ref().map_err(Clone::clone).and_then(|paths| watch.as_mut().map_err(|error| error.clone())?.replace(paths)).err()
    } else { previous_watch_error };
    #[cfg(test)]
    if let Some((entered, release)) = inner.prepared_sources.pause.lock().unwrap_or_else(|p| p.into_inner()).take() {
        entered.send(()).unwrap();
        release.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    let prepared = if needs_files { crate::game_artwork::prepare_local(inner.installations.root(), &catalogue) } else { Ok(previous) };
    let error = metadata_error.or_else(|| prepared.as_ref().err().cloned()).or_else(|| watch_error.clone());
    let artwork = prepared.unwrap_or_default();
    // Capture target JSON without locks; only the small exact target replacement
    // is admitted behind the same library/discovery/source boundary below.
    let discovery = inner.discovery.lock().unwrap_or_else(|p| p.into_inner()).clone();
    let mut decorated = catalogue.clone();
    artwork.decorate(&mut decorated);
    let targets = json!({"catalogue":decorated,"discovery":discovery});
    let settings = inner.settings.lock().unwrap_or_else(|p| p.into_inner());
    let current = inner.discovery.lock().unwrap_or_else(|p| p.into_inner());
    let mut state = inner.prepared_sources.state.lock().unwrap_or_else(|p| p.into_inner());
    if state.closed || inner.closed.load(Ordering::Acquire) || state.requested != generation { return; }
    if settings.saved.revision != revision || settings.catalogue != catalogue {
        drop(state); drop(current); drop(settings);
        inner.prepared_sources.request(false, None);
        return;
    }
    // Discovery changes do not invalidate local images; they do invalidate the
    // captured online target set. Its completion source will request another pass.
    let mut owner_error_changed = false;
    if current.scan_id == discovery.scan_id && current.status == discovery.status && current.revision == discovery.revision {
        if let Some(owner) = &inner.artwork_owner {
            let error = owner.replace(&targets).err();
            let mut previous = inner.artwork_error.lock().unwrap_or_else(|p| p.into_inner());
            owner_error_changed = *previous != error;
            *previous = error;
        }
    }
    let changed = owner_error_changed || !state.artwork.same_images(&artwork) || state.error != error;
    state.artwork = artwork;
    state.error = error;
    state.watch_error = watch_error;
    state.paths = paths.ok().map(Arc::new);
    state.invalidated = false;
    state.handled = generation;
    drop(state); drop(current); drop(settings);
    if changed { inner.prepared_sources.changed(); }
    inner.prepared_sources.completed.notify_all();
}

impl Inner {
    /// Called only by startup and completed source/metadata work, after outer
    /// gateway admission is released. Passive getters cannot enter this path.
    pub(super) fn prepare_online_artwork(&self) {
        self.prepare_artwork(true);
    }
    pub(super) fn prepare_committed_artwork(&self) {
        self.prepare_artwork(false);
    }
    fn prepare_artwork(&self, invalidate: bool) {
        #[cfg(test)]
        {
            let hook = self.artwork_prepare_hook.lock().unwrap_or_else(|p| p.into_inner()).clone();
            if let Some(hook) = hook { hook(); }
        }
        if !self.closed.load(Ordering::Acquire) { self.prepared_sources.refresh(invalidate); }
    }
}

#[cfg(test)]
mod tests;
