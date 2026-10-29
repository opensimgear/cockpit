//! Expected absence of optional components stays visible in the compiled plan,
//! without making otherwise available profile work unhealthy.
use osg_profiles::{
    compiler_models::{ExecutionPlan, PlanReadiness},
    Endpoint, ProfileDocument,
};
use std::collections::BTreeMap;

fn required(document: &ProfileDocument, id: &str) -> bool {
    document
        .resources
        .iter()
        .find(|resource| resource.id == id)
        .map(|resource| resource.required)
        .or_else(|| {
            document
                .virtual_outputs
                .iter()
                .find(|output| output.id == id)
                .map(|output| output.required)
        })
        .or_else(|| {
            document
                .desktop_outputs
                .iter()
                .find(|output| output.id == id)
                .map(|output| output.required)
        })
        .unwrap_or(true)
}

fn expected_absence(
    plan: &ExecutionPlan,
    document: &ProfileDocument,
    sink: &Endpoint,
    sources: &[Endpoint],
) -> bool {
    let unavailable = plan
        .sources
        .iter()
        .filter(|source| {
            source.readiness == PlanReadiness::Unavailable && sources.contains(&source.endpoint)
        })
        .map(|source| source.endpoint.node_id.as_str())
        .chain(
            plan.sinks
                .iter()
                .filter(|target| {
                    target.readiness == PlanReadiness::Unavailable && target.endpoint == *sink
                })
                .map(|target| target.endpoint.node_id.as_str()),
        )
        .collect::<Vec<_>>();
    !unavailable.is_empty() && unavailable.iter().all(|id| !required(document, id))
}

pub(super) fn admission_degradation(
    plan: &ExecutionPlan,
    document: &ProfileDocument,
    unavailable_peripherals: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut messages = plan
        .branches
        .iter()
        .filter(|branch| match branch.readiness {
            PlanReadiness::Unavailable => {
                !expected_absence(plan, document, &branch.sink, &branch.source_ports)
            }
            PlanReadiness::Blocked => !branch.required,
            _ => false,
        })
        .map(|branch| {
            format!(
                "{}:{} is unavailable; this branch is inactive.",
                branch.sink.node_id, branch.sink.port_id
            )
        })
        .collect::<Vec<_>>();
    messages.extend(
        plan.sources
            .iter()
            .filter(|source| {
                source.readiness == PlanReadiness::Unavailable
                    && required(document, &source.endpoint.node_id)
            })
            .map(|source| {
                format!(
                    "{}:{} is offline; its input is inactive.",
                    source.endpoint.node_id, source.endpoint.port_id
                )
            }),
    );
    messages.extend(
        plan.visibility
            .iter()
            .filter(|intent| {
                intent.readiness == PlanReadiness::Unavailable
                    && required(document, &intent.resource_id)
            })
            .map(|intent| {
                format!(
                    "{} is offline; its visibility setting is inactive.",
                    intent.resource_id
                )
            }),
    );
    messages.extend(
        unavailable_peripherals
            .keys()
            .filter(|id| required(document, id))
            .map(|id| {
                let name = document
                    .resources
                    .iter()
                    .find(|resource| &resource.id == id)
                    .map_or(id.as_str(), |resource| resource.name.as_str());
                format!("{name} is offline; its settings and peripheral controls are inactive.")
            }),
    );
    messages.sort();
    messages.dedup();
    messages
}

#[cfg(test)]
#[path = "health_policy_tests.rs"]
mod tests;
