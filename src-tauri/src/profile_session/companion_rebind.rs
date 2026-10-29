//! Read-only delivery preparation never runs on the output lease renewal path.
use crate::profile_session::RunOwner;
use osg_profiles::compiler_models::ExecutionPlan;
use std::{
    collections::BTreeSet,
    sync::mpsc::{self, Receiver, TryRecvError},
};

pub(crate) struct Captured {
    pub delivery: ExecutionPlan,
    pub allowed: BTreeSet<String>,
}
struct Pending<T> {
    owner: RunOwner,
    program_hash: Option<String>,
    receiver: Receiver<Result<T, String>>,
}
pub(crate) struct Preparation<T = Captured> {
    pending: Option<Pending<T>>,
    attempted: Option<u64>,
}
impl<T> Default for Preparation<T> {
    fn default() -> Self {
        Self {
            pending: None,
            attempted: None,
        }
    }
}
impl<T: Send + 'static> Preparation<T> {
    pub fn ready(&self, now: u64) -> bool {
        self.pending.is_none()
            && self
                .attempted
                .is_none_or(|last| now.saturating_sub(last) >= 100_000)
    }
    pub fn start(
        &mut self,
        now: u64,
        owner: RunOwner,
        program_hash: Option<String>,
        capture: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> bool {
        if !self.ready(now) {
            return false;
        }
        self.attempted = Some(now);
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = sender.send(capture());
        });
        self.pending = Some(Pending {
            owner,
            program_hash,
            receiver,
        });
        true
    }
    pub fn poll(
        &mut self,
        owner: &RunOwner,
        program_hash: &Option<String>,
    ) -> Option<Result<T, String>> {
        let pending = self.pending.as_ref()?;
        // A worker has no authority: only its original still-current run may
        // consider the result, then rebind validates every current native token.
        if pending.owner != *owner || pending.program_hash != *program_hash {
            self.pending = None;
            return Some(Err(
                "The companion delivery preparation owner expired.".into()
            ));
        }
        match pending.receiver.try_recv() {
            Ok(result) => {
                self.pending = None;
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                Some(Err(
                    "The companion delivery preparation worker ended; waiting to retry.".into(),
                ))
            }
        }
    }
}

#[cfg(test)]
#[path = "companion_rebind_tests.rs"]
mod tests;
