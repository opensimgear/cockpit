use super::*;

pub(super) fn validate_ack(request: &Request, ack: &Ack) -> Result<(), String> {
    if let Command::DeliverPointerBatch { deliveries } = &request.command {
        let expected = crate::pointer_batch::describe(deliveries)?;
        if ack.operation_id.is_some() || ack.pointer_batch.as_ref() != Some(&expected) {
            return Err(
                "Desktop pointer batch acknowledgement has incorrect participants or vector."
                    .into(),
            );
        }
        let attempted = usize::from(!ack.skipped && (expected.dx != 0 || expected.dy != 0));
        if ack.ok && (ack.attempted != attempted || ack.confirmed != attempted || ack.collision) {
            return Err(
                "Desktop pointer batch acknowledgement has invalid physical counts.".into(),
            );
        }
    } else if ack.pointer_batch.is_some() {
        return Err("Unexpected desktop pointer batch acknowledgement.".into());
    }
    Ok(())
}

impl Coordinator {
    /// Consume all logical operations atomically and submit exactly one vector.
    /// Only the first logical ACK owns the physical submission counters.
    pub fn submit_pointer_batch(
        &mut self,
        deliveries: Vec<Delivery>,
        source_now_us: u64,
    ) -> Result<Vec<Ack>, String> {
        crate::pointer_batch::describe(&deliveries)?;
        let source_deadline = deliveries
            .iter()
            .filter_map(|d| d.deadline_us)
            .min_by_key(|d| d.0);
        let expired = source_deadline.is_some_and(|d| source_now_us >= d.0);
        let deadline_us = source_deadline
            .map(|d| Counter((self.clock)().saturating_add(d.0.saturating_sub(source_now_us))));
        let next = crate::pointer_batch::reduce(&self.reducer, &deliveries, expired)?;
        let ack = self.send(
            Command::DeliverPointerBatch {
                deliveries: deliveries.clone(),
            },
            deadline_us,
            next.held(),
        )?;
        self.reducer = next;
        if self.update_owners() {
            self.persist()?;
        }
        Ok(deliveries
            .iter()
            .enumerate()
            .map(|(index, delivery)| {
                let mut participant = ack.clone();
                participant.operation_id = Some(delivery.operation_id);
                if index != 0 {
                    participant.attempted = 0;
                    participant.confirmed = 0;
                }
                participant
            })
            .collect())
    }
}

#[cfg(test)]
#[path = "pointer_batch_tests.rs"]
mod tests;
