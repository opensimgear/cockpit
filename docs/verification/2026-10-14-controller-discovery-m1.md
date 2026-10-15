# M1 Windows controller discovery — 14 October 2026

Implementation for cockpit #22 is experimental. The user authorized continuing implementation with unqualified capabilities disabled and qualification gaps kept open. M0 is not closed by this record.

## Delivered behavior

The `osg-controller-input` crate provides a bounded persistent registry and a dedicated Windows Raw Input/HidP adapter. Devices and Topology share host-controller cards, local names/roles, explicit reassignment, capability diagnostics and a paged live monitor. Gateway setup controls remain on gateway devices. Read-only inventory/inspection remain available during game sessions; conflicting metadata edits use the existing game guard. Controller requests have an independent frontend queue.

HID report IDs, signed fields, null/four-way/eight-way hats, link collections, implicit button releases and unknown usages are covered by authored fixtures. Registry fixtures cover identical devices, duplicate serials, port changes, changed identity, offline reassignment, stationary input, reconnect generations, bounded pagination and invalid persisted data. Native fixtures exercise descriptor errors, enumeration races, oversized/short report buffers and a real hidden message-window lifecycle.

No virtual output, hiding policy, controller acquisition or firmware configuration is activated by discovery. vJoy VID 1234/PID BEAD is excluded. Linux/macOS report capture unavailable in this slice.

## Executed checks

- Windows 64-bit MSVC, pinned Rust 1.90.0: 11 controller tests passed.
- Windows full Cockpit suite: 229 library tests and one native service framing test passed.
- WSL/Linux full Cockpit suite: 239 library tests and one service framing test passed.
- Frontend: Svelte check has zero errors/warnings; 130 transport/helper tests pass; production Vite build passes.
- Native Windows production line coverage: service 150/161 (93.17%), registry 294/296 (99.32%), Windows adapter 446/472 (94.49%). The adapter is measured independently; authored API fixtures and live window startup are included.
- Linux production coverage: service 135/138 (97.83%), registry 314/316 (99.37%). This measures the portable code, not a Linux capture adapter.
- Controller frontend helper coverage: 100% lines, branches and functions. CI enforces at least 71% per compiled native module and frontend helper metric on Ubuntu, Windows and macOS.

Adding native Windows regression execution exposed an existing portable-runtime defect: the generated telemetry profile used `/synthetic`, which is not an absolute Windows path. The internal placeholder now uses the platform temporary directory without reading or creating a catalogue. Windows Steam VDF fixtures also now escape their generated paths correctly and compare executable suffixes as paths.

## Read-only hardware evidence

A five-second native production-adapter capture observed seven collections, all with complete baselines. Device names below are OS-reported strings and do not establish broader vendor qualification.

| VID:PID | Reported product | Collections / fields | Observed reports |
| --- | --- | --- | --- |
| 231D:0200 | VKBsim Gladiator EVO R | 1 / 137 | 62 |
| 231D:011F | VKBSim T-Rudder | 1 / 10 | 63 |
| CAFE:4010 | Debug controllers | 5 / 16, 1, 3, 3, 1 | 497–498 each |

A separate native Windows service IPC check returned `ok: true`, backend `ready`, seven sources and complete baselines in a 2,984-byte correlated inventory response. It used isolated temporary settings and emitted no controls. vJoy was absent from both source inventories. Private paths and full device observations remain in ignored local evidence.

The requested Windows Tauri dev app was built and launched successfully. Because Windows notifications do not propagate reliably from the WSL checkout, a temporary Windows build mirror follows the same source files; the native Tauri watcher rebuilds from that mirror while Vite serves the workspace frontend at 127.0.0.1:1420. Multiple source edits caused successful native rebuilds and responsive app windows. This local dev runner does not change production packaging.

## Qualification still open

The physical background-capture, same-port reconnect and port-change checks below
supersede those gaps for the tested Gladiator. The live monitor comparisons below
verify buttons, unipolar/signed axes and hats while Cockpit is unfocused.
The identical-controller check below completes the remaining physical identity
check. Additional racing/flight models and latency/overload measurements remain
separate M7 work. Signed-axis hardware and
monitor evidence on the MOZA R5 is recorded below. M0's earlier foreground-game
probe remains evidence for that prototype only.

The initial UI inspection tools failed to initialize. Subsequent native WebView
inspection reached the actual Tauri monitor and verified its displayed controls,
as recorded below; this does not establish a full visual layout review.

Linux/macOS capture, output and visibility capabilities remain unavailable until their separate implementation and qualification milestones. Inverse HidHide and the remaining driver/game gates from M0 stay open.

## Interactive Windows checks — 15 October 2026

The user explicitly performed these actions after a capture-start cue. An ignored
probe at `.research/review-audit/capture` links the production
`osg-controller-input` crate, starts `ControllerService::new(None, true)`, and
records descriptors, nonconsuming stream pages, availability transitions and the
foreground window's process ID for 75 seconds. It does not persist settings,
emit virtual reports, acquire controllers exclusively or apply hiding.
Production capture code is unchanged from commit `689ba6c`.

The physical device reports `VKBsim Gladiator EVO R`, VID `231D`, PID `0200`.
Its descriptor contains four unsigned axes, 128 buttons, one hat and four
diagnostic fields. This does not qualify signed-axis hardware or MOZA devices.

| Check | Observation | Acceptance implication |
| --- | --- | --- |
| Foreground-game capture | 5,976 reports; 5,889 with process 33284 (`acr`, window `Assetto Corsa Rally`) in the foreground. Four axes traversed their full declared ranges: X/Y 0–4095, Z/Rz 0–2047. 23 buttons changed between 0 and 1; the hat visited all eight directions and null/neutral 8. Zero stream gaps. | Production background input verified for this hardware. The follow-up signed-axis and native monitor checks below complete criterion 2. |
| Unplug / same-port reconnect | At 22.366 s, `online` and `baselineReady` became false and a disconnect event arrived. At 38.671 s, the same source ID became available with generation 2, a fresh baseline and 1,571 subsequent reports. Zero gaps. | Criterion 5, unplug clears availability, is verified. |
| USB port change | At 20.344 s the original local-path identity went offline. At 26.370 s a different ID appeared with its own fresh baseline while the old identity stayed offline. Zero gaps. Return to the original port was not observed within the timed capture. | Port change does not silently transfer assignments. The subsequent identical-controller check below completes the remaining physical identity check. |

Private raw observations and summaries remain under `.research/review-audit`.
SHA-256 hashes of the three JSONL records, in table order:

- `9355cfdd4220341a624756e2060f53c7f41fbc1cf90db83fcc7130264a2ae0e7`
- `fc85f868293586421a2cc5481c07d212f06579455e390f7158909d7ad9c17112`
- `68160e96730240d5ed26539660868e52ed488f01c0b04242ef4d930d8c4e8a53`

The physical checks below complete #22's hardware criteria. Software commit
`c9114a33cd57ec1bee8af9720bc500cd8344c423` passed
[all 11 CI jobs](https://github.com/opensimgear/cockpit/actions/runs/38727635108),
completing the regression criterion. All seven criteria now have supporting
evidence; closure is tracked on GitHub. The final evidence-only record passes its
validator, hash and diff checks. No output, hiding, script or platform
qualification gate changes as a result of these checks.

The user also connected a left-hand Gladiator. A separate production inventory
observed both units simultaneously with different source IDs and complete
baselines: right `231D:0200` and left `231D:3201` (`VKBsim Gladiator EVO OT L`).
The left descriptor has six unsigned axes (X/Y 0–4095, Z/Rz 0–2047, Rx/Ry
0–1023). These are different product identities, so this inspection does not
complete the identical-device or signed-axis criteria. The inspection is saved
as `.research/review-audit/windows-pair-inspect.jsonl`.

### Follow-up racing hardware and signed monitor

The user confirmed both Gladiators use VKB v2.20C firmware and connect directly to
the PC. The MOZA R5 (346E:0004), firmware v1.2.6.7, is also directly connected;
its HBP handbrake (v1.2.1.3) connects through the base. A cued production capture
received 26,751 R5 reports with no stream gaps, wheel movement, 11 button
press/release pairs and a signed Dial axis crossing −32767–30026.

The actual Windows Tauri monitor then displayed negative and positive signed
values matching native readings, including −32767 and 32767, while ChatGPT was
foreground during the cued movements. The detailed evidence, hashes and hardware
limitations are in [the M7 qualification record](2026-10-15-mapping-qualification-m7.md#user-confirmed-hardware-and-read-only-moza-capture).

### Follow-up Gladiator monitor comparison

An initial short monitor sample showed no deliberate button/hat transitions and
was inconclusive. A synchronized repeat recorded 1,650 production raw reports
with zero stream gaps, four trigger transitions and every hat value (0–7 plus
neutral 8). The actual native monitor was sampled 478 times, all while unfocused.
Comparing displayed values with the monitor's native readings verified 1,144
unipolar-axis and 286 hat values with no mismatches or JavaScript exceptions.
The observed display labels were N, NE, E, S, W, NW and Neutral; all eight raw hat
directions were captured, but SE/SW did not coincide with a monitor sample.

The first comparison changed to the axes/hat page before the user's trigger
holds, so its lack of displayed presses was a sampling-timing problem, not
evidence of a capture failure. A separate fixed-page check waited for an observed
press followed by release. It recorded 1,060 unfocused samples, including
**Pressed / Native 1** and **Released / Native 0**, with no JavaScript exceptions.
Together with the R5 signed-axis comparison, this verifies criterion 2. No
production code or qualification gate was changed for these read-only checks.
The left/right Gladiators have different product IDs and cannot themselves
qualify simultaneous identical controllers; the separate matching pair below
provides that evidence.

The private records remain in `.research/review-audit`:

| Record | SHA-256 |
| --- | --- |
| `windows-gladiator-comparison-2.jsonl` | `65a654667d0c930c0d3c3bf5035eecbd31442e4a416e5cfdbfb470594bc0692b` |
| `gladiator-comparison-ui.json` | `34171dcf346bb70199c1a106d718d538ce2d7b0202be9002ad4a5b1f51445d75` |
| `gladiator-trigger-monitor.json` | `0699ddcc2df5c96c2ade370f68f0ee9a1253dab14166ad08820ef51eb57feb3b` |

### Two identical controllers

The user connected two physical devices named **Controller**, both **0079:0126**,
and confirmed that each connects directly to the PC. Their product model and
firmware were not supplied, so this evidence identifies the tested pair by USB
identity and descriptor, without a vendor-wide claim. Both expose the same 34
fields: four unsigned 8-bit axes (0–255), 13 buttons, one hat and 16 diagnostic
fields. Each receives a separate source ID, a complete baseline and explicitly
limited `local-path` identity confidence. The descriptors compare equal.

One continuous read-only production capture lasted 271.109 seconds. The user
held button 2 on A, released A and held the same button on B. The held states
belonged to different source IDs, without merging or cross-talk. A's same-port
unplug cleared availability and baseline at 99.783 s. At 113.576 s its original
source ID returned with generation 2 and a fresh baseline; its deliberate
press/release then appeared in that generation. B stayed online and baseline-ready
at generation 1 throughout.

Moving A to a different port produced a new source ID and fresh baseline at
181.097 s. By 181.305 s the old A identity was offline with its baseline cleared;
it was not reassigned to the new path. B retained its identity. The final
inventory contained two online matching controllers and the remembered offline
A identity. Captured report counts were 20,175 for A's original identity, 33,054
for B and 10,958 for A's new path, with zero stream gaps. The user was told they
could restore A's original port after capture; that restoration is not claimed
as an observed result. No application settings, output or hiding were changed.

Together with the earlier hotplug/port evidence, independently enumerated debug
HID collections and authored collection/identity fixtures, this completes
criterion 1 for the tested Windows setup. Cross-port matching for these serialless
devices remains an explicit user reassignment; identical labels are not treated
as permanent physical identity. The scratch probe uses an in-memory registry, so
UUID comparisons above are within its continuous session; initial inspection
invocations are not evidence of persisted UUIDs across process restart.

Private records in `.research/review-audit`:

| Record | SHA-256 |
| --- | --- |
| `windows-identical-inspect.jsonl` | `55300b3fc64969d2e32d04b5782299f418d7ce830e0a8a3a72bf10dae33541b2` |
| `windows-identical-comparison.jsonl` | `f507a6009953f2c1df8d1945e5bea678b942c0e98165596bf762ef272a55c1f2` |
| `windows-identical-comparison-summary.json` | `235191d4095d9f0ac6ecfb50c691726800cf1fcf72d97b7ccb0cd4517d349216` |
