use super::*;
use osg_mapping::{desktop::Delivery, engine::Counter};

impl<A: Adapter> Worker<A> {
    pub(super) fn pointer_batch(
        &mut self,
        deliveries: Vec<Delivery>,
        deadline: Option<Counter>,
        now: u64,
        ack: &mut Ack,
    ) -> Result<Vec<NativeEvent>, String> {
        let batch = crate::pointer_batch::describe(&deliveries)?;
        ack.pointer_batch = Some(batch.clone());
        let expired = deadline.is_some_and(|d| now >= d.0);
        let next = crate::pointer_batch::reduce(&self.reducer, &deliveries, expired)?;
        ack.skipped = expired;
        let events = if !expired && (batch.dx != 0 || batch.dy != 0) {
            vec![NativeEvent::Pointer {
                dx: batch.dx,
                dy: batch.dy,
            }]
        } else {
            vec![]
        };
        self.reducer = next;
        Ok(events)
    }
}
