//! Event acquisition and independent local authority deadlines for gateway inventory.
use super::*;

impl Gateway {
    pub(super) fn refresh_inventory(&mut self) -> bool {
        let Some(cache) = &self.workspace else { return false; };
        let (Some(fence), Some(authority)) = cache.publication.inventory_evidence() else { return false; };
        if self.connection.is_none() || [&self.inventory_fence, &self.inventory_attempted]
            .into_iter().flatten().any(|old| old.same_record(&fence))
        {
            return false;
        }
        if self.state["epoch"] != fence.epoch() {
            self.close();
            self.publication_reconnect = self.automatic();
            return true;
        }
        self.inventory_attempted = Some(fence);
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

    pub(super) fn maintenance_deadline(&self, seen: Instant) -> Option<Instant> {
        self.claims.values().filter_map(|claim| claim.seen.checked_add(self.timing.claim_timeout))
            .chain(self.inactivity_deadline(seen)).chain(self.debug_watchdog).min()
    }

    pub(super) fn maintain(&mut self, seen: Instant) -> bool {
        if self.inactivity_deadline(seen).is_some_and(|deadline| Instant::now() >= deadline) {
            self.close();
            return true;
        }
        let claims = self.claims.len();
        self.expire_claims(None);
        let now = Instant::now();
        if self.debug_watchdog.is_some_and(|deadline| now >= deadline) {
            if self.renew_debug_watchdog(now).is_err() {
                self.close();
                self.publication_reconnect = self.automatic();
            }
            return true;
        }
        self.claims.len() != claims
    }

    fn renew_debug_watchdog(&mut self, now: Instant) -> Result<(), String> {
        let epoch = self.workspace.as_ref().and_then(|cache| cache.publication.current())
            .filter(|proof| self.state["epoch"] == proof.epoch())
            .ok_or(PUBLICATION_EVIDENCE_CHANGED)?.epoch();
        let Some(GatewayConnection::Parked(connection)) = self.connection.as_mut() else {
            return Err(NO_CONNECTION.into());
        };
        self.request_id = self.request_id.wrapping_add(1).max(1);
        let id = self.request_id;
        let deadline = now + self.timing.command;
        connection.send(format!("debug-watchdog {id} {epoch}\n").as_bytes(), deadline)?;
        if connection.reply(id, deadline)? != json!({"id":id,"ok":true}) {
            return Err("Pico Debug rejected watchdog renewal.".into());
        }
        self.debug_watchdog = Some(now + self.timing.heartbeat);
        Ok(())
    }
}
