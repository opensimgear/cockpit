use super::*;
use std::time::{Duration, Instant};

enum Queued<S, E> {
    Snapshot(Arc<Snapshot<S>>),
    Event(Arc<Event<E>>),
    Loss(Loss),
}

impl<S: Clone + RetainedBytes, E: Clone + RetainedBytes> NativeSubscription<S, E> {
    pub fn try_recv(&mut self) -> Result<Option<Delivery<S, E>>, StreamError> {
        let queued = {
            let mut state = self.shared.lock();
            let subscriber = state
                .subscribers
                .get_mut(&self.id)
                .ok_or(StreamError::Closed)?;
            if let Some(loss) = subscriber.loss.take() {
                Some(Queued::Loss(loss))
            } else if let Some(snapshot) = subscriber.snapshot.take() {
                subscriber.awaiting_baseline = false;
                Some(Queued::Snapshot(snapshot))
            } else if let Some((event, bytes)) = subscriber.events.pop_front() {
                subscriber.pending_bytes -= bytes;
                Some(Queued::Event(event))
            } else {
                None
            }
        };
        // Cloning user payloads outside the owner lock preserves native shared
        // allocations and permits unrelated readers and publishers to progress.
        Ok(queued.map(|queued| match queued {
            Queued::Snapshot(snapshot) => Delivery::Snapshot((*snapshot).clone()),
            Queued::Event(event) => Delivery::Event((*event).clone()),
            Queued::Loss(loss) => Delivery::Loss(loss),
        }))
    }

    /// True after consuming the current baseline. False while awaiting an
    /// initial or replacement baseline, and permanently false after overflow,
    /// close, cancellation, or unregistering.
    pub fn is_continuous(&self) -> bool {
        self.shared
            .lock()
            .subscribers
            .get(&self.id)
            .is_some_and(|reader| !reader.permanent && !reader.awaiting_baseline)
    }

    pub fn recv(&mut self) -> Result<Delivery<S, E>, StreamError> {
        loop {
            if let Some(delivery) = self.try_recv()? {
                return Ok(delivery);
            }
            let state = self.shared.lock();
            let subscriber = state.subscribers.get(&self.id).ok_or(StreamError::Closed)?;
            if subscriber.has_delivery() {
                continue;
            }
            if state.closed || subscriber.permanent {
                return Err(StreamError::Closed);
            }
            drop(
                self.shared
                    .wake
                    .wait(state)
                    .unwrap_or_else(|poison| poison.into_inner()),
            );
        }
    }

    /// `Ok(None)` means the timeout elapsed; waiting uses the journal condition
    /// variable and has no periodic polling.
    pub fn recv_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<Delivery<S, E>>, StreamError> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(StreamError::InvalidTimeout)?;
        loop {
            if let Some(delivery) = self.try_recv()? {
                return Ok(Some(delivery));
            }
            let state = self.shared.lock();
            let subscriber = state.subscribers.get(&self.id).ok_or(StreamError::Closed)?;
            if subscriber.has_delivery() {
                continue;
            }
            if state.closed || subscriber.permanent {
                return Err(StreamError::Closed);
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Ok(None);
            };
            let (state, _) = self
                .shared
                .wake
                .wait_timeout(state, remaining)
                .unwrap_or_else(|poison| poison.into_inner());
            drop(state);
        }
    }

    pub fn cancellation_handle(&self) -> NativeCancellation<S, E> {
        NativeCancellation {
            shared: self.shared.clone(),
            id: self.id,
        }
    }
}

impl<S, E> NativeCancellation<S, E> {
    pub fn cancel(&self) {
        self.shared.lock().subscribers.remove(&self.id);
        self.shared.wake.notify_all();
    }
}

impl<S, E> Drop for NativeSubscription<S, E> {
    fn drop(&mut self) {
        self.shared.lock().subscribers.remove(&self.id);
        self.shared.wake.notify_all();
    }
}
