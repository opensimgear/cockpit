//! Sequence receipts reconcile autonomous releases before subsequent ACKs.
use super::*;
use std::collections::BTreeSet;

fn same_owner(first: &Delivery, producer: &Token, action: &str) -> bool {
    first.producer.run == producer.run && first.producer.node_id == producer.node_id
        && first.action_id == action
}

impl Coordinator {
    pub(super) fn stage_guard_receipts(&self, ack: &Ack) -> Result<Option<Reducer>, String> {
        if ack.released_guards.len() > MAX_OWNERS {
            return Err("Desktop release receipt budget exceeded.".into());
        }
        if ack.released_guards.is_empty() { return Ok(None); }
        let mut seen = BTreeSet::new();
        // Validate the entire list before changing even the staged projection.
        for id in &ack.released_guards {
            if !seen.insert(id.0) || !self.guard_deliveries.contains_key(&id.0) {
                return Err("Unknown or duplicate desktop release guard receipt.".into());
            }
        }
        let mut next = self.reducer.clone();
        for id in &ack.released_guards {
            let delivery = &self.guard_deliveries[&id.0];
            if next.release_exact(delivery)?.is_none() {
                return Err("Obsolete desktop release guard receipt.".into());
            }
        }
        Ok(Some(next))
    }

    pub(super) fn check_guard_capacity(&self, delivery: &Delivery) -> Result<(), String> {
        if self.guard_deliveries.len() >= MAX_OWNERS && !self.guard_deliveries.values()
            .any(|old| same_owner(old, &delivery.producer, &delivery.action_id)) {
            return Err("Desktop release guard budget exhausted.".into());
        }
        Ok(())
    }

    pub(super) fn finish_guard_receipts(&mut self, request: &Request, ack: &Ack) {
        for id in &ack.released_guards { self.guard_deliveries.remove(&id.0); }
        self.guard_deliveries.retain(|_, old| self.reducer.owns_exact(old));
        if let Command::DeliverGuarded { delivery, .. } = &request.command {
            if ack.ok && !ack.skipped { self.guard_deliveries.insert(request.sequence.0, delivery.clone()); }
        }
    }
}
