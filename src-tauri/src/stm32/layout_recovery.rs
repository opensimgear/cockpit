use super::*;
use super::publication_feed::PublicationFeed;

impl Gateway {
    pub(super) fn current_publication(&self) -> Option<PublicationFence> {
        self.workspace.as_ref().and_then(|cache| self.current_layout_fence(&cache.publication))
    }

    fn current_layout_fence(&self, feed: &PublicationFeed) -> Option<PublicationFence> {
        if !matches!(self.connection, Some(GatewayConnection::Parked(_))) {
            return None;
        }
        feed.current().filter(|fence| self.state["epoch"] == fence.epoch() && fence.is_current())
    }

    fn watch_publication(&self) -> Option<(PublicationFeed, Arc<NativeWake>)> {
        let feed = self.workspace.as_ref()?.publication.clone();
        let wake = Arc::new(NativeWake::default());
        wake.bind_thread(thread::current()).ok()?;
        feed.watch(&wake).then_some((feed, wake))
    }

    fn wait_for_publication_change(
        &self,
        feed: &PublicationFeed,
        wake: &NativeWake,
        previous: Option<&PublicationFence>,
        deadline: Instant,
    ) -> Result<Option<PublicationFence>, String> {
        loop {
            let observed = wake.observed();
            if self.cancel.load(Ordering::Acquire) { return Err(CLOSED.into()); }
            let Some(fence) = self.current_layout_fence(feed) else { return Ok(None); };
            if previous.is_none_or(|previous| !previous.same_record(&fence)) {
                return Ok(Some(fence));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() { return Ok(None); }
            wake.wait(observed, remaining);
        }
    }

    pub(super) fn wait_for_command_route(
        &self,
        previous: Option<&PublicationFence>,
        deadline: Instant,
        retry_delay: Duration,
    ) -> Result<bool, String> {
        if self.role == Role::Can && self.state["configurationRouteEvents"] == true {
            if let Some((feed, wake)) = self.watch_publication() {
                if self.wait_for_publication_change(&feed, &wake, previous, deadline)?.is_some() {
                    return Ok(Instant::now() < deadline);
                }
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() { return Ok(false); }
        self.sleep(retry_delay.min(remaining))?;
        Ok(Instant::now() < deadline)
    }

    pub(super) fn observe_layout(
        &mut self,
        target: Option<(&str, &str)>,
        mut previous: Option<PublicationFence>,
    ) -> Result<(), String> {
        let deadline = Instant::now() + self.timing.reconnect;
        let watch = self.watch_publication();
        loop {
            if self.connection.is_none() {
                self.recover_automatic(deadline)?;
                previous = None;
            }
            if let Some(error) = self.state["layoutError"].as_str() {
                return Err(format!("The gateway could not publish its USB controller layout ({error}). Its saved state must be reviewed; the operation was not repeated."));
            }
            if self.state["layoutPending"] == false {
                if target.is_none_or(|(action, id)| self.lifecycle_matches(action, id)) {
                    self.state["reconnecting"] = json!(false);
                    return Ok(());
                }
                return Err("The gateway returned, but its current controller state does not confirm this change. Review the inventory; the operation was not repeated.".into());
            }
            if Instant::now() >= deadline {
                return Err("The gateway controller update is still pending. Review its inventory before another change; the operation was not repeated.".into());
            }
            self.state["reconnecting"] = json!(true);
            let mut fence = match &watch {
                Some((feed, wake)) => self.wait_for_publication_change(feed, wake, previous.as_ref(), deadline)?,
                None => None,
            };
            if fence.is_none() {
                self.sleep(self.timing.poll.min(deadline.saturating_duration_since(Instant::now())))?;
                fence = watch.as_ref().and_then(|(feed, _)| self.current_layout_fence(feed));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() { continue; }
            // Advance before dispatch. A change during this read invalidates
            // its reply and leaves the next publication available to retry.
            previous = fence;
            if let Err(error) = self.exchange_guarded(
                "snapshot", remaining.min(self.timing.command), None, None, previous.as_ref(),
            ) {
                if error != PUBLICATION_EVIDENCE_CHANGED && self.connection.is_some() {
                    return Err(error);
                }
            }
        }
    }
}
