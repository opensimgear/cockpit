use super::{DebugInputs, Request};
#[cfg(all(windows, debug_assertions, feature = "reactive-qualification"))]
use super::qualification::CaptureBracket;
use crate::profile_session::RunOwner;
use osg_mapping::engine::{typed::{Frame, Phase}, Counter};
use serde_json::Value;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub(crate) struct Ingress(Arc<Mutex<Option<Active>>>);

struct Active {
    owner: RunOwner,
    inputs: Option<DebugInputs>,
    sources: Arc<[String]>,
    clock_floor: u64,
}

#[derive(Default)]
pub(crate) struct Drain {
    pub(crate) frames: Vec<Frame>,
    sources: Arc<[String]>,
    generation: Counter,
}

#[cfg(all(windows, debug_assertions, feature = "reactive-qualification"))]
pub(crate) struct QualifiedAdmission {
    pub state: Value,
    pub capture: CaptureBracket,
    pub pending_owned_bytes: u32,
    pub work: std::time::Duration,
}

impl Drain {
    pub(crate) fn refresh_liveness(&self, reducer: &mut Phase, now: Counter) -> Result<(), String> {
        for id in self.sources.iter() {
            if reducer.has_source_baseline(id, self.generation) {
                reducer.observe_liveness(id, self.generation, now)?;
            }
        }
        Ok(())
    }
}

impl Ingress {
    pub(crate) fn activate(&self, owner: RunOwner, inputs: DebugInputs, clock_floor: u64) {
        let sources = inputs.sources.iter()
            .filter(|(_, source)| !source.used.is_empty())
            .map(|(id, _)| id.clone())
            .collect();
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(Active {
            owner, inputs: Some(inputs), sources, clock_floor,
        });
    }

    pub(crate) fn revoke(&self) {
        let retired = self.0.lock().unwrap_or_else(|p| p.into_inner()).take();
        drop(retired);
    }

    pub(crate) fn pause(&self, owner: &RunOwner) {
        let retired = self.0.lock().unwrap_or_else(|p| p.into_inner()).as_mut()
            .filter(|active| active.owner == *owner)
            .and_then(|active| active.inputs.take());
        drop(retired);
    }

    pub(crate) fn drain(&self, owner: &RunOwner) -> Drain {
        let mut slot = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let Some(active) = slot.as_mut().filter(|active| active.owner == *owner) else {
            return Drain::default();
        };
        let Some(inputs) = active.inputs.as_mut() else { return Drain::default(); };
        Drain {
            frames: inputs.poll(),
            sources: active.sources.clone(),
            generation: Counter(owner.generation),
        }
    }

    pub(crate) fn request(&self, request: &Request, now: impl FnOnce() -> u64) -> Result<Value, String> {
        admit(&mut self.0.lock().unwrap_or_else(|p| p.into_inner()), request, now)
    }

    #[cfg(all(windows, debug_assertions, feature = "reactive-qualification"))]
    pub(crate) fn batch<T>(
        &self,
        mut capture: impl FnMut(u64) -> Result<CaptureBracket, String>,
        offer: impl FnOnce(
            &mut dyn FnMut(&Request) -> Result<QualifiedAdmission, String>,
            std::time::Duration,
        ) -> T,
    ) -> T {
        let waiting = std::time::Instant::now();
        let mut slot = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let wait = waiting.elapsed();
        offer(
            &mut |request| {
                let entered = std::time::Instant::now();
                let (inputs, floor) = active_inputs(&mut slot, request)?;
                let capture = capture(floor)?;
                let state = inputs.request(request, capture.native_us)?;
                let work = entered.elapsed();
                let pending_owned_bytes = u32::try_from(inputs.qualification_pending_owned_bytes())
                    .map_err(|_| "Qualification pending byte count overflowed.")?;
                Ok(QualifiedAdmission { state, capture, pending_owned_bytes, work })
            },
            wait,
        )
    }
}

fn admit(slot: &mut Option<Active>, request: &Request, now: impl FnOnce() -> u64) -> Result<Value, String> {
    let (inputs, floor) = active_inputs(slot, request)?;
    inputs.request(request, now().max(floor))
}

fn active_inputs<'a>(slot: &'a mut Option<Active>, request: &Request) -> Result<(&'a mut DebugInputs, u64), String> {
    let active = slot.as_mut().filter(|active| active.owner == request.owner)
        .ok_or("The requested debug controller owner expired; inspect the current running revision.")?;
    let inputs = active.inputs.as_mut().ok_or("Wait for the active vehicle profile variant.")?;
    Ok((inputs, active.clock_floor))
}
