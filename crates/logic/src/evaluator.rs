use osg_mapping::script::{Invocation, Transaction};
use rhai::{CallFnOptions, Dynamic, Engine, Map, Scope, AST};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};
pub struct Program {
    engine: Engine,
    ast: AST,
    deadline: Rc<RefCell<Option<Instant>>>,
}
impl Program {
    pub fn compile(source: &str) -> Result<Self, String> {
        Self::compile_mode(source, false)
    }
    pub(crate) fn compile_typed(source: &str) -> Result<Self, String> {
        Self::compile_mode(source, true)
    }
    fn compile_mode(source: &str, typed: bool) -> Result<Self, String> {
        if source.is_empty() || source.len() > 16_384 {
            return Err("Script source must contain 1–16384 bytes.".into());
        }
        let mut engine = Engine::new_raw();
        use rhai::packages::Package;
        rhai::packages::BasicArrayPackage::new().register_into_engine(&mut engine);
        rhai::packages::BasicMapPackage::new().register_into_engine(&mut engine);
        rhai::packages::BasicMathPackage::new().register_into_engine(&mut engine);
        engine.set_optimization_level(rhai::OptimizationLevel::None);
        engine
            .set_max_operations(100_000)
            .set_max_call_levels(16)
            .set_max_expr_depths(32, 32)
            .set_max_string_size(16_384)
            .set_max_array_size(256)
            .set_max_map_size(256);
        engine.set_max_variables(128);
        engine.disable_symbol("eval");
        engine.register_fn("max", |a: i64, b: i64| a.max(b));
        engine.register_fn("min", |a: i64, b: i64| a.min(b));
        let deadline = Rc::new(RefCell::new(None::<Instant>));
        let check = deadline.clone();
        engine.on_progress(move |_| {
            check
                .borrow()
                .is_some_and(|at| Instant::now() >= at)
                .then_some(Dynamic::UNIT)
        });
        // No standard package, resolver, clock, randomness, printing or native handles.
        if typed {
            crate::typed_evaluator::register(&mut engine);
        } else {
            engine.register_fn("button",|output:rhai::ImmutableString,value:bool|->Map {rhai::serde::to_dynamic(serde_json::json!({"output":output.as_str(),"value":{"kind":"button","value":value}})).unwrap().cast()});
            engine.register_fn("axis", |output: rhai::ImmutableString, value: i64| -> Map {
                rhai::serde::to_dynamic(
                serde_json::json!({"output":output.as_str(),"value":{"kind":"axis","value":value}}),
            )
            .unwrap()
            .cast()
            });
            engine.register_fn("hat",|output:rhai::ImmutableString,value:i64|->Map {rhai::serde::to_dynamic(serde_json::json!({"output":output.as_str(),"value":{"kind":"hat","value":if value<0 {serde_json::Value::Null}else{value.into()}}})).unwrap().cast()});
            engine.register_fn("timer", |id: rhai::ImmutableString, after: i64| -> Map {
                rhai::serde::to_dynamic(serde_json::json!({"id":id.as_str(),"afterUs":after}))
                    .unwrap()
                    .cast()
            });
            engine.register_fn(
                "transaction",
                |state: Map, outputs: rhai::Array, timers: rhai::Array| -> Map {
                    let mut result = Map::new();
                    result.insert("state".into(), state.into());
                    result.insert("outputs".into(), outputs.into());
                    result.insert("timers".into(), timers.into());
                    result
                },
            );
        }
        let ast = engine
            .compile(source)
            .map_err(|e| e.to_string())?
            .clone_functions_only();
        let callbacks: &[&str] = if typed {
            &["on_start", "on_event"]
        } else {
            &["on_start", "on_event", "on_timer"]
        };
        for name in callbacks {
            if !ast
                .iter_functions()
                .any(|f| f.name == *name && f.params.len() == 1)
            {
                return Err(format!(
                    "Define {name}(ctx). Store persistent values in the returned state."
                ));
            }
        }
        Ok(Self {
            engine,
            ast,
            deadline,
        })
    }
    pub fn call(&self, request: &Invocation) -> Result<Transaction, String> {
        if !["on_start", "on_event", "on_timer"].contains(&request.callback.as_str()) {
            return Err("Unknown callback.".into());
        }
        let mut context = serde_json::to_value(request).map_err(|e| e.to_string())?;
        // Clock is supplied by the reducer. There is no script-visible OS clock.
        context["nowUs"] = i64::try_from(request.at_us.0)
            .map_err(|_| "Clock exceeds signed script range.")?
            .into();
        let result = self.call_context(&request.callback, context)?;
        let transaction: Transaction =
            rhai::serde::from_dynamic(&result).map_err(|e| e.to_string())?;
        if serde_json::to_vec(&transaction)
            .map_err(|e| e.to_string())?
            .len()
            > osg_mapping::script::IPC_LIMIT
        {
            return Err("Script reply exceeds 32 KiB.".into());
        }
        Ok(transaction)
    }
    pub(crate) fn call_context(
        &self,
        callback: &str,
        context: serde_json::Value,
    ) -> Result<Dynamic, String> {
        let input = rhai::serde::to_dynamic(context).map_err(|e| e.to_string())?;
        *self.deadline.borrow_mut() = Some(Instant::now() + Duration::from_millis(5));
        let result = self.engine.call_fn_with_options::<Dynamic>(
            CallFnOptions::new().eval_ast(false),
            &mut Scope::new(),
            &self.ast,
            callback,
            (input,),
        );
        let expired = self.deadline.borrow().is_some_and(|at| Instant::now() > at);
        *self.deadline.borrow_mut() = None;
        let result = result.map_err(|e| e.to_string())?;
        if expired {
            return Err("Script evaluation exceeded 5 ms.".into());
        }
        Ok(result)
    }
}

#[cfg(test)]
#[path = "evaluator_tests.rs"]
mod tests;
