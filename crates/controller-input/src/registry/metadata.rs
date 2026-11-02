use super::*;
impl Registry {
    /// Publish committed authoring metadata while retaining input samples and
    /// discoveries received during database I/O. Input capture never waits on
    /// the database or receives a restored historical baseline.
    pub(crate) fn commit_metadata(&mut self, before: &Registry, staged: &Registry) {
        for (id, desired) in &staged.sources {
            if before
                .sources
                .get(id)
                .is_some_and(|old| old.saved == desired.saved)
            {
                continue;
            }
            let reassigned = before
                .sources
                .get(id)
                .is_some_and(|old| old.saved.identity != desired.saved.identity);
            if reassigned {
                let observed = before
                    .sources
                    .iter()
                    .find(|(observed, source)| {
                        *observed != id && source.saved.identity == desired.saved.identity
                    })
                    .map(|(id, _)| id.clone());
                if let Some(observed) = observed {
                    let replacement = self.sources.get(&observed).cloned();
                    // The remembered slot may reconnect while persistence is in
                    // flight. Fence every generation either live source or the
                    // staged transaction has seen before installing its identity.
                    let newest = self
                        .sources
                        .get(id)
                        .map_or(0, |source| source.generation)
                        .max(replacement.as_ref().map_or(0, |source| source.generation))
                        .max(desired.generation);
                    // Observation generations reserve the following integer for
                    // their nonzero wire domain, so both increments must fit.
                    let generation = newest.checked_add(2).map(|wire| wire - 1);
                    let Some(generation) = generation else {
                        self.disconnect(id);
                        let message =
                            "Controller generation exhausted; reassignment requires restart.";
                        if let Some(current) = self.sources.get_mut(id) {
                            current.diagnostics = vec![message.into()];
                        }
                        self.diagnostic = message.into();
                        continue;
                    };
                    self.disconnect(&observed);
                    self.sources.remove(&observed);
                    if let Some(mut current) = replacement {
                        current.saved.id = id.clone();
                        current.saved.name = desired.saved.name.clone();
                        current.saved.role = desired.saved.role.clone();
                        current.generation = generation;
                        current.sequence = 0;
                        current.values.clear();
                        self.sources.insert(id.clone(), current);
                        continue;
                    }
                }
            }
            if let Some(current) = self.sources.get_mut(id) {
                current.saved.name = desired.saved.name.clone();
                current.saved.role = desired.saved.role.clone();
            } else {
                let mut offline = desired.clone();
                offline.online = false;
                offline.values.clear();
                offline.sequence = 0;
                self.sources.insert(id.clone(), offline);
            }
        }
        for (id, old) in &before.sources {
            if staged.sources.contains_key(id) {
                continue;
            }
            // A positive reconnect after an offline Forget is a new observation.
            if self
                .sources
                .get(id)
                .is_some_and(|current| current.online && !old.online)
            {
                continue;
            }
            self.disconnect(id);
            self.sources.remove(id);
        }
        self.dirty = self.saved() != staged.saved();
        self.observation_metadata();
    }
}
