# Windows keyboard and mouse development verification — 18 October 2026

Feature [#97](https://github.com/opensimgear/cockpit/issues/97) has a development
implementation through the unified Profiles graph and ProfileSession. This
record distinguishes software checks, native receiver observations and remaining
release qualification. **Production desktop activation remains disabled.** J5
sequences, assignment rotation and application associations remain separate work.

The interactive experiments ran on Windows 11 Pro x64, version `10.0.26200`,
build `26200`, as an ordinary user. This is one development machine, not a
supported Windows or hardware matrix. Debug acceptance requires an explicit
`OSG_DEV_DESKTOP_INPUT=1` build; an environment variable cannot open the release
gate. Virtual controller, hiding and script qualification remain independent.

## Evidence and measurement boundaries

| Area | Verified boundary | Evidence |
| --- | --- | --- |
| Portable graph and scheduler | Typed desktop-only branches; exact physical keys; explicit hats; ordered taps, repeat and pointer integration; bounded replay; ownership and source-generation cancellation | `crates/profiles/src/desktop_tests.rs`, `crates/mapping/src/desktop_tests.rs` |
| Native host bridge | Direct source bindings; source-loss cancellation; ten ordered encoder occurrences; fresh neutral after focus return; stale/rebased input rejection; bounded outcome/collision inspection | `crates/mapping-host/src/desktop_tests.rs` |
| Windows API | Requested versus returned native record counts, US/German physical-key behavior, left/right modifiers, AltGr, extended keys, five mouse buttons, wheel axes and sustained motion | [Native receiver](2026-10-18-desktop-native-receiver.json) |
| Worker and coordinator | Shared ownership through real private-worker IPC; focus/lease cleanup; actual parent loss; worker exit after successful submission before ACK; retained uncertainty, no replay and explicit recovery | [Worker receiver](2026-10-18-desktop-worker-receiver.json) |
| Worker lifecycle | Second-injector exclusion; repeated Stop preserves a clean journal; Drop closes transport; session lock immediately reacquired over three cycles | [Worker lifecycle](2026-10-18-desktop-worker-lifecycle.json) |
| Whole development app interruption | Exact worker and application tree termination while holding KeyA; ordinary restart sends no input; explicit recovery releases only recorded KeyA and remains stopped | [Crash recovery](2026-10-18-desktop-crash-native.json); `OSG_DESKTOP_CRASH_NATIVE=1 OSG_DESKTOP_NATIVE_INJECT=1 pnpm desktop:crash:native` |
| Running cockpit authoring | Create/save/reopen/export/import, no fabricated virtual controller, native compilation and pure recording preview through actual Tauri IPC | `pnpm desktop:native:ui`; local artifacts `.research/desktop-output-native-authoring/` |
| Running cockpit live output | Actual authenticated WSS source through the applied native ProfileSession and worker: shared Control, ten ordered taps, stationary pointer, mouse/key/chord release, focus loss with fresh-neutral rearming and companion lease loss | [Companion receiver](2026-10-18-desktop-companion-receiver.json); `OSG_DESKTOP_NATIVE_INJECT=1 pnpm desktop:companion:native` |
| Actual game launch and scope | Native Cockpit launches installed Steam Assetto Corsa Rally, binds its actual process/creation identity, submits a scoped key, releases on focus loss, waits for neutral on return, preserves the applied revision after a save, and Stops cleanly | [ACR native scope](2026-10-18-desktop-game-native.json); `OSG_DESKTOP_GAME_NATIVE=1 OSG_DESKTOP_NATIVE_INJECT=1 pnpm desktop:game:native` |
| Staged development package | Existing packaging script copies the same executable; three hidden private-worker launches receive zero command bytes, emit the exact hello, exit on EOF and reacquire the session lock | [Package worker](2026-10-18-desktop-package-worker.json); `scripts/desktop-package-worker.ps1` |

Native `SendInput` return counts establish submission to Windows, not delivery
to a particular application. Receiver messages are a separate observation and
can differ in count through translation or coalescing. Unit/recording tests do
not establish Windows delivery. Companion timestamps and receiver timestamps
have not been correlated; no capture-to-submission percentile is claimed.

Coverage scripts require at least 71% production line coverage per measured
portable module. The frontend additionally gates branches and functions.
Native process transport and Windows APIs retain their separate interactive
acceptance boundary. Reproduce with `pnpm desktop:coverage`,
`python scripts/mapping-coverage.py`,
`python scripts/mapping-host-coverage.py` and
`python scripts/desktop-output-coverage.py`. Ordinary tests do not inject input.

The final portable runs passed 123 mapping, 112 profile, 97 mapping-host and 26
desktop-backend tests. The native library run covered 843 cases: 836 passed,
six explicit environment-dependent tests were ignored, and one shared compiler
fixture needed its new capability fields and capture hash. After updating that
fixture, all eight native compiler tests and ten frontend compiler tests passed.
The desktop frontend coverage suite passed 13 tests; the portable profile
validator coverage suite passed 37. Svelte checks and the production UI build
passed. These software results do not complete the release matrix below.

## Remaining release qualification

| Required check | Current limit |
| --- | --- |
| DCS or Assetto Corsa Rally | Actual ACR launch, foreground scope, neutral rearming, applied-revision isolation and Stop verified. Game consumption/displayed response, driving controls, DCS, and the remaining game matrix are unverified. Failed-launch rollback and PID replacement have software coverage. |
| Physical controller and panel input | Real model/firmware matrix, ten physical encoder detents, unplug/hotplug, stationary physical stick, and simultaneous hardware/companion sources remain unqualified. |
| Physical keyboard overlap | Cockpit owners share one injected ledger, but the OS stream also contains physical keys. Actual overlap behavior and product limits need native qualification. |
| Integrity and desktop transitions | Ordinary receiver tested; elevated-target refusal, session lock/unlock and sleep/wake require interactive acceptance. |
| Optional driver absence | Desktop-only compiler/host paths request no virtual devices or visibility. A clean installation with both optional drivers absent still needs acceptance. |
| Process termination | Parent loss, worker death after submission, and whole development application tree interruption with explicit recorded-only recovery tested. Termination can still prevent immediate release; installed release deployment remains unqualified. |
| Packaging | Staged development executable startup/EOF and unchanged hash verified. Installer/upgrade, signed release deployment and packaged input delivery remain unqualified. |
| Loaded performance | High-rate and simultaneous-source traffic, UI load and clock-correlated capture-to-successful-submission p95/p99 remain unmeasured. Pointer settings and focus-switch race remain OS limits. |
| Other platforms and advanced actions | Linux/macOS native injection, live script desktop output and J5 are unavailable. Portable authoring/recording does not qualify them. |

Only capabilities with completed evidence may be enabled. This record does not
close J0–J4 or J6 acceptance merely because their software paths exist. The
[behavior contract](../desktop-output-contract.md) and
[user guide](../keyboard-mouse.md) describe the available development behavior.
