# Tauri desktop services

Cockpit is a Tauri 2 desktop app using the existing Svelte 5/TypeScript interface. `pnpm dev` prepares desktop resources and launches Tauri; `pnpm build` packages the app. Rust 1.90.0 is pinned in the repository toolchain file. Install the [platform prerequisites](https://v2.tauri.app/start/prerequisites/) before building.

## Native gateway services

The main window invokes `pico_gateway` with the existing JSON request/state contract. The desktop UI never falls back to HTTP gateway requests. The explicit `pnpm web:dev` mode retains the Pico Vite/Node bridge for browser development.

The Pico service owns USB serial enumeration, explicit connection, bounded requests with correlation IDs, firmware/protocol validation, control lines, heartbeat and UI liveness, module setup leases, and reconnection to the same physical gateway identity. Setup ownership gates configuration and layout changes; disconnect, failure, and reconnect invalidate pending work and claims. Stop/disconnect and application exit neutralize reports and release serial access. Firmware inventories and settings remain in Pico RAM.

On macOS, individual Pico HID service names are applied directly from Rust through IOKit, matching VID/PID, usage, board serial, and USB interface. No runtime Swift compiler or virtual-HID entitlement is required for Pico communication or naming. The composite USB product retains its gateway name, and applications that cache names may need a rescan.

## Build resources and permissions

`pnpm desktop:prepare` copies the built Pico firmware into the bundle resources. The installed Pico workflow needs no external development tools. Locally built macOS apps can use ad-hoc signing for Pico testing; distribution signing/notarization is separate release work.

Only the `main` window receives native gateway command permissions. External opening is restricted to the listed documentation HTTPS URLs. The flashing guide is a bundled Vite entry point in a separate local window without gateway command access. Its only native action exports the fixed bundled UF2 to Downloads without overwriting existing files; it accepts no arbitrary path or firmware input and never flashes a board. Packaged content uses a restrictive content security policy; the Tauri development server on port 1420 does not install gateway HTTP bridges.

## Validation

```sh
pnpm check
pnpm transport:test
pnpm native:test
pnpm build
```

Transport tests verify native-only IPC and browser fallback, ownership identity, error reconciliation, release behavior, firmware export routing, and link routing. Rust tests verify Pico protocol, serial lifecycle, naming, and firmware export; mocked transports do not establish physical USB enumeration or report delivery. On 2026-10-12, the Pico-only build passed 22 Rust tests and 11 frontend tests, and macOS release app/DMG packaging succeeded.

Independent [native hardware evidence](../docs/verification/2026-10-12-tauri-pico.md) confirms actual Pico inputs and names, adding/removing real controller interfaces, held-input neutralization on removal and quit, and serial release. The final release app ran from bundled assets with no development server; native firmware export was checked against the source UF2 hash. Historical browser evidence remains in `output/pico-hardware/`. Windows/Linux device access and packaging remain unverified on their target systems.

Physical CAN discovery, direct-USB/CAN inventory merging, multiple gateway routes, production protocol adapters, persistent settings, and firmware update automation remain future work described by the repository's version-one scope.
