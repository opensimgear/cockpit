//! Immutable node declarations plus bounded value-only reducer captures.
use crate::profile_session::{OwnedRun, RunOwner};
use osg_mapping::{engine::typed::Phase, program::{Endpoint as RuntimeEndpoint, SignalSample, SignalValue}};
use osg_profiles::{Direction, Endpoint, Port, PortContract, PortValue, ProfileDocument, session::SnapshotSummary};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, mem::size_of, sync::Arc};

pub(crate) const MAX_PAYLOAD_BYTES: usize = 1024 * 1024 - 2048;
const MAX_PORTS: usize = 128;

/// JSON array order is part of the topic contract shared with browser clients.
pub(crate) fn topic(owner: &RunOwner, node_id: &str) -> String {
    let identity = json!([owner.session, owner.generation, owner.profile_id,
        owner.profile_revision, owner.game_id, node_id]);
    format!("profiles.samples.{:x}", Sha256::digest(serde_json::to_vec(&identity).expect("typed owner identity")))
}

#[derive(Debug)]
struct PortPlan {
    endpoint: Endpoint,
    direction: Direction,
    contract: PortContract,
    observed: Option<RuntimeEndpoint>,
}
#[derive(Debug)]
struct NodeTemplate {node_id:String,ports:Arc<Vec<PortPlan>>}
#[derive(Debug)]
pub(crate) struct NodeTemplates {
    profile_id:String,
    revision:u64,
    source_hash:String,
    nodes:Vec<NodeTemplate>,
}
#[derive(Debug)]
pub(crate) struct NodePlan {
    pub owner: RunOwner,
    pub node_id: String,
    pub topic: String,
    applied_snapshot: SnapshotSummary,
    program_hash: String,
    ports: Arc<Vec<PortPlan>>,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CapturedSamples {
    pub revision: u64,
    pub captured_us: u64,
    pub samples: Vec<SignalSample>,
}
impl NodeTemplates {
    /// Static preparation runs outside authority locks. The templates retain
    /// every immutable descriptor and the source semantic hash exactly once.
    pub(crate) fn prepare(document: &ProfileDocument, source: &ProfileDocument) -> Result<Arc<Self>, String> {
        if source.id != document.id || source.revision != document.revision {
            return Err("The sample declarations do not belong to the immutable source revision.".into());
        }
        if document.resources.len()>osg_profiles::validation::MAX_RESOURCES
            || document.processing_nodes.len()>osg_profiles::validation::MAX_PROCESSING_NODES
            || document.virtual_outputs.len()>osg_profiles::validation::MAX_VIRTUAL_OUTPUTS {
            return Err("The applied sample node count exceeds the immutable graph bound.".into());
        }
        let source_hash = osg_profiles::semantic_hash(source)?;
        let declarations = document.resources.iter().map(|node| (node.id.as_str(), node.ports.iter().collect::<Vec<_>>()))
            .chain(document.processing_nodes.iter().map(|node| (node.id.as_str(), node.inputs.iter().chain(&node.outputs).collect::<Vec<_>>())))
            .chain(document.virtual_outputs.iter().map(|node| (node.id.as_str(), node.ports.iter().collect::<Vec<_>>())));
        let mut nodes = Vec::new();let mut total = 0usize;let mut ids = BTreeSet::new();
        for (node_id,mut declarations) in declarations {
            total = total.checked_add(declarations.len()).ok_or("Applied sample port count overflow.")?;
            if declarations.len()>MAX_PORTS || total>1024 || !ids.insert(node_id) {
                return Err("The applied nodes exceed their bounded sample declaration.".into());
            }
            declarations.sort_by(|a, b| a.id.cmp(&b.id));
            let ports = declarations.into_iter().map(|port: &Port| {
                let endpoint = Endpoint { node_id: node_id.into(), port_id: port.id.clone() };
                let observed = if port.direction == Direction::Input {
                    document.bindings.iter().find(|binding| binding.to == endpoint).map(|binding| &binding.from)
                } else { Some(&endpoint) }.map(|from| RuntimeEndpoint {node_id:from.node_id.clone(),port_id:from.port_id.clone()});
                PortPlan {endpoint, direction:port.direction, contract:port.contract.clone(), observed}
            }).collect::<Vec<_>>();
            nodes.push(NodeTemplate {node_id:node_id.into(),ports:Arc::new(ports)});
        }
        Ok(Arc::new(Self {profile_id:document.id.clone(),revision:document.revision,source_hash,nodes}))
    }
    /// Bind an exact owner using cached metadata and shared descriptors. Only
    /// the small canonical topic identities are hashed while authority is held.
    pub(crate) fn bind(&self, run: &OwnedRun, requested_topic: &str) -> Result<Arc<NodePlan>, String> {
        if self.profile_id != run.owner.profile_id || self.revision != run.owner.profile_revision
            || self.profile_id != run.snapshot.profile_id || self.revision != run.snapshot.revision
            || self.source_hash != run.snapshot.semantic_hash || run.owner.generation==0
            || uuid::Uuid::parse_str(&run.owner.session).ok().is_none_or(|id| id.to_string()!=run.owner.session) {
            return Err("The samples do not belong to this exact immutable applied revision.".into());
        }
        let node = self.nodes.iter().find(|node| topic(&run.owner,&node.node_id)==requested_topic)
            .ok_or("Choose a declared node of the current exact applied owner.")?;
        Ok(Arc::new(NodePlan {owner:run.owner.clone(),node_id:node.node_id.clone(),topic:requested_topic.into(),
            applied_snapshot:run.snapshot.clone(),program_hash:run.program_hash.clone(),ports:node.ports.clone()}))
    }
}
impl NodePlan {
    /// Convenience for isolated schema callers; native runs retain templates
    /// prepared off their authority lock and invoke only NodeTemplates::bind.
    pub(crate) fn prepare(document: &ProfileDocument, source: &ProfileDocument,
        run: &OwnedRun, requested_topic: &str) -> Result<Arc<Self>, String> {
        NodeTemplates::prepare(document,source)?.bind(run,requested_topic)
    }
    pub(crate) fn port_count(&self) -> usize {self.ports.len()}
    /// Only cached values are copied here. No document/contract cloning, hash,
    /// JSON encoding, source polling, clock advancement or output draining.
    pub(crate) fn capture(&self, phase: &Phase, run: &OwnedRun) -> Result<CapturedSamples, String> {
        let token = phase.run_token_ref();
        if !phase.active() || self.owner != run.owner || self.applied_snapshot != run.snapshot
            || self.program_hash != run.program_hash || token.session != self.owner.session
            || token.generation.0 != self.owner.generation {
            return Err("The node sample owner expired or its reducer stopped.".into());
        }
        let revision = phase.sample_revision().ok_or("The reducer sample revision exhausted its observation bound.")?;
        let unknown = SignalSample::Unknown;
        let values = self.ports.iter().map(|port| port.observed.as_ref()
            .and_then(|endpoint| phase.sample(endpoint)).unwrap_or(&unknown)).collect::<Vec<_>>();
        let mut bytes = size_of::<CapturedSamples>() + values.len() * size_of::<SignalSample>();
        // Account for every owned allocation before any SignalSample clone.
        for sample in &values {
            bytes = bytes.checked_add(sample_bytes(sample)?).ok_or("Node sample allocation overflow.")?;
            if bytes > MAX_PAYLOAD_BYTES { return Err("Node samples exceed the bounded capture budget.".into()); }
        }
        Ok(CapturedSamples {revision,captured_us:phase.sample_revision_us().0,
            samples:values.into_iter().cloned().collect()})
    }
    /// Called off the authority lock; descriptors become transport-owned here.
    pub(crate) fn encode(&self, capture: &CapturedSamples) -> Result<Value, String> {
        if capture.revision == 0 || capture.samples.len() != self.ports.len() {
            return Err("Node samples do not match their immutable declaration.".into());
        }
        let ports = self.ports.iter().zip(&capture.samples).map(|(port, sample)| json!({
            "endpoint":port.endpoint,"direction":port.direction,"contract":port.contract,"sample":sample,
        })).collect::<Vec<_>>();
        let payload = json!({"owner":self.owner,"nodeId":self.node_id,"appliedSnapshot":self.applied_snapshot,
            "programHash":self.program_hash,"ports":ports});
        if serde_json::to_vec(&payload).map_err(|error| error.to_string())?.len() > MAX_PAYLOAD_BYTES {
            return Err("Node samples exceed the bounded stream payload.".into());
        }
        Ok(payload)
    }
}
fn sample_bytes(sample: &SignalSample) -> Result<usize, String> {
    let scalar = |value: &PortValue| if let PortValue::Text(text) = value { text.len() } else { 0 };
    match sample {
        SignalSample::Known(SignalValue::Scalar(value)) => Ok(scalar(value)),
        SignalSample::Known(SignalValue::ActionPack(pack)) => {
            let mut bytes = pack.arguments.len().checked_mul(size_of::<osg_mapping::program::NamedArgument>())
                .ok_or("Node action allocation overflow.")?;
            for argument in &pack.arguments {
                bytes = bytes.checked_add(argument.id.len()).and_then(|bytes| bytes.checked_add(scalar(&argument.value)))
                    .ok_or("Node action allocation overflow.")?;
            }
            Ok(bytes)
        }
        _ => Ok(0),
    }
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
