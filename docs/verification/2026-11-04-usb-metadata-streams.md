# Direct USB committed metadata cutover

Windows local qualification, 2026-11-04, on main after 0216f71. This delivers the
bounded USB metadata slice of #188/R2/R5/R7; full R0–R9 acceptance remains open.
CI is deferred until all feature implementation is complete, as requested.

Firmware producer:
[`device-template 6b27f1a`](https://github.com/opensimgear/device-template/commit/6b27f1a21308230bf07aa87808981558192cf9b4).
Wire contract:
[`protocol 6d72cae`](https://github.com/opensimgear/protocol/commit/6d72cae8a0311521ce177c47a97a7a840a0cfe6a).
The implementation and ownership boundaries are documented in
[`usb-metadata.md`](../reactive/usb-metadata.md).

## Delivered behavior

The connected worker parks on native wakes instead of querying `can status` and
`config get_active` every two seconds. Committed runtime metadata, including
editing expiry and external CAN writes, updates the shared cache. Initial
baseline acquisition has an explicit deadline. Source loss, malformed data,
sequence gaps and exhaustion withdraw the verified route and private authority.

A private acknowledged writer revision guards firmware mutations atomically
after actual expiry processing. Plain reads and public observations cannot
refresh that capability. Guards are never replayed. CDC delivery alternates new
metadata and input frames under backpressure; continuous metadata commits cannot
starve new input. Metadata observers use their own cached projection and do not
access or serialize native input capture.

The exact-key public metadata subscription drives USB details. Private command
receipts, action data and local drafts remain separate. Late receipts, lagging
public updates, source loss/recovery and rejected commands cannot restore stale
editing. Existing input capture, profile routing and passive previews retain
their independent stream.

## Local verification

| Check | Result |
| --- | --- |
| Complete native suite | 2,196 pass, zero failures, 17 existing explicit ignores |
| Changed native coverage | All 11 production-module line gates pass; decoder and metadata hub 100%, metadata control 97.37%, public bridge 94.85% |
| Native coverage helpers | Six tests pass |
| Devices frontend task | All 207 tests and configured coverage tasks pass |
| Focused USB tasks | All 49 tests pass. Each portable test runs once in its coverage task. |
| Metadata observer coverage | 93.89% lines, 95.31% branches, 90.32% functions |
| Ownership | All 12 Nx ownership/coverage-boundary tests pass |
| Type checks | Main and browser-companion Svelte checks: zero errors/warnings; widget/compiler/fixture checks pass |
| Builds | Production browser and frontend builds, native dev build and all six STM32 profiles pass |
| Firmware tests/coverage | All 22 suites pass; runtime, metadata delivery and fair arbiter have 100% lines/functions and 75.79%, 90.24%, 87.04% branches respectively |
| Actual rebuilt Cockpit | All seven subscription, preview, authored pulse/script, input-reader replacement and Stop checks pass |

The actual-Cockpit metadata check mounts the real USB component, shares two
observers, rejects an unobserved endpoint, waits beyond the old refresh period
and observes no acquisition or authority command. There is no matching assembled
direct-USB source in this qualification; known-source transitions are exercised
with the native blocking protocol peer and compiled C formatter fixtures.
The final actual profile state is stopped, with no owner or fault.

Qualified native dev executable SHA256:
`ea5bf748c8596cc076a63a0ead5052634f7c165a924a323422557a2a84634881`.
The complete native run uses the exact coverage executable SHA256
`9877106eff86ea83873e2c56226907dd72e24961ef81288776afbc63371d4c74`.
The nine C metadata frames have SHA256
`f54957ffaff7f1c49ef435e8a11b29f9effeafff729d3ce37e544b303d81bcc6`
as emitted on the wire. Git stores the fixture with LF line endings.
Windows binary stdout preserves the wire bytes in the generator.
See [generator correction](https://github.com/opensimgear/device-template/commit/a0ac1f7fa08209121532adf0138b17c320a98e32).

## Corrections retained in the evidence

Review caught a raced ownership transition hidden by a newer read/command
receipt, and metadata starvation under continuous commits. Both production fixes
have regressions. Metadata projection was also separated from input capture.
UI review corrected null-route validation, disconnected historical configuration,
lagging public state, rejected-command reconciliation and claims spanning loss.
The UI now matches connection identity without ordering opaque connection tokens.

Initial native fixture runs hit sandbox localhost error 10013; the exact fixtures
and final module/full-suite runs pass with localhost access. One test held its
fake device mutex across Shutdown and hung; that run was interrupted, the guard
is now released, and the final complete suite passes. A firmware fairness test
initially supplied its only edge before baseline and regressed its clock; it now
samples monotonically and produces actual post-baseline edges. Three Svelte
diagnostics and a duplicate coverage-task invocation were fixed before the final
passing checks. An unintended release-packaging task was stopped after its UI
builds passed; no release/package was published.

## Evidence and remaining limits

Local artifacts are under `.research/reactive/`: `usb-metadata-full-native.log`,
`usb-metadata-native-coverage/{evidence.json,summary.json}`,
`usb-metadata-devices-final.log`, `usb-metadata-frontend-final-coverage.log`,
`usb-metadata-final-{check,ownership}.log`, `usb-metadata-native-build.log`,
`usb-metadata-actual-cockpit-final.log`, `usb-native-metadata.json`,
`usb-metadata-c-{tests,coverage}.log`, `usb-metadata-c-coverage.json` and
`usb-metadata-final-arm-*.log`. Source/staging proof is recorded separately in
`usb-metadata-staged-proof.json`.

No firmware was flashed. Images use compile-only VID/PID 65535. Physical USB/CAN,
suspend/resume, flash-write effects and raw-input-to-preview/virtual timing stay
unqualified in user-operated #199, kept first in progress. Linux/macOS runtime,
CAN input push, remaining read models/integrations/routing, companions, full
negotiated MCP streaming and integrated qualification remain original work.
This record does not establish an overall remaining-work percentage.
