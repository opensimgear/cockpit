use super::*;

impl Overrides {
    /// Check the lease against the host's captured gateway view without waiting
    /// on its adapter or on the publication worker's status locks.
    pub(crate) fn try_verify_disabled(
        &self,
        gateway: &str,
        module: &str,
        current: &Value,
        panels: &ProfilePanelIo,
    ) -> Result<Option<()>, String> {
        use std::sync::TryLockError;
        let slot = self.slots.iter().find(|slot| slot.value.gateway == gateway).ok_or(INVALID)?;
        if !slot.value.targets.iter().any(|target| target.module == module && !target.enabled)
            || !slot.shared.authority.load(Ordering::Acquire)
        {
            return Err(INVALID.into());
        }
        {
            let status = match slot.shared.status.try_lock() {
                Ok(status) => status,
                Err(TryLockError::WouldBlock) => return Ok(None),
                Err(TryLockError::Poisoned(poison)) => poison.into_inner(),
            };
            if !status.active || status.error.is_some() {
                return Err(INVALID.into());
            }
        }
        let verified = match slot.shared.verified.try_lock() {
            Ok(verified) => verified,
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Poisoned(poison)) => poison.into_inner(),
        };
        let proof = verified.as_ref().ok_or(INVALID)?;
        let (deadline, fence) = proof.live.as_ref().ok_or(INVALID)?;
        let Some(latest) = panels.try_publication_fence(gateway) else {
            return Ok(None);
        };
        if Instant::now() >= *deadline
            || !latest.is_some_and(|latest| fence.same_stream(&latest) && fence.is_current())
            || !slot.shared.authority.load(Ordering::Acquire)
            || !settled(&slot.value, current)
            || current["epoch"] != proof.state["epoch"]
            || !same_publication(&proof.state, current)
            || slot.value.targets.iter().any(|target| {
                module_delivery(target, &proof.state) != module_delivery(target, current)
            })
        {
            return Err(INVALID.into());
        }
        Ok(Some(()))
    }
}
