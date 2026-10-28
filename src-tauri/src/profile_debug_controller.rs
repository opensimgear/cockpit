//! Volatile, owner-fenced input for authored debug controllers. No persistence.
use crate::profile_session::RunOwner;
use osg_mapping::{
    engine::{
        typed::{Change as FrameChange, Frame, Phase},
        Counter, Reason,
    },
    program::SignalSample,
};
use osg_profiles::{
    compiler_models::ExecutionPlan, Port, PortContract, PortValue, ProfileDocument, ResourceKind,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Change {
    pub port_id: String,
    pub value: PortValue,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Request {
    command: String,
    pub owner: RunOwner,
    pub node_id: String,
    stream_id: Option<String>,
    sequence: Option<Counter>,
    changes: Option<Vec<Change>>,
}
impl Request {
    pub(crate) fn parse(value: &Value) -> Result<Self, String> {
        let request: Self = serde_json::from_value(value.clone())
            .map_err(|error| format!("Invalid debug controller request: {error}"))?;
        let uuid = |value: &str| {
            uuid::Uuid::parse_str(value)
                .ok()
                .is_some_and(|id| id.to_string() == value)
        };
        if !uuid(&request.owner.session)
            || !uuid(&request.owner.profile_id)
            || request.owner.generation == 0
            || request.owner.generation > osg_profiles::MAX_REVISION
            || request.owner.profile_revision == 0
            || request.owner.profile_revision > osg_profiles::MAX_REVISION
            || request.node_id.is_empty()
            || request.node_id.len() > 80
            || !request.node_id.as_bytes()[0].is_ascii_alphabetic()
            || !request
                .node_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        {
            return Err("Choose an exact applied owner and debug controller.".into());
        }
        match request.command.as_str() {
            "profile_debug_controller_status"
                if request.stream_id.is_none()
                    && request.sequence.is_none()
                    && request.changes.is_none() => {}
            "profile_debug_controller_input"
                if request.stream_id.as_deref().is_some_and(uuid)
                    && request.sequence.is_some_and(|sequence| sequence.0 > 0)
                    && request
                        .changes
                        .as_ref()
                        .is_some_and(|changes| !changes.is_empty() && changes.len() <= 128) => {}
            _ => return Err("Use debug status or an ordered bounded input update.".into()),
        }
        Ok(request)
    }
    pub(crate) fn is_input(&self) -> bool {
        self.command == "profile_debug_controller_input"
    }
}

struct Stream {
    sequence: u64,
    changes: Vec<Change>,
}
struct Source {
    ports: Vec<Port>,
    values: BTreeMap<String, PortValue>,
    used: BTreeSet<String>,
    sequence: u64,
    streams: BTreeMap<String, Stream>,
}
#[derive(Default)]
pub(crate) struct DebugInputs {
    sources: BTreeMap<String, Source>,
    pending: VecDeque<Frame>,
    generation: u64,
}

/// Inventory still binds every external source with its exact preparation pin.
/// Authored debug sources remain wholly inside the native profile owner.
pub(crate) fn bind_delivery(
    resources: &crate::profile_resources::Runtime,
    prepared: &ExecutionPlan,
    document: &ProfileDocument,
) -> Result<ExecutionPlan, String> {
    let debug = document
        .resources
        .iter()
        .filter(|resource| resource.kind == ResourceKind::DebugController)
        .map(|resource| resource.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut external = prepared.clone();
    external
        .sources
        .retain(|source| !debug.contains(source.endpoint.node_id.as_str()));
    let mut delivery = resources.bind_delivery_reads(&external)?;
    delivery.sources.extend(
        prepared
            .sources
            .iter()
            .filter(|source| debug.contains(source.endpoint.node_id.as_str()))
            .cloned(),
    );
    delivery.sources.sort_by(|a, b| {
        (&a.endpoint.node_id, &a.endpoint.port_id).cmp(&(&b.endpoint.node_id, &b.endpoint.port_id))
    });
    Ok(delivery)
}
impl DebugInputs {
    pub(crate) fn capture(
        document: &ProfileDocument,
        plan: &ExecutionPlan,
        generation: u64,
        at: u64,
    ) -> Result<(Self, Vec<Frame>), String> {
        let mut inputs = Self {
            generation,
            ..Self::default()
        };
        let mut baselines = vec![];
        for resource in document
            .resources
            .iter()
            .filter(|resource| resource.kind == ResourceKind::DebugController)
        {
            let values = resource
                .ports
                .iter()
                .map(|port| {
                    Ok((
                        port.id.clone(),
                        osg_profiles::debug_controller::neutral(port)?,
                    ))
                })
                .collect::<Result<BTreeMap<_, _>, String>>()?;
            let used = plan
                .sources
                .iter()
                .filter(|source| source.endpoint.node_id == resource.id)
                .map(|source| source.endpoint.port_id.clone())
                .collect::<BTreeSet<_>>();
            let sequence = if used.is_empty() { 0 } else { 1 };
            if !used.is_empty() {
                baselines.push(Frame {
                    source: resource.id.clone(),
                    generation: Counter(generation),
                    sequence: Counter(sequence),
                    captured_us: Counter(at),
                    reason: Reason::Baseline,
                    changes: used
                        .iter()
                        .map(|port_id| FrameChange {
                            port_id: port_id.clone(),
                            sample: SignalSample::scalar(values[port_id].clone()),
                        })
                        .collect(),
                });
            }
            inputs.sources.insert(
                resource.id.clone(),
                Source {
                    ports: resource.ports.clone(),
                    values,
                    used,
                    sequence,
                    streams: BTreeMap::new(),
                },
            );
        }
        Ok((inputs, baselines))
    }
    pub(crate) fn request(&mut self, request: &Request, at: u64) -> Result<Value, String> {
        let source = self
            .sources
            .get_mut(&request.node_id)
            .ok_or("Choose a debug controller in this applied profile variant.")?;
        let mut replay = false;
        if request.is_input() {
            let stream_id = request.stream_id.as_ref().unwrap();
            let sequence = request.sequence.unwrap().0;
            let changes = request.changes.as_ref().unwrap();
            if let Some(stream) = source.streams.get(stream_id) {
                if sequence < stream.sequence
                    || sequence == stream.sequence && *changes != stream.changes
                {
                    return Err(
                        "The debug input sequence is stale or was reused with different changes."
                            .into(),
                    );
                }
                replay = sequence == stream.sequence;
            } else if source.streams.len() >= 16 {
                return Err("This debug controller already has sixteen input streams.".into());
            }
            if !replay {
                let mut seen = BTreeSet::new();
                for change in changes {
                    let port = source
                        .ports
                        .iter()
                        .find(|port| port.id == change.port_id)
                        .ok_or("Unknown debug controller port.")?;
                    let PortContract::State { schema, .. } = &port.contract else {
                        return Err("Debug inputs require state ports.".into());
                    };
                    if !seen.insert(&change.port_id) || !schema.accepts(&change.value) {
                        return Err(
                            "Debug input contains a duplicate port or an invalid control value."
                                .into(),
                        );
                    }
                }
                let frame_changes = changes
                    .iter()
                    .filter(|change| source.used.contains(&change.port_id))
                    .map(|change| FrameChange {
                        port_id: change.port_id.clone(),
                        sample: SignalSample::scalar(change.value.clone()),
                    })
                    .collect::<Vec<_>>();
                if !frame_changes.is_empty() && self.pending.len() >= 256 {
                    return Err(
                        "The debug input queue is full; wait for native delivery before retrying."
                            .into(),
                    );
                }
                let next = source
                    .sequence
                    .checked_add(1)
                    .ok_or("Debug source sequence exhausted.")?;
                if !frame_changes.is_empty() {
                    self.pending.push_back(Frame {
                        source: request.node_id.clone(),
                        generation: Counter(self.generation),
                        sequence: Counter(next),
                        captured_us: Counter(at),
                        reason: Reason::Change,
                        changes: frame_changes,
                    });
                    source.sequence = next;
                }
                for change in changes {
                    source
                        .values
                        .insert(change.port_id.clone(), change.value.clone());
                }
                source.streams.insert(
                    stream_id.clone(),
                    Stream {
                        sequence,
                        changes: changes.clone(),
                    },
                );
            }
        }
        Ok(
            json!({"owner":request.owner,"nodeId":request.node_id,"values":source.values.iter().map(|(port_id,value)|json!({"portId":port_id,"value":value})).collect::<Vec<_>>(),"streamId":request.stream_id,"sequence":request.sequence,"replayed":replay,"pendingFrames":self.pending.len()}),
        )
    }
    pub(crate) fn poll(&mut self) -> Vec<Frame> {
        self.pending.drain(..).collect()
    }
    pub(crate) fn refresh_liveness(&self, reducer: &mut Phase, now: Counter) -> Result<(), String> {
        for (id, source) in &self.sources {
            if !source.used.is_empty() {
                reducer.observe_liveness(id, Counter(self.generation), now)?;
            }
        }
        Ok(())
    }
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
#[path = "profile_debug_controller_tests.rs"]
mod tests;
