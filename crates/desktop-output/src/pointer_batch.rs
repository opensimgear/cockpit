//! Validation shared by parent and worker; no partial owner-fence updates.
use crate::{
    protocol::{PointerBatchAck, MAX_BATCH},
    reducer::Reducer,
};
use osg_mapping::desktop::{Delivery, Intent};

pub(crate) fn describe(deliveries: &[Delivery]) -> Result<PointerBatchAck, String> {
    if !(2..=MAX_BATCH).contains(&deliveries.len()) {
        return Err("Pointer batch requires between 2 and 64 deliveries.".into());
    }
    let first = &deliveries[0];
    let (mut dx, mut dy) = (0i32, 0i32);
    for delivery in deliveries {
        if delivery.producer.run != first.producer.run || delivery.at_us != first.at_us {
            return Err("Pointer batch participants must share a run and source timestamp.".into());
        }
        let Intent::Pointer { dx: x, dy: y } = delivery.intent else {
            return Err("Pointer batch contains a non-pointer delivery.".into());
        };
        dx = dx
            .checked_add(x)
            .ok_or("Pointer batch horizontal vector overflow.")?;
        dy = dy
            .checked_add(y)
            .ok_or("Pointer batch vertical vector overflow.")?;
    }
    Ok(PointerBatchAck {
        operation_ids: deliveries
            .iter()
            .map(|delivery| delivery.operation_id)
            .collect(),
        dx,
        dy,
    })
}

pub(crate) fn reduce(
    reducer: &Reducer,
    deliveries: &[Delivery],
    expired: bool,
) -> Result<Reducer, String> {
    let mut next = reducer.clone();
    for delivery in deliveries {
        next.delivery(delivery, expired)?;
    }
    Ok(next)
}
