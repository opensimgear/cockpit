//! Portable reducer desktop.
use super::super::*;
impl Phase {
    /// Isolate desktop rule state from controller/game branches. Only native
    /// source snapshots cross this boundary; timers, latches, script state and
    /// previously produced effects never do.
    pub fn fork_desktop(&self) -> Result<Self, String> {
        if !self.active {
            return Err("A stopped mapping cannot seed desktop output.".into());
        }
        let mut program = self.compiled.program.clone();
        program
            .sinks
            .retain(|sink| matches!(sink.kind, SinkKind::Desktop { .. }));
        program.controllers.clear();
        let mut needed = program
            .sinks
            .iter()
            .map(|sink| sink.from.node_id.clone())
            .collect::<BTreeSet<_>>();
        loop {
            let previous = needed.len();
            for node in &program.nodes {
                if needed.contains(&node.id) {
                    needed.extend(node.inputs.iter().map(|input| input.from.node_id.clone()));
                }
            }
            if needed.len() == previous {
                break;
            }
        }
        program.nodes.retain(|node| needed.contains(&node.id));
        if program
            .nodes
            .iter()
            .any(|node| matches!(node.operation, Operation::Script { .. }))
        {
            return Err(
                "Desktop script branches remain behind their native qualification gate.".into(),
            );
        }
        let mut desktop = Self::initialize(
            program,
            self.run.generation,
            self.run.session.clone(),
            self.clock,
        )?;
        desktop.sources = self.sources.clone();
        desktop.native = self.native.clone();
        for port in &desktop.compiled.program.sources {
            desktop
                .values
                .insert(port.endpoint.clone(), self.values[&port.endpoint].clone());
        }
        desktop.desktop_neutrals = Some(BTreeMap::new());
        desktop.refresh_all(ReportReason::Baseline)?;
        desktop.effects.clear();
        desktop.reports.clear();
        Ok(desktop)
    }
    /// Cached snapshots may display neutral, but cannot arm a stateful rule
    /// after scope return. Its effective input must observe neutral again.
    pub fn require_desktop_neutral_after(&mut self, boundary: Counter) -> Result<(), String> {
        if self.desktop_neutrals.is_none() || boundary != self.at_us() {
            return Err("A desktop neutral boundary requires its current isolated phase.".into());
        }
        self.desktop_rule_boundary = Some(boundary);
        self.timers.clear();
        for state in self.states.values_mut() {
            state.rule = crate::rules::State::default();
            state.boolean = false;
            state.previous = None;
        }
        Ok(())
    }
    /// Mirror connection evidence without replacing native samples or their
    /// capture provenance. Independent port freshness stays independent.
    pub fn mirror_source_liveness(&mut self, canonical: &Self) -> Result<(), String> {
        if self.run != canonical.run {
            return Err("Desktop source liveness belongs to another run.".into());
        }
        let mut observations = BTreeMap::<(String, u64, u64), Vec<String>>::new();
        for port in &self.compiled.program.sources {
            let source = &self.sources[&port.endpoint.node_id];
            let other_source = canonical
                .sources
                .get(&port.endpoint.node_id)
                .ok_or("Desktop source liveness does not match the captured program.")?;
            let current = &self.values[&port.endpoint];
            let other = canonical
                .values
                .get(&port.endpoint)
                .ok_or("Desktop source liveness does not match the captured program.")?;
            if source.available
                && other_source.available
                && source.generation == other_source.generation
                && current.epoch == other.epoch
                && current.captured == other.captured
                && current.captured_sequence == other.captured_sequence
                && current.sample == other.sample
                && other.observed >= self.clock
                && other.observed > current.observed
                && matches!(port.contract, PortContract::State { .. })
                && matches!(current.sample, SignalSample::Known(_))
            {
                observations
                    .entry((
                        port.endpoint.node_id.clone(),
                        source.generation,
                        other.observed,
                    ))
                    .or_default()
                    .push(port.endpoint.port_id.clone());
            }
        }
        for ((source, generation, observed), ports) in observations {
            self.observe_port_liveness(&source, Counter(generation), Counter(observed), &ports)?;
        }
        Ok(())
    }
}
