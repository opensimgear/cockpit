use super::{native::Owner, Registry};
use std::{sync::{Arc, Mutex}, time::Instant};

/// Keep any upgraded owner alive across the ready callback. If the last owner
/// is dropping, wait outside the registry lock until its native cleanup exits.
pub(super) fn with_available<T>(
    registry: &Mutex<Registry>,
    deadline: Instant,
    ready: impl FnOnce(&mut Registry, Option<Arc<Owner>>) -> Result<T, String>,
) -> Result<T, String> {
    loop {
        let mut slot = registry.lock().unwrap_or_else(|error| error.into_inner());
        if Instant::now() >= deadline {
            return Err("Desktop scope observer initialization deadline expired.".into());
        }
        let owner = slot.owner.upgrade();
        if let Some(retired) = owner.as_ref().filter(|owner|
            owner.shared.terminal().is_some() || owner.shared.lifetime.exited())
        {
            let shared = retired.shared.clone();
            if shared.lifetime.exited() {
                // A diagnostic subscriber may still retain the completed owner.
                // Only native teardown, not its last Arc, permits replacement.
                slot.owner = Default::default();
                slot.shared = Default::default();
            }
            drop(slot);
            drop(owner);
            shared.lifetime.wait(deadline)?;
            continue;
        }
        if owner.is_none() {
            if let Some(shared) = slot.shared.upgrade().filter(|shared| !shared.lifetime.exited()) {
                drop(slot);
                shared.lifetime.wait(deadline)?;
                continue;
            }
        }
        return ready(&mut slot, owner);
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
