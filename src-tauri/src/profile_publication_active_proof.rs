use super::*;

impl Shared {
    pub(super) fn notify_status_waiters(&self) {
        if let Some(wake) = self.activation.lock().unwrap_or_else(std::sync::PoisonError::into_inner).upgrade() {
            wake.notify();
        }
        if let Some(wake) = self.routing.lock().unwrap_or_else(|p| p.into_inner()).upgrade() {
            wake.notify();
        }
    }
}

impl Overrides {
    pub(crate) fn watch_activation(&self, wake: &Arc<osg_streams::NativeWake>) {
        for slot in &self.slots {
            *slot.shared.activation.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Arc::downgrade(wake);
        }
    }

    pub(crate) fn watch_routing(&self, wake: &Arc<osg_streams::NativeWake>) {
        for slot in &self.slots {
            *slot.shared.routing.lock().unwrap_or_else(|p| p.into_inner()) = Arc::downgrade(wake);
        }
    }

    pub(crate) fn notification_driven(&self) -> bool {
        self.slots.iter().all(|slot| {
            slot.shared.routing.lock().unwrap_or_else(|p| p.into_inner()).strong_count() > 0
        })
    }

    /// The host checks the original proof expiry even if the actor cannot
    /// publish a terminal result before its current I/O returns.
    pub(crate) fn proof_deadline(&self) -> Option<Instant> {
        self.slots.iter().map(|slot| {
            let verified = slot.shared.verified.lock().unwrap_or_else(|p| p.into_inner());
            match verified.as_ref().and_then(|proof| proof.live.as_ref()) {
                Some((deadline, _)) => *deadline,
                None => Instant::now(),
            }
        }).min()
    }

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
