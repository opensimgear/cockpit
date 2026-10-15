# Custom mapping scripts (experimental)

Use Mappings → Draft editor → Custom Rhai logic. Add an example, inspect its declared inputs and named output bindings, edit the source, and check syntax. Save creates a draft; Start recording preview activates a separately compiled candidate. Syntax/compile errors preserve the existing run. Stop is always independent of script code. Live script output is disabled until resource controls and a virtual backend are qualified.

The examples cover a shift layer, two-control condition, axis hysteresis, short/long press and a timed pulse. Examples select existing compatible inputs and a target button; add the required roles or target first if none exist. Button examples require a release after startup before acting on a press. Bind roles to two distinct controllers to test source-loss behavior across devices.

## Callback contract

Define `on_start(ctx)`, `on_event(ctx)` and `on_timer(ctx)`. Only function definitions are retained; top-level statements are not executed. Every invocation gets a new scope and copies of:

- `ctx.inputs[role][control]`: a declared control with `kind` and `value`. Buttons are boolean, axes use the profile range, hats are 0–7 or null.
- `ctx.parameters`: the profile's parameter object.
- `ctx.state`: the previous successfully committed state object.
- `ctx.nowUs`: the reducer's deterministic microsecond clock.
- `ctx.event`: source event metadata, or a timer ID in `ctx.event.timer`.

Return `transaction(state, outputs, timers)`. An output is `button("name", true)`, `axis("name", 12000)` or `hat("name", -1)`; negative hat values mean neutral. A timer is `timer("id", 5000)`. A transaction may also contain a `logs` array of strings. Basic arrays, maps, arithmetic and selected math functions are available, including integer `min`/`max`. Files, imports, dynamic eval, printing, randomness, OS time, native handles and device writes are unavailable.

Each reply **replaces the complete state, output contribution and timer set** belonging to that script. An empty output list releases its contribution. Timer delays are relative to transaction commit, range from 1 ms to 60 seconds, and deliver `on_timer`. Replacing the timer set also cancels already queued callbacks from the old set. Other input callbacks retain their captured inputs and receive the last committed state when dispatched. One invocation per script is in flight; the shared queue is bounded. Scripts do not depend on UI polling.

Buttons combine all producer contributions with OR. Axes and hats with multiple writers need explicit priority, using script owners named `script-id:output-name`. All declared input roles participate in ownership. Losing any declared source clears the whole script's state, outputs and timers and requires explicit Start with fresh baselines. A failed callback has no partial commit; only that script is faulted. Built-in rules and unrelated scripts continue. Stop invalidates queued work and worker replies immediately; cleanup never calls user code.

## Bounds and process controls

Code is limited to 16 KiB within a 64 KiB profile. Each callback permits 100,000 operations, call depth 16, a 5 ms evaluation deadline, 16 KiB state, 128 output intents, 64 timers and four log lines. The run shares 256 timers and 32 queued/in-flight callbacks. Logs are limited to 256 bytes each and 16 messages per second. IPC frames are at most 32 KiB. Parent supervision stops an unresponsive callback after 100 ms, separately from the interpreter's 5 ms evaluation budget. Compilation has a two-second process deadline. Deadlines are failure bounds, not latency guarantees.

Windows workers are assigned to an owned Job Object before code is sent, with a 256 MiB process memory cap, one process and kill-on-close. Linux workers set a 256 MiB data limit and a 32-descriptor limit before accepting code. macOS bounds requested Rust heap allocations at 256 MiB and limits descriptors to 32; native-library allocations and total RSS remain outside that heap budget. These controls complement interpreter limits; they are not an OS permission sandbox, and their native release qualification remains open. Live script output stays disabled.

## Recording and replay

Stop preview, then choose **Export completed replay trace**. Capture is limited to 4096 steps and 4 MiB and fails explicitly instead of dropping history. Adjacent clock advances are coalesced; input edges, dispatches, replies and faults are retained. Traces contain the profile, explicit source assignments, profile/script SHA-256 fingerprints, declared/native capability snapshots, generations, callback inputs/state and timer decisions.

Build and replay on the host:

```sh
cargo build --locked --manifest-path crates/logic/Cargo.toml
cargo run --locked --manifest-path crates/logic/Cargo.toml -- replay cockpit-mapping-trace.json
```

Replay evaluates recorded invocations in fresh helper processes, compares successful transactions and applies the recorded scheduling through the production reducer. It reports a stable output hash. It never opens controller or virtual-output handles. Captured helper/source failures remain explicit trace events; replay does not erase those failures because a later machine is faster.

See the [M5 validation record](verification/2026-10-15-script-runtime-m5.md) for software evidence and remaining qualification gaps.

On macOS, the helper uses a 256 MiB requested Rust-heap allocation budget and descriptor limit. Darwin rejects the small RLIMIT_DATA cap at startup; native-library allocations and total RSS are not covered by the heap budget. Whole-process memory qualification remains open and live scripts remain disabled.
