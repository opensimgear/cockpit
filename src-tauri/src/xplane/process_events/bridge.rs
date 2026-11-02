//! Explicit, operation-scoped elevation. No service is installed and ordinary
//! monitor startup never calls this path.
use super::{native::Handle, Monitor};
use crate::game_discovery::ProcessIdentity;
use std::sync::mpsc::{self, SyncSender};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    net::windows::named_pipe::ClientOptions,
    sync::{oneshot, watch},
    time::{timeout, Duration},
};

#[path = "bridge/launch.rs"]
mod launch;
#[path = "bridge/protocol.rs"]
mod protocol;
#[path = "bridge/security.rs"]
mod security;
#[path = "bridge/wait.rs"]
mod wait;
use protocol::{Frame, Request};
const STARTUP: Duration = Duration::from_secs(30);

pub(super) struct Worker {
    stop: watch::Sender<bool>,
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
        self.stop.send_replace(true);
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel();
        if let Some(thread) = self.thread.take() {
            if thread.is_finished() {
                let _ = thread.join();
            }
        }
    }
}
pub(super) fn start(
    expected: ProcessIdentity,
) -> Result<(watch::Receiver<Option<String>>, Worker), String> {
    start_with(expected, launch::elevated)
}
pub(super) fn start_with_cancel(
    expected: ProcessIdentity,
    cancel: watch::Receiver<bool>,
) -> Result<(watch::Receiver<Option<String>>, Worker), String> {
    start_controlled(expected, Some(cancel), launch::elevated)
}
fn start_with<F>(
    expected: ProcessIdentity,
    launch: F,
) -> Result<(watch::Receiver<Option<String>>, Worker), String>
where
    F: FnOnce(&Request) -> Result<Handle, String> + Send + 'static,
{
    start_controlled(expected, None, launch)
}
fn start_controlled<F>(
    expected: ProcessIdentity,
    external_cancel: Option<watch::Receiver<bool>>,
    launch: F,
) -> Result<(watch::Receiver<Option<String>>, Worker), String>
where
    F: FnOnce(&Request) -> Result<Handle, String> + Send + 'static,
{
    check_external_cancel(&external_cancel)?;
    protocol::validate_process(&expected)?;
    // Reject stale selections before the user can be shown a consent prompt.
    Handle::process(&expected)?;
    let (failure_sender, failure) = watch::channel(None);
    let worker_failure = failure_sender.clone();
    let (stop, stop_receiver) = watch::channel(false);
    let (ready_sender, ready) = mpsc::sync_channel(1);
    let thread = std::thread::Builder::new()
        .name("xplane-process-bridge".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| e.to_string())?;
                runtime.block_on(parent(
                    expected,
                    launch,
                    stop_receiver,
                    external_cancel,
                    &ready_sender,
                ))
            }));
            let error = match result {
                Ok(Ok(())) => return,
                Ok(Err(error)) => error,
                Err(_) => "The native process-events bridge panicked.".into(),
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
        .map_err(|e| format!("Cannot start native process-events bridge: {e}"))?;
    let worker = Worker {
        stop,
        failure: worker_failure,
        thread: Some(thread),
    };
    ready.recv_timeout(STARTUP).map_err(|error| match error {
        mpsc::RecvTimeoutError::Timeout => "The process-events enable request timed out. A later Windows consent cannot activate that cancelled request.".to_string(),
        mpsc::RecvTimeoutError::Disconnected => "The process-events bridge stopped during startup.".to_string(),
    })??;
    Ok((failure, worker))
}
async fn cancelled(stop: &mut watch::Receiver<bool>) {
    while !*stop.borrow() {
        if stop.changed().await.is_err() {
            break;
        }
    }
}
fn check_external_cancel(cancel: &Option<watch::Receiver<bool>>) -> Result<(), String> {
    if cancel
        .as_ref()
        .is_some_and(|receiver| *receiver.borrow() || receiver.has_changed().is_err())
    {
        Err("The native process-events enable request was cancelled.".into())
    } else {
        Ok(())
    }
}
async fn external_cancelled(cancel: &mut Option<watch::Receiver<bool>>) {
    match cancel {
        Some(receiver) => cancelled(receiver).await,
        None => std::future::pending::<()>().await,
    }
}
async fn peer_closed<R: AsyncRead + Unpin>(pipe: &mut R) -> Result<(), String> {
    let mut extra = [0u8; 1];
    match pipe.read(&mut extra).await {
        Ok(0) => Ok(()),
        Ok(_) => Err("The process-events pipe received an extra request.".into()),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::UnexpectedEof
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(format!("The process-events pipe failed: {error}")),
    }
}
async fn parent<F>(
    expected: ProcessIdentity,
    launch: F,
    mut stop: watch::Receiver<bool>,
    mut external_cancel: Option<watch::Receiver<bool>>,
    ready: &SyncSender<Result<(), String>>,
) -> Result<(), String>
where
    F: FnOnce(&Request) -> Result<Handle, String>,
{
    let selected = Handle::process(&expected)?;
    security::same_user(&selected)?;
    let own = security::current()?;
    let request = Request {
        nonce: security::nonce()?,
        parent_pid: own.pid,
        parent_epoch: own.epoch,
    };
    // First-instance creation with a protected user-only ACL precedes ShellExecute.
    let mut pipe = security::server(&request)?;
    check_external_cancel(&external_cancel)?;
    if *stop.borrow() {
        return Ok(());
    }
    let child = launch(&request)?;
    // ShellExecute may have waited for Windows consent beyond our caller's
    // deadline. A late approval must never receive a Begin/configuration frame.
    if *stop.borrow() {
        return Ok(());
    }
    check_external_cancel(&external_cancel)?;
    let mut child_exit = wait::Exit::new(child)?;
    let mut selected_exit = wait::Exit::new(selected)?;
    tokio::select! {
        biased;
        _ = external_cancelled(&mut external_cancel) => return Err("The native process-events enable request was cancelled.".into()),
        _ = cancelled(&mut stop) => return Ok(()),
        result = child_exit.wait() => { result?; return Err("The process-events helper exited before connecting.".into()); },
        result = selected_exit.wait() => { result?; return Err("The selected X-Plane process exited.".into()); },
        result = timeout(STARTUP, pipe.connect()) => result.map_err(|_| "The process-events helper connection timed out.")?.map_err(|e| e.to_string())?,
    }
    security::authenticate_child(&pipe, child_exit.process())?;
    check_external_cancel(&external_cancel)?;
    let startup = async {
        protocol::write(&mut pipe, &Frame::begin(&request, &expected)).await?;
        match protocol::read(&mut pipe, &request.nonce).await? {
            Frame::Ready { .. } => Ok(()),
            Frame::Failed { error, .. } => Err(error),
            _ => Err("The process-events helper returned an invalid startup response.".into()),
        }
    };
    tokio::select! {
        biased;
        _ = external_cancelled(&mut external_cancel) => return Err("The native process-events enable request was cancelled.".into()),
        _ = cancelled(&mut stop) => return Ok(()),
        result = child_exit.wait() => { result?; return Err("The process-events helper exited during startup.".into()); },
        result = selected_exit.wait() => { result?; return Err("The selected X-Plane process exited.".into()); },
        result = timeout(STARTUP, startup) => result.map_err(|_| "The process-events helper startup timed out.")??,
    }
    check_external_cancel(&external_cancel)?;
    ready
        .send(Ok(()))
        .map_err(|_| "The process-events enable request was cancelled.")?;
    tokio::select! {
        biased;
        _ = external_cancelled(&mut external_cancel) => Err("Native X-Plane process monitoring was cancelled.".into()),
        _ = cancelled(&mut stop) => Ok(()),
        result = child_exit.wait() => { result?; Err("The process-events helper exited unexpectedly.".into()) },
        result = selected_exit.wait() => { result?; Err("The selected X-Plane process exited.".into()) },
        result = protocol::read(&mut pipe, &request.nonce) => match result? {
            Frame::Failed { error, .. } => Err(error),
            _ => Err("The process-events helper sent an unexpected message.".into()),
        },
    }
}

/// Dispatch before Tauri initialization. A reserved invocation never falls through
/// to normal UI startup, including malformed or unelevated helper invocations.
pub(crate) fn helper_main() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some(protocol::ARGUMENT) {
        return None;
    }
    let result = (|| {
        let request = Request::parse(&args)?;
        if !security::elevated()? {
            return Err("The process-events helper requires explicit Windows elevation.".into());
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        runtime.block_on(helper(request, Monitor::start))
    })();
    match result {
        Ok(()) => Some(0),
        Err(error) => {
            eprintln!("{error}");
            Some(1)
        }
    }
}
async fn helper<F>(request: Request, factory: F) -> Result<(), String>
where
    F: FnOnce(&ProcessIdentity) -> Result<Monitor, String> + Send + 'static,
{
    // The server already exists before this helper is launched; no reconnect or
    // name-based retry can switch us onto a later, substituted pipe instance.
    let mut pipe = ClientOptions::new()
        .open(request.pipe())
        .map_err(|e| format!("Cannot open the Cockpit process-events pipe: {e}"))?;
    let parent = security::authenticate_parent(&request, &pipe)?;
    let mut parent_exit = wait::Exit::new(parent)?;
    let expected = tokio::select! {
        result = parent_exit.wait() => { result?; return Ok(()); },
        result = timeout(STARTUP, protocol::read(&mut pipe, &request.nonce)) => result.map_err(|_| "The process-events Begin request timed out.")??.into_process()?,
    };
    let selected = Handle::process(&expected)?;
    security::same_user(&selected)?;
    let mut selected_exit = wait::Exit::new(selected)?;
    // COM bootstrap may block in Windows. Keep it off the pipe/exit reactor; if
    // the parent disappears, helper_main returns and the process exits promptly.
    let (sender, mut receiver) = oneshot::channel();
    std::thread::Builder::new()
        .name("xplane-elevated-wmi".into())
        .spawn(move || {
            let _ = sender.send(factory(&expected).map(DeferredMonitor::new));
        })
        .map_err(|e| format!("Cannot start elevated process monitoring: {e}"))?;
    let monitor = tokio::select! {
        result = parent_exit.wait() => { result?; return Ok(()); },
        result = selected_exit.wait() => { result?; return Err("The selected X-Plane process exited during helper startup.".into()); },
        result = peer_closed(&mut pipe) => return result,
        result = timeout(STARTUP, &mut receiver) => match result.map_err(|_| "Elevated process monitoring startup timed out.")?.map_err(|_| "Elevated process monitoring stopped during startup.")? {
            Ok(monitor) => monitor,
            Err(error) => { let _ = timeout(Duration::from_secs(2), protocol::write(&mut pipe, &protocol::failure(&request.nonce, error.clone()))).await; return Err(error); },
        },
    };
    if let Some(error) = monitor.error() {
        let _ = timeout(
            Duration::from_secs(2),
            protocol::write(&mut pipe, &protocol::failure(&request.nonce, error.clone())),
        )
        .await;
        return Err(error);
    }
    let ready = Frame::Ready {
        nonce: request.nonce.clone(),
    };
    tokio::select! {
        result = parent_exit.wait() => { result?; return Ok(()); },
        result = timeout(STARTUP, protocol::write(&mut pipe, &ready)) => result.map_err(|_| "The process-events Ready response timed out.")??,
    }
    tokio::select! {
        result = parent_exit.wait() => { result?; Ok(()) },
        result = selected_exit.wait() => { result?; Err("The selected X-Plane process exited.".into()) },
        result = peer_closed(&mut pipe) => result,
        error = monitor.failed() => {
            let _ = timeout(Duration::from_secs(2), protocol::write(&mut pipe, &protocol::failure(&request.nonce, error.clone()))).await;
            Err(error)
        },
    }
}

// This guard is used only inside the short-lived helper. No helper exit path may
// join a stalled WMI teardown. Releasing the pipe immediately removes authority;
// helper_main's caller then exits the process, including any stalled COM thread.
struct DeferredMonitor(std::mem::ManuallyDrop<Monitor>);
impl DeferredMonitor {
    fn new(monitor: Monitor) -> Self {
        Self(std::mem::ManuallyDrop::new(monitor))
    }
}
impl std::ops::Deref for DeferredMonitor {
    type Target = Monitor;
    fn deref(&self) -> &Monitor {
        &self.0
    }
}
impl std::ops::DerefMut for DeferredMonitor {
    fn deref_mut(&mut self) -> &mut Monitor {
        &mut self.0
    }
}
impl Drop for DeferredMonitor {
    fn drop(&mut self) {
        let monitor = unsafe { std::mem::ManuallyDrop::take(&mut self.0) };
        // On thread creation failure ManuallyDrop intentionally leaves cleanup to
        // process exit, rather than reintroducing blocking Drop on this reactor.
        let mut monitor = std::mem::ManuallyDrop::new(monitor);
        let _ = std::thread::Builder::new()
            .name("xplane-wmi-cleanup".into())
            .spawn(move || unsafe {
                std::mem::ManuallyDrop::drop(&mut monitor);
            });
    }
}

#[cfg(test)]
#[path = "bridge/tests.rs"]
mod tests;
