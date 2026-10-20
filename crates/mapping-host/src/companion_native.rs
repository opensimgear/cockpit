//! Native profile subscriptions reuse the retained companion feed and its loss,
//! bounded queue and generation rules. They do not create legacy mapping roles.
use super::*;

#[derive(Clone, Debug)]
pub struct NativeSelection {
    pub definition: Definition,
    pub controls: Vec<String>,
}
pub struct NativeFeed {
    feed: Feed,
    refreshed: BTreeMap<String, (u64, u64)>,
}

/// The canonical conversion used by the actual SourceSink registration path.
/// Axis contracts remain integer native ranges; callers must not reinterpret
/// the wire's normalized decimal representation as the delivered graph range.
pub fn native_definition(
    definition: osg_companion_service::SourceDefinition,
) -> Result<Definition, String> {
    use osg_companion_service::{AxisMode, SourceKind};
    let controls = definition
        .controls
        .into_iter()
        .map(|control| {
            let (kind, fallback) = match control.kind {
                SourceKind::Button => (Kind::Button, Value::Button(false)),
                SourceKind::Hat => (Kind::Hat, Value::Hat(None)),
                SourceKind::Axis {
                    axis_mode,
                    fallback,
                } => (
                    Kind::Axis {
                        range: if axis_mode == AxisMode::Bipolar {
                            profile::AxisRange::Bipolar
                        } else {
                            profile::AxisRange::Unipolar
                        },
                    },
                    Value::Axis(osg_companion_service::quantize_axis(
                        fallback, axis_mode, fallback,
                    )?),
                ),
            };
            Ok(ControlDefinition {
                id: control.id,
                kind,
                fallback,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let result = Definition {
        source_id: definition.source_id,
        name: definition.name,
        publication_hash: definition.publication_hash,
        controls,
    };
    validate_definition(&result)?;
    Ok(result)
}

impl CompanionProvider {
    pub fn subscribe_native(&self, selections: &[NativeSelection]) -> Result<NativeFeed, String> {
        if selections.is_empty() || selections.len() > 8 {
            return Err("Select one to eight native companion instances.".into());
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        expire(&mut state, self.now());
        if state.subscriptions.len() >= MAX_SUBSCRIPTIONS {
            return Err("Too many pending companion mapping subscriptions.".into());
        }
        let mut sources = BTreeMap::new();
        for selection in selections {
            validate_definition(&selection.definition)?;
            let instance = state
                .instances
                .get(&selection.definition.source_id)
                .ok_or("Register the verified owned companion descriptor before subscribing.")?;
            if instance.definition != selection.definition {
                return Err(
                    "The selected companion publication or native descriptor changed.".into(),
                );
            }
            if selection.controls.len() > 128 {
                return Err("Choose at most 128 declared controls for each companion.".into());
            }
            let controls: BTreeSet<_> = selection.controls.iter().cloned().collect();
            if controls.len() != selection.controls.len()
                || controls.iter().any(|id| {
                    !instance
                        .definition
                        .controls
                        .iter()
                        .any(|control| &control.id == id)
                })
            {
                return Err(
                    "Native companion controls must be unique and currently declared.".into(),
                );
            }
            if sources
                .insert(
                    selection.definition.source_id.clone(),
                    Queue {
                        controls,
                        publication_hash: selection.definition.publication_hash.clone(),
                        ..Queue::default()
                    },
                )
                .is_some()
            {
                return Err("Select each native companion instance once.".into());
            }
        }
        let id = state
            .serial
            .checked_add(1)
            .ok_or("Companion subscription counter exhausted.")?;
        state.serial = id;
        state
            .subscriptions
            .insert(id, Subscription { sources, next: 0 });
        Ok(NativeFeed {
            feed: Feed {
                provider: self.clone(),
                subscription: id,
            },
            refreshed: BTreeMap::new(),
        })
    }
}
impl NativeFeed {
    /// Diagnostic aliases only; no writer or output authority is exposed.
    pub fn diagnostic_identity(&self) -> (u64, u64) {
        (self.feed.provider.audit_provider, self.feed.subscription)
    }
    /// Discards accumulated transitions and captures the actual current neutral
    /// state. Missing/held/stale inputs are errors, never invented fallbacks.
    pub fn baseline(&mut self) -> Result<Vec<Frame>, String> {
        Ok(self
            .baseline_audited()?
            .into_iter()
            .map(|frame| frame.frame)
            .collect())
    }
    pub fn baseline_audited(&mut self) -> Result<Vec<NativeFrame>, String> {
        let frames = self.feed.initial_audited()?;
        self.feed.require_live_ready()?;
        let state = self
            .feed
            .provider
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(subscription) = state.subscriptions.get(&self.feed.subscription) {
            for id in subscription.sources.keys() {
                let instance = &state.instances[id];
                self.refreshed
                    .insert(id.clone(), (instance.generation, instance.last_seen_us));
            }
        }
        Ok(frames)
    }
    pub fn poll(&mut self) -> Result<Vec<Frame>, String> {
        Ok(self
            .poll_audited()?
            .into_iter()
            .map(|frame| frame.frame)
            .collect())
    }
    pub fn poll_audited(&mut self) -> Result<Vec<NativeFrame>, String> {
        let mut frames = self.feed.drain_audited();
        let mut state = self
            .feed
            .provider
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let ids = state
            .subscriptions
            .get(&self.feed.subscription)
            .ok_or("The native companion subscription ended.")?
            .sources
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for id in ids {
            let instance = &state.instances[&id];
            let marker = (instance.generation, instance.last_seen_us);
            if instance.owner.is_none() || self.refreshed.get(&id) == Some(&marker) {
                continue;
            }
            let generation = instance.generation;
            let at = instance.last_seen_us;
            let publication = instance.definition.publication_hash.clone();
            let values = instance.values.clone();
            let queue = state
                .subscriptions
                .get_mut(&self.feed.subscription)
                .unwrap()
                .sources
                .get_mut(&id)
                .unwrap();
            // Ordered transitions already captured by the shared feed always
            // precede a current-state renewal. A browser poll is not evidence
            // of freshness: only actual native input/baseline/heartbeat time is.
            if !queue.started
                || !queue.frames.is_empty()
                || queue.loss.is_some()
                || queue.restart_required
                || queue.generation != generation
                || queue.publication_hash != publication
            {
                continue;
            }
            let Some(sequence) = queue.sequence.checked_add(1) else {
                lose(
                    &mut state,
                    &id,
                    at,
                    "Native companion subscription sequence exhausted.",
                );
                return Err(
                    "Native companion subscription sequence exhausted; acquire a new feed.".into(),
                );
            };
            queue.sequence = sequence;
            let changes = queue
                .controls
                .iter()
                .map(|control| {
                    values.get(control).copied().map(|value| Change {
                        control: control.clone(),
                        value,
                    })
                })
                .collect::<Option<Vec<_>>>()
                .ok_or("The native companion state is incomplete.")?;
            let frame = Frame {
                source: source(&id),
                generation: osg_mapping::engine::Counter(generation),
                sequence: osg_mapping::engine::Counter(sequence),
                captured_us: osg_mapping::engine::Counter(at),
                reason: osg_mapping::engine::Reason::Change,
                changes,
            };
            frames.push(audit_native_frame(
                self.feed.provider.audit_provider,
                self.feed.subscription,
                queue,
                &id,
                frame,
                None,
                2,
            ));
            self.refreshed.insert(id, marker);
        }
        Ok(frames)
    }
    pub fn validate_baseline(&self) -> Result<(), String> {
        self.feed.require_live_ready()
    }
}

#[cfg(test)]
#[path = "companion_native_tests.rs"]
mod tests;
