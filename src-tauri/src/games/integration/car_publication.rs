//! A per-operation commit reservation keeps accepted cancellation durable.
use std::sync::atomic::{AtomicU8, Ordering};

const OPEN: u8 = 0;
const CANCELLED: u8 = 1;
const COMMITTING: u8 = 2;
const FINISHED: u8 = 3;

#[derive(Default)]
pub(super) struct Publication(AtomicU8);

impl Publication {
    pub(super) fn begin_commit(&self) -> Result<(), String> {
        self.0
            .compare_exchange(OPEN, COMMITTING, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| "Assetto telemetry check was cancelled.".into())
    }

    pub(super) fn cancel(&self) -> Result<(), String> {
        match self
            .0
            .compare_exchange(OPEN, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) | Err(CANCELLED | FINISHED) => Ok(()),
            Err(_) => Err(
                "The Assetto car catalogue is already finalizing. Wait for setup to finish.".into(),
            ),
        }
    }

    pub(super) fn finish(&self) {
        self.0.store(FINISHED, Ordering::Release);
    }
}
