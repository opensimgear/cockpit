//! Exact file evidence for the passive DCS reader.
use super::{DcsCatalogueGuard, TelemetryCatalogue, SOURCE_CHANGED};
use crate::games::ExactFileWatch;
use osg_streams::NativeWake;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, Weak,
};

#[derive(Default)]
struct State {
    invalidated: AtomicBool,
    wake: Mutex<Weak<NativeWake>>,
}

struct Watch {
    _owner: ExactFileWatch,
    state: Arc<State>,
}

impl Watch {
    fn open(guard: &DcsCatalogueGuard) -> Result<Self, String> {
        let state = Arc::<State>::default();
        let signal = Arc::clone(&state);
        let mut owner = ExactFileWatch::new(Arc::new(move |_| {
            // File changes and lost observation both retire this baseline.
            if !signal.invalidated.swap(true, Ordering::AcqRel) {
                if let Some(wake) = signal.wake.lock().unwrap_or_else(|error| error.into_inner()).upgrade() {
                    wake.notify();
                }
            }
        }))?;
        let paths = guard.files.iter().map(|(path, _)| path.clone()).collect::<Vec<_>>();
        owner.replace(&paths)?;
        Ok(Self { _owner: owner, state })
    }

    fn check(&self) -> Result<(), String> {
        if self.state.invalidated.load(Ordering::Acquire) {
            return Err(SOURCE_CHANGED.into());
        }
        Ok(())
    }
}

pub(crate) struct PassiveCatalogueGuard {
    guard: DcsCatalogueGuard,
    watch: Option<Watch>,
}

impl PassiveCatalogueGuard {
    pub(crate) fn open(guard: DcsCatalogueGuard) -> Result<Self, String> {
        let watch = Watch::open(&guard).ok();
        if let Some(watch) = &watch {
            // Confirm the already prepared decoder against a baseline read
            // after registration. An event during that read rejects the proof.
            let confirmed = TelemetryCatalogue::load(&guard.path, &guard.module)?;
            if confirmed.revision != guard.revision || confirmed.files != guard.files {
                return Err(SOURCE_CHANGED.into());
            }
            watch.check()?;
        }
        Ok(Self { guard, watch })
    }

    #[cfg(test)]
    pub(crate) fn without_watch(guard: DcsCatalogueGuard) -> Self {
        Self { guard, watch: None }
    }

    pub(crate) fn subscribe(&self, wake: &Arc<NativeWake>) {
        if let Some(watch) = &self.watch {
            *watch.state.wake.lock().unwrap_or_else(|error| error.into_inner()) = Arc::downgrade(wake);
            if watch.state.invalidated.load(Ordering::Acquire) {
                wake.notify();
            }
        }
    }

    pub(crate) fn check_watch(&self) -> Result<(), String> {
        self.watch.as_ref().map_or(Ok(()), Watch::check)
    }

    pub(crate) fn validate(&mut self) -> Result<(), String> {
        self.check_watch()?;
        self.guard.validate_at(false, self.watch.is_none())
    }
}

#[cfg(all(test, any(target_os = "windows", target_os = "linux", target_os = "macos")))]
#[path = "dcs_passive_watch_tests.rs"]
mod tests;
