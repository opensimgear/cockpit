use super::*;
use std::{collections::BTreeMap, sync::{Condvar, Mutex, OnceLock}};

#[derive(Default)]
struct Probe { encoded: usize, full_captures: usize, gate: Option<Arc<Gate>>, panic: bool }
#[derive(Default)]
pub(super) struct Gate { state: Mutex<(bool, bool)>, wake: Condvar }
static PROBES: OnceLock<Mutex<BTreeMap<String, Probe>>> = OnceLock::new();
fn probes() -> &'static Mutex<BTreeMap<String, Probe>> { PROBES.get_or_init(Mutex::default) }
pub(super) fn before_publication(session: &str) {
    let (gate, panic) = {
        let mut probes = probes().lock().unwrap();
        let probe = probes.entry(session.into()).or_default();
        probe.encoded += 1;
        (probe.gate.clone(), std::mem::take(&mut probe.panic))
    };
    if let Some(gate) = gate {
        let mut state = gate.state.lock().unwrap();
        state.0 = true;gate.wake.notify_all();
        while !state.1 { state = gate.wake.wait(state).unwrap(); }
    }
    assert!(!panic, "injected inventory encoder panic");
}
pub(super) fn block_publication(session: &str) -> Arc<Gate> {
    let gate = Arc::new(Gate::default());
    probes().lock().unwrap().entry(session.into()).or_default().gate = Some(gate.clone());
    gate
}
pub(super) fn panic_next(session: &str) { probes().lock().unwrap().entry(session.into()).or_default().panic = true; }
pub(super) fn encoded_count(session: &str) -> usize {
    probes().lock().unwrap().get(session).map_or(0, |probe| probe.encoded)
}
pub(super) fn full_capture(session: &str) {
    probes().lock().unwrap().entry(session.into()).or_default().full_captures += 1;
}
pub(super) fn full_capture_count(session: &str) -> usize {
    probes().lock().unwrap().get(session).map_or(0, |probe| probe.full_captures)
}
impl Gate {
    pub fn entered(&self) {
        let state = self.state.lock().unwrap();
        let (state, result) = self.wake.wait_timeout_while(state, std::time::Duration::from_secs(5), |s| !s.0).unwrap();
        assert!(!result.timed_out() && state.0, "inventory worker did not enter publication gate");
    }
    pub fn release(&self) { self.state.lock().unwrap().1 = true;self.wake.notify_all(); }
}
impl Drop for Gate { fn drop(&mut self) { self.release(); } }
pub(super) fn wait_processed(registry: &Registry) {
    registry.inventory_observation.actor.as_ref().unwrap().shared.wait(registry.inventory_observation.epoch).unwrap();
}
