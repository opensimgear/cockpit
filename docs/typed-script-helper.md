# Typed profile script helper

An explicit schema-2 Script node runs in `--osg-logic-worker-typed-v2`, a disposable child of the native profile coordinator. It uses the retained process resource limits, bounded 32 KiB frames, one in-flight request, a 100 ms absolute host deadline, and a 5 ms interpreter evaluation budget. No script runs in the native coordinator. Helper faults end that worker; an ambiguous or late invocation is never retried.

The native host constructs `TypedWorker::spawn(executable, &node)`. The immutable handshake pins that node's source, named input/output contracts and scalar parameters. `submit(ScriptInvocation)` accepts the typed reducer's request; `poll()` returns its exact `ScriptTicket` and `Result<ScriptTransaction, String>`. The host must admit every success through `Phase::script_reply`, and call `script_fault` after a worker error. Native run/source generation and port validity epochs remain authoritative. `pending`, `alive`, `id`, `terminate`, and `source_hash` support coordinated lifetime management.

Define `on_start(ctx)` and `on_event(ctx)`. A callback receives only `ctx.inputs`, `ctx.parameters`, `ctx.state`, `ctx.nowUs` and `ctx.eventTrigger`. Each named input retains its exact typed envelope: `{status:"known", value:{kind:"scalar",value:...}}`, `{status:"unknown"}`, `{status:"stale"}`, or `{status:"unavailable"}`. Known invocation values retain an ordered named ActionPack. No route, ticket, backend handle, secret, wall clock, random source, file API, import or dynamic eval is exposed.

```rhai
fn on_start(ctx) {
    transaction(#{count:0}, #{lamp:scalar(false)}, [])
}
fn on_event(ctx) {
    if !ctx.eventTrigger { return transaction(ctx.state, #{}, []); }
    transaction(#{count:ctx.state.count+1}, #{
        lamp:scalar(true),
        "command":action([argument("trigger", true), argument("gain:0", 3)])
    }, [after("release", 1000, #{lamp:scalar(false)})])
}
```

`scalar` preserves booleans, finite safe numbers and text. `action` preserves each `argument(id,value)` in declaration order; outputs must match the declared invocation semantics and complete ordered parameter contracts. One-shot actions require the reducer's actual event authority. A script cannot create that authority by editing its context.

Every callback returns `transaction(stateMap, outputMap, timerArray)`. State contains only named scalar values and is bounded to 16 KiB. Outputs may name only this node's declared outputs and must satisfy their exact contracts. A timer is `after(id, delayUs, outputMap)`, with delay 1–60,000,000 µs. The reducer schedules those typed outputs directly; timers do not invoke another interpreter callback. Each successful transaction replaces the whole output contribution and timer set; omitted outputs become unknown. Shared runtime output/timer limits, source loss, Stop, validity changes and run replacement still fence all contributions. Recovery never replays callbacks or timers.

The helper API is independent of the retained controller-only script wire. There is no conversion through button/axis/hat values. Real-process fixture tests establish protocol and lifecycle behavior, while live output still requires its separately retained qualification gates.
