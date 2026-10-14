# Linux and macOS controller capture implementation — 14 October 2026

Issue: [#23](https://github.com/opensimgear/cockpit/issues/23). Software implementation proceeds under the user's explicit decision to keep unqualified capabilities disabled. This record does not close platform/hardware qualification.

## Delivered behavior

- Normal Linux/macOS desktop startup leaves controller capture unavailable with an explanation in the existing host-controller workspace. Windows capture retains its existing startup behavior.
- The separate inspect --qualification-probe example explicitly starts a read-only capture worker, without persisted settings, virtual output, device grabs/seizures, helper installation or permission changes.
- Linux uses pinned evdev 0.13.2 and udev 0.9.3, read-only/nonblocking descriptors, joystick-only udev selection, topology/interface/serial observations, shared UUID reassignment and descriptor normalization. It excludes keyboard/mouse interfaces, virtual sysfs devices, virtual-bus inputs and Cockpit virtual identity markers.
- Linux commits coherent SYN_REPORT frames; SYN_DROPPED or bounded-frame loss clears the source generation/baseline, ignores the damaged stream through a subsequent SYN_REPORT, then queries current kernel button and absolute-axis state. Four paired evdev hats normalize to eight directions plus neutral. Unsupported axes remain diagnostics.
- macOS uses pinned iohidmanager 0.11.0, shared IOHID controller subscriptions, USB/Bluetooth transport filtering, bounded callbacks, registry/location/serial observations and supported HID element normalization. It rejects unknown/software transports and Cockpit virtual serial markers. Read-only manager recreation refreshes hotplug independently of the UI run loop.
- macOS obtains baseline values from IOHID, requires observed timestamps and rejects callbacks older than each baseline value. Callback overflow invalidates the source and requires a fresh snapshot. Unknown/array/relative elements remain diagnostics.
- Both adapters use the same bounded inventory, live monitor, naming, roles, explicit reassignment, disconnect cleanup and source generations. Incremental native values preserve held buttons. Reassignment requests a new baseline even when the device is stationary. Diagnostic-only descriptors never become baseline-ready.
- Access failures explain the affected platform's permission setup. Discovery does not require an optional output or visibility backend.

## Executed locally

- Linux controller tests and coverage: service 81.98%, registry 99.68%, shared capture 100%, Linux adapter 72.31% production lines. Each measured module exceeds 70%.
- Windows controller tests/coverage were rerun after the shared lifecycle changes; the Windows adapter retained 94.49% production line coverage.
- Full Linux desktop regression: 239 library tests and one native transport framing test passed.
- Five-second WSL qualification probe: worker started and shut down cleanly, zero evdev controllers exposed. This verifies the empty inventory path, not physical Linux hardware.
- macOS ARM64 Rust type-check, including tests, passed from Linux with dependency build scripts in documentation mode. This does not link/run the native frameworks or Swift bridges; the macOS CI job must perform that validation.
- Existing Windows native Tauri dev remains running and rebuilding from workspace edits.

Tests cover dropped-frame boundaries, kernel re-query instead of synthetic neutral values, held-button deltas, queue loss, stale timestamps, permission errors, metadata/virtual-source filters, duplicate identities, reconnect, reassignment, inventory bounds, shutdown and the normal-startup qualification gate. Authored OS boundary fixtures are not substituted for actual device evidence.

## Open qualification

No native Linux/macOS physical controller or foreground game was tested. Permissions, OS/architecture support, real hotplug, Bluetooth/udev variations, identical hardware, port moves, latency and game background behavior remain open. Linux does not read vendor-only hidraw controls; virtual-sysfs filtering may intentionally exclude a physical device presented through a userspace bridge.

IOHID input-value callbacks currently preserve individual element order; coherent native report grouping and baseline behavior on real devices still require qualification before this adapter can feed an enabled routing session. Neither transport labels nor authored fixtures prove arbitrary third-party virtual devices are physical. The release gate stays closed until the supported device paths, source exclusion policy and native report/baseline behavior are qualified.

Optional Linux/macOS output and hiding remain separate later milestones. No signing, entitlement, helper availability or visibility policy is claimed by this change.

## CI result for the issue commit

Commit b7060cd02d39a18e13da34234d0a0f9d9670cbb0 passed [all CI jobs](https://github.com/opensimgear/cockpit/actions/runs/38616526349), including frontend, full Windows desktop regression, Linux integration acceptance and native controller coverage on Windows/Linux/macOS. The macOS job linked the native frameworks/Swift bridges and passed 18 tests with 71.88% adapter production-line coverage. The Linux controller suite passed 19 tests. These results leave the hardware qualification gaps above open.
