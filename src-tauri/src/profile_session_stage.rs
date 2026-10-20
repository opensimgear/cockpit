//! Native preparation can block on a catalogue, driver or isolated compiler.
//! Its payload crosses into the coordinator once, after that work completes.
use super::{Candidate, Coordinator, Host};
use sha2::{Digest, Sha256};
use std::{
    any::Any,
    fmt,
    sync::{Arc, Mutex},
};

pub(crate) struct Stage {
    fingerprint: [u8; 32],
    payload: Mutex<Option<Box<dyn Any + Send>>>,
}
impl fmt::Debug for Stage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativePreparation")
            .finish_non_exhaustive()
    }
}
fn fingerprint(candidate: &Candidate) -> Result<[u8; 32], String> {
    osg_profiles::validate_document(&candidate.document)?;
    let bytes = serde_json::to_vec(&(
        &candidate.document,
        &candidate.assignments,
        candidate.read_only,
        &candidate.game_id,
        candidate.launch_preparation,
        candidate.admission_epoch,
    ))
    .map_err(|error| error.to_string())?;
    Ok(Sha256::digest(bytes).into())
}
impl Stage {
    pub(crate) fn new<T: Any + Send>(
        candidate: &Candidate,
        payload: T,
    ) -> Result<Arc<Self>, String> {
        Ok(Arc::new(Self {
            fingerprint: fingerprint(candidate)?,
            payload: Mutex::new(Some(Box::new(payload))),
        }))
    }
    /// A changed document, local assignment, storage status or selected game
    /// needs a new native preparation. Neither Clone nor a second Apply replays it.
    pub(crate) fn take<T: Any + Send>(&self, candidate: &Candidate) -> Result<T, String> {
        self.validate(candidate)?;
        self.payload
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
            .ok_or_else(|| {
                "This native preparation was already consumed; prepare again.".to_owned()
            })?
            .downcast::<T>()
            .map(|payload| *payload)
            .map_err(|_| "The native preparation has an incompatible payload.".into())
    }
    pub(crate) fn validate(&self, candidate: &Candidate) -> Result<(), String> {
        if fingerprint(candidate)? != self.fingerprint {
            return Err("The profile candidate changed after native preparation.".into());
        }
        Ok(())
    }
}

/// The long read-only phase deliberately has no coordinator guard. The health
/// worker keeps renewing the previous run while it waits. Apply rechecks recovery
/// and native pins after the one-use payload returns.
pub(crate) fn apply_staged<H: Host>(
    coordinator: &Mutex<Coordinator<H>>,
    candidate: Candidate,
    prepare: impl FnOnce(&Candidate) -> Result<Arc<Stage>, String>,
) -> Result<(), String> {
    apply_staged_admitted(coordinator, candidate, prepare, |_| Ok(()), || {})
}

/// Native admission is acquired only after the slow preparation. The caller
/// rechecks cancellation and saved identity under that guard; successful Apply
/// records its admission epoch before another staged request can enter.
pub(crate) fn apply_staged_admitted<H: Host, Guard>(
    coordinator: &Mutex<Coordinator<H>>,
    mut candidate: Candidate,
    prepare: impl FnOnce(&Candidate) -> Result<Arc<Stage>, String>,
    admit: impl FnOnce(&Candidate) -> Result<Guard, String>,
    committed: impl FnOnce(),
) -> Result<(), String> {
    if candidate.stage.is_some() {
        return Err("Apply requires a fresh native preparation.".into());
    }
    if candidate.read_only {
        return Err("Profile storage is read-only; repair it before activation.".into());
    }
    osg_profiles::validate_document(&candidate.document)?;
    coordinator
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .check_apply()?;
    candidate.stage = Some(prepare(&candidate)?);
    let admission = admit(&candidate)?;
    let result = coordinator
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .apply(candidate);
    if result.is_ok() {
        committed();
    }
    drop(admission);
    result
}
