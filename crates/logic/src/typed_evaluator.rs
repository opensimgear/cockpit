//! The same restricted interpreter, with a lossless typed context and result.
use crate::typed::{Definition, Invocation};
use osg_mapping::engine::typed::ScriptTransaction;
use rhai::{Dynamic, Engine, Map};
use serde_json::json;

pub(crate) struct Program {
    definition: Definition,
    evaluator: crate::evaluator::Program,
}
impl Program {
    pub(crate) fn compile(definition: Definition) -> Result<Self, String> {
        definition.validate()?;
        let evaluator = crate::evaluator::Program::compile_typed(&definition.source)?;
        Ok(Self {
            definition,
            evaluator,
        })
    }
    pub(crate) fn call(&self, request: &Invocation) -> Result<ScriptTransaction, String> {
        self.definition.validate_invocation(request)?;
        // Tickets, dependency identities and routes are host authority, never
        // editable script context. Only this node's declared samples are visible.
        let context = json!({"inputs":request.inputs,"parameters":request.parameters,
            "state":request.state,"nowUs":request.at_us.0 as i64,"eventTrigger":request.event_trigger});
        let result = self.evaluator.call_context(&request.callback, context)?;
        let transaction: ScriptTransaction =
            rhai::serde::from_dynamic(&result).map_err(|e| e.to_string())?;
        self.definition
            .validate_transaction(request, &transaction)?;
        Ok(transaction)
    }
}
fn map(value: serde_json::Value) -> Map {
    rhai::serde::to_dynamic(value)
        .expect("bounded helper literal")
        .cast()
}
pub(crate) fn register(engine: &mut Engine) {
    engine.register_fn("scalar", |value: bool| -> Map {
        map(json!({"kind":"scalar","value":value}))
    });
    engine.register_fn("scalar", |value: i64| -> Map {
        map(json!({"kind":"scalar","value":value}))
    });
    engine.register_fn("scalar", |value: f64| -> Map {
        map(json!({"kind":"scalar","value":value}))
    });
    engine.register_fn("scalar", |value: rhai::ImmutableString| -> Map {
        map(json!({"kind":"scalar","value":value.as_str()}))
    });
    engine.register_fn(
        "argument",
        |id: rhai::ImmutableString, value: Dynamic| -> Map {
            let mut result = Map::new();
            result.insert("id".into(), id.into());
            result.insert("value".into(), value);
            result
        },
    );
    engine.register_fn("action", |arguments: rhai::Array| -> Map {
        let mut value = Map::new();
        value.insert("arguments".into(), arguments.into());
        let mut result = Map::new();
        result.insert("kind".into(), "action-pack".into());
        result.insert("value".into(), value.into());
        result
    });
    engine.register_fn(
        "after",
        |id: rhai::ImmutableString, delay: i64, outputs: Map| -> Map {
            let mut result = Map::new();
            result.insert("id".into(), id.into());
            result.insert("afterUs".into(), delay.to_string().into());
            result.insert("outputs".into(), outputs.into());
            result
        },
    );
    engine.register_fn(
        "transaction",
        |state: Map, outputs: Map, timers: rhai::Array| -> Map {
            let mut result = Map::new();
            result.insert("state".into(), state.into());
            result.insert("outputs".into(), outputs.into());
            result.insert("timers".into(), timers.into());
            result
        },
    );
}

#[cfg(test)]
#[path = "typed_evaluator_tests.rs"]
mod tests;
