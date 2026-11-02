use super::{
    native::{self, Handle},
    sink, started,
};
use crate::game_discovery::ProcessIdentity;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender, TryRecvError},
    Arc,
};
use tokio::sync::watch;
use windows::{
    core::{BSTR, PCWSTR},
    Win32::System::{
        Com::{
            CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CoUninitialize,
            CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED, EOAC_NONE,
            RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
        },
        Wmi::{
            IWbemLocator, IWbemObjectSink, IWbemServices, IWbemUnsecuredApartment,
            UnsecuredApartment, WbemLocator, WBEM_FLAG_SEND_STATUS,
            WBEM_FLAG_UNSECAPP_CHECK_ACCESS,
        },
    },
};

pub(super) enum Event {
    Started(u32),
    Failed(String),
}
pub(super) struct Inbox {
    sender: SyncSender<Event>,
    overflow: AtomicBool,
    wake: Handle,
}
impl Inbox {
    pub(super) fn push(&self, event: Event) {
        if self.sender.try_send(event).is_err() {
            self.overflow.store(true, Ordering::Release);
        }
        self.wake.signal();
    }
    fn drain(&self, receiver: &Receiver<Event>, pid: u32) -> Result<(), String> {
        if self.overflow.load(Ordering::Acquire) {
            return Err("The WMI process notification queue overflowed.".into());
        }
        loop {
            match receiver.try_recv() {
                Ok(Event::Started(observed)) => started(pid, observed)?,
                Ok(Event::Failed(error)) => return Err(error),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    return Err("The WMI process notification queue disconnected.".into())
                }
            }
        }
        if self.overflow.load(Ordering::Acquire) {
            return Err("The WMI process notification queue overflowed.".into());
        }
        Ok(())
    }
}
pub(super) struct Worker {
    stop: Arc<Handle>,
    cancelled: Arc<AtomicBool>,
    failure: watch::Sender<Option<String>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Worker {
    pub(super) fn cancel(&self) {
        self.failure.send_if_modified(|failure| {
            if failure.is_none() {
                *failure = Some("Native X-Plane process monitoring was disabled.".into());
                true
            } else {
                false
            }
        });
        self.cancelled.store(true, Ordering::Release);
        self.stop.signal();
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel();
        if let Some(thread) = self.thread.take() {
            // A local COM RPC may stall. Its worker owns all resources and no
            // output authority; never make a caller's Drop wait for that RPC.
            if thread.is_finished() {
                let _ = thread.join();
            }
        }
    }
}
pub(super) fn start(
    expected: ProcessIdentity,
) -> Result<(watch::Receiver<Option<String>>, Worker), String> {
    start_with(
        expected,
        subscribe,
        crate::game_discovery::exclusive_xplane,
        std::time::Duration::from_secs(30),
    )
}
fn start_with<S, E, T>(
    expected: ProcessIdentity,
    subscription: S,
    exclusive: E,
    startup_deadline: std::time::Duration,
) -> Result<(watch::Receiver<Option<String>>, Worker), String>
where
    S: FnOnce(Arc<Inbox>, &AtomicBool) -> Result<T, String> + Send + 'static,
    E: FnOnce(&ProcessIdentity) -> Result<(), String> + Send + 'static,
{
    let stop = Arc::new(Handle::event()?);
    let thread_stop = stop.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    let thread_cancelled = cancelled.clone();
    let (failure_sender, failure) = watch::channel(None);
    let worker_failure = failure_sender.clone();
    let (ready_sender, ready) = mpsc::sync_channel(1);
    let thread = std::thread::Builder::new()
        .name("xplane-process-events".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_with(
                    &expected,
                    &thread_stop,
                    &thread_cancelled,
                    &ready_sender,
                    subscription,
                    exclusive,
                )
            }));
            let error = match result {
                Ok(Ok(())) => return,
                Ok(Err(error)) => error,
                Err(_) => "The X-Plane native process monitor panicked.".into(),
            };
            failure_sender.send_if_modified(|failure| {
                if failure.is_none() {
                    *failure = Some(error.clone());
                    true
                } else {
                    false
                }
            });
            let _ = ready_sender.try_send(Err(error));
        })
        .map_err(|error| format!("Cannot start X-Plane native process monitor: {error}"))?;
    let worker = Worker {
        stop,
        cancelled,
        failure: worker_failure,
        thread: Some(thread),
    };
    ready
        .recv_timeout(startup_deadline)
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => {
                "Native X-Plane process monitoring startup timed out.".to_string()
            }
            mpsc::RecvTimeoutError::Disconnected => {
                "The X-Plane native process monitor stopped during startup.".to_string()
            }
        })??;
    Ok((failure, worker))
}
struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
struct Subscription {
    services: IWbemServices,
    sinks: Vec<IWbemObjectSink>,
    local_sinks: Vec<IWbemObjectSink>,
    _apartment: IWbemUnsecuredApartment,
}
impl Drop for Subscription {
    fn drop(&mut self) {
        for sink in &self.sinks {
            let _ = unsafe { self.services.CancelAsyncCall(sink) };
        }
        // Stub references must be released before the local sink/apartment references.
        self.sinks.clear();
        self.local_sinks.clear();
    }
}
fn run_with<S, E, T>(
    expected: &ProcessIdentity,
    stop: &Handle,
    cancelled: &AtomicBool,
    ready: &SyncSender<Result<(), String>>,
    subscription: S,
    exclusive: E,
) -> Result<(), String>
where
    S: FnOnce(Arc<Inbox>, &AtomicBool) -> Result<T, String>,
    E: FnOnce(&ProcessIdentity) -> Result<(), String>,
{
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok() }
        .map_err(|e| format!("Cannot initialize process monitor COM apartment: {e}"))?;
    let _apartment = Apartment;
    let (sender, receiver) = mpsc::sync_channel(256);
    let inbox = Arc::new(Inbox {
        sender,
        overflow: AtomicBool::new(false),
        wake: Handle::event()?,
    });
    let _subscription = subscription(inbox.clone(), cancelled)?;
    check_cancelled(cancelled)?;
    // Subscribe first. Starts racing native inventory are queued and admitted by
    // this same worker before it publishes readiness. The exact handle survives
    // inventory and signals exit even if Windows subsequently reuses the PID.
    let process = Handle::process(expected)?;
    exclusive(expected)?;
    inbox.drain(&receiver, expected.pid)?;
    process.running()?;
    check_cancelled(cancelled)?;
    ready
        .send(Ok(()))
        .map_err(|_| "The X-Plane process monitor startup was cancelled.")?;
    loop {
        match native::wait(stop, &inbox.wake, &process)? {
            0 => return Ok(()),
            1 => {
                inbox.drain(&receiver, expected.pid)?;
                process.running()?;
            }
            _ => return Err("The selected X-Plane process exited.".into()),
        }
    }
}
fn subscribe(inbox: Arc<Inbox>, cancelled: &AtomicBool) -> Result<Subscription, String> {
    let result = unsafe { subscribe_com(inbox, cancelled) };
    result.map_err(|error| {
        let action = if error.contains("0x80041003") || error.contains("0x80070005") {
            " Windows denied process-event access. Use the explicit native monitoring enable action to request a temporary elevated helper."
        } else { "" };
        format!("Cannot subscribe to native X-Plane process notifications (ROOT\\CIMV2): {error}.{action}")
    })
}
fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) {
        Err("Native X-Plane process monitoring startup was cancelled.".into())
    } else {
        Ok(())
    }
}
unsafe fn subscribe_com(inbox: Arc<Inbox>, cancelled: &AtomicBool) -> Result<Subscription, String> {
    let locator: IWbemLocator =
        unsafe { CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER) }
            .map_err(|e| format!("WMI locator: {e}"))?;
    check_cancelled(cancelled)?;
    let services = unsafe {
        locator.ConnectServer(
            &BSTR::from("ROOT\\CIMV2"),
            &BSTR::new(),
            &BSTR::new(),
            &BSTR::new(),
            0,
            &BSTR::new(),
            None,
        )
    }
    .map_err(|e| format!("WMI namespace: {e}"))?;
    check_cancelled(cancelled)?;
    // RPC_C_AUTHN_WINNT=10, RPC_C_AUTHZ_NONE=0; the local user's credentials.
    unsafe {
        CoSetProxyBlanket(
            &services,
            10,
            0,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )
    }
    .map_err(|e| format!("WMI proxy security: {e}"))?;
    check_cancelled(cancelled)?;
    let apartment: IWbemUnsecuredApartment =
        unsafe { CoCreateInstance(&UnsecuredApartment, None, CLSCTX_LOCAL_SERVER) }
            .map_err(|e| format!("WMI callback apartment: {e}"))?;
    check_cancelled(cancelled)?;
    let mut subscription = Subscription {
        services,
        sinks: Vec::new(),
        local_sinks: Vec::new(),
        _apartment: apartment,
    };
    for (query, dropped) in [
        // Includes the __EventQueueOverflowEvent subclass. Any loss fails closed,
        // even if it belonged to another consumer: ownership cannot be inferred.
        // Register loss detection before the process subscription itself.
        ("SELECT * FROM __EventDroppedEvent", true),
        (
            "SELECT * FROM Win32_ProcessStartTrace WHERE ProcessName = 'X-Plane.exe'",
            false,
        ),
    ] {
        let local = sink::create(inbox.clone(), dropped);
        let stub = unsafe {
            subscription._apartment.CreateSinkStub(
                &local,
                WBEM_FLAG_UNSECAPP_CHECK_ACCESS.0 as u32,
                PCWSTR::null(),
            )
        }
        .map_err(|e| format!("WMI authenticated callback: {e}"))?;
        check_cancelled(cancelled)?;
        subscription.local_sinks.push(local);
        // Retain the stub even on registration failure so cleanup always cancels it.
        subscription.sinks.push(stub);
        unsafe {
            subscription.services.ExecNotificationQueryAsync(
                &BSTR::from("WQL"),
                &BSTR::from(query),
                WBEM_FLAG_SEND_STATUS,
                None,
                subscription.sinks.last().unwrap(),
            )
        }
        .map_err(|e| format!("WMI event query {query}: {e}"))?;
        check_cancelled(cancelled)?;
    }
    Ok(subscription)
}

#[cfg(test)]
pub(super) fn test_inbox(capacity: usize) -> (Arc<Inbox>, Receiver<Event>) {
    let (sender, receiver) = mpsc::sync_channel(capacity);
    (
        Arc::new(Inbox {
            sender,
            overflow: AtomicBool::new(false),
            wake: Handle::event().unwrap(),
        }),
        receiver,
    )
}
#[cfg(test)]
pub(super) fn test_drain(
    inbox: &Inbox,
    receiver: &Receiver<Event>,
    pid: u32,
) -> Result<(), String> {
    inbox.drain(receiver, pid)
}

#[cfg(test)]
pub(super) fn test_subscription() -> Result<(), String> {
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok() }
        .map_err(|error| error.to_string())?;
    let _apartment = Apartment;
    let (inbox, _) = test_inbox(256);
    let _subscription = subscribe(inbox, &AtomicBool::new(false))?;
    Ok(())
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
