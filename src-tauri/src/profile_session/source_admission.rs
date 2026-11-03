//! Refresh only admitted quiet-source evidence before the reducer evaluates
//! current deadlines. Recovery baselines acquire validity through the batch.
use osg_mapping::engine::{typed::{native::Admission, Phase}, Counter};

pub(crate) fn admit<I, F>(
    phase: &mut Phase,
    admissions: I,
    now: Counter,
    ports: impl IntoIterator<Item = (String, Counter, Vec<String>)>,
    sources: impl IntoIterator<Item = (String, Counter)>,
) -> Result<Vec<String>, String>
where
    I: IntoIterator<Item = F>,
    F: Into<Admission>,
{
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
    phase.accept_admissions(admissions.into_iter().map(Into::into), now)
}
