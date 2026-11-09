use super::{AdmissionSize, Completion, Delivery, Instant, Job, Output, ScopeAdmission, Shared};
use osg_mapping::{
    desktop::{Action, Control, DispatchPermit, Intent, Scheduler},
    engine::Counter,
};

pub(super) struct Press {
    pub delivery: Delivery,
    permit: DispatchPermit,
    source_now: u64,
    admitted: Instant,
}

impl Press {
    pub fn source_now(&self) -> u64 {
        super::execute::effective_source_now(self.source_now, self.admitted)
    }

    pub fn valid(&self) -> bool {
        self.permit.valid()
    }
}

pub(super) enum Slot {
    Closed,
    Open,
    Queued(Press),
    Executing(Counter, DispatchPermit),
}

impl Slot {
    pub fn permitted(&self, delivery: &Delivery) -> bool {
        match self {
            Self::Executing(id, permit) if *id == delivery.operation_id => permit.valid(),
            _ => true,
        }
    }

    pub fn retire(&mut self, completion: &mut Completion) {
        // A first-delivery failure can bypass its ACK callback. An accepted
        // head still in this slot has never been submitted.
        match std::mem::replace(self, Self::Closed) {
            Self::Queued(press) => completion.dropped.push((press.delivery, 4)),
            Self::Executing(operation, permit) => completion.late_permit = Some((operation, permit)),
            Self::Closed | Self::Open => {}
        }
    }
}

pub(super) fn single_key_press(delivery: &Delivery) -> bool {
    matches!(&delivery.intent, Intent::Held { controls }
        if matches!(controls.as_slice(), [Control::Key { .. }]))
}

impl Shared {
    pub fn open_late_press(&self, job: &Job, delivery: &Delivery) {
        let mut state = self.lock();
        if Shared::can_arm(&state, &delivery.action_id, &delivery.producer, job.admission) {
            state.late_press = Slot::Open;
        }
    }

    // The original ACK callback closes admission even when there is no head
    // yet. An empty slot never delays a receipt or keeps the worker awake.
    pub fn take_late_press(&self) -> Option<Press> {
        let mut state = self.lock();
        let Slot::Queued(press) = std::mem::replace(&mut state.late_press, Slot::Closed) else { return None };
        state.late_press = Slot::Executing(press.delivery.operation_id, press.permit.clone());
        Some(press)
    }
}

impl Output {
    pub(in crate::desktop) fn enqueue_late_press(
        &self,
        scheduler: &mut Scheduler,
        source_now: u64,
        audit_shadow: u64,
        admission: Option<ScopeAdmission>,
    ) -> Result<(), String> {
        let admitted = Instant::now();
        let Some(head) = scheduler.pending_deliveries().next() else { return Ok(()) };
        if !single_key_press(head)
            || !matches!(scheduler.action(&head.action_id), Some(Action::KeyHold { .. }))
            || serde_json::to_writer(AdmissionSize(2), head).is_err()
        {
            return Ok(());
        }
        let mut state = self.shared.lock();
        if !matches!(state.late_press, Slot::Open) || state.busy_shadow != audit_shadow
            || state.executing_admission != admission
            || !Shared::can_arm(&state, &head.action_id, &head.producer, admission)
        {
            return Ok(());
        }
        let (delivery, permit) = scheduler.take_dispatch_head(head.operation_id)?;
        state.late_press = Slot::Queued(Press { delivery, permit, source_now, admitted });
        Ok(())
    }
}
