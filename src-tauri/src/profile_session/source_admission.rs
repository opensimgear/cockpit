//! Refresh only admitted quiet-source evidence before the reducer evaluates
//! current deadlines. Recovery baselines acquire validity through the batch.
use osg_mapping::engine::{typed::{Frame, Phase}, Counter};

pub(crate) fn admit(
    phase: &mut Phase,
    frames: impl IntoIterator<Item = Frame>,
    now: Counter,
    ports: impl IntoIterator<Item = (String, Counter, Vec<String>)>,
    sources: impl IntoIterator<Item = (String, Counter)>,
) -> Result<Vec<String>, String> {
    for (source, generation, ports) in ports {
        if phase.has_source_baseline(&source, generation) {
            phase.observe_port_liveness(&source, generation, now, &ports)?;
        }
    }
    for (source, generation) in sources {
        if phase.has_source_baseline(&source, generation) {
            phase.observe_liveness(&source, generation, now)?;
        }
    }
    phase.accept_batch(frames, now)
}
