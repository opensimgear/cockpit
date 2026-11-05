use super::*;

impl NativeFeed {
    pub(super) fn accept_baseline(
        &mut self,
        frames: Vec<NativeFrame>,
    ) -> Result<Vec<NativeFrame>, String> {
        let now = self.feed.provider.now();
        let mut state = self
            .feed
            .provider
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        super::super::expire(&mut state, now);
        let ids = state
            .subscriptions
            .get(&self.feed.subscription)
            .ok_or("The native companion subscription ended.")?
            .sources
            .keys()
            .cloned()
            .collect::<Vec<_>>();

        let clock_exhausted = ids.iter().find(|id| {
            state.instances.get(*id).is_some_and(|instance| {
                instance.owner.is_some()
                    && instance
                        .last_seen_us
                        .checked_add(super::super::LEASE_US)
                        .is_none()
            })
        });
        if let Some(id) = clock_exhausted {
            super::super::lose(
                &mut state,
                id,
                now,
                "Companion native lease clock exhausted; acquire a fresh baseline.",
            );
            return Err("Companion native lease clock exhausted; acquire a fresh baseline.".into());
        }

        let subscription = state.subscriptions.get(&self.feed.subscription).unwrap();
        let mut markers = BTreeMap::new();
        for id in &ids {
            let queue = &subscription.sources[id];
            let instance = state
                .instances
                .get(id)
                .ok_or("The companion source was removed.")?;
            let owner = instance
                .owner
                .as_ref()
                .ok_or("The companion writer ended during baseline capture.")?;
            let deadline = instance
                .last_seen_us
                .checked_add(super::super::LEASE_US)
                .ok_or("Companion native lease clock exhausted; acquire a fresh baseline.")?;
            if now >= deadline {
                return Err("The companion lease expired during baseline capture.".into());
            }
            if !queue.started
                || queue.generation == 0
                || queue.restart_required
                || queue.loss.is_some()
                || queue.generation != instance.generation
                || queue.publication_hash != instance.definition.publication_hash
                || owner.source_id != *id
                || owner.publication_hash != queue.publication_hash
            {
                return Err(
                    "The companion writer, publication or baseline changed during capture.".into(),
                );
            }
            markers.insert(id.clone(), (instance.generation, instance.last_seen_us));
        }

        let mut seen = BTreeSet::new();
        for frame in &frames {
            let Some(queue) = subscription.sources.get(&frame.frame.source.id) else {
                return Err("Unexpected companion source in captured baseline.".into());
            };
            if frame.frame.reason != osg_mapping::engine::Reason::Baseline
                || frame.frame.generation.0 != queue.generation
                || !seen.insert(frame.frame.source.id.clone())
            {
                return Err("The companion baseline changed during capture.".into());
            }
        }
        if seen.len() != ids.len() {
            return Err("A companion source did not provide a complete baseline.".into());
        }

        self.refreshed.extend(markers);
        Ok(frames)
    }

    /// Whether the provider has a source-publication callback that can wake a
    /// consumer after committed companion input changes.
    pub fn notification_driven(&self) -> bool {
        self.feed.provider.wake.is_some()
    }

    /// Return the next time this exact subscription needs service. This is a
    /// read-only query: lease expiration and queued-frame delivery remain the
    /// responsibility of `poll_audited`.
    pub fn next_deadline_us(&self) -> Option<u64> {
        let now = self.feed.provider.now();
        let state = self
            .feed
            .provider
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let Some(subscription) = state.subscriptions.get(&self.feed.subscription) else {
            return Some(now);
        };

        let mut deadline = None;
        for (id, queue) in &subscription.sources {
            if !queue.frames.is_empty() || queue.loss.is_some() {
                return Some(now);
            }

            let Some(instance) = state.instances.get(id) else {
                // The loss was already drained or this feed never had a live
                // baseline. A missing descriptor cannot make progress here.
                continue;
            };

            let marker = (instance.generation, instance.last_seen_us);
            let needs_renewal = instance.owner.is_some()
                && queue.started
                && !queue.restart_required
                && queue.generation == instance.generation
                && queue.publication_hash == instance.definition.publication_hash
                && self.refreshed.get(id) != Some(&marker);
            if needs_renewal {
                return Some(now);
            }

            if queue.started
                && queue.generation == instance.generation
                && queue.publication_hash == instance.definition.publication_hash
            {
                if instance.owner.is_some() {
                    // Polling an unrepresentable deadline withdraws the owner
                    // and queues a disconnect before returning an error.
                    let source_deadline = instance
                        .last_seen_us
                        .checked_add(super::super::LEASE_US)
                        .unwrap_or(now);
                    deadline = Some(deadline.map_or(source_deadline, |current: u64| {
                        current.min(source_deadline)
                    }));
                }
            }
        }
        deadline
    }
}

#[cfg(test)]
#[path = "scheduling_tests.rs"]
mod tests;
