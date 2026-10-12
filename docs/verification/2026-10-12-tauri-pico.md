# Native Tauri Cockpit and Pico W verification

Verified on macOS on **2026-10-12** with Pico W gateway `OSG-PICO-E661640843565431`. The native application drove the controls. An independent IOHID observer read actual controller descriptors, input reports, OS element values, and attach/remove events; it did not open CDC, send inputs, or change names. All observed inputs belonged to that board.

The detailed lifecycle checks used the native development build. The final standalone release was checked separately at `tauri://localhost`, with its development server stopped, to confirm bundled assets and native services worked together.

## Physical HID checks

| Check | Actual OS evidence |
| --- | --- |
| Four original controller names | Fresh startup and final snapshots showed `OSG Sequential shifter`, `OSG Pedal set`, `OSG Button panel`, and `OSG Handbrake` on interfaces 2–5. Each actual HID Product matched its native USB interface string. |
| Shifter press/release | Raw `01 → 00`; OS Button 1 `1 → 0`. Passed in both the initial native app and final standalone release. |
| Pedal axes | All three OS axes reached 32768. Raw combined report `008000800080` returned to neutral `000000000000`; all axes returned to 0. |
| Original panel highest button | Raw `0080 → 0000`; OS Button 16 `1 → 0` when the gateway stopped. |
| Add module and USB reconnect | Inventory changed from four to five real joystick interfaces. Temporary module `OSG-TAURI-TEST1` published actual HID Product `OSG Tauri test panel` on interface 6, with four buttons and one 16-bit axis. |
| New-module input | OS Button 4 was 1 and axis X was 32768; raw report `080080`. |
| Remove a held module | Raw `080080 → 000000`; OS Button 4 and axis X both returned to 0 before its controller disappeared on reconnect. Fresh inventory returned to exactly interfaces 2–5, with interface 6 absent. |
| Quit while holding a button | Original panel Button 16 was held, then the app quit without a UI release or stop command. The independent observer saw `0080 → 0000` and OS Button 16 `1 → 0`. The application closed its CDC serial handle. |

The session ended with the release app connected and running, original settings restored, all inputs neutral, setup ownership released, and the default handbrake offline. All observer processes were stopped. Workbench, Device focus, and Rig map were exercised in the native window.

The physical composite USB product remains `OpenSimGear Pico Gateway`; individual macOS HID Product names are the module labels.

## Build and firmware export

Application version **0.1.0**, bundle identifier `org.opensimgear.cockpit`, Tauri **2.12.3**, and Rust **1.90.0** were used. The macOS release app and Apple Silicon DMG built successfully under `src-tauri/target/release/bundle/`.

Validation passed:

- **28 Rust tests**.
- **12 frontend transport/guide tests**.
- Svelte checks with **0 errors and 0 warnings**.
- Rust formatting check.

The standalone release's flashing guide saved `osg-pico-gateway.uf2` to Downloads. The exported file was **92,160 bytes** and matched the source UF2 byte for byte, with SHA-256:

```text
888f51190042910f82c35b03edd4a73fef5317bf554962aee742429cb8bdc3ab
```

Exporting did not flash the connected board.

## Local evidence and limits

Raw evidence remains in the ignored local directory `output/tauri-hardware/`; these files are not supplied by a clean checkout:

- `hid-verification-checks.json` and `build-verification.json`: check outcomes, build versions, serial closure, release origin, and firmware export details.
- `hid-events.ndjson`: initial inputs, module addition, held removal, and USB re-enumeration.
- `added-module-hid-snapshot.ndjson`, `restored-hid-snapshot.ndjson`, and `final-release-hid-snapshot.ndjson`: fresh five-controller and restored four-controller inventories and names.
- `release-hid-events.ndjson` and `release-lifecycle-hid-events.ndjson`: highest-button stop and app-quit neutralization.
- `final-release-hid-events.ndjson`: standalone release shifter press/release.
- `cockpit-release.png`, `release-final-ax.txt`, and `firmware-export.png`: standalone UI, final accessibility state, and successful native firmware export.

These observations establish the tested macOS Pico workflows. Windows/Linux hardware behavior remains unverified. Inversion, a CDC-only empty inventory, serial timeout, and physical unplug/replug recovery require separate hardware checks. Automated protocol tests do not substitute for those observations.

## Pico-only follow-up

The Pico-only desktop build was checked on the same date. The simulator selection was removed, and bundled resources contain only firmware and resource documentation. The standalone app connected to the same Pico, discovered the original four modules, and started with neutral shifter inputs and released setup ownership. The screenshot remains local at `output/pico-only/cockpit.png`.

Validation passed: 22 Rust tests, 11 frontend transport/guide tests, 41 Pico bridge/naming/integration tests, two UF2 tests, and the C protocol/lifecycle/report/descriptor checks. Svelte reported zero errors and warnings, formatting passed, and both macOS app and DMG rebuilt successfully. The narrowed independent HID observer enumerated the four Pico controllers with their individual OSG names. This follow-up did not repeat the full physical button/axis/add/remove sequence recorded above.
