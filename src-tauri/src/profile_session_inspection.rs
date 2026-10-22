//! Read-only projection of one actual applied reducer. Requests never poll a
//! provider, advance execution, drain effects, change ownership or save a draft.
use super::{Coordinator, Host, OwnedRun, RunOwner};
use osg_mapping::{
    engine::typed::Phase,
    program::{Endpoint as RuntimeEndpoint, SignalSample},
};
use osg_profiles::{
    session::SnapshotSummary, Direction, Endpoint, Port, PortContract, ProfileDocument,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Request {
    command: String,
    pub owner: RunOwner,
    node_id: Option<String>,
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_limit() -> usize {
    32
}
impl Request {
    pub(crate) fn parse(value: &serde_json::Value) -> Result<Self, String> {
        let request: Self = serde_json::from_value(value.clone())
            .map_err(|error| format!("Invalid applied-port inspection: {error}"))?;
        let uuid = |value: &str| {
            uuid::Uuid::parse_str(value)
                .ok()
                .is_some_and(|id| id.to_string() == value)
        };
        if request.command != "profile_runtime_inspect"
            || request.offset > 1024
            || !(1..=64).contains(&request.limit)
            || !uuid(&request.owner.session)
            || !uuid(&request.owner.profile_id)
            || request.owner.generation == 0
            || request.owner.generation > osg_profiles::MAX_REVISION
            || request.owner.profile_revision == 0
            || request.owner.profile_revision > osg_profiles::MAX_REVISION
            || request.owner.game_id.as_deref().is_some_and(|id| !osg_profiles::activation::valid_game_id(id))
            || request.node_id.as_deref().is_some_and(|id| {
                id.is_empty()
                    || id.len() > 80
                    || !id.as_bytes()[0].is_ascii_alphabetic()
                    || !id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
            })
        {
            return Err("Inspect an exact applied owner and a bounded declared-port page.".into());
        }
        Ok(request)
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InspectedPort {
    endpoint: Endpoint,
    direction: Direction,
    contract: PortContract,
    sample: SignalSample,
    observed_us: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Page {
    schema_version: u32,
    owner: RunOwner,
    applied_snapshot: SnapshotSummary,
    program_hash: String,
    captured_us: String,
    offset: usize,
    total: usize,
    ports: Vec<InspectedPort>,
}
impl<H: Host> Coordinator<H> {
    pub(crate) fn inspect(&self, request: &Request) -> Result<Page, String> {
        if !self.validates(&request.owner) {
            return Err(
                "The requested applied owner expired; inspect the current running revision.".into(),
            );
        }
        let run = self
            .manifest
            .active
            .as_ref()
            .filter(|run| run.owner == request.owner)
            .ok_or("The native applied manifest no longer owns this revision.")?;
        self.host.inspect(run, request)
    }
}
pub(crate) fn capture(
    document: &ProfileDocument,
    phase: &Phase,
    run: &OwnedRun,
    request: &Request,
) -> Result<Page, String> {
    capture_variant(document,document,phase,run,request)
}
pub(crate) fn capture_variant(
    document: &ProfileDocument,
    source: &ProfileDocument,
    phase: &Phase,
    run: &OwnedRun,
    request: &Request,
) -> Result<Page, String> {
    let token = phase.run_token();
    if request.owner != run.owner
        || token.session != run.owner.session
        || token.generation.0 != run.owner.generation
        || document.id != run.owner.profile_id
        || document.revision != run.owner.profile_revision
        || run.snapshot.profile_id != document.id
        || run.snapshot.revision != document.revision
        || source.id != document.id || source.revision != document.revision
        || osg_profiles::semantic_hash(source)? != run.snapshot.semantic_hash
        || !phase.active()
    {
        return Err("The reducer does not belong to this exact immutable applied revision.".into());
    }
    let mut ports: Vec<(&str, &Port)> = document
        .resources
        .iter()
        .flat_map(|node| node.ports.iter().map(move |port| (node.id.as_str(), port)))
        .chain(document.processing_nodes.iter().flat_map(|node| {
            node.inputs
                .iter()
                .chain(&node.outputs)
                .map(move |port| (node.id.as_str(), port))
        }))
        .chain(
            document
                .virtual_outputs
                .iter()
                .flat_map(|node| node.ports.iter().map(move |port| (node.id.as_str(), port))),
        )
        .filter(|(node, _)| {
            request
                .node_id
                .as_deref()
                .is_none_or(|selected| selected == *node)
        })
        .collect();
    let known_node = request.node_id.as_deref().is_none_or(|selected| {
        document.resources.iter().any(|r| r.id == selected)
            || document.processing_nodes.iter().any(|r| r.id == selected)
            || document.virtual_outputs.iter().any(|r| r.id == selected)
    });
    if !known_node || ports.len() > 1024 || request.offset > ports.len() {
        return Err("Choose a declared applied node and an available bounded port page.".into());
    }
    ports.sort_by(|a, b| (a.0, &a.1.id).cmp(&(b.0, &b.1.id)));
    let mut page = Page {
        schema_version: 1,
        owner: run.owner.clone(),
        applied_snapshot: run.snapshot.clone(),
        program_hash: run.program_hash.clone(),
        captured_us: phase.at_us().0.to_string(),
        offset: request.offset,
        total: ports.len(),
        ports: vec![],
    };
    for (node, port) in ports.into_iter().skip(request.offset).take(request.limit) {
        let endpoint = Endpoint {
            node_id: node.into(),
            port_id: port.id.clone(),
        };
        let observed = if port.direction == Direction::Input {
            document
                .bindings
                .iter()
                .find(|binding| binding.to == endpoint)
                .map(|binding| &binding.from)
        } else {
            Some(&endpoint)
        };
        let sample = observed.and_then(|from| {
            phase.inspected_sample(&RuntimeEndpoint {
                node_id: from.node_id.clone(),
                port_id: from.port_id.clone(),
            })
        });
        let (sample, observed_us) = sample.map_or((SignalSample::Unknown, None), |(sample, at)| {
            (sample.clone(), at.map(|at| at.0.to_string()))
        });
        page.ports.push(InspectedPort {
            endpoint,
            direction: port.direction,
            contract: port.contract.clone(),
            sample,
            observed_us,
        });
        if serde_json::to_vec(&page)
            .map_err(|error| error.to_string())?
            .len()
            > 1_900_000
        {
            page.ports.pop();
            if page.ports.is_empty() {
                return Err("This graph port exceeds the bounded inspection reply.".into());
            }
            break;
        }
    }
    Ok(page)
}

#[cfg(test)]
#[path = "profile_session_inspection_tests.rs"]
mod tests;
