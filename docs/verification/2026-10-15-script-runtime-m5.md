# M5: supervised custom logic and trace replay

Issue #31 adds a restricted Rhai 1.26.1 helper, explicit script input/output declarations, transactional reducer ownership, native preview integration, compile diagnostics, editor examples and recording/replay. Physical live script output remains disabled.

## Implemented checks

Portable reducer tests cover full replacement transactions, failed reply atomicity, independent output owners, script-generated pulse timing, loss of either declared source, old replies after Stop, cancellation of queued obsolete timers, source/queue/state/output/timer/log limits, fingerprint tampering and repeated replay. Interpreter tests cover syntax errors, recursion, infinite loops, array/string growth, malformed replies and unavailable filesystem/import/eval/printing/device APIs. Real subprocess tests cover compilation, framing, pulse execution, replay, helper death and orderly EOF. Windows Job Objects and Unix resource setup are exercised with owned test processes.

The native Windows Tauri UI added and compiled the timed-pulse example, saved its draft, started recording preview, and deliberately occupied the UI thread for 150 ms. The independent worker recorded a 7,531 microsecond pulse for a requested 5,000 microsecond delay. Syntax errors returned diagnostics; no JavaScript exceptions occurred. Live-output controls remained disabled. The 6,413-byte exported trace replayed twice with output SHA-256 `57ffe2dc2e974c2f4e64afa627512615b92eea18886c4c62f543287809bc6b2d`. Evidence is under `.research/controller-mapping-m0/inspect-script-ui.mjs`, `script-preview-trace.json` and `cockpit-m5-script-editor.png`.

Linux and Windows script modules exceed 70% production line coverage. Portable mapping and host modules also exceed 70%. CI runs the production script helper tests and per-module coverage on Linux, Windows and macOS. Frontend type/Svelte checks pass without errors or warnings.

## Open qualification

Real user-written script output requires a qualified virtual backend; none is enabled. Native macOS resource-control behavior, adversarial memory exhaustion under the final packaged helpers, signed packaging and physical end-to-end latency remain qualification work. The local resource-control tests do not establish an OS permission sandbox. Live script output remains disabled. The 16 October acceptance split below moves the missing Windows checks to a separate story; macOS qualification remains deferred separately.

Darwin CI exposed that `RLIMIT_DATA=256 MiB` is rejected during helper startup, including with nano malloc disabled. macOS now bounds requested Rust heap allocations at 256 MiB before compilation and limits descriptors, with independent interpreter and parent deadlines. This is **not a cap on native-library allocations or total process RSS**; that qualification remains open and live script output stays disabled. Linux retains RLIMIT_DATA; Windows retains the process Job memory limit. No test is skipped for macOS.

## Live-path completion after implementation audit

The original live entry rejected every scripted profile and never attached the
compiled helpers to its output router. The prepared-start path now carries the
workers and source subscription through complete baseline validation, target
acquisition, reducer transactions and feeder delivery. Qualification is checked
before any helper/output process starts. `SCRIPT_LIVE_QUALIFIED` remains false,
as do the independent platform output gates; only injected test services exercise
the candidate path.

Supervision now observes idle helper death on every pump. Previously an idle
helper could exit while holding a contribution, with no error until the next
callback. Death now removes its outputs/state/timers without requiring input.
Feeder failure also drops remaining script workers and panel subscriptions.

Tests use actual restricted Rhai subprocesses and the production feeder protocol
with an authored driver adapter. They cover delivered script transactions,
syntax rejection before acquisition, runtime exceptions, infinite-loop budgets,
loss of either declared source, idle helper termination and stale input after
Stop. These are software integration checks, not physical virtual-device or
game qualification. Native coverage and final exact-commit CI are recorded on
issue #31; its four composite physical/resource criteria remain unchecked.

Local validation after this change: 30 mapping-host tests pass on Linux and 29
on Windows (one Linux-specific launch test). All six measured host modules
exceed 70% production line coverage: the minimum is 79.84% on Linux and 72.59%
on Windows. Linux's seven script modules also exceed 70%. The running Windows
Tauri window still renders the overview without JavaScript exceptions.

## Implementation acceptance — 16 October 2026

At the user's request, #31's four unverified composite criteria move, unchecked,
to [#52](https://github.com/opensimgear/cockpit/issues/52): actual script-to-vJoy
delivery, packaged resource/failure cleanup, native two-source loss, and loaded
capture/UI/feeder response. Existing deterministic replay and software failure
checks remain valid; transferring a composite does not mark its missing portions
passed. Three original criteria are verified, and implementation may close while
ordinary live script/output gates remain false.

Local CI on a clean committed export passes real restricted worker tests and all
seven Windows logic module coverage gates (minimum 77.42%). The mapping-host
script/feeder integration suite and all six host coverage gates pass (minimum
72.99%). Portable reducer/rule/runtime coverage passes (minimum 92.72%); Windows
virtual-output coverage passes all measured modules (minimum 76.32%). Frontend
type/Svelte checks, transport, coverage and production UI build also pass. These
checks use actual isolated script processes and authored output adapters; no
physical qualification or production enablement is inferred from them.
