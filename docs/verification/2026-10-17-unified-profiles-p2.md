# Unified profiles P2 verification — 17 October 2026

Implements the compiler and typed runtime scope of #82, under #79. See
`docs/unified-profile-compilation.md` for contracts, ownership and preview limits.

## Evidence and scope

The pure mixed replay compiles exact ports from third-party A/B, multiple CAN
modules, a separate DCS command source, physical actions and a companion altitude
indicator branch. It uses explicit normalization/unit conversion/hysteresis,
multiple virtual layouts and typed adapters. Stop, stale input and replacement
produce safe contributions without replay. It proves deterministic coordination
in the recording runtime; it does not establish device/driver/game qualification.

Compiler regressions cover deterministic semantic/policy hashes, missing or
changed ports, implicit unit/range conversions, malformed ordered actions,
internal cycles, independent external integration endpoints, physical route and
writer conflicts, virtual input feedback, backend/visibility readiness and large
selected catalogues. Every report retains logical resource/port/node/edge traces.

Typed runtime regressions distinguish baseline/state/event/action authority,
unknown/stale/unavailable, loss and replacement; preserve button arbitration and
explicit priorities; and exercise bounded timers, source generation/sequence/time
fences and script output/timer replacement. Source loss withdraws ownership before
timers advance. Invalid callbacks cannot partially publish output.

Native tests verify cached-only immutable capture, canonical local assignment
resolution, read-only storage behavior and strict browser authority rejection.
Preview tests cover complete short pages, the 8 MiB cache bound, four-preview
expiry and semantic-invalid draft diagnostics. A shared summary fixture is
asserted by the native serializer and the frontend client. Compile and inspect do
not save/select profiles, initialize device owners or submit effects.

## Local integration checks

- Portable mapping: **88 passed**; all 13 measured modules pass (90.37–100%).
  Signal contracts are 99.63%, program validation 93.37% and typed execution 96.19%.
- Portable profiles: **80 passed**; all 13 measured modules pass (92.55–100%).
  The compiler is 96.31%; capture, conflict analysis and report models are 100%.
- Mapping host: **71 passed**; all nine measured modules pass (75.99–100%),
  including the native-value typed source adapter at 100%.
- Windows desktop library: **477 passed**, three existing live hardware/game tests
  ignored. The actual native gateway service framing integration test passed.
- Frontend transport: **181 passed**, including preview/profile queue ordering and
  independent inventory reads.
- Compiler client: **8 passed**, 100% lines, 98.50% branches, 96% functions.
- Svelte/browser checks: zero errors and warnings. UI/browser production builds
  passed, with existing bundle size and mixed import warnings.

All eight native USB/inventory/compiler modules pass (87.65–100%). The expanded
inventory capture is 96.88%, and compiler preview is 99.32%. The runner retains
source/test/manifest hashes, timestamps, annotated lines and test logs, including
the exact portable compiler/engine sources. Existing 71% gates remain in force;
the new signal/program/engine/compiler/adapter/preview modules are added to those
same gates. Coverage parsing includes LLVM abbreviated positive counts.

Frontend profile guards pass 22 tests (100% lines, 99.78% branches); inventory
passes 12 tests. They preserve exact native action parameter IDs such as `0` and
`gain:0`, while logical node/resource identifiers keep their stricter syntax.

## Remaining milestones and qualification

Native preview captures deliberately advertise no qualified output/hiding backend
or verified game context. P3 supplies game-owned setup and verified context; P4
admits and coordinates live activation. P5 supplies profile-owned companion
behavior, and P6/P7 expose the Profiles list/Add/editor and graph interactions.
Saved graphs use the fresh schema-2 namespace; no legacy migration is included.

Existing #3/#4 and #50–#54/#66/#67 retain their own open scope. No physical device,
Windows driver or simulator acceptance is claimed by these fixtures. Linux/macOS
activation gates remain in force.
