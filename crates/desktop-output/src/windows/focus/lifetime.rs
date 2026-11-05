//! Native cleanup completion, separate from subscription retirement/failure.
use super::Shared;
use std::{sync::{Arc, Condvar, Mutex}, thread::{self, ThreadId}, time::Instant};

struct State {
    thread: Option<ThreadId>,
    exited: bool,
    #[cfg(test)]
    wait_notice: Option<std::sync::mpsc::SyncSender<()>>,
}
pub(super) struct Lifetime {
    state: Mutex<State>,
    changed: Condvar,
}
impl Lifetime {
    pub(super) fn new() -> Self {
        Self { state: Mutex::new(State { thread: None, exited: false,
            #[cfg(test)] wait_notice: None,
        }), changed: Condvar::new() }
    }
    pub(super) fn exited(&self) -> bool {
        self.state.lock().unwrap_or_else(|error| error.into_inner()).exited
    }
    pub(super) fn finish(&self) {
        self.state.lock().unwrap_or_else(|error| error.into_inner()).exited = true;
        self.changed.notify_all();
    }
    #[cfg(test)]
    pub(super) fn notify_wait(&self, notice: std::sync::mpsc::SyncSender<()>) {
        self.state.lock().unwrap_or_else(|error| error.into_inner()).wait_notice = Some(notice);
    }
    pub(super) fn wait(&self, deadline: Instant) -> Result<(), String> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.exited && state.thread == Some(thread::current().id()) {
            return Err("Desktop scope observer cannot wait for its own retirement.".into());
        }
        while !state.exited {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("Desktop scope observer retirement did not complete before initialization deadline.".into());
            }
            #[cfg(test)]
            if let Some(notice) = state.wait_notice.take() { let _ = notice.send(()); }
            let (next, _) = self.changed.wait_timeout(state, remaining)
                .unwrap_or_else(|error| error.into_inner());
            state = next;
        }
        Ok(())
    }
}
pub(super) struct Exit(Arc<Shared>);
impl Exit {
    pub(super) fn enter(shared: Arc<Shared>) -> Self {
        shared.lifetime.state.lock().unwrap_or_else(|error| error.into_inner()).thread = Some(thread::current().id());
        Self(shared)
    }
}
impl Drop for Exit {
    fn drop(&mut self) { self.0.lifetime.finish(); }
}

#[cfg(test)]
#[path = "lifetime_tests.rs"]
mod tests;
