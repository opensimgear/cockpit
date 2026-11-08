//! A fail-closed, event-backed fence around the selected native X-Plane process.
use crate::game_discovery::ProcessIdentity;
use tokio::sync::watch;

#[cfg(windows)]
#[path = "process_events/bridge.rs"]
mod bridge;
#[cfg(windows)]
#[path = "process_events/native.rs"]
mod native;
#[path = "process_events/exit_watch.rs"]
mod exit_watch;
pub(crate) use exit_watch::{ExitEvidence, ExitStatus, ExitWatch};
#[cfg(windows)]
#[path = "process_events/sink.rs"]
mod sink;
#[cfg(windows)]
#[path = "process_events/windows.rs"]
mod windows;
#[cfg(windows)]
pub(crate) use windows::InventoryEvents;

#[cfg(any(windows, test))]
#[allow(dead_code)] // Variants retain the owning worker until Monitor is dropped.
enum Backend {
    #[cfg(windows)]
    Direct(windows::Worker),
    #[cfg(windows)]
    Elevated(bridge::Worker),
    #[cfg(test)]
    Test(watch::Sender<Option<String>>),
}

pub(crate) struct Monitor {
    failure: watch::Receiver<Option<String>>,
    #[cfg(any(windows, test))]
    _worker: Backend,
}
impl Monitor {
    pub(super) fn start(expected: &ProcessIdentity, integration: &str) -> Result<Self, String> {
        #[cfg(windows)]
        {
            Self::start_scoped(expected, windows::ProcessScope::for_integration(integration)?)
        }
        #[cfg(not(windows))]
        {
            let _ = (expected, integration);
            Err("Native game process events are available only on Windows.".into())
        }
    }
    #[cfg(windows)]
    fn start_scoped(expected: &ProcessIdentity, scope: windows::ProcessScope) -> Result<Self, String> {
        Self::direct(windows::start(expected.clone(), scope))
    }
    #[cfg(windows)]
    fn direct(
        start: Result<(watch::Receiver<Option<String>>, windows::Worker), String>,
    ) -> Result<Self, String> {
        let (failure, worker) = start?;
        Ok(Self {
            failure,
            _worker: Backend::Direct(worker),
        })
    }
    /// Explicit user-requested enable only. Never called as an automatic fallback.
    pub(super) fn start_elevated(expected: &ProcessIdentity) -> Result<Self, String> {
        #[cfg(windows)]
        {
            let (failure, worker) = bridge::start(expected.clone())?;
            Ok(Self {
                failure,
                _worker: Backend::Elevated(worker),
            })
        }
        #[cfg(not(windows))]
        {
            let _ = expected;
            Err("Elevated native X-Plane monitoring is available only on Windows.".into())
        }
    }
    /// The enable job owns the sender. True or sender closure cancels startup
    /// and the retained monitor; pending Windows consent itself cannot be dismissed.
    pub(super) fn start_elevated_with_cancel(
        expected: &ProcessIdentity,
        cancel: watch::Receiver<bool>,
        integration: &str,
    ) -> Result<Self, String> {
        #[cfg(windows)]
        {
            let scope = windows::ProcessScope::for_integration(integration)?;
            let (failure, worker) = bridge::start_with_cancel(expected.clone(), cancel, scope)?;
            Ok(Self {
                failure,
                _worker: Backend::Elevated(worker),
            })
        }
        #[cfg(not(windows))]
        {
            let _ = (expected, cancel, integration);
            Err("Elevated native X-Plane monitoring is available only on Windows.".into())
        }
    }
    pub(crate) fn error(&self) -> Option<String> {
        self.failure
            .borrow()
            .clone()
            .or_else(|| self.failure.has_changed().is_err().then(|| STOPPED.into()))
    }
    /// A receiver does not retain the native worker or request permission.
    pub(super) fn failure_receiver(&self) -> watch::Receiver<Option<String>> {
        self.failure.clone()
    }
    pub(super) async fn failed(&self) -> String {
        let mut failure = self.failure.clone();
        wait_failure(&mut failure).await
    }
    pub(super) fn cancel(&self) {
        #[cfg(any(windows, test))]
        match &self._worker {
            #[cfg(windows)]
            Backend::Direct(worker) => worker.cancel(),
            #[cfg(windows)]
            Backend::Elevated(worker) => worker.cancel(),
            #[cfg(test)]
            Backend::Test(sender) => {
                sender.send_if_modified(|failure| {
                    if failure.is_none() {
                        *failure = Some("Native X-Plane process monitoring was disabled.".into());
                        true
                    } else {
                        false
                    }
                });
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn test_monitor() -> (Monitor, watch::Receiver<Option<String>>) {
    let (sender, failure) = watch::channel(None);
    let observed = failure.clone();
    (
        Monitor {
            failure,
            _worker: Backend::Test(sender),
        },
        observed,
    )
}

#[cfg(test)]
pub(super) fn test_monitor_receiver(failure: watch::Receiver<Option<String>>) -> Monitor {
    // An independent backend sender lets tests model the native worker closing
    // its notification channel without retaining that sender in the monitor.
    let (sender, _) = watch::channel(None);
    Monitor {
        failure,
        _worker: Backend::Test(sender),
    }
}

pub(crate) fn helper_main() -> Option<i32> {
    #[cfg(windows)]
    {
        bridge::helper_main()
    }
    #[cfg(not(windows))]
    {
        None
    }
}
const STOPPED: &str = "The X-Plane process event monitor stopped unexpectedly.";
pub(super) async fn wait_failure(failure: &mut watch::Receiver<Option<String>>) -> String {
    loop {
        if let Some(error) = failure.borrow().clone() {
            return error;
        }
        if failure.changed().await.is_err() {
            return STOPPED.into();
        }
    }
}

fn started(selected: u32, observed: u32) -> Result<(), String> {
    if observed == 0 || observed != selected {
        Err("Another X-Plane process started. Stop and activate the game profile again.".into())
    } else {
        // A subscription can report the selected process while bootstrap is in flight.
        // Its retained handle, rather than its reusable PID, proves its lifetime.
        Ok(())
    }
}

#[cfg(test)]
#[path = "process_events/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "process_events/failure_tests.rs"]
mod failure_tests;
