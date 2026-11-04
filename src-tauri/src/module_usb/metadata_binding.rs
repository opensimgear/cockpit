//! Connection-scoped capture for unsolicited committed-configuration metadata.
//!
//! The console actor calls `accept` on its reader path. This hub only decodes
//! `@meta` lines, retaining the newest frame and a sticky terminal failure.
//! Workers can call `snapshot` or `take_changed` after waking on `wake`.
use crate::{
    module_console::{
        actor::Input,
        metadata::{Decoder, Frame},
    },
    transport::duplex::CancelIo,
};
use osg_streams::NativeWake;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    /// Incremented whenever the latest frame or terminal error changes.
    pub revision: u64,
    pub latest: Option<Frame>,
    pub error: Option<String>,
    pub terminal: bool,
}

struct State {
    decoder: Decoder,
    snapshot: Snapshot,
    cancel_sent: bool,
}

/// One instance belongs to exactly one verified boot and duplex connection.
pub(crate) struct Capture {
    state: Mutex<State>,
    wake: Arc<NativeWake>,
    /// This must be the raw transport cancellation handle, never the actor's
    /// cancellation wrapper (which synchronously delivers `Input::Lost`).
    cancel: Arc<dyn CancelIo>,
}

impl Capture {
    pub(crate) fn new(
        expected_boot: u32,
        wake: Arc<NativeWake>,
        cancel: Arc<dyn CancelIo>,
    ) -> Self {
        Self {
            state: Mutex::new(State {
                decoder: Decoder::new(expected_boot),
                snapshot: Snapshot {
                    revision: 0,
                    latest: None,
                    error: None,
                    terminal: false,
                },
                cancel_sent: false,
            }),
            wake,
            cancel,
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn advance_revision(snapshot: &mut Snapshot) -> bool {
        snapshot.revision = snapshot.revision.checked_add(1).unwrap_or(u64::MAX);
        snapshot.revision == u64::MAX
    }

    /// Accept a console input. Ordinary replies and `@events` are intentionally
    /// ignored; only this connection's `@meta` stream reaches the decoder.
    pub(crate) fn accept(&self, input: Input) {
        let (changed, cancel) = {
            let mut state = self.lock();
            if state.snapshot.terminal {
                return;
            }
            match input {
                Input::Line { line, received_at } if line.starts_with("@meta ") => {
                    match state.decoder.decode(&line, received_at) {
                        Ok(frame) => {
                            let exhausted = frame.exhausted;
                            state.snapshot.latest = Some(frame);
                            let revision_exhausted = Self::advance_revision(&mut state.snapshot);
                            if exhausted || revision_exhausted {
                                state.snapshot.error = Some(if revision_exhausted {
                                    "USB metadata capture revision is exhausted.".into()
                                } else {
                                    "USB metadata sequence is exhausted.".into()
                                });
                                state.snapshot.terminal = true;
                                let cancel = !state.cancel_sent;
                                state.cancel_sent = true;
                                (true, cancel)
                            } else {
                                (true, false)
                            }
                        }
                        Err(error) => {
                            state.snapshot.error = Some(error);
                            state.snapshot.terminal = true;
                            Self::advance_revision(&mut state.snapshot);
                            let cancel = !state.cancel_sent;
                            state.cancel_sent = true;
                            (true, cancel)
                        }
                    }
                }
                Input::Lost(error) => {
                    state.snapshot.error = Some(error);
                    state.snapshot.terminal = true;
                    Self::advance_revision(&mut state.snapshot);
                    (true, false)
                }
                Input::Line { .. } => (false, false),
            }
        };

        // Wake and cancel may invoke arbitrary transport/worker behavior. Keep
        // both outside the state lock so readers and reentrant callbacks are safe.
        if changed {
            self.wake.notify();
        }
        if cancel {
            self.cancel.cancel();
        }
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        self.lock().snapshot.clone()
    }

    /// Return the latest state only when the worker's observed revision is old.
    /// This is a bounded latest-value handoff; intermediate frames coalesce.
    pub(crate) fn take_changed(&self, observed_revision: u64) -> Option<Snapshot> {
        let state = self.lock();
        (state.snapshot.revision != observed_revision).then(|| state.snapshot.clone())
    }

    pub(crate) fn current_revision(&self) -> u64 {
        self.lock().snapshot.revision
    }
}

#[cfg(test)]
#[path = "metadata_binding/tests.rs"]
mod tests;
