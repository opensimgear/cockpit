use super::*;

impl PanelInputs {
    /// None means actual native baseline acquisition is still pending. It never
    /// substitutes retained values or manufactures a token/sample.
    pub(crate) fn baseline(
        &mut self,
        delivery: &ExecutionPlan,
        validate: impl Fn(&ResourceToken) -> Result<(), String>,
    ) -> Result<Option<Vec<Frame>>, String> {
        self.capture_baseline(delivery, validate, false)
    }
    /// Capture the actual first live snapshot before the Host arms its reducer.
    /// This consumes only duplicate baseline frames, retaining later changes.
    pub(crate) fn live_baseline(
        &mut self,
        delivery: &ExecutionPlan,
        validate: impl Fn(&ResourceToken) -> Result<(), String>,
    ) -> Result<Option<Vec<Frame>>, String> {
        self.capture_baseline(delivery, validate, true)
    }
    fn capture_baseline(
        &mut self,
        delivery: &ExecutionPlan,
        validate: impl Fn(&ResourceToken) -> Result<(), String>,
        live: bool,
    ) -> Result<Option<Vec<Frame>>, String> {
        if self.revoked.load(Ordering::Acquire)
            || self.clock.is_none()
            || self.active.load(Ordering::Acquire) != live
            || live && self.admitted.load(Ordering::Acquire)
        {
            return Err("The native panel reader is not acquired.".into());
        }
        let mut state = self.mailbox.lock().unwrap_or_else(|p| p.into_inner());
        if self
            .specs
            .iter()
            .any(|s| !state.baselines.contains_key(&s.logical))
        {
            return Ok(None);
        }
        let now = self
            .clock
            .map(|(start, origin)| {
                origin
                    .saturating_add(u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX))
            })
            .unwrap_or(0);
        if state
            .baselines
            .values()
            .any(|frame| now.saturating_sub(frame.captured_us.0) > 250_000)
        {
            return Ok(None);
        }
        let mut tokens: BTreeMap<String, Vec<ResourceToken>> = BTreeMap::new();
        for spec in &self.specs {
            for port in &spec.ports {
                let source = delivery
                    .sources
                    .iter()
                    .find(|s| {
                        s.endpoint.node_id == spec.logical
                            && s.endpoint.port_id == port.logical
                            && s.readiness == PlanReadiness::Ready
                    })
                    .ok_or("The delivered panel source disappeared.")?;
                let token = source.token.as_ref().ok_or(
                    "Acquire an actual native read token before admitting a panel baseline.",
                )?;
                if token.resource_id != port.resource_id
                    || token.port_id != port.observed
                    || token.contract_hash != port.contract_hash
                    || source.inventory_resource_id.as_ref() != Some(&port.resource_id)
                    || source.route.as_ref().is_none_or(|route| {
                        token.route_id != route.id
                            || token.provider_generation != route.provider_generation
                    })
                    || token.access != osg_profiles::resources::Access::Read
                    || source.observed_port_id.as_ref() != Some(&port.observed)
                    || source.contract != port.contract
                    || source.route.as_ref().is_none_or(|route| {
                        Target::from_route(route, self.io.as_ref()).ok().as_ref()
                            != Some(&spec.target)
                    })
                {
                    return Err(
                        "The native panel baseline does not match the selected route/contract."
                            .into(),
                    );
                }
                validate(token)?;
                tokens
                    .entry(spec.logical.clone())
                    .or_default()
                    .push(token.clone());
            }
        }
        let frames = self
            .specs
            .iter()
            .map(|s| state.baselines[&s.logical].clone())
            .collect::<Vec<_>>();
        if live {
            state.frames.retain(|frame| {
                !frames.iter().any(|baseline| {
                    baseline.source == frame.source
                        && (frame.generation < baseline.generation
                            || frame.generation == baseline.generation
                                && frame.sequence <= baseline.sequence)
                })
            });
            // No page/event queue is published before this common admission.
            // All sources now use exactly the baselines captured under this lock.
            self.loss_epoch = state.loss_epoch;
            self.admitted.store(true, Ordering::Release);
            state.control();
        }
        drop(state);
        for frame in &frames {
            self.accepted.insert(
                frame.source.clone(),
                (frame.generation.0, frame.sequence.0, frame.captured_us.0),
            );
        }
        self.tokens = tokens;
        self.bound = true;
        Ok(Some(frames))
    }
}
