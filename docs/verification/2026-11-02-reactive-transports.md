# Reactive transport verification — 2026-11-02

Scope: R1 implementation work for #190, first committed-preference producer work
for #194, under #188. Windows local development environment; no simulator or active
profile. User acceptance remains a separate follow-up, #199, first in the progress
lane. These results do not complete R0's live acquisition budgets or R2–R9.

## Actual running cockpit

`node --test tests/ui/reactive-observations-native.mjs` passed against a native
executable built from an isolated staged source tree, excluding unrelated editor
work in the working directory. Native source tree:
`5435cf2ee7bd8e1361dc2a8a2db26b5962ae32bb`.
Executable SHA-256:
`f04d4e6f3f12f2a1591f41f28444d4b395eee9f6dc2848710b0e5061497c7fb6`.
Later changes in this milestone affect the browser cancellation controls, tests
and documentation; the native executable's source is unchanged.

Two independent clients received the same committed theme change. The preference
store applied it, and the captured change interval contained the save plus delivery
receipts, with no preference refresh read. Canceling the first consumer left the
second working. The original preference was restored. Three actual page reloads
then acquired fresh subscriptions without exhausting the window reader limit.
The test now waits for initial page readiness before requesting its first reload.

The browser test passed through the real local native gateway process and typed
frontend transport. Two independent consumers shared one physical HTTP connection,
received a theme change made through Settings → Styling in the running portal,
continued after one canceled, restored the original preference, and reopened with
a fresh snapshot. A subsequent test passed after explicitly stopping the owned
gateway process and allowing a new service process to start. This establishes a
new connection/baseline after restart; it does not claim visual loss-state timing
or hardware freshness qualification.

Desktop and browser runs were sequential: the existing database owner guard
correctly rejects a competing native service while the desktop owns the database.
The earlier per-topic HTTP implementation starved the browser connection pool;
the verified implementation multiplexes topics on one stream. No profile was
activated and no simulator/device output was exercised. Recovery-needed state was
preserved. Local raw evidence is in ignored `.research/reactive/` JSON and PNGs.

## Automated checks

- Rust streams: 22 tests pass; production modules meet the 71% line floor
  (registry module 98.15%, publisher 90.16%, reader 100%, wire 97.67%).
- Native observation selection: 12 tests pass; persistence selection: 9 pass.
  Production line coverage: commands 100%, runtime 93.91%, service 100%, bounded
  output 90.41%, persistence owner 100%, preference producer 90%.
- Native and browser bridge: 24 tests pass, including exact receipts, slow sinks,
  output limits, early delivery, process exit, same-origin controls, out-of-order
  cancellation and 198 canceled opens without exhausting a live connection.
  Observation helper coverage: 99.38% lines, 85.15% branches, 94.29% functions.
- All 47 frontend streams and preference tests pass, including actual Tauri JS channel
  decoding, default browser factory, shared consumers, stale save/read fencing,
  pending cancellation and explicit loss/baseline behavior. The combined coverage
  run passes the 71% line/branch/function gates (98.80% lines, 93.27% branches,
  94.57% functions).
- The isolated candidate passes Svelte checking with zero errors or warnings,
  the 12 Nx plugin tests, CI helper tests, native library/binary checking and
  the Vite production build. The build retains its existing large-chunk advisory.
  Native debug executable compilation also succeeds.
- The broader frontend run passes all 1,652 tests across the cockpit, browser
  companion, widget runtime and widget compiler. This final run uses concurrency
  two and loopback access. An initial sandboxed/high-concurrency run had two
  failures: denied loopback access in the resource test and a diagram timing
  assertion under contention; both also pass independently (37 tests).

Coverage checks are wired into the workspace tasks and native CI. The focused
runtime tests remain opt-in because they require the running cockpit and exclusive
database ownership. Full cross-platform CI, other native repository suites, broader
idle measurements and hardware acceptance are not represented by this record.

## R1 recheck, 2026-11-04

The current Windows dev Cockpit passed the native two-consumer mutation, no
refresh, disposal, and reload fixture. The browser-dev gateway ran against an
isolated local database. It passed the two-consumer mutation and reopen fixture.
A second browser test interrupted its owned gateway process with an observation
already active, received a transport error, disposed the old listener, and
opened one fresh connection. The new session delivered one baseline and the old
listener received no further frames. The 57 broker and 24 bridge tests passed
locally. Full producer migration, representative rig budgets, and
simulator/hardware qualification have separate acceptance criteria.
