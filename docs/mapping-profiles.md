# Portable mapping profiles and preview runtime

The M2 implementation is a portable Rust library in [crates/mapping](../crates/mapping). It validates and stores mapping documents, resolves explicit local assignments, runs deterministic input/timer rules, and records complete output reports. It has no controller, vJoy, uinput, IOHID, Tauri or visibility dependency. The desktop direct-mapping controls and real output adapters are subsequent milestones; this preview cannot send input to a game.

Run the authored synthetic trace from the repository root:

~~~sh
cargo run --locked --manifest-path crates/mapping/Cargo.toml --example replay -- crates/mapping/fixtures/direct-trace.json
~~~

The trace emits 11 ordered reports for two logical targets: initial fallback, baseline synchronization, a short press/release, a 30 ms timer pulse, and Stop fallback. Replaying it produces identical reports. [The profile](../crates/mapping/fixtures/direct-profile.json) and [trace](../crates/mapping/fixtures/direct-trace.json) are editable examples.

## Schema 1

A portable document contains a canonical lowercase UUID, name, logical source roles, logical targets, direct mapping nodes, output arbitration and optional feature/visibility intent. It contains no instance paths, executable paths, backend slot numbers, installation settings or running flag.

- Documents are limited to 64 KiB, 16 roles, eight targets and 256 nodes.
- Each role has an explicit origin (host-controller, osg-panel-events or synthetic) and up to 256 named logical controls. There is no virtual-output source kind.
- Controls are buttons, bipolar axes (-32768 through 32767), unipolar axes (0 through 32767), or eight-way hats (0 north, then clockwise; null is neutral).
- Target layouts contain up to 128 buttons, 16 axes and four hats. Every axis declares its range and fallback. Buttons fall back to released and hats to neutral. At least one control is required.
- Direct nodes take one logical role/control or another node. Graph validation rejects cycles, missing references, duplicate IDs, type/range mismatches and unsupported operations. Node IDs are evaluated in a deterministic topological order.
- Momentary button nodes suppress an initially held button until it is released and pressed again. Level nodes synchronize maintained buttons and valid axis/hat values from the baseline.
- Button writers combine with OR. A release from one producer cannot release a button held by another. Axes and hats have one writer unless an explicit priority lists every writer exactly once. The first available writer wins, including an explicit neutral hat.
- Visibility intent uses a physical role and an optional logical application ID. It cannot contain an executable path. Hidden intent requires the separate optional hiding requirement. Saving/importing intent does not perform it.
- Unknown fields, unsupported versions/operations, duplicate object fields and malformed values are rejected. Advanced transformations, modes, macros and scripts are later milestones.

Local assignments map every logical role/control to an explicit source ID and native control ID. Source kinds must match; conflicting type declarations and known virtual-source markers are rejected. A single native source is limited to 256 controls across all roles. The engine does not guess by display name. Hardware adapters must verify the selected identity, actual capabilities and source provenance before activation.

The Store API saves portable profiles atomically. Local assignments use a separate, bounded 1 MiB file and are revalidated against the current profile when loaded. They are not included in portable export. Up to 128 profiles are supported. Invalid existing files report errors; reading ignores unrelated files and refuses profile/assignment symlinks. Store operations do not construct or start an engine.

## Deterministic input and ownership

Every input frame names its source, generation, sequence, monotonic capture time, reason and typed changes. Counters use canonical decimal strings so JavaScript cannot round 64-bit values. A complete baseline with a newer generation is required before routing that source. Changes are validated atomically and require consecutive sequences. A duplicate/stale frame does not rewrite state. A gap, disconnect or generation transition releases affected contributions and timers; unaffected producers retain their claims.

A quiet controller retains its valid state. Time without an input report does not imply disconnection. Input adapters must signal actual loss and obtain a new complete baseline. Provider restarts must invalidate the mapping session or establish increasing source generations; persisted IDs alone are not a stream incarnation.

Time advances explicitly in microseconds. At equal deadlines, timers run in deterministic node/timer-ID order. Due timers precede an ordinary input change; a loss or new generation first cancels the affected producer's pending timers. Timer intents carry both run generation and dependent source generations. Old tokens cannot write after replacement, source loss or Stop. Limits are 1024 timers per run, 16 per producer and a 24-hour scheduling horizon. Pulse edges need distinct deadlines; the example uses a 30 ms press interval.

The runtime supports one active preview. A replacement candidate is fully validated before the existing run changes. A valid replacement emits old fallback reports, creates a new run generation and waits for fresh baselines. A failed preflight leaves the previous run intact. Editing or saving a profile does not modify its captured running copy.

Output reports contain the full target state. The engine preserves ordered transitions and queues at most 4096 reports. If its consumer falls behind, it stops and replaces the pending queue with explicit fallback reports and a fault. The recording sink retains 4096 reports and reports a history gap to slow readers; pages contain at most 64 records. This history policy does not silently drop real controller output because this runtime is a recording preview.

Traces are limited to 4 MiB and 4096 steps. Replay refuses more than 65,536 output records. Real output acquisition, feeder leases, IPC envelope accounting and physical-source streaming are handled at the platform integration milestone; no existing gateway IPC limit changes in M2.
