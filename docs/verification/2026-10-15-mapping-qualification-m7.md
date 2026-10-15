# M7 qualification record — 15 October 2026

The feature remains **experimental**. This record consolidates software evidence
and outstanding deployment checks; it does not qualify a backend. #33 and the
unfinished prerequisite milestones remain open/In review for qualification;
only an issue undergoing implementation is In progress. Live virtual output,
provisioning, visibility changes, Linux/macOS physical capture and live script
output retain their existing false qualification gates.

## Evidence matrix

| Area | Recorded evidence | Still required before qualification |
| --- | --- | --- |
| Portable engine/profile | Native three-OS deterministic reducer/replay, validation, generation, arbitration, state-machine and migration tests; M2 closed. | Retain regression gates as platform support changes. |
| Windows input | Authored HID fixtures; production read-only capture during foreground ACR with full four-axis ranges, 23 button transitions and all hat positions; same-port reconnect establishes a fresh baseline, and a port change preserves the prior offline identity (71f5f3a). Follow-up native monitor checks verify buttons, unipolar/signed axes and hats while unfocused. | Identical-device identity matrix and loaded timing. |
| Linux/macOS input | Native evdev/IOHID compilation, adapter coverage and injected error/baseline/hotplug tests. | Native physical models, permission denial/regrant, background-game capture and identity/baseline checks. WSL is not a native desktop qualification system. |
| vJoy | Pinned signed 2.2.2.0 artifacts; M0 unchanged report/readback and implicit-default slot-2 add/delete/restore; production descriptor, provisioner, ownership and failure fixtures. | Production UAC/refresh/cancel/recovery, custom layouts, two live targets, real source routing, contention, helper/whole-app failure and restart/game retention. |
| Linux virtual | Native ABI/syscall-boundary tests, report/lifetime fixtures, real WSL permission-denied inspection. | Actual device creation/destruction, two physical sources, native game and qualified Proton bindings, permissions/distributions. |
| macOS virtual | Native codec/helper/API boundary tests, unsigned staging, unavailable-helper inspection. | Authorized signing/entitlement/notarization, protected installation and upgrades, actual device/game recognition, cleanup/recreation and supported OS/architecture matrix. |
| Optional dependencies | Production native service startup, ordered synthetic preview, restart and unavailable output rejection on all three OSes; separate opt-ins, saved settings and injected dependency failures. Linux additionally runs the service and its private feeder in a disposable device namespace with `/dev/uinput` absent. | Physical installation combinations: neither, each alone, both, installed/disabled, unsupported versions, required reboot, removal while active and user enable/disable. |
| HidHide | M0 normal-mode DirectInput/direct-HID test plus normal/error/forced-exit restoration; production policy/journal/conflict fixtures. Raw Input remained visible. | Inverse mode, every advertised API/process scope, authorized capture access, external live conflicts, restart/whole-process failure and game binding. Existing external policy was preserved. |
| Linux/macOS visibility | Native suppression boundary and policy/cleanup fixtures; implemented Linux isolated-launch supervisor with disposable namespace, descriptor, cancellation and parent-death checks; production gates remain closed. | Alternative access paths, actual suppression/cleanup/reconnect and physical Linux controlled-launch qualification, including device-number reuse; graphical/Steam/Proton paths and macOS process mode remain unsupported. |
| Custom logic | Real isolated workers on all three OSes, compile/runtime/budget failures, transaction rollback, deterministic timer replay, stale reply/process death tests. | Real virtual-output scripts, loaded capture/feeder responsiveness and physical source loss; macOS native-library/whole-RSS ceiling qualification (Rust heap cap alone is narrower). |
| Existing behavior | Full application/integration regressions, output leases, game rollback, ordered panel consumers and independent gateway acceptance. | Assembled hardware and live DCS/racing acceptance. Debug Pico fixtures are not physical panel qualification. |
| Vendor hardware | Windows reports VKBsim Gladiator EVO R (231D:0200), Gladiator EVO OT L (231D:3201), VKBSim T-Rudder (231D:011F) and MOZA R5 Base (346E:0004). User-confirmed firmware/topology, R5 signed-axis capture/monitor and Gladiator R button/axis/hat monitor checks are recorded. The Gladiator pair has different product IDs and unsigned axes. | Identical-device qualification and full device/game matrix. T-Rudder firmware/topology remains unknown. No blanket vendor claim. |
| Games | Existing Windows profile coordinator and schema-2 immutable captures; synthetic failed-launch/recovery/ownership tests; native Tauri capture/save without activation. | Real DCS and racing game, vJoy-only originals visible and combined hiding, captured requirements, controls, restart/reconnect and rollback. |

The historic M0 device label "MOZA VID 231D/PID 0200" was based on the selection
request. Later OS-reported evidence identifies it as **VKBsim Gladiator EVO R**;
the M0 wording is corrected. MOZA RacingLab owning a vJoy slot was a software
ownership observation, not evidence of a MOZA physical racing device. Subsequent
R5 observations below establish that particular model independently.

### User-confirmed hardware and read-only MOZA capture

On 15 October the user reported **VKB v2.20C** on both Gladiators (vendor firmware,
not OpenSimGear), **v1.2.6.7** on the MOZA R5 and **v1.2.1.3** on its HBP handbrake.
Both Gladiators and the R5 connect directly to the PC; the HBP connects through
the R5. These firmware/topology details are user-confirmed, not inferred from HID.

The Windows production capture service observed R5 VID/PID **346E:0004** with a
complete baseline and 148 controls. Its X axis declares 0–65535; seven other axes
declare −32768–32767. During the cued 75-second read-only test it received 26,751
R5 reports without a stream gap. X changed through 11123–59954, 11 buttons had
press/release transitions, and signed Dial (usage 0x37, field 2) crossed
−32767–30026 over 137 transitions. The HBP is represented through the base, not
claimed as an independently enumerated USB device. Evidence:
`.research/review-audit/windows-moza-capture-2026-10-15.jsonl` and its summary.
SHA-256: `5715ede0fd4c7164182d2cc67431c43dd0d636b3bc7a2fdc3ec06bb0467ce11e`.
A second cued test sampled the actual Windows Tauri monitor 221 times. Its final
98 samples included the full −32767–32767 Dial range; every displayed signed
value matched its native value, with no JavaScript exceptions. Foreground sampling
during the movements recorded ChatGPT for all 147 observations. Evidence:
`.research/review-audit/moza-monitor.json` (SHA-256
`a6d96b0753a504f13ea7fdd520df6eaa94423f99e788702034c84a2167f718ff`) and
`moza-monitor-foreground-movement.json`. This verifies signed capture and monitor
display on this particular hardware/firmware combination; real virtual-output/game
binding remains separate.

The subsequent Gladiator comparison verified 1,144 displayed unipolar-axis and
286 hat values against native readings, with zero mismatches across 478 unfocused
samples. A separate fixed-page trigger check recorded Pressed/native 1 followed
by Released/native 0 across 1,060 unfocused samples. Neither check raised a
JavaScript exception. These checks complete #22's combined live-display criterion;
the [M1 record](2026-10-14-controller-discovery-m1.md#follow-up-gladiator-monitor-comparison)
retains the timing limitation of the first attempt and hashes of the final records.

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

M7 changes verification tools, CI and documentation, and fixes an empty-inspector
serialization bug found during native UI acceptance. Reading a missing `nodes`
field through mutable JSON indexing had changed `null` into an incomplete object,
causing the stopped UI to display "1–NaN of". Inspection now preserves `null`;
native acceptance checks the empty state before preview and on restart,
as well as populated rule pagination during preview and retained inspection after Stop.
The latency analyzer has authored unit tests on all three CI platforms. The
Windows staging step records the same executable that contains private workers;
unsigned CI artifacts remain explicitly development builds. macOS staging still
does not sign or install anything. Neither staging tool can enable a capability.

## Local M7 checks

Artifact inspection also found a false-green Windows coverage step: Python lookup
failed in the harmless ShellExecute child fixture, but PowerShell returned the
exit code of a later successful coverage command. M7 splits controller, virtual
and host coverage into separate CI steps and separates Windows build/acceptance.
Coverage supplies its actual Python interpreter, and plain cargo tests resolve
Python through PATH before canonicalizing it. Historical overall job success
alone must not be read as proof that every component suite ran successfully;
local Windows virtual coverage had passed with an explicit interpreter path.
After the fix, plain cargo Python discovery passes and all 77 Windows virtual
controller tests pass under instrumentation; all 23 measured modules exceed 71%
(minimum 76.32%, native launcher 81.88%).

The first full macOS desktop run exposed a test timing assumption: a 30 ms sleep
could outlast the test's 150 ms export freshness on a loaded runner. The display
correctly became STALE while the assertion still expected UNKNOWN. The test now
drives the production ExportSession through the existing feed boundary with an
explicit clock, checking the partial frame/counter before committing it, then
advancing past the freshness boundary. No assertion is skipped and no production
timeout is relaxed.

A subsequent macOS run exposed another legacy service-test assumption: LED
feedback followed by a fixed sleep did not prove the input baseline was armed.
That test now observes armed/recovered/error/stale states, uses the normal
1500 ms configured freshness window, and stops its export peer on any exit.
It retains real UDP command delivery and asserts both ordered press/release
datagrams, recovery and final fallback. No production behavior changed.

The first post-audit macOS run passed all 37 mapping-host tests, then failed
coverage aggregation because a raw LLVM profile had a corrupt header. The host
coverage command had included profiles from build tools and deliberately killed
script helpers. It now merges only profiles from the successfully exited host-test
PID, where all host code and fixture threads run. Missing or corrupt host profiles
still fail; no failed test, module or coverage threshold is suppressed. Script
helper production coverage remains the independent logic suite. Local Windows
and Linux host coverage pass with this selection. The corrected macOS job and all
11 jobs passed in [run 38722740479](https://github.com/opensimgear/cockpit/actions/runs/38722740479)
at branch commit `2f216b7`, which contains the implementation fixes `c108fb7` and
`4266c84`. This includes native startup/preview with optional dependencies absent
on Windows, Linux and macOS. The original `4266c84` run was superseded by the
subsequent branch push; the successful result is for that descendant commit.

- Windows 11 build 26200: rebuilt native service, ordered recording preview and
  persistence/restart checks passed with the installed vJoy driver. Inspection
  reported slot 1 FREE, 16 axes, eight buttons, no hats. No acquisition/report,
  layout change or hiding inspection/mutation occurred in the ordinary startup
  path. Local evidence: `.research/m7/windows-native-acceptance.json`.
- WSL/Linux: rebuilt native service; unavailable output assertion, preview and
  restart passed. uinput inspection reported permission denied. This is the local
  unavailable-dependency check, not physical Linux qualification. Evidence:
  `.research/m7/linux-native-acceptance.json`.
- Literal absence is tested separately with bubblewrap's private `/dev`, a
  read-only root, disposable `/tmp` and a writable evidence directory. The same
  namespace contains the acceptance script, native service and private feeder.
  It asserts `/dev/uinput` has no directory entry, `stat` returns ENOENT, inspection
  reports OS error 2, output remains unavailable, and preview/restart still work.
  No host device is removed or remapped. CI uses Ubuntu's distro helper AppArmor
  profile on its disposable runner without disabling global user-namespace
  restrictions. Evidence: `.research/m7/linux-uinput-absent.json` locally and
  `.research/mapping-m3/native-uinput-absent.json` in the CI artifact.
  Local run passed on WSL/Linux; native Ubuntu CI supplies the deployment-host
  acceptance evidence. This does not qualify Linux physical input or output.
- Four latency-analyzer tests passed, including percentile boundary failures,
  script/delay separation, missing diagnostics, ordering and malformed data.
- Windows packaging staged the development desktop artifact and recorded
  `signatureStatus:NotSigned`, `qualified:false`. No signing or installation took
  place. Release-mode staging of that unsigned artifact is rejected.
- The Windows Tauri dev session was restarted and its native WebView returned a
  populated Overview with one gateway/five devices and no active game/integration.
  No physical controller or visibility state was changed for this check.
- README and the GitHub repository description were checked against delivered
  capabilities during this audit: both label mapping experimental and state that
  virtual output/hiding remain qualification-gated. No broader support claim was
  added. Physical/deployment and latency criteria below remain open.
- Post-audit validation: Linux/Windows mapping-host coverage passes 38/37 tests
  with every module above 70% (service 80.00%/72.99%); rebuilt native services pass
  startup, ordered preview, inspector and restart checks. The running Windows
  Tauri Mappings page shows the empty-inspector prompt with no NaN pagination or
  JavaScript exceptions (`.research/m7/native-empty-inspector.png`). Existing
  vJoy configuration and visibility policy are unchanged. Exact-commit CI results
  are recorded on issue #33.

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
