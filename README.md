# OpenSimGear cockpit

Desktop configuration and HID test application built with **Tauri 2, Rust, Svelte 5, TypeScript, and Vite**. The existing interface retains the website's green and slate palette, light/dark logos, and three switchable layouts:

- **Workbench**: device cards beside an inspector; recommended daily default.
- **Device focus**: more room for one module's configuration and input tuning.
- **Rig map**: gateway and CAN-chain context for connection diagnostics.

## Current behavior

Cockpit supports device discovery and selection, search and filtering, setup ownership, separate settings drafts with explicit Apply & save/Discard, simulated input controls, adding/removing modules, explicit USB layout reconnects, activity history, and saved appearance preferences.

A **Pico W USB gateway** publishes simulated CAN module controls as real USB HID joystick interfaces. The desktop app communicates through a native Rust serial service; it does not require a Node server or HTTP bridge at runtime. The service owns bounded protocol requests, firmware identity checks, heartbeat/liveness handling, setup leases, and reconnection to the same board. On macOS, Rust applies individual controller names directly through IOKit. The Pico backend requires no Apple virtual-HID entitlement or signing identity. Follow the [Pico flashing and setup instructions](simulator/pico/README.md).

The optional **macOS · CoreHID** backend uses a bundled Swift helper supervised by Rust. Virtual controller creation still requires Apple-authorized signing and provisioning. CoreHID settings stay in the helper until it exits; Pico settings and registration stay in board RAM until reset. Physical CAN/sensors, persistent configuration, production protocol adapters, firmware update automation, and usable SimHub profiles remain pending. Axis calibration remains a visual demo.

Serial access opens only after **Connect Pico**. Stop/disconnect neutralizes inputs, and **Disconnect Pico**, application exit, lost UI liveness, or serial failure releases the connection. USB controller interfaces remain enumerated until a physical disconnect or explicit layout reconnect. Setup sessions are independent of input testing.

## Run the desktop app

Install the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your operating system, Node.js 20.19+ or 22.12+, and pnpm. Rust **1.90.0** is pinned in `rust-toolchain.toml` for the current Tauri dependencies. On macOS, the development/build preparation also compiles the optional CoreHID helper with Apple's command-line Swift tools; the packaged Pico workflow needs neither Node nor a Swift compiler.

From this repository:

```sh
pnpm install
pnpm dev
```

`pnpm dev` prepares the helper, runs Tauri, and opens the desktop window. Tauri supplies `TAURI_ENV_PLATFORM`, so its Vite UI server uses `127.0.0.1:1420` and installs no Node gateway bridges. Hardware requests go through native IPC.

To validate and package:

```sh
pnpm check
pnpm transport:test
pnpm native:test
pnpm build
```

`pnpm build` prepares desktop resources and runs `tauri build`; packaged artifacts are written under `src-tauri/target/release/bundle/`. Local macOS development can use ad-hoc signing without an Apple identity for the Pico backend. Distribution signing and notarization are separate release work. Windows/Linux device access and packaging have not been verified on physical machines.

### Browser development fallback

The browser prototype remains available explicitly:

```sh
pnpm web:dev
```

Open `http://127.0.0.1:5173/`. This mode retains the local Vite/Node serial and CoreHID bridges. Do not connect the same Pico from desktop and browser simultaneously. `pnpm ui:build` and `pnpm web:build` build frontend assets only; `pnpm preview` serves those assets with the browser bridges. The normal desktop build and installed app use Rust native services.

### Pico W USB simulator

```sh
pnpm pico:build
pnpm pico:test
```

The firmware build produces `simulator/pico/build/osg-pico-gateway.uf2` and a copy in `public/firmware/`. Cockpit's **Flashing instructions** opens its local guide. In the desktop guide, **Download gateway firmware** saves the bundled UF2 to Downloads without overwriting an existing file; copy the saved file onto RPI-RP2 yourself. Rebuild the desktop app after a firmware build to refresh its bundled image. No build or download command flashes a connected board.

Choose **Pico W · USB**, refresh ports, **Connect Pico**, then **Start gateway**. Select a module and open **Inputs**. Use **Add device** or **Remove module**, followed by **Reconnect gateway** and **Start gateway**, to test a changed USB layout. Up to eight independent interfaces can each expose 32 buttons and six axes. Offline modules retain neutral slots; an empty inventory leaves CDC serial available. Logical module identities remain stable while compact interface numbers can change after removal.

A physical Pico W was tested with the earlier browser bridge on **2026-10-12**, including real buttons/axes, addition/removal of a fifth controller, neutralization, and recovery across USB layout reconnects. Firmware 0.1.1 was then flashed and its individual OSG names verified after the automatic reboot. These historical observations are recorded in [the Pico acceptance notes](simulator/pico/README.md#hardware-acceptance-check) and `output/pico-hardware/`. They establish firmware/browser behavior; native Tauri acceptance is recorded separately.

### macOS CoreHID signing prerequisite

This Mac has no Apple signing assets for virtual HID. The unentitled helper was denied controller creation, and ad-hoc signing with the restricted entitlement was rejected. Administrator access does not grant it. Pico USB controllers work independently of this prerequisite.

Follow the [CoreHID setup instructions](simulator/macos/README.md), then use a provisioned executable:

```sh
OSG_HID_HELPER='/absolute/path/to/provisioned/osg-gateway-simulator' pnpm dev
```

Desktop preparation copies that executable into the bundle. The Rust supervisor also accepts the same absolute-path environment override when launching the app. Otherwise it uses the helper packaged during the build, without runtime compilation. CoreHID requires macOS 15 or later.

Input controls, module presence, settings ownership, and auxiliary-panel registration retain the existing workflows. Label changes appear on the next CoreHID reconnect. Actual CoreHID creation and report delivery remain unverified until signed/provisioned assets are available.

### Independent simulator checks

```sh
pnpm simulator:build
pnpm simulator:test
pnpm simulator:verify --watch --timeout 20
```

The IOHIDManager verifier prints OS enumeration, descriptors, raw reports, and parsed elements for both backends. It observes devices independently of Cockpit state. Host C sanitizer tests, firmware/UF2 checks, mocked Rust/Node transport tests, and UI checks do not substitute for physical device tests. The Pico's `cafe:4010` identifiers and simulator wire protocol are experimental; production USB allocation and protocol integration remain pending.

### Native acceptance

The macOS Tauri app was tested on **2026-10-12** with the physical Pico W and an independent IOHID observer. Individual controller names, shifter/panel buttons, all three pedal axes, real **four → five → four** controller enumeration, held-input neutralization on removal and app quit, and serial release passed. The final release app also delivered shifter inputs from bundled assets with its development server stopped, and exported the bundled firmware to Downloads with the expected SHA-256. All three UI layouts remain available.

The release app and DMG built successfully; **28 Rust tests**, **12 frontend transport/guide tests**, formatting, and Svelte checks passed. See the [native hardware and build report](docs/verification/2026-10-12-tauri-pico.md). Raw logs, build records, and screenshots remain local in the ignored `output/tauri-hardware/` directory. Windows/Linux hardware behavior and entitled CoreHID delivery require their own platform checks.

## Layout

- src/: Svelte/TypeScript interface.
- [simulator/pico/](simulator/pico/README.md): Pico W firmware, C protocol tests, UF2 build, browser bridge fallback, and flashing instructions.
- [simulator/macos/](simulator/macos/README.md): optional Swift CoreHID helper; the independent HID observer supports both backends.
- [src-tauri/](src-tauri/README.md): Tauri shell, native Rust gateway services, capabilities, and bundled resources.
- [integration-profiles/](integration-profiles/README.md): versioned SimHub profiles; initial integration is Windows-specific.

## Version-one scope

Configure connected modules through direct USB serial or any of multiple USB-connected gateways. Present one device inventory keyed by stable module identity, show current and last-known gateway attachment separately, and resolve actions through current routes. Each gateway owns a separate single CAN chain through two parallel sockets; node IDs are scoped by stable gateway identity, not globally unique. Merge direct USB/CAN sightings, report duplicate identities across independent chains, and invalidate routes and pending sessions when their gateway disconnects or restarts. Module moves retain logical identity and module settings; destination registration and affected gateway HID layout changes remain explicit, with no guarantee that game bindings follow the move.

Discover capabilities, register/replace modules explicitly, preserve compatible settings, and require fresh calibration for replacement hardware where applicable. Use explicit Apply/Save, a single configuration writer, and clear USB reconnect notices for layout changes. Offline backup/restore is deferred.

Open device serial ports only during an explicit setup/test session and release them afterward so SimHub can access them. Firmware updates are user-initiated through direct USB, with model/hardware checks and interrupted-update recovery; a debug recovery path remains required. No automatic flashing.

End configuration sessions when a route changes, leave drafts uncommitted, and require a fresh writer claim and explicit Apply/Save on the new route. Cached gateway inventory outside a serial session is last-known state, not live reachability.

Use a compatible pinned protocol release through Rust/TypeScript adapters and shared conformance vectors. Builds on all three operating systems do not substitute for testing real device access on each platform.

## Related repositories

- [Protocol](https://github.com/opensimgear/protocol): shared definitions and compatibility rules.
- [Firmware common](https://github.com/opensimgear/firmware-common): reusable C implementation.
- [Device template](https://github.com/opensimgear/device-template): new-device scaffold with planned hello-world firmware and starter KiCad board.
- [Cockpit](https://github.com/opensimgear/cockpit): desktop configuration application.
- [Website](https://github.com/opensimgear/opensimgear.github.io): system architecture and user documentation.

## Licensing

License selection is pending. No license grant is implied by this scaffold. Firmware, mechanical design, and electronics will each need documented license coverage before releases and any public USB identifier application.
