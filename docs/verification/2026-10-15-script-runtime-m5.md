# M5: supervised custom logic and trace replay

Issue #31 adds a restricted Rhai 1.26.1 helper, explicit script input/output declarations, transactional reducer ownership, native preview integration, compile diagnostics, editor examples and recording/replay. Physical live script output remains disabled.

## Implemented checks

Portable reducer tests cover full replacement transactions, failed reply atomicity, independent output owners, script-generated pulse timing, loss of either declared source, old replies after Stop, cancellation of queued obsolete timers, source/queue/state/output/timer/log limits, fingerprint tampering and repeated replay. Interpreter tests cover syntax errors, recursion, infinite loops, array/string growth, malformed replies and unavailable filesystem/import/eval/printing/device APIs. Real subprocess tests cover compilation, framing, pulse execution, replay, helper death and orderly EOF. Windows Job Objects and Unix resource setup are exercised with owned test processes.

The native Windows Tauri UI added and compiled the timed-pulse example, saved its draft, started recording preview, and deliberately occupied the UI thread for 150 ms. The independent worker recorded a 7,531 microsecond pulse for a requested 5,000 microsecond delay. Syntax errors returned diagnostics; no JavaScript exceptions occurred. Live-output controls remained disabled. The 6,413-byte exported trace replayed twice with output SHA-256 `57ffe2dc2e974c2f4e64afa627512615b92eea18886c4c62f543287809bc6b2d`. Evidence is under `.research/controller-mapping-m0/inspect-script-ui.mjs`, `script-preview-trace.json` and `cockpit-m5-script-editor.png`.

Linux and Windows script modules exceed 70% production line coverage. Portable mapping and host modules also exceed 70%. CI runs the production script helper tests and per-module coverage on Linux, Windows and macOS. Frontend type/Svelte checks pass without errors or warnings.

## Open qualification

Real user-written script output requires a qualified virtual backend; none is enabled. Native macOS resource-control behavior, adversarial memory exhaustion under the final packaged helpers, signed packaging and physical end-to-end latency remain qualification work. The local resource-control tests do not establish an OS permission sandbox. Keep #31 open/In progress and all live script output disabled while these gaps remain.
