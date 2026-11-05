use super::*;

impl Desktop {
    pub fn bind_scope_before_start(&mut self, scope: Scope) -> Result<(), String> {
        if self.started || self.registered {
            return Err(
                "A running desktop scope is immutable; reactivate the profile to change it.".into(),
            );
        }
        self.retire_scope_watch();
        self.scope = Some(scope);
        self.register_scope_watch()?;
        if let Err(error) = self.output.bind_scope_before_activate(&self.run,self.scope.as_ref().unwrap()) {
            self.retire_scope_watch();
            return Err(error);
        }
        Ok(())
    }
    pub(super) fn flush(&mut self, now: u64) -> Result<(), String> {
        self.flush_dispatch(now)
    }
    pub(super) fn baseline(&mut self, reducer: &Phase) -> Result<(), String> {
        let definitions = reducer
            .program()
            .sinks
            .iter()
            .filter(|sink| matches!(sink.kind, SinkKind::Desktop { .. }))
            .cloned()
            .collect::<Vec<_>>();
        let mut tokens = BTreeMap::new();
        for sink in definitions {
            let Some(sample) = reducer.sample(&sink.from).cloned() else {
                continue;
            };
            let producer = reducer.endpoint_token(&sink.from)?;
            tokens.insert(sink.id.clone(), producer.clone());
            self.scheduler
                .accept(&sink.id, producer, sample, reducer.at_us(), true)?;
        }
        self.output.reconcile(&tokens);
        Ok(())
    }
    pub fn start(&mut self, reducer: &Phase, scope: Option<Scope>) -> Result<(), String> {
        if self.scope.is_none() {
            self.scope = Some(
                scope.ok_or("Choose an exact application scope before starting desktop output.")?,
            );
        }
        let notice = self.register_scope_watch()?;
        self.scope_error = notice.error.clone();
        self.start_focused(reducer, notice.eligible && notice.error.is_none())
    }
    /// A replacement process never inherits actions held before it existed.
    /// Register its verified scope without replaying the current baseline;
    /// the usual focus-return path then requires fresh neutral observations.
    pub fn start_waiting_for_neutral(&mut self) -> Result<(), String> {
        let scope = self.scope.clone().ok_or("A verified desktop scope is required before rearming.")?;
        self.register_scope_watch()?;
        self.output.activate(self.run.clone(), scope)?;
        self.registered = true;
        self.started = true;
        self.focused = false;
        self.shadow = None;
        Ok(())
    }
    pub(super) fn start_focused(&mut self, reducer: &Phase, focused: bool) -> Result<(), String> {
        let shadow = reducer.fork_desktop()?;
        self.output
            .activate(self.run.clone(), self.scope.clone().unwrap())?;
        self.registered = true;
        self.started = true;
        self.focused = focused;
        self.authorization_deadline = focused.then(|| Counter(reducer.at_us().0.saturating_add(100_000)));
        self.baseline(&shadow)?;
        self.audit_shadow = osg_desktop_output::audit::next_id().unwrap_or(0);
        self.shadow = Some(shadow);
        self.flush(reducer.at_us().0)?;
        Ok(())
    }
}
