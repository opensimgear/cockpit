# M7 qualification record — 15 October 2026

The feature remains **experimental**. This record consolidates software evidence
and outstanding deployment checks; it does not qualify a backend. #33 and the
unfinished prerequisite milestones remain open/In progress. Live virtual output,
provisioning, visibility changes, Linux/macOS physical capture and live script
output retain their existing false qualification gates.

## Evidence matrix

| Area | Recorded evidence | Still required before qualification |
| --- | --- | --- |
| Portable engine/profile | Native three-OS deterministic reducer/replay, validation, generation, arbitration, state-machine and migration tests; M2 closed. | Retain regression gates as platform support changes. |
| Windows input | Authored HID fixtures and production-adapter read-only captures; M0 foreground ACR background-read probe. | Controlled physical transitions, unplug/replug/port changes, duplicate hardware, foreground game and loaded timing on the production path. |
| Linux/macOS input | Native evdev/IOHID compilation, adapter coverage and injected error/baseline/hotplug tests. | Native physical models, permission denial/regrant, background-game capture and identity/baseline checks. WSL is not a native desktop qualification system. |
| vJoy | Pinned signed 2.2.2.0 artifacts; M0 unchanged report/readback and implicit-default slot-2 add/delete/restore; production descriptor, provisioner, ownership and failure fixtures. | Production UAC/refresh/cancel/recovery, custom layouts, two live targets, real source routing, contention, helper/whole-app failure and restart/game retention. |
| Linux virtual | Native ABI/syscall-boundary tests, report/lifetime fixtures, real WSL permission-denied inspection. | Actual device creation/destruction, two physical sources, native game and qualified Proton bindings, permissions/distributions. |
| macOS virtual | Native codec/helper/API boundary tests, unsigned staging, unavailable-helper inspection. | Authorized signing/entitlement/notarization, protected installation and upgrades, actual device/game recognition, cleanup/recreation and supported OS/architecture matrix. |
| Optional dependencies | Production native service startup, ordered synthetic preview, restart and unavailable output rejection; separate opt-ins, saved settings and injected dependency failures. M7 extends this acceptance to macOS and asserts output unavailability in CI. | Physical installation combinations: neither, each alone, both, installed/disabled, unsupported versions, required reboot, removal while active and user enable/disable. |
| HidHide | M0 normal-mode DirectInput/direct-HID test plus normal/error/forced-exit restoration; production policy/journal/conflict fixtures. Raw Input remained visible. | Inverse mode, every advertised API/process scope, authorized capture access, external live conflicts, restart/whole-process failure and game binding. Existing external policy was preserved. |
| Linux/macOS visibility | Native suppression boundary and policy/cleanup fixtures; unsupported process hiding fails closed. | Alternative access paths, actual suppression/cleanup/reconnect, separately implemented and qualified Linux controlled launcher; macOS process mode remains unsupported. |
| Custom logic | Real isolated workers on all three OSes, compile/runtime/budget failures, transaction rollback, deterministic timer replay, stale reply/process death tests. | Real virtual-output scripts, loaded capture/feeder responsiveness and physical source loss; macOS native-library/whole-RSS ceiling qualification (Rust heap cap alone is narrower). |
| Existing behavior | Full application/integration regressions, output leases, game rollback, ordered panel consumers and independent gateway acceptance. | Assembled hardware and live DCS/racing acceptance. Debug Pico fixtures are not physical panel qualification. |
| Vendor hardware | Windows reports VKBsim Gladiator EVO R (231D:0200) and VKBSim T-Rudder (231D:011F), with read-only reports and complete baselines. | Firmware and USB/hub topology, deliberate controls and reconnects; at least one real MOZA/SIMAGIC racing specimen with exact model/firmware. No blanket vendor claim. |
| Games | Existing Windows profile coordinator and schema-2 immutable captures; synthetic failed-launch/recovery/ownership tests; native Tauri capture/save without activation. | Real DCS and racing game, vJoy-only originals visible and combined hiding, captured requirements, controls, restart/reconnect and rollback. |

The historic M0 device label "MOZA VID 231D/PID 0200" was based on the selection
request. Later OS-reported evidence identifies it as **VKBsim Gladiator EVO R**;
the M0 wording is corrected. MOZA RacingLab owning a vJoy slot was a software
ownership observation, not evidence of a MOZA physical racing device. Firmware
and physical USB topology were not recorded and remain unknown.

## Automated evidence and coverage

The dependency checkpoint `42a8ff1` passed all 11 jobs in
[CI run 38665396686](https://github.com/opensimgear/cockpit/actions/runs/38665396686):
frontend, Linux application/integration, and three-OS controller/virtual/host,
portable mapping and production-script suites. M7 additionally runs full macOS
desktop compilation/tests and real native service startup/preview without an
authorized helper. Final M7 commit/run results are recorded on
[issue #33](https://github.com/opensimgear/cockpit/issues/33).

CI retains per-module coverage artifacts for input/visibility, virtual adapters,
mapping host, portable engine and scripts. Production Rust modules are gated at
71% measurable line coverage; frontend helpers require 71% lines, branches and
functions. Native adapters are measured separately, including injected native
failures. This is not hardware coverage. M6 measured panel adapter 92.48%, game
schema/overlap 97.92%, game coordinator 92.47%, and mapping-host modules
78.42–100%. The historical records retain their exact checkpoint results.

M7 changes verification tools, CI and documentation, not production behavior.
The latency analyzer has authored unit tests on all three CI platforms. The
Windows staging step records the same executable that contains private workers;
unsigned CI artifacts remain explicitly development builds. macOS staging still
does not sign or install anything. Neither staging tool can enable a capability.

## Local M7 checks

The first full macOS desktop run exposed a test timing assumption: a 30 ms sleep
could outlast the test's 150 ms export freshness on a loaded runner. The display
correctly became STALE while the assertion still expected UNKNOWN. The test now
drives the production ExportSession through the existing feed boundary with an
explicit clock, checking the partial frame/counter before committing it, then
advancing past the freshness boundary. No assertion is skipped and no production
timeout is relaxed.

- Windows 11 build 26200: rebuilt native service, ordered recording preview and
  persistence/restart checks passed with the installed vJoy driver. Inspection
  reported slot 1 FREE, 16 axes, eight buttons, no hats. No acquisition/report,
  layout change or hiding inspection/mutation occurred in the ordinary startup
  path. Local evidence: `.research/m7/windows-native-acceptance.json`.
- WSL/Linux: rebuilt native service; unavailable output assertion, preview and
  restart passed. uinput inspection reported permission denied. This is the local
  unavailable-dependency check, not physical Linux qualification. Evidence:
  `.research/m7/linux-native-acceptance.json`.
- Four latency-analyzer tests passed, including percentile boundary failures,
  script/delay separation, missing diagnostics, ordering and malformed data.
- Windows packaging staged the development desktop artifact and recorded
  `signatureStatus:NotSigned`, `qualified:false`. No signing or installation took
  place. Release-mode staging of that unsigned artifact is rejected.
- The Windows Tauri dev session was restarted and its native WebView returned a
  populated Overview with one gateway/five devices and no active game/integration.
  No physical controller or visibility state was changed for this check.

## Latency protocol and missing measurements

**No physical capture-to-successful-vJoy-submit latency result is recorded.**
Recording-preview timings and the M5 scripted UI-delay experiment are different
measurements. They do not satisfy p95 < 5 ms / p99 < 10 ms live-output targets.

Use several exact physical sources and sustained high-rate axis traffic on a
qualification machine after the live path is authorized for testing. Record host
monotonic capture timestamps and timestamps immediately after successful native
UpdateVJD submission, using one clock and run-relative microseconds. Record the
OS/build, CPU, application commit, signed driver version/hash, exact device models,
firmware, USB/hub topology, mappings, traffic rates, duration and game separately.
Keep raw evidence and logs under ignored `.research/`; publish a redacted summary.

Use `python scripts/mapping-latency.py <evidence.json> --output <summary.json>`.
The bounded schema-1 input contains:

```json
{
  "schema": 1,
  "backend": "vjoy",
  "clock": "host-monotonic-us",
  "evidenceKind": "authored",
  "machine": {"os": "fixture", "cpu": "fixture", "appCommit": "fixture", "driverVersion": "fixture", "game": "none"},
  "devices": [{"source": "stick", "model": "fixture", "firmware": "fixture", "connection": "fixture"}],
  "durationUs": 1000000,
  "diagnostics": {"queueCapacity": 512, "queueHighWater": 1, "coalescedAxisReports": 0, "queueGaps": 0, "missedTransitions": 0, "feederFaults": 0, "submitFailures": 0},
  "samples": [{"source": "stick", "sequence": 1, "kind": "builtin", "control": "button", "capturedUs": 0, "submittedUs": 2000, "configuredDelayUs": 0, "successfulSubmit": true}]
}
```

This example is authored, not a measurement. The analyzer validates sequence/
timestamp order and uses nearest-rank percentiles, comparing built-in and script
latency separately. It excludes samples with configured gesture/pulse delay rather
than subtracting an assumed delay. Game sampling is outside the boundary.
Report worker evaluation time separately from capture-to-submit script latency.
Report queue capacity/high water, axis coalescing, missed transitions, source gaps,
submission failures and feeder faults even when successful-sample percentiles pass.
A sequence gap can reflect axis coalescing; it must not silently excuse a missing
button release. Physical button-order assertions and fault/rebaseline evidence
are required. Small samples, authored data and filtered successful submissions
cannot establish qualification; the analyzer always emits `qualification:false`.

## User-facing state and release work

The README and repository description describe experimental recording/mapping,
not qualified virtual output/hiding. [Platform guides](../controller-mapping-platforms.md)
cover independent optional setup, device lifetimes, visibility limits, explicit
recovery and migration. Existing vJoy layout and HidHide policy were not modified
for M7. Signing authorization, native Linux/macOS machines, missing vendor devices,
actual game bindings and live latency remain external qualification work, not
completed checkboxes or reasons to enable a production gate.
