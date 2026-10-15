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
supersede those gaps for the tested Gladiator. Simultaneous identical hardware,
signed-axis hardware and matching monitor display, additional racing/flight
models and latency/overload measurements remain open. M0's earlier foreground-game
probe remains evidence for that prototype only.

The native UI inspection tool and browser inspection tool both failed to initialize with `windows sandbox failed: helper_unknown_error: setup refresh had errors`. Native process responsiveness, actual service IPC, Svelte validation and compilation were verified; a visual layout acceptance check was not completed.

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
| Foreground-game capture | 5,976 reports; 5,889 with process 33284 (`acr`, window `Assetto Corsa Rally`) in the foreground. Four axes traversed their full declared ranges: X/Y 0–4095, Z/Rz 0–2047. 23 buttons changed between 0 and 1; the hat visited all eight directions and null/neutral 8. Zero stream gaps. | Production background input verified for this hardware. Criterion 2 remains unchecked because signed-axis hardware and matching live monitor display are still outstanding. |
| Unplug / same-port reconnect | At 22.366 s, `online` and `baselineReady` became false and a disconnect event arrived. At 38.671 s, the same source ID became available with generation 2, a fresh baseline and 1,571 subsequent reports. Zero gaps. | Criterion 5, unplug clears availability, is verified. |
| USB port change | At 20.344 s the original local-path identity went offline. At 26.370 s a different ID appeared with its own fresh baseline while the old identity stayed offline. Zero gaps. Return to the original port was not observed within the timed capture. | Port change does not silently transfer assignments. Criterion 1 remains unchecked for the simultaneous identical-controller matrix. |

Private raw observations and summaries remain under `.research/review-audit`.
SHA-256 hashes of the three JSONL records, in table order:

- `9355cfdd4220341a624756e2060f53c7f41fbc1cf90db83fcc7130264a2ae0e7`
- `fc85f868293586421a2cc5481c07d212f06579455e390f7158909d7ad9c17112`
- `68160e96730240d5ed26539660868e52ed488f01c0b04242ef4d930d8c4e8a53`

Issue #22 remains open/In review with 5/7 acceptance criteria checked. No output,
hiding, script or platform qualification gate changes as a result of these checks.

The user also connected a left-hand Gladiator. A separate production inventory
observed both units simultaneously with different source IDs and complete
baselines: right `231D:0200` and left `231D:3201` (`VKBsim Gladiator EVO OT L`).
The left descriptor has six unsigned axes (X/Y 0–4095, Z/Rz 0–2047, Rx/Ry
0–1023). These are different product identities, so this inspection does not
complete the identical-device or signed-axis criteria. The inspection is saved
as `.research/review-audit/windows-pair-inspect.jsonl`.
