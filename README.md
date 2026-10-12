# OpenSimGear Cockpit

Desktop application for discovering OpenSimGear gateways, registering controllers and viewing HID inputs, built with Tauri 2, Rust, Svelte 5, TypeScript and Vite. It supports a physical Pico W USB simulator and the STM32 [device-gateway](https://github.com/opensimgear/device-gateway) prototype through separate native serial adapters.

## Current implementation

The interface provides gateway inventory, device search/filtering, controller registration, a connection map, dedicated device detail/input pages, activity history and saved appearance preferences. Compatible gateways are detected automatically; only one gateway backend is selected at a time. Pending controller changes become active through an explicit USB layout reconnect.

| Backend | Implemented behavior | Validation as of 2026-10-12 |
| --- | --- | --- |
| Pico W simulator | Simulated inputs published as real USB HID controllers; add/remove modules, setup claims, settings drafts with Apply & save/Discard, automatic start and reconnect | Physical macOS Tauri and browser tests passed, including real inputs, controller names, four → five → four enumeration, neutralization and serial release |
| STM32 CAN gateway | CAN device discovery, Add controller, persistent registry Apply layout, offline slot retention and read-only input views | Rust/frontend tests, C formatter conformance, cross-firmware integration and macOS app/DMG builds passed; physical STM32 USB/CAN acceptance remains pending |

The **Pico** supports up to eight joystick interfaces, each with 32 buttons and six axes. Settings and registrations remain in board RAM until reset. Rust owns bounded serial requests, protocol checks, UI liveness, setup leases and reconnection to the same board. On macOS, IOKit applies individual controller names directly from Rust. App exit or failed UI liveness neutralizes simulated inputs and releases serial access.

The **STM32 gateway** supports up to ten retained controller slots, each with 128 buttons, four hats and eight axes. Add controller stages discovered capabilities; Apply layout saves the registry in gateway flash, rebuilds USB HID and reconnects by gateway UID. Offline or stale modules keep neutral slots. Direct module USB takes input ownership and leaves the gateway slot neutral. Physical HID continues independently when Cockpit closes and releases serial access. Device settings writes and simulated input controls are unavailable for this backend.

STM32 discovery requires OpenSimGear manufacturer metadata, a product name containing `gateway`, a valid 24-hex UID and a bounded protocol-2 snapshot handshake. Pico discovery requires experimental VID/PID `cafe:4010`, an `OSG-PICO-` serial and a compatible handshake. macOS cu/tty aliases are deduplicated. Controller registration and layout changes remain explicit after automatic discovery.

The STM32 generic device/gateway transport uses **Cyphal/CAN**, identified by `canProtocol=3` in the unchanged CDC v2 console. Cockpit also accepts older gateways reporting private CAN v2 and preserves the observed CAN version in its state. Pico keeps its separate simulator protocol. Runtime module profiles/calibration, output sessions, firmware update/recovery UI, SimHub profiles, direct USB/CAN inventory merging and simultaneous multi-gateway routing remain future work. Windows/Linux physical serial/HID acceptance and distribution signing/notarization are also pending.

## Run and build

Install the [Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/), Node.js 20.19+ or 22.12+, and the pnpm version declared in `package.json`. Rust 1.90.0 is pinned in `rust-toolchain.toml`.

From this repository:

```sh
pnpm install
pnpm dev
```

`pnpm dev` prepares the bundled Pico firmware, launches Tauri and serves the development UI on `127.0.0.1:1420`. Desktop hardware requests use native Rust IPC. The installed app needs neither Node nor a Swift compiler.

```sh
pnpm check
pnpm transport:test
pnpm native:test
pnpm build
```

`pnpm build` prepares resources and packages the app under `src-tauri/target/release/bundle/`. Local macOS builds can use ad-hoc signing; distribution signing is separate release work. Some serial tests use process-wide state; run `pnpm native:test -- --test-threads=1` when reproducing the recorded serial validation.

### Browser development

```sh
pnpm web:dev
```

Open `http://127.0.0.1:5173/`. This mode uses the Vite/Node Pico bridge. STM32 CAN gateway access uses the desktop app. Avoid simultaneous desktop/browser access to the same Pico. `pnpm ui:build` and `pnpm web:build` build frontend assets; `pnpm preview` serves them with the Pico bridge.

### Pico firmware and independent HID checks

```sh
pnpm pico:build
pnpm pico:test
pnpm pico:verify --watch --timeout 20
```

The Pico build produces `simulator/pico/build/osg-pico-gateway.uf2` and a copy in `public/firmware/`. Rebuild the desktop app to refresh its bundled image. The flashing guide exports that fixed UF2 to Downloads without overwriting an existing file; copy it to RPI-RP2 manually. Build/export commands do not flash a connected board. See the [Pico setup guide](simulator/pico/README.md) for SDK/toolchain preparation and lifecycle behavior.

`pico:verify` uses an independent macOS IOHIDManager observer to inspect actual OS descriptors, reports and parsed elements. It requires Apple's developer tools; it is a development verifier rather than an installed-app dependency.

## Validation records

The [native Pico acceptance report](docs/verification/2026-10-12-tauri-pico.md) records actual controller naming, shifter/panel buttons, pedal axes, controller addition/removal, held-input neutralization, serial release, packaged execution without a development server and bundled firmware export. Earlier browser acceptance is recorded in the [Pico guide](simulator/pico/README.md#hardware-acceptance-check).

The [STM32 integration report](docs/verification/2026-10-12-stm32-can.md) records 35 Rust tests, 24 frontend transport/discovery/guide tests, Svelte validation, frontend and macOS packaging builds, C-generated fixtures and cross-firmware software checks. Its browser scenario used simulated responses. No assembled STM32 gateway/module chain was tested. Counts describe that recorded run; host checks do not establish physical or cross-platform acceptance.

Raw Pico logs, screenshots and packaging evidence are retained locally in ignored `output/` directories.

## Repository layout

- `src/`: Svelte/TypeScript interface and transport/discovery tests.
- [src-tauri/](src-tauri/README.md): Rust gateway services, IPC permissions and bundled resources.
- [simulator/pico/](simulator/pico/README.md): Pico firmware, lifecycle tests, build and flashing instructions.
- `simulator/verify-hid.swift`: independent macOS HID observer.
- [integration-profiles/](integration-profiles/README.md): placeholder for future SimHub profiles.
- `docs/verification/`: recorded software and hardware acceptance.

## Planned production scope

A future inventory will merge direct USB and CAN sightings by stable module identity across independent gateways. Routes will bind gateway/module identity to the current session and diagnose duplicate identities. Moving a module will preserve its logical identity and device settings while destination registration and USB layout changes remain explicit; game bindings across different gateway USB identities are not guaranteed.

Runtime configuration will use one writer, separate drafts and explicit Apply/Save with interrupted-write protection. Calibration, replacement compatibility and firmware updates require model/hardware checks and recovery paths. Output sessions need freshness and ownership rules. Canonical Cyphal application definitions cover discovery and input delivery; the STM32 adapter provides registry management through CDC. Module configuration over CAN remains future work.

## Related repositories

- [Device gateway](https://github.com/opensimgear/device-gateway): STM32 USB/CAN gateway firmware and electronics.
- [Device template](https://github.com/opensimgear/device-template): generic Rev B module, build-time control profiles and production/home-milling layouts.
- [Firmware common](https://github.com/opensimgear/firmware-common): shared C acquisition, profiles, USB support and dependency pins.
- [Protocol](https://github.com/opensimgear/protocol): Cyphal DSDL, shared transport, CDC contracts and conformance checks.
- [Website](https://github.com/opensimgear/opensimgear.github.io): architecture, guides, equipment database and calculators.

## Licensing

OpenSimGear license selection remains pending; no license grant is implied for newly authored application or firmware code. Upstream components retain their own licenses. Production USB allocation and release licensing remain pending.
