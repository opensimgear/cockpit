# Typed virtual report startup — 18 October 2026

The first actual VKB/vJoy/browser profile Apply rejected its complete virtual report with “Stale or invalid complete virtual report.” Explicit Recover, Shutdown and fresh guardian verification succeeded. The exact original HidHide policy and temporary DCS bindings were restored; no DCS session or combined physical acceptance is claimed. The operational evidence is recorded in `2026-10-17-native-profile-rig-preflight.md`.

Two production constraints were violated. The typed reducer constructor published its initial report at clock zero, while the feeder requires a nonzero report timestamp. It also shared a sequence counter between ordinary Effects and complete virtual Reports. Emitting an Effect before the initial Report therefore skipped report sequence one. The feeder correctly rejects both conditions.

`Phase::new_at` now validates a nonzero capture clock within the existing safe deadline bound before evaluating or publishing startup output. The native Host samples the shared controller capture clock and retains an adjacent `Instant` through startup work, so later elapsed time includes that work. Actual source observation timestamps are preserved, including a stale observation at zero. The existing zero-time constructor remains available for pure replay.

Complete virtual Reports have a separate counter. Their sequence is contiguous across all virtual controllers, while Effects retain their own ordered stream. Session and feeder admission checks are unchanged; no report is rewritten, dropped or relabeled to pass them.

A regression runs the actual typed reducer, RecordingSink, Session and Feeder with an injected driver and the selected three-axis, sixteen-button, two-hat layout. The original code fails with the same feeder error. A clock-only intermediate correction still fails its sequence check. The final correction delivers Start/Baseline/Stop reports exactly once as sequences 1, 2 and 3, preserves source observation time, and verifies complete states plus exact acquisition/release. The existing two-controller case also verifies report sequences 1 through 6 alongside nine ordered Effects.

All 105 mapping tests and 86 virtual-controller tests pass. All thirteen unchanged mapping coverage gates and twenty-three virtual-controller gates pass; typed engine coverage is 97.09%, feeder coverage 97.74% and Session coverage 92.45%. Before/intermediate/final executables, actual Cargo JSON, source hashes and coverage records are retained under `.research/p9-complete-report-failure/`.

Final native Host validation of the paired capture clock passes 41 tests with zero failures and two explicit live-browser prerequisite ignores: eleven physical-action tests, eight companion feedback/handover tests and twenty-two publication lifecycle tests. All 385 recorded source hashes remain unchanged during the build and tests. The copied Cargo-identified executable SHA-256 is `ee2286ef5109042c80b95bcb599fd7377d7e0d2220b9a5d979ca0d2ef1d66269`; evidence is `.research/p9-native-clock-focused/final/`. No Manager, live service or driver operation is exercised by these filters.

These injected-driver and Host results do not establish a successful live retry, actual DCS control response or physical hiding qualification. The three ordinary qualification flags remain unchanged.
