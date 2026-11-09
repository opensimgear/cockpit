//! Exact file notifications for DCS catalogue baselines.
use super::{DcsCatalogueGuard, SOURCE_CHANGED};
use crate::games::ExactFileWatch;
use osg_streams::NativeWake;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc, Mutex, Weak,
};

#[derive(Default)]
struct State {
    invalidated: AtomicU8,
    wake: Mutex<Weak<NativeWake>>,
}

const CHANGED: u8 = 1;
const WATCH_LOST: u8 = 2;

pub(crate) struct CatalogueWatch {
    _owner: ExactFileWatch,
    state: Arc<State>,
}

impl CatalogueWatch {
    pub(crate) fn open(guard: &DcsCatalogueGuard) -> Result<Option<Self>, String> {
        let Ok(watch) = Self::register(guard) else { return Ok(None); };
        // A previously prepared decoder needs a new baseline after the
        // notification handles are armed. An event during that read retires it.
        guard.files.validate_metadata()?;
        guard.validate_content()?;
        watch.check()?;
        Ok(Some(watch))
    }

    fn register(guard: &DcsCatalogueGuard) -> Result<Self, String> {
        let state = Arc::<State>::default();
        let signal = Arc::clone(&state);
        let mut owner = ExactFileWatch::new(Arc::new(move |event| {
            // File changes and lost observation both retire this baseline.
            let reason = if event.is_ok() { CHANGED } else { WATCH_LOST };
            if signal.invalidated.compare_exchange(0, reason, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                if let Some(wake) = signal.wake.lock().unwrap_or_else(|error| error.into_inner()).upgrade() {
                    wake.notify();
                }
            }
        }))?;
        let paths = guard.files.paths().cloned().collect::<Vec<_>>();
        owner.replace(&paths)?;
        Ok(Self { _owner: owner, state })
    }

    pub(crate) fn check(&self) -> Result<(), String> {
        match self.state.invalidated.load(Ordering::Acquire) {
            CHANGED => Err(SOURCE_CHANGED.into()),
            WATCH_LOST => Err("DCS catalogue notifications were lost; prepare a new native reader.".into()),
            _ => Ok(()),
        }
    }

    pub(crate) fn set_wake(&self, wake: Option<&Arc<NativeWake>>) {
        *self.state.wake.lock().unwrap_or_else(|error| error.into_inner()) =
            wake.map_or_else(Weak::new, Arc::downgrade);
        if self.state.invalidated.load(Ordering::Acquire) != 0 {
            if let Some(wake) = wake { wake.notify(); }
        }
    }
}

impl Drop for CatalogueWatch {
    fn drop(&mut self) { self.set_wake(None); }
}

pub(crate) struct PassiveCatalogueGuard {
    guard: DcsCatalogueGuard,
    watch: Option<CatalogueWatch>,
}

impl PassiveCatalogueGuard {
    pub(crate) fn open(guard: DcsCatalogueGuard) -> Result<Self, String> {
        let watch = CatalogueWatch::open(&guard)?;
        Ok(Self { guard, watch })
    }

    #[cfg(test)]
    pub(crate) fn without_watch(guard: DcsCatalogueGuard) -> Self {
        Self { guard, watch: None }
    }

    pub(crate) fn subscribe(&self, wake: &Arc<NativeWake>) {
        if let Some(watch) = &self.watch {
            watch.set_wake(Some(wake));
        }
    }

    pub(crate) fn check_watch(&self) -> Result<(), String> {
        self.watch.as_ref().map_or(Ok(()), CatalogueWatch::check)
    }

    pub(crate) fn validate(&mut self) -> Result<(), String> {
        self.check_watch()?;
        self.guard.validate_at(false, self.watch.is_none())
    }
}

#[cfg(all(test, any(target_os = "windows", target_os = "linux", target_os = "macos")))]
#[path = "dcs_passive_watch_tests.rs"]
mod tests;
