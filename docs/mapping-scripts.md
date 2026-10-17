# Custom profile scripts (experimental)

Open **Profiles**, add/open a graph, then choose **Script with declared ports** in the transformation selector. Its inspector edits the Rhai source, scalar parameters and declared typed input/output ports on that graph node. Connect those exact ports explicitly. **Save profile** does not execute a script; draft preflight and **Apply saved revision** use separate immutable captures. Syntax/compile errors cannot replace a healthy run. Host **Stop** is independent of script code. Live script output remains behind its existing qualification gate.

The old Mappings → Draft editor, standalone recording Start and completed-trace export controls are retired. No controller-only script/profile configuration is read or migrated into unified Profiles. The retained library/CLI implementation and historical [M5 verification record](verification/2026-10-15-script-runtime-m5.md) do not describe a second desktop authoring path.

## Typed callback contract

The [typed helper contract](typed-script-helper.md) is authoritative for schema-2 Script nodes. Define `on_start(ctx)` and `on_event(ctx)`. Only retained function definitions execute; callbacks receive copies of `ctx.inputs`, `ctx.parameters`, `ctx.state`, `ctx.nowUs` and `ctx.eventTrigger`. Inputs are keyed by this node's declared port IDs, rather than controller roles.

Each input keeps its exact typed envelope: known scalar/invocation, unknown, stale or unavailable. Known false and zero are values; unavailable input is not silently converted to either. A callback returns `transaction(stateMap, outputMap, timerArray)`. Outputs may name only declared outputs and must fit their full scalar or ordered invocation contract.

```rhai
fn on_start(ctx) {
    transaction(#{}, #{lamp:scalar(false)}, [])
}
fn on_event(ctx) {
    if !ctx.eventTrigger { return transaction(ctx.state, #{}, []); }
    transaction(ctx.state, #{lamp:scalar(true)}, [
        after("release", 1000, #{lamp:scalar(false)})
    ])
}
```

This example requires a declared Boolean `lamp` output. Named action outputs use `action([argument("id", value), ...])` in declaration order. One-shot actions also require the reducer's actual event authority; editing callback context cannot invent it. Every successful reply replaces the script's complete output contribution and timer set. Omitted outputs become unknown.

A timer uses `after(id, delayUs, outputMap)`, with delay 1–60,000,000 microseconds. The reducer schedules those typed outputs directly; it does not call an `on_timer` interpreter callback. Explicit visible OR/priority nodes resolve fan-in. Source loss, validity changes, Stop and run replacement revoke contributions and timers. Recovery never replays a callback or timer.

## Bounds and process controls

Script source is limited to 16 KiB within the 512 KiB portable unified profile. State contains named scalar values and is bounded to 16 KiB. The helper uses bounded 32 KiB IPC frames, one in-flight request, the existing 100 ms absolute host deadline and 5 ms interpreter evaluation budget. Worker faults end that worker; ambiguous or late invocations are not retried. These are failure bounds, not latency guarantees.

The immutable handshake pins the node's source, ports and parameters. Native run/source generations and port validity epochs fence every result before the reducer accepts it. Scripts run in disposable child processes, never in the coordinator or UI. Files, imports, dynamic eval, native device handles and wall-clock/random APIs are unavailable. Output goes only through the declared graph ports and qualified adapters.

The retained Windows Job Object and Unix resource limits complement interpreter limits; they are not a permission sandbox or qualification evidence. Windows uses an owned Job Object with a 256 MiB process limit, one process and kill-on-close. Linux bounds data memory and descriptors. macOS bounds requested Rust heap allocations and descriptors; native-library allocations and total RSS remain outside that heap budget. Native release/resource-control qualification remains open and live scripts stay disabled.

## Inspection and verification

Applied port/path inspection reads the running graph's declared inputs, outputs, summaries and freshness, independently of a newer draft. It does not evaluate callbacks or expose script code, secrets or backend handles. A requested action tuple does not confirm physical or game delivery.

Real-process typed helper tests cover immutable handshakes, transactions, declared contracts, faults and lifecycle fencing. Portable reducer fixtures test scheduling without hardware. These do not qualify real driver output, simulator effects or process resource controls. See [typed helper details](typed-script-helper.md), [Profiles](profiles-guide.md) and [platform/recovery limits](controller-mapping-platforms.md).
