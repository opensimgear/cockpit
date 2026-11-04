use super::Hub;
use osg_streams::NativeWake;
use std::sync::{Arc, Mutex, Weak};

const MAX_OBSERVERS: usize = 64;

#[derive(Clone, Default)]
pub(super) struct Signal(Arc<Mutex<Vec<Weak<NativeWake>>>>);

impl Signal {
    pub(super) fn observe(&self, wake: Arc<NativeWake>) {
        let mut watchers = self.0.lock().unwrap_or_else(|p| p.into_inner());
        watchers.retain(|watch| watch.strong_count() > 0);
        let weak = Arc::downgrade(&wake);
        if watchers.len() < MAX_OBSERVERS && !watchers.iter().any(|old| old.ptr_eq(&weak)) {
            watchers.push(weak);
            wake.notify();
        }
    }

    pub(super) fn notify(&self) {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).retain(|watch| {
            if let Some(wake) = watch.upgrade() {
                wake.notify();
                true
            } else {
                false
            }
        });
    }
}

impl Hub {
    /// Notifications carry no credentials, transport frames, or input values.
    pub fn observe_presence(&self, wake: Arc<NativeWake>) {
        self.presence.observe(wake);
    }

    /// Wake roster readers after a companion transport commits visible status.
    pub fn notify_presence(&self) {
        self.presence.notify();
    }

    /// Wake transport discovery when retained pairing or publication state changes.
    pub fn observe_record_changes(&self, wake: Arc<NativeWake>) {
        self.record_changes.observe(wake);
    }
}
