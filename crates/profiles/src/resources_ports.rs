//! Provider catalogue contracts use the same canonical types as portable graph ports.
use crate::Port;
pub use osg_mapping::signal::*;

/// Local assignment pins the complete observed contract, including freshness and fallback.
pub fn graph_compatible(observed: &PortDescriptor, graph: &Port) -> bool {
    fn semantic_contract(contract: &PortContract) -> PortContract {
        let mut contract = contract.clone();
        if let PortContract::Action { parameters, .. }
        | PortContract::Invocation { parameters, .. } = &mut contract
        {
            for parameter in parameters {
                parameter.name = parameter.id.clone();
            }
        }
        contract
    }
    observed.direction == graph.direction
        && semantic_contract(&observed.contract) == semantic_contract(&graph.contract)
}
#[cfg(test)]
#[path = "resources_ports_tests.rs"]
mod tests;
