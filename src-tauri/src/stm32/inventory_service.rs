//! Event acquisition and independent local authority deadlines for CAN inventory.
use super::*;

impl Gateway {
    pub(super) fn refresh_inventory(&mut self) -> bool {
        if self.role != Role::Can { return false; }
        let Some(cache) = &self.workspace else { return false; };
        let (Some(fence), Some(authority)) = cache.publication.inventory_evidence() else { return false; };
        if self.connection.is_none() || [&self.inventory_fence, &self.inventory_attempted]
            .into_iter().flatten().any(|old| old.same_record(&fence))
        {
            return false;
        }
        self.inventory_attempted = Some(fence.clone());
        if self.exchange_guarded("snapshot", self.timing.command, None, None, Some(&authority)).is_err()
            && self.connection.is_none() && self.automatic()
        {
            self.publication_reconnect = true;
        }
        true
    }

    fn inactivity_deadline(&self, seen: Instant) -> Option<Instant> {
        if self.connection.is_none() || self.workspace.as_ref().is_some_and(|cache| cache.owned()) {
            return None;
        }
        seen.checked_add(self.timing.ui_timeout)
    }

    pub(super) fn maintenance_deadline(&self, seen: Instant, debug_poll: Instant) -> Option<Instant> {
        let debug = (self.connection.is_some() && self.role == Role::PicoDebug).then_some(debug_poll);
        self.claims.values().filter_map(|claim| claim.seen.checked_add(self.timing.claim_timeout))
            .chain(self.inactivity_deadline(seen)).chain(debug).min()
    }

    pub(super) fn maintain(&mut self, seen: Instant, debug_poll: &mut Instant) -> bool {
        if self.inactivity_deadline(seen).is_some_and(|deadline| Instant::now() >= deadline) {
            self.close();
            return true;
        }
        let claims = self.claims.len();
        self.expire_claims(None);
        if self.connection.is_some() && self.role == Role::PicoDebug && Instant::now() >= *debug_poll {
            if self.automatic() {
                let _ = self.automatic_snapshot();
            } else {
                let _ = self.exchange("snapshot", self.timing.command);
            }
            *debug_poll = Instant::now() + self.timing.heartbeat;
            return true;
        }
        self.claims.len() != claims
    }
}
