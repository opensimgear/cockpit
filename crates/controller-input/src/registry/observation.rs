use super::*;
use osg_streams::{Broker, Publisher, TopicSpec};

pub(super) const MAX_BINDINGS: usize = 64;
#[derive(Default)]
pub(super) struct Observations {
    bindings: BTreeMap<String, Binding>,
}
// Metadata transactions are speculative and must never publish through a clone.
impl Clone for Observations {
    fn clone(&self) -> Self {
        Self::default()
    }
}
struct Binding {
    publisher: Publisher,
    generation: u64,
    snapshot: Value,
}
impl Registry {
    /// Called with the service registry lock held. Registration and the supplied
    /// runtime subscription therefore share the same atomic native report boundary.
    pub fn observe<T>(
        &mut self,
        broker: &Broker,
        id: &str,
        action: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        if !self.sources.contains_key(id) {
            return Err("Unknown controller.".into());
        }
        if !self.observations.bindings.contains_key(id) {
            if self.observations.bindings.len() >= MAX_BINDINGS {
                self.prune_observations(broker);
            }
            if self.observations.bindings.len() >= MAX_BINDINGS {
                return Err("Controller observation limit reached.".into());
            }
            let at = self.sources[id].captured_us;
            let snapshot = self.observation_snapshot(id, at);
            let spec = TopicSpec {
                topic: format!("controllers.input.{id}"),
                source: "controller-input".into(),
                audiences: osg_streams::KNOWN_AUDIENCES
                    .iter()
                    .map(|s| (*s).into())
                    .collect(),
            };
            let publisher =
                match broker.register(spec.clone(), snapshot.clone(), at, self.observation_clock())
                {
                    Ok(publisher) => publisher,
                    Err(osg_streams::RegistryError::TopicLimit) => {
                        self.prune_observations(broker);
                        broker
                            .register(spec, snapshot.clone(), at, self.observation_clock())
                            .map_err(|e| e.to_string())?
                    }
                    Err(error) => return Err(error.to_string()),
                };
            let generation = self.sources[id]
                .generation
                .checked_add(1)
                .ok_or("Controller generation exhausted.")?;
            if generation > 1 {
                publisher
                    .replace_generation(generation, snapshot.clone(), at, &self.observation_clock())
                    .map_err(|e| e.to_string())?;
            }
            self.observations.bindings.insert(
                id.into(),
                Binding {
                    publisher,
                    generation: self.sources[id].generation,
                    snapshot,
                },
            );
        }
        action()
    }
    fn prune_observations(&mut self, broker: &Broker) {
        self.observations
            .bindings
            .retain(|_, binding| !broker.retire_if_idle(&binding.publisher));
    }
    fn observation_clock(&self) -> String {
        format!("controllers:{}", self.session)
    }
    fn observation_snapshot(&self, id: &str, at: u64) -> Value {
        let source = &self.sources[id];
        json!({"session":self.session,"source":source.summary(),"identity":source.saved.identity,
            "diagnostics":source.diagnostics,"controls":source.controls.iter().map(|control| {
                json!({"capability":control,"value":if source.ready() {source.values.get(&control.id)} else {None}})
            }).collect::<Vec<_>>(),"offset":0,"total":source.controls.len(),"limit":MAX_CONTROLS,
            "cursor":source.cursor.to_string(),"capturedUs":at.to_string()})
    }
    /// Reports update cached descriptor slots, never reacquire native descriptors.
    pub(super) fn publish_observation(
        &mut self,
        id: &str,
        reason: &str,
        captured: Option<&Event>,
        descriptors: bool,
    ) {
        // Capture survives periods without observers; metadata and subscription
        // never make an old sample appear newer.
        if let Some(source) = self.sources.get_mut(id) {
            if let Some(event) = captured {
                source.captured_us = event.captured_us;
                source.cursor = event.cursor;
            } else if reason == "baseline" || reason == "change" {
                source.captured_us = self.journal.now();
                source.cursor = self.journal.cursor();
            }
        }
        let Some(mut binding) = self.observations.bindings.remove(id) else {
            return;
        };
        let Some(source) = self.sources.get(id) else {
            binding.publisher.close();
            self.observations.bindings.insert(id.into(), binding);
            return;
        };
        let at = source.captured_us;
        let cursor = source.cursor;
        let replaced = binding.generation != source.generation;
        let ready = source.ready();
        if descriptors || replaced {
            binding.snapshot = self.observation_snapshot(id, at);
        } else {
            binding.snapshot["source"] = source.summary();
            binding.snapshot["diagnostics"] = json!(source.diagnostics);
            binding.snapshot["cursor"] = json!(cursor.to_string());
            binding.snapshot["capturedUs"] = json!(at.to_string());
            for slot in binding.snapshot["controls"]
                .as_array_mut()
                .expect("snapshot controls")
            {
                let value = slot["capability"]["id"]
                    .as_str()
                    .and_then(|id| ready.then(|| source.values.get(id)).flatten());
                slot["value"] = json!(value);
            }
        }
        let result = if replaced {
            binding.generation = source.generation;
            binding
                .publisher
                .replace_generation(
                    source.generation.saturating_add(1),
                    binding.snapshot.clone(),
                    at,
                    &self.observation_clock(),
                )
                .map(|_| ())
        } else {
            let values = if source.ready() {
                source.values.clone()
            } else {
                BTreeMap::new()
            };
            let change = json!({"session":self.session,"source":source.summary(),"reason":reason,
                "values":values,"cursor":cursor.to_string(),"capturedUs":at.to_string()});
            binding
                .publisher
                .publish(
                    binding.publisher.generation(),
                    binding.snapshot.clone(),
                    change,
                    at,
                    &self.observation_clock(),
                )
                .map(|_| ())
        };
        if result.is_err() {
            binding.publisher.close();
        }
        self.observations.bindings.insert(id.into(), binding);
    }
    pub(super) fn observation_metadata(&mut self) {
        let ids: Vec<_> = self.observations.bindings.keys().cloned().collect();
        for id in ids {
            self.publish_observation(&id, "metadata", None, true);
        }
    }
}

#[cfg(test)]
#[path = "observation_tests.rs"]
mod tests;
