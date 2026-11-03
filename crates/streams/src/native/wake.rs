use super::*;

/// Passive notification lease. Holds the journal, never a source owner or a
/// payload queue; readers retain responsibility for continuity and loss.
pub struct NativeWatch<S, E> {
    shared: Arc<Shared<S, E>>,
    id: u64,
}

impl<S: Clone + RetainedBytes, E: Clone + RetainedBytes> NativeStream<S, E> {
    /// Register atomically with an initial notification so the worker drains
    /// the current cache even when no later commit arrives.
    pub fn watch(&self, wake: Arc<NativeWake>) -> Result<NativeWatch<S, E>, StreamError> {
        let mut state = self.shared.lock();
        if state.closed {
            return Err(StreamError::Closed);
        }
        if state.subscribers.len() + state.watchers.len() >= self.shared.limits.max_subscribers {
            return Err(StreamError::SubscriberLimit);
        }
        let id = state
            .next_subscriber
            .checked_add(1)
            .ok_or_else(|| StreamError::Serialization("Subscriber counter exhausted.".into()))?;
        state.next_subscriber = id;
        state.watchers.insert(id, wake.clone());
        wake.notify();
        Ok(NativeWatch {
            shared: self.shared.clone(),
            id,
        })
    }
}

impl<S, E> NativeWatch<S, E> {
    pub fn is_closed(&self) -> bool {
        self.shared.lock().closed
    }
}

impl<S, E> Drop for NativeWatch<S, E> {
    fn drop(&mut self) {
        self.shared.lock().watchers.remove(&self.id);
    }
}
