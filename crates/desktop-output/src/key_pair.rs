//! A native pair is eligible only when both operations produce one new key down.
use crate::{protocol::NativeEvent, reducer::Reducer};
use osg_mapping::desktop::{Control, Delivery, Intent};

pub(crate) fn validate([first, second]: &[Delivery; 2]) -> Result<(), String> {
    if first.producer.run != second.producer.run || first.operation_id.0 >= second.operation_id.0
        || first.action_id == second.action_id
        || ![first, second].into_iter().all(|delivery| matches!(&delivery.intent,
            Intent::Held { controls } if matches!(controls.as_slice(), [Control::Key { .. }])))
    {
        return Err("Desktop key pair requires ordered, distinct KeyHold presses in the same run.".into());
    }
    Ok(())
}

pub(crate) fn project(reducer: &Reducer, deliveries: &[Delivery; 2]) -> Result<Option<(Reducer, Vec<NativeEvent>)>, String> {
    let mut next = reducer.clone();
    let mut events = Vec::with_capacity(2);
    for delivery in deliveries {
        let reduction = next.delivery(delivery, false)?;
        if reduction.collision || !matches!(reduction.events.as_slice(),
            [NativeEvent::Key { down: true, repeat: false, .. }])
        {
            return Ok(None);
        }
        events.extend(reduction.events);
    }
    Ok(Some((next, events)))
}
