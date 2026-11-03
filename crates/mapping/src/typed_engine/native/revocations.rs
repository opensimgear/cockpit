use super::super::*;

impl Phase {
    pub(in crate::engine::typed) fn clamp_native_reports(&mut self, revoked: &BTreeSet<Endpoint>) {
        let controls = self
            .compiled
            .program
            .sinks
            .iter()
            .filter(|sink| {
                revoked.contains(&sink.from)
                    || self
                        .compiled
                        .dependencies
                        .get(&sink.from.node_id)
                        .is_some_and(|dependencies| {
                            dependencies
                                .iter()
                                .any(|endpoint| revoked.contains(endpoint))
                        })
            })
            .filter_map(|sink| {
                if let SinkKind::Virtual {
                    controller_id,
                    control,
                } = &sink.kind
                {
                    Some((controller_id.clone(), control.clone()))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for report in &mut self.reports {
            let Some(current) = self.last_controllers.get(&report.target) else {
                continue;
            };
            for (target, control) in &controls {
                if target != &report.target {
                    continue;
                }
                match control {
                    crate::profile::OutputControl::Button { index } => {
                        report.state.buttons[*index as usize] = current.buttons[*index as usize]
                    }
                    crate::profile::OutputControl::Axis { index } => {
                        report.state.axes[*index as usize] = current.axes[*index as usize]
                    }
                    crate::profile::OutputControl::Hat { index } => {
                        report.state.hats[*index as usize] = current.hats[*index as usize]
                    }
                }
            }
        }
    }
}
