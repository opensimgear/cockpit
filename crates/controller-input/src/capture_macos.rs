//! Controller capture worker woken by retained IOHID callbacks.
use super::*;
use osg_streams::NativeWake;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    PoisonError,
};

const MAX_REMOVALS: usize = 1024;

#[derive(Default)]
pub(crate) struct Wake(NativeWake);

impl Wake {
    #[cfg(target_os = "macos")]
    pub(crate) fn new() -> Result<Self, String> {
        Ok(Self::default())
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn signal(&self) -> Result<(), String> {
        self.notify();
        Ok(())
    }

    pub(crate) fn notify(&self) {
        self.0.notify();
    }
}

#[derive(Default)]
struct Changes {
    refresh: bool,
    removed: BTreeSet<String>,
    error: Option<String>,
}

pub(crate) struct Topology {
    changes: Mutex<Changes>,
    wake: Arc<Wake>,
}

impl Topology {
    pub(crate) fn new(wake: Arc<Wake>) -> Self {
        Self {
            changes: Mutex::default(),
            wake,
        }
    }

    pub(crate) fn arrived(&self) {
        self.changes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .refresh = true;
        self.wake.notify();
    }

    pub(crate) fn removed(&self, path: Result<String, String>) {
        let mut changes = self.changes.lock().unwrap_or_else(PoisonError::into_inner);
        if changes.error.is_none() {
            match path {
                Ok(path)
                    if changes.removed.contains(&path) || changes.removed.len() < MAX_REMOVALS =>
                {
                    changes.removed.insert(path);
                    changes.refresh = true;
                }
                Ok(_) => {
                    changes.error = Some("IOHID removal notifications exceeded their bound.".into())
                }
                Err(error) => changes.error = Some(error),
            }
        }
        drop(changes);
        self.wake.notify();
    }

    fn take(&self) -> Result<Changes, String> {
        let mut changes =
            std::mem::take(&mut *self.changes.lock().unwrap_or_else(PoisonError::into_inner));
        if let Some(error) = changes.error.take() {
            return Err(error);
        }
        Ok(changes)
    }
}

pub(crate) fn run<P: Platform>(
    platform: P,
    shared: Shared,
    stop: &AtomicBool,
    control: crate::visibility::suppression::Control,
    wake: &Arc<Wake>,
    topology: &Topology,
) -> Result<(), String> {
    wake.0
        .bind_thread(std::thread::current())
        .map_err(|_| "IOHID worker wake was already bound.")?;
    #[cfg(target_os = "macos")]
    control.watch_capture(wake);
    let mut capture = Capture::new(platform, shared);
    capture.control = control;
    let mut initial_scan = true;
    while !stop.load(Ordering::Acquire) {
        let observed = wake.0.observed();
        let changes = topology.take()?;
        for path in changes.removed {
            if let Some(device) = capture.devices.remove(&path) {
                let mut registry = capture
                    .shared
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if let Some(id) = registry.id_for_path(&device.identity_path) {
                    registry.disconnect(&id);
                }
            }
        }
        let recover =
            capture.tick_selected(|device| device.input.take_ready() || device.baseline_due);
        if initial_scan || changes.refresh {
            capture.scan()?;
            initial_scan = false;
        } else if !recover {
            let timeout = capture
                .baseline_deadline
                .map(|deadline| deadline.saturating_duration_since(Instant::now()));
            wake.0.wait_scheduled(observed, timeout);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_topology_retains_exact_removals_and_fails_closed_on_loss() {
        let topology = Topology::new(Arc::new(Wake::default()));
        topology.arrived();
        for _ in 0..2 {
            topology.removed(Ok("old".into()));
        }
        let changes = topology.take().ok().unwrap();
        assert!(changes.refresh);
        assert_eq!(changes.removed, BTreeSet::from(["old".into()]));
        assert!(!topology.take().ok().unwrap().refresh);
        topology.removed(Err("identity lost".into()));
        assert_eq!(topology.take().err().as_deref(), Some("identity lost"));
        for id in 0..=MAX_REMOVALS {
            topology.removed(Ok(id.to_string()));
        }
        assert!(topology
            .take()
            .err()
            .unwrap()
            .contains("exceeded their bound"));
    }
}
