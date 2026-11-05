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
