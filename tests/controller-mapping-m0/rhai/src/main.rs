//! M0 experiment only: no device APIs and no production integration.
use rhai::{Dynamic, Engine, EvalAltResult};
use std::time::{Duration, Instant};

fn bounded_engine(deadline: Option<Duration>) -> Engine {
    let mut engine = Engine::new_raw();
    engine.set_max_operations(100_000);
    engine.set_max_call_levels(16);
    engine.set_max_expr_depths(32, 32);
    engine.set_max_string_size(16_384);
    engine.set_max_array_size(256);
    engine.set_max_map_size(256);
    if let Some(budget) = deadline {
        let started = Instant::now();
        engine.on_progress(move |_| (started.elapsed() >= budget).then_some(Dynamic::UNIT));
    }
    engine
}

fn main() {
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "operations".into());
    let deadline = (mode == "deadline").then_some(Duration::from_millis(5));
    let engine = bounded_engine(deadline);
    let started = Instant::now();
    let result = engine.eval::<Dynamic>("loop {}");
    match result {
        Err(error)
            if matches!(
                *error,
                EvalAltResult::ErrorTooManyOperations(..) | EvalAltResult::ErrorTerminated(..)
            ) =>
        {
            println!(
                "infinite-loop: terminated; elapsed_us={}; reason={error}",
                started.elapsed().as_micros()
            );
        }
        result => panic!("infinite loop did not terminate through a budget: {result:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infinite_loop_hits_operation_budget() {
        let error = bounded_engine(None).eval::<Dynamic>("loop {}").unwrap_err();
        assert!(matches!(*error, EvalAltResult::ErrorTooManyOperations(..)));
    }

    #[test]
    fn cancellation_is_independent_of_operation_budget() {
        let mut engine = bounded_engine(Some(Duration::ZERO));
        engine.set_max_operations(1_000_000);
        let error = engine.eval::<Dynamic>("loop {}").unwrap_err();
        assert!(matches!(*error, EvalAltResult::ErrorTerminated(..)));
    }

    #[test]
    fn syntax_error_cannot_activate() {
        assert!(bounded_engine(None).compile("let = ;").is_err());
    }

    #[test]
    fn raw_engine_has_no_filesystem_import_or_device_access() {
        let engine = bounded_engine(None);
        assert!(engine
            .eval::<Dynamic>("import \"anything\" as module;")
            .is_err());
        assert!(engine.eval::<Dynamic>("write_device(1);").is_err());
    }

    #[test]
    fn recursive_function_is_bounded() {
        let error = bounded_engine(None)
            .eval::<Dynamic>("fn recurse() { recurse() } recurse()")
            .unwrap_err();
        assert!(error.to_string().contains("Stack overflow"), "{error:?}");
    }

    #[test]
    fn simple_logic_is_deterministic() {
        let engine = bounded_engine(None);
        let ast = engine.compile("let x = 4; x * x + 1").unwrap();
        assert_eq!(engine.eval_ast::<i64>(&ast).unwrap(), 17);
        assert_eq!(engine.eval_ast::<i64>(&ast).unwrap(), 17);
    }
}
