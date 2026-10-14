# Portable mapping runtime verification — 14 October 2026

Issue: [#24](https://github.com/opensimgear/cockpit/issues/24).

The portable profile, engine, preview runtime, local store and trace replay are implemented in a separate Rust crate. No optional driver/helper is loaded and no source capture or game-visible output is activated by these tests.

## Acceptance evidence

- Authored direct-profile and trace fixtures exercise two sources, two targets, shared button ownership, axes, hats, held-at-start versus level activation, explicit local assignments and timer pulses.
- The production CLI emits 11 complete reports. Repeated replay matches exactly, including separate press/release reports, deterministic timestamps and per-axis Stop fallback.
- Unit acceptance checks cover malformed/oversized documents, duplicate fields, unknown fields/types, cycles, missing references, incompatible axis ranges, ambiguous writers, explicit axis/hat priority, diagonal/neutral hats, source gaps/disconnects, stale generations/sequences/times, complete baselines, stationary state, producer tokens, timer limits, cancellation, backpressure and profile replacement.
- Persistence checks prove import/save and local-assignment changes do not activate or modify a running preview. Local bindings remain outside portable export. Corrupt files, file/UUID mismatches, symlinks, storage failures and inventory limits fail explicitly.
- Failed replacement validation leaves the existing preview intact. A valid replacement records its fallback and requires a fresh baseline for the new run.
- Native Linux tests: 20 passed. Production-line coverage: profile 100%, engine 98.29%, runtime 100%, store 97.14%, trace 96.83%.
- Native Windows final tests: 20 passed. Production-line coverage: profile 100%, engine 98.29%, runtime 100%, store 96.19%, trace 96.83%. The committed CI matrix runs the final code on Windows, Linux and macOS and executes the same authored CLI trace.

The engine uses canonical decimal-string counters up to u64::MAX, bounded report/timer/history queues, deterministic topological evaluation and explicit time. It does not infer source disconnection from inactivity.

## Remaining delivery

M2 supplies the pure implementation and developer preview, not the desktop mapping editor or real output. Windows direct-mapping controls and the physical-input/feeder composition belong to M3; advanced editor/rules and Rhai belong to M4/M5. Existing M0/native Linux/macOS hardware qualification gaps remain open. No real game binding, timing qualification, helper signing or driver recovery claim follows from these portable tests.
