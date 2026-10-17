//! Lossless script process contracts for explicit typed graph nodes.
use osg_mapping::{
    engine::typed::{self, ScriptInvocation, ScriptTicket, ScriptTransaction},
    program::{Node, Operation, SignalSample, SignalValue},
    signal::{ActionSemantics, Direction, PortContract, PortDescriptor, PortValue},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Definition {
    pub node_id: String,
    pub source: String,
    pub inputs: Vec<PortDescriptor>,
    pub outputs: Vec<PortDescriptor>,
    pub parameters: BTreeMap<String, PortValue>,
}
impl Definition {
    pub fn from_node(node: &Node) -> Result<Self, String> {
        let Operation::Script { source, parameters } = &node.operation else {
            return Err("A typed helper requires an explicit Script node.".into());
        };
        let definition = Self {
            node_id: node.id.clone(),
            source: source.clone(),
            parameters: parameters.clone(),
            inputs: node
                .inputs
                .iter()
                .map(|input| PortDescriptor {
                    id: input.port_id.clone(),
                    name: input.port_id.clone(),
                    direction: Direction::Input,
                    contract: input.contract.clone(),
                })
                .collect(),
            outputs: node.outputs.clone(),
        };
        definition.validate()?;
        Ok(definition)
    }
    pub fn validate(&self) -> Result<(), String> {
        if !osg_mapping::profile::identifier(&self.node_id) {
            return Err("Invalid typed script node identity.".into());
        }
        osg_mapping::program::validate_operation(
            &Operation::Script {
                source: self.source.clone(),
                parameters: self.parameters.clone(),
            },
            &self.inputs,
            &self.outputs,
        )?;
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len()
            > osg_mapping::script::IPC_LIMIT - 128
        {
            return Err("Typed script definition exceeds its IPC budget.".into());
        }
        Ok(())
    }
    pub fn validate_invocation(&self, request: &Invocation) -> Result<(), String> {
        if request.node_id != self.node_id
            || request.ticket.producer.node_id != self.node_id
            || !matches!(request.callback.as_str(), "on_start" | "on_event")
            || request.callback == "on_start" && request.event_trigger
            || request.parameters != self.parameters
            || request.inputs.len() != self.inputs.len()
            || request
                .inputs
                .keys()
                .any(|id| !self.inputs.iter().any(|port| &port.id == id))
            || request.ticket.sequence.0 == 0
            || request.ticket.producer.run.generation.0 == 0
            || uuid_like(&request.ticket.producer.run.session).is_err()
            || request.at_us.0 > i64::MAX as u64
        {
            return Err("Typed script invocation differs from its immutable declaration.".into());
        }
        for port in &self.inputs {
            if matches!(&request.inputs[&port.id], SignalSample::Known(value) if !typed::accepts(&port.contract, value))
            {
                return Err("A script input exceeds its declared contract.".into());
            }
        }
        scalars(&request.state)?;
        budget(request)
    }
    pub fn validate_transaction(
        &self,
        request: &Invocation,
        transaction: &ScriptTransaction,
    ) -> Result<(), String> {
        scalars(&transaction.state)?;
        if transaction.timers.len() > osg_mapping::engine::MAX_TIMERS
            || serde_json::to_vec(&transaction.state)
                .map_err(|e| e.to_string())?
                .len()
                > osg_mapping::script::STATE_LIMIT
        {
            return Err("Typed script state or timer budget exceeded.".into());
        }
        self.validate_outputs(&transaction.outputs, request.event_trigger)?;
        let mut ids = BTreeSet::new();
        for timer in &transaction.timers {
            osg_mapping::signal::text(&timer.id, 80)?;
            if !ids.insert(&timer.id)
                || !(1..=60_000_000).contains(&timer.after_us.0)
                || request.at_us.0.checked_add(timer.after_us.0).is_none()
            {
                return Err("Invalid or duplicate typed script timer.".into());
            }
            self.validate_outputs(&timer.outputs, request.event_trigger)?;
        }
        budget(transaction)
    }
    fn validate_outputs(
        &self,
        outputs: &BTreeMap<String, SignalValue>,
        event_trigger: bool,
    ) -> Result<(), String> {
        for (id, value) in outputs {
            let port = self
                .outputs
                .iter()
                .find(|port| &port.id == id)
                .ok_or("A script wrote an undeclared output.")?;
            if !typed::accepts(&port.contract, value)
                || !event_trigger
                    && matches!(
                        port.contract,
                        PortContract::Invocation {
                            semantics: ActionSemantics::OneShot,
                            ..
                        }
                    )
            {
                return Err("A script output exceeds its contract or event authority.".into());
            }
        }
        Ok(())
    }
}
// Canonical UUID validation without adding a second identity dependency.
fn uuid_like(value: &str) -> Result<(), String> {
    osg_mapping::runtime::Runtime::owned_session(value).map(|_| ())
}
fn scalars(values: &BTreeMap<String, PortValue>) -> Result<(), String> {
    for (id, value) in values {
        osg_mapping::signal::text(id, 80)?;
        if matches!(value, PortValue::Number(value) if !value.is_finite() || value.abs() > osg_mapping::signal::MAX_SAFE_INTEGER as f64)
        {
            return Err("Typed script state needs finite safe scalar values.".into());
        }
    }
    if serde_json::to_vec(values).map_err(|e| e.to_string())?.len()
        > osg_mapping::script::STATE_LIMIT
    {
        return Err("Typed script state budget exceeded.".into());
    }
    Ok(())
}
fn budget(value: &impl Serialize) -> Result<(), String> {
    if serde_json::to_vec(value).map_err(|e| e.to_string())?.len()
        > osg_mapping::script::IPC_LIMIT - 128
    {
        Err("Typed script process payload exceeds its bounded frame.".into())
    } else {
        Ok(())
    }
}

/// A strict wire twin: the reducer's invocation is intentionally Serialize-only.
/// Fields retain the canonical types; there is no legacy controller projection.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Invocation {
    pub node_id: String,
    pub ticket: ScriptTicket,
    pub at_us: osg_mapping::engine::Counter,
    pub callback: String,
    pub event_trigger: bool,
    pub inputs: BTreeMap<String, SignalSample>,
    pub parameters: BTreeMap<String, PortValue>,
    pub state: BTreeMap<String, PortValue>,
}
impl From<ScriptInvocation> for Invocation {
    fn from(value: ScriptInvocation) -> Self {
        Self {
            node_id: value.node_id,
            ticket: value.ticket,
            at_us: value.at_us,
            callback: value.callback,
            event_trigger: value.event_trigger,
            inputs: value.inputs,
            parameters: value.parameters,
            state: value.state,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum Request {
    Compile { definition: Definition },
    Invoke { request: Invocation },
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum Reply {
    Compiled,
    Completed {
        ticket: ScriptTicket,
        result: Result<ScriptTransaction, String>,
    },
    Error {
        message: String,
    },
}

#[cfg(test)]
#[path = "typed_tests.rs"]
mod tests;
