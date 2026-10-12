# Tauri desktop services

Cockpit is a Tauri 2 desktop app using the existing Svelte 5/TypeScript interface. `pnpm dev` prepares desktop resources and launches Tauri; `pnpm build` packages the app. Rust 1.90.0 is pinned in the repository toolchain file. Install the [platform prerequisites](https://v2.tauri.app/start/prerequisites/) before building.

## Native gateway services

The main window invokes `pico_gateway` with the existing JSON request/state contract. The desktop UI never falls back to HTTP gateway requests. The explicit `pnpm web:dev` mode retains the Pico Vite/Node bridge for browser development.

The Pico service owns USB serial enumeration, automatic or explicit connection, bounded requests with correlation IDs, firmware/protocol validation, control lines, heartbeat and UI liveness, module setup leases, and reconnection to the same physical gateway identity. Setup ownership gates configuration and layout changes; disconnect, failure, and reconnect invalidate pending work and claims. Stop/disconnect and application exit neutralize reports and release serial access. Firmware inventories and settings remain in Pico RAM.

On macOS, individual Pico HID service names are applied directly from Rust through IOKit, matching VID/PID, usage, board serial, and USB interface. No runtime Swift compiler or virtual-HID entitlement is required for Pico communication or naming. The composite USB product retains its gateway name, and applications that cache names may need a rescan.

The separate `stm32_gateway` command adapts the physical STM32 CAN gateway's
CDC protocol 2 with either legacy CAN v2 or Cyphal application contract 3. USB metadata selects OpenSimGear gateway candidates with
24-digit hexadecimal UID serials; a correlated bounded snapshot verifies their role,
versions, identity and capabilities before accepting state. No experimental
VID/PID is baked into discovery. Its worker polls inventory and decodes up to
128 buttons, four hats and eight axes per module. Registry changes carry the
observed gateway epoch; stale epochs refresh state and reject the write. Apply
reconnects to the same UID, including after a changed serial port path. A lost
apply acknowledgement triggers inventory reconciliation without replaying the
flash operation. Offline registered slots remain visible.

Physical gateway HID input continues independently of Cockpit and CDC. Closing
the service releases serial access without sending a stop or changing inputs.
Simulated input, presence, Add device and start/stop are unsupported on physical
STM32 gateways. Runtime configuration routes bounded commands to the module over
Cyphal, using its exclusive writer, drafts and Apply/Save/Discard. Claims bind to
gateway epoch and module identity/boot/route; settings readback waits for activation
and reconciles lost commit replies without repeating the mutation. Trusted active
configuration caches bind to the module's boot and configuration generation. The prototype gateway uses
one HID interface with separate report IDs/collections, so the Pico macOS naming
worker is not used. Each backend currently owns one connection; routing several
physical gateways and merging direct USB device sightings remain future work.

In debug builds, `OSG_NATIVE_EMULATOR` selects an absolute executable for the native
production-C emulator. Its serial factory keeps one process across connections so
USB layout reconnects retain the emulated flash. `osg-emulator-service` exposes the
same Rust STM32 service to the local development browser bridge; it provides no
production HTTP endpoint. See the [native configuration acceptance](../docs/verification/2026-10-12-native-configuration.md).

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

The earlier [STM32 integration run](../docs/verification/2026-10-12-stm32-can.md)
passed 35 Rust tests, including 13 STM32 gateway adapter tests using mocked
transport and [C formatter fixtures](tests/fixtures/README.md).
These are host and mocked transport checks; physical STM32 USB/CAN hardware
acceptance remains pending. Direct-USB/CAN inventory merging, multiple gateway
routes and firmware update automation remain future work.

The subsequent runtime configuration checks passed 38 Rust tests and 29 frontend
transport/configuration tests. They exercise the actual Rust adapter
against the native C gateway/module and real Cyphal frames, including all control
types, flash recovery, descriptor changes, boot/generation cache invalidation and
lost-reply readback. The browser
acceptance report above records the full visible Cockpit flow and its physical
USB/HAL test boundary.
