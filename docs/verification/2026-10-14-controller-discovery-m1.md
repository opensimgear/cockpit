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

Physical unplug/replug, simultaneous identical hardware, port moves, deliberate button/axis/hat movements under a foreground game, additional racing/flight models and latency/overload measurements still need production-adapter qualification. Fixtures establish software behavior, not those hardware claims. M0's earlier foreground-game probe remains evidence for that prototype only.

The native UI inspection tool and browser inspection tool both failed to initialize with `windows sandbox failed: helper_unknown_error: setup refresh had errors`. Native process responsiveness, actual service IPC, Svelte validation and compilation were verified; a visual layout acceptance check was not completed.

Linux/macOS capture, output and visibility capabilities remain unavailable until their separate implementation and qualification milestones. Inverse HidHide and the remaining driver/game gates from M0 stay open.
