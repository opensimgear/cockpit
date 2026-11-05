# Idle vehicle capture worker

The vehicle telemetry worker now parks without a maintenance deadline when it
owns no admitted reader. The context owner still opens and retries providers.
Successful reader admission explicitly wakes capture processing before the
first packet. A reader detached for I/O retains maintenance until its result
is accepted or retired. Source notifications, process qualification, source
freshness and the independent shutdown wake retain their existing boundaries.

This removes an unconditional 100 ms idle wake. It does not remove the 100 ms
process qualification pass for admitted readers, the remaining Windows
process-discovery fallback, or the CAN publication fallback.

Local evidence, 2026-11-05:

- The instrumented native suite passed: 2,277 tests, zero failures, 17 existing
  explicit ignores. The focused vehicle run passed all 45 tests; the final
  focused context/vehicle run passed all 62 tests after layout-only edits.
- New regressions cover quiet reader admission, detached reader maintenance,
  withdrawal, rejected late completion, readmission and failed opening.
- Production line coverage: context runtime including its validation fragment
  92.41%, vehicle tracker 97.81%, reader observation 98.44%. Each exceeds 71%.
- The isolated candidate's 1,148 tracked Rust/manifests/lock files matched HEAD
  except the four intended changed Rust files. Unrelated worktree changes were
  excluded from the native build.
- The rebuilt running Cockpit passed actual Windows VKB T-Rudder shared
  observation/reopen without inspector queries, and two navigation cycles
  with exact reader acquisition/disposal. Fixtures must run sequentially:
  an initial concurrent invocation let the controller fixture's temporary
  reader enter the navigation fixture's global instrumentation.
- Authored Assetto qualification was attempted and failed before telemetry
  verification. Setup returned `needs-attention`, `verified: false`, with
  `Preparing native process monitoring.` The temporary installation was
  restored and cleanup errors were empty. This run does not qualify any of
  the four Assetto variants. Its artifact is
  `.research/assetto-capture-native/1793855573263/failure.json`.

Local logs and coverage are under `.research/reactive/vehicle-idle-*`.
Physical capture-to-preview/output timing and simulator effects remain in
[#199](https://github.com/opensimgear/cockpit/issues/199). This change does not
complete an entire R0–R9 acceptance checklist item. CI remains deferred until
the feature implementation is complete, as requested.
