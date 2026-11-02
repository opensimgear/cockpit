use super::*;
use std::sync::{Condvar, Mutex, OnceLock};
#[derive(Default)]
struct Probe {
    encoded: usize,
    gate: Option<Arc<Gate>>,
}
#[derive(Default)]
pub(super) struct Gate {
    state: Mutex<(bool, bool)>,
    wake: Condvar,
}
static PROBES: OnceLock<Mutex<BTreeMap<String, Probe>>> = OnceLock::new();
fn probes() -> &'static Mutex<BTreeMap<String, Probe>> {
    PROBES.get_or_init(Mutex::default)
}
pub(super) fn before_publication(id: &str) {
    let gate = {
        let mut probes = probes().lock().unwrap();
        let probe = probes.entry(id.into()).or_default();
        probe.encoded += 1;
        probe.gate.clone()
    };
    if let Some(gate) = gate {
        let mut state = gate.state.lock().unwrap();
        state.0 = true;
        gate.wake.notify_all();
        while !state.1 {
            state = gate.wake.wait(state).unwrap();
        }
    }
}
pub(super) fn block_publication(id: &str) -> Arc<Gate> {
    let gate = Arc::new(Gate::default());
    probes().lock().unwrap().entry(id.into()).or_default().gate = Some(gate.clone());
    gate
}
impl Gate {
    pub fn entered(&self) {
        let state = self.state.lock().unwrap();
        let (state, result) = self
            .wake
            .wait_timeout_while(state, std::time::Duration::from_secs(5), |s| !s.0)
            .unwrap();
        assert!(
            !result.timed_out() && state.0,
            "worker did not reach publication gate"
        );
    }
    pub fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.wake.notify_all();
    }
}
impl Drop for Gate {
    fn drop(&mut self) {
        self.release();
    }
}
pub(super) fn encoded_count(id: &str) -> usize {
    probes()
        .lock()
        .unwrap()
        .get(id)
        .map_or(0, |probe| probe.encoded)
}
pub(super) fn wait_processed(registry: &Registry, id: &str) {
    registry
        .observations
        .actor
        .as_ref()
        .unwrap()
        .shared
        .wait_processed(id, registry.observations.epoch);
}
