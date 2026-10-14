# Pico debug gateway verification — 2026-10-14

The Pico W debug target and ordinary Cockpit adapters now exercise current gateway/device behavior with eight virtual devices. The firmware links the production registry, layout service, HID descriptors and flash journal plus pinned shared acquisition, runtime configuration, actions and ordered events. Its virtual bus carries application messages internally; it does not model CAN electrical or Cyphal frame transport.

## Software evidence

- Sanitized firmware tests pass for acquisition, maximum input types, configuration/calibration, automatic enrollment and controller changes, HID-disabled events, actions/readback, reboot/persistence, stale boots, watchdog/disconnect recovery and failed-add faults.
- New portable line coverage: debug gateway 100%, virtual device 98.92%, storage 100%; each has an automatic gate above 70%.
- The legacy parser/USB-layout and UF2 suites pass. All 41 legacy bridge/integration/HID-name helper tests pass.
- The Pico native snapshot passes 157 Rust tests; a subsequent combined-workspace run passes 175 tests. Measured Pico-task coverage includes actions 97.86%, CDC3 adapter ranges 95.45%, Pico 86.28% and local discovery 88.40%; CI enforces the thresholds.
- Frontend type checks have zero errors/warnings; all 102 transport, input queue and catalogue-preset tests pass. New helper coverage is 89.42% lines, 99.06% branches and 100% functions. Production frontend assets build successfully. Official Svelte autofixer checks found no outstanding issues in changed components.

Cross-process acceptance runs the actual asynchronous Pico firmware host harness, ordinary Rust gateway service and authored DCS-BIOS UDP peer in a private discovery namespace. It excludes physical ports and saves profile settings in scratch storage. Successful reports are at `device-gateway/.research/pico/acceptance/pico-997ae4a58121/report.json` and `pico-0c643976148a/report.json` in this workspace; the latter uses the rebuilt combined-workspace service. The reusable runner is [debug-acceptance.py](https://github.com/opensimgear/device-gateway/blob/main/simulation/pico/debug-acceptance.py).

It verifies discovery, raw axes/buttons, 128 logical buttons/eight axes/four hats, configuration and inversion, per-panel HID disable, ordered momentary/switch/encoder/axis DCS commands, exact detent counts, LED acknowledgement, frequency/preset/formatted-volume display feedback, stale fallback/recovery and reboot/stale-boot guards. The peer uses synthetic addresses; these are not aircraft addresses for a live profile.

Real browser acceptance uses ordinary Cockpit's browser transport and the firmware process, with no frontend inventory mocks. It adds panel presets through the gateway UI, drives raw/calibrated axes and button/switch events, observes applied LED/display values, edits calibration during Setup, saves a catalogue-validated six-input/five-output profile, and verifies hats, selectors and encoder HID release against the corrected firmware. Bounded direct-USB/offline/reboot/reset checks pass; the final browser console has zero warnings/errors. Local evidence includes `.research/output/playwright/pico-debug-gateways.png` and `.research/pico-debug/ui-dcs-settings.json` in the parent workspace. Test browser, Vite and firmware processes were stopped after acceptance.

## Recovery fixes verified

- USB serial casing remains identical to the older SDK-derived board serial; only the internal UID is canonicalized.
- Reset maps declared neutral through inverse calibration and cancels incomplete quadrature cycles without generating a DCS detent.
- A failed gateway metadata commit cannot make the next added preset restore orphan settings from a different device type.
- Successful immutable USB delivery supplies the virtual ACK identity omitted by the compact wire ACK. Encoder press/release queues now advance and return to neutral.
- Stale UI queues retain boot guards, and pending axes reconcile with authoritative raw samples after completion.

## RP2040 build

The final UF2 is `device-gateway/.research/pico/build/osg-pico-gateway.uf2` (197,120 bytes), SHA-256 `b040a27e3ca88e091ba61557d6c633bc4ce37ac777f4a1668aedcea3a6e9f1e6`. The manifest records Pico SDK `079c6f39023649b154152db30f1d781e884879bc`, firmware-common `8418239579b084277e7932627c86e03a4b82b345` and device-template `d2570a4ff4781e8eebaeca8a167ee007f3a1303a`.

Static RAM is 154,352 bytes, with a reserved 24 KiB core-0 stack and 72 KiB of independently erasable journal sectors. The linker and builder reject firmware overlap with persistent storage. All builds, cloned dependencies and scratch evidence live under `.research/`.

## Qualification boundary

No attached physical Pico was flashed during this work. Software-peer results establish the firmware/application path, not physical USB enumeration, Windows game binding retention, electrical inputs/displays/CAN, or live Windows DCS behavior. The host harness uses volatile settings; journal persistence is covered by C/storage tests and compiled into the physical target. Follow the [Pico guide](https://github.com/opensimgear/device-gateway/tree/main/simulation/pico) to flash and qualify the board with the installed DCS-BIOS catalogue.
