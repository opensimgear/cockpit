use crate::server::{socket_io::Cancel, BudgetIo, DeadlineTcp};
use rustls::{ClientConnection, StreamOwned};
use std::{
    collections::BTreeMap,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, Weak},
    thread::Thread,
    time::{Duration, Instant},
};
use tungstenite::WebSocket;

pub(crate) type RawSocket = WebSocket<BudgetIo<StreamOwned<ClientConnection, DeadlineTcp>>>;

#[derive(Default)]
pub(crate) struct Registry(Arc<Mutex<RegistryState>>);

#[derive(Default)]
struct RegistryState {
    shutdown: bool,
    scopes: Vec<Weak<ScopeInner>>,
}

struct ScopeInner {
    epoch: Option<u64>,
    state: Mutex<ScopeState>,
    wake: osg_streams::NativeWake,
}

#[derive(Default)]
struct ScopeState {
    closed: bool,
    next_id: u64,
    controls: BTreeMap<u64, Arc<Cancel>>,
}

#[derive(Clone)]
pub(crate) struct IoScope(Arc<ScopeInner>);

impl Default for IoScope {
    fn default() -> Self { Self::with_epoch(None) }
}

pub(crate) struct Registration {
    scope: Arc<ScopeInner>,
    id: u64,
}

impl Registry {
    pub(crate) fn scope(
        &self,
        epoch: u64,
        current: impl FnOnce() -> u64,
    ) -> Result<IoScope, String> {
        let mut state = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.shutdown {
            return Err("Native companion I/O registry is shut down".into());
        }
        if current() != epoch {
            return Err("Native companion epoch changed before I/O scope registration".into());
        }
        state.scopes.retain(|scope| scope.strong_count() != 0);
        if state.scopes.len() >= 32 {
            return Err("Native companion I/O scope limit reached".into());
        }
        let scope = IoScope::with_epoch(Some(epoch));
        state.scopes.push(Arc::downgrade(&scope.0));
        Ok(scope)
    }

    pub(crate) fn retire(&self, through: u64) {
        let retired = {
            let mut state = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut retired = Vec::new();
            state.scopes.retain(|scope| {
                let Some(scope) = scope.upgrade() else { return false };
                if scope.epoch.is_some_and(|epoch| epoch <= through) {
                    retired.push(scope);
                    false
                } else {
                    true
                }
            });
            retired
        };
        for scope in retired {
            close_scope(&scope);
        }
    }

    pub(crate) fn shutdown(&self) {
        let scopes = {
            let mut state = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            state.shutdown = true;
            std::mem::take(&mut state.scopes)
                .into_iter()
                .filter_map(|scope| scope.upgrade())
                .collect::<Vec<_>>()
        };
        for scope in scopes {
            close_scope(&scope);
        }
    }
}

impl IoScope {
    fn with_epoch(epoch: Option<u64>) -> Self {
        Self(Arc::new(ScopeInner {
            epoch,
            state: Mutex::new(ScopeState::default()),
            wake: osg_streams::NativeWake::default(),
        }))
    }

    pub(crate) fn close(&self) {
        close_scope(&self.0);
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.0.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).closed
    }

    pub(crate) fn register(&self, cancel: Arc<Cancel>) -> Result<Registration, String> {
        let mut rejected = None;
        let result = {
            let mut state = self.0.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.closed {
                rejected = Some("Native companion I/O scope is closed");
                None
            } else if state.controls.len() >= 16 {
                rejected = Some("Native companion I/O connection limit reached");
                None
            } else if let Some(id) = state.next_id.checked_add(1) {
                state.next_id = id;
                state.controls.insert(id, cancel.clone());
                Some(Registration { scope: self.0.clone(), id })
            } else {
                rejected = Some("Native companion I/O registration IDs exhausted");
                None
            }
        };
        match result {
            Some(registration) => Ok(registration),
            None => {
                cancel.cancel();
                Err(rejected.unwrap_or("Native companion I/O registration failed").into())
            }
        }
    }

    pub(crate) fn bind_thread(&self) -> Result<(), Thread> {
        self.0.wake.bind_thread(std::thread::current())
    }

    pub(crate) fn observed(&self) -> u64 {
        self.0.wake.observed()
    }

    /// Waits until notified or the duration expires; returns true if scope closed.
    pub(crate) fn wait(&self, observed: u64, timeout: Duration) -> bool {
        let _ = self.0.wake.wait(observed, timeout);
        self.is_closed()
    }
}

fn close_scope(scope: &ScopeInner) {
    let controls = {
        let mut state = scope.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.closed {
            return;
        }
        state.closed = true;
        std::mem::take(&mut state.controls).into_values().collect::<Vec<_>>()
    };
    for control in controls {
        control.cancel();
    }
    scope.wake.notify();
}

impl Drop for Registration {
    fn drop(&mut self) {
        let control = self.scope.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
            .controls.remove(&self.id);
        drop(control);
    }
}

pub(crate) struct Socket {
    raw: RawSocket,
    _registration: Registration,
}

impl Socket {
    pub(crate) fn new(raw: RawSocket, registration: Registration) -> Self {
        Self { raw, _registration: registration }
    }

    pub(crate) fn operation(&mut self, deadline: Option<Instant>) -> Operation<'_> {
        let previous = self.raw.get_mut().inner.sock.replace_operation_deadline(deadline);
        Operation { socket: self, previous }
    }
}

impl Deref for Socket {
    type Target = RawSocket;
    fn deref(&self) -> &Self::Target { &self.raw }
}

impl DerefMut for Socket {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.raw }
}

pub(crate) struct Operation<'a> {
    socket: &'a mut Socket,
    previous: Option<Instant>,
}

impl Deref for Operation<'_> {
    type Target = RawSocket;
    fn deref(&self) -> &Self::Target { &self.socket.raw }
}

impl DerefMut for Operation<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.socket.raw }
}

impl Drop for Operation<'_> {
    fn drop(&mut self) {
        let _ = self.socket.raw.get_mut().inner.sock.replace_operation_deadline(self.previous);
    }
}

#[cfg(test)]
#[path = "io_tests.rs"]
mod tests;
