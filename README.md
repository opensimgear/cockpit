# OpenSimGear Cockpit

Work-in-progress desktop application for discovering OpenSimGear gateways, configuring devices, registering controllers and viewing live inputs. Built with Tauri 2, Rust, Svelte 5, TypeScript and Vite.

Cockpit connects to gateways through its normal USB serial and local stream transports. Gateway and device simulations are maintained independently in [device-gateway/simulation](https://github.com/opensimgear/device-gateway/tree/main/simulation) and [device-template/simulation](https://github.com/opensimgear/device-template/tree/main/simulation). Cockpit neither creates simulated devices nor manages their processes.

## Current behavior

Every compatible gateway connects automatically and stays connected independently, including multiple gateways of the same type. The Gateways view shows a card for each gateway and its devices; Devices combines their inventories, and Topology shows one computer branching to every gateway and its device chain. Devices appear as they join after Cockpit starts. Configuration, controller registration, layout changes, live inputs and reconnects are routed by gateway identity. A busy, failed or restarting gateway does not interrupt the others, and identical module identities on separate gateways keep separate state and drafts. Device names come from normal runtime configuration. Previously connected gateways retain their last-known devices while offline.

The STM32 CAN gateway supports ten retained controller slots, each with up to 128 buttons, four hats and eight axes. A setup session edits module names, expansion banks, buttons, hats, encoders, selectors and calibrated axes. Apply activates a validated draft; Apply & save journals it on the device. Add controller stages discovered capabilities; Apply layout saves the gateway registry, rebuilds USB HID and reconnects by gateway UID. Changed profiles, stale devices and offline devices retain neutral controller slots until a valid configuration/layout is active. Direct device USB takes input ownership while its gateway slot remains neutral.

The Pico USB adapter retains normal discovery, setup claims, settings and USB reconnection. Settings remain in board RAM until reset. On macOS, Rust applies individual controller names through IOKit. Its firmware and input-generation tools live in the [gateway repository's Pico directory](https://github.com/opensimgear/device-gateway/tree/main/simulation/pico).

STM32 discovery verifies manufacturer/product metadata, a 24-hex UID and a bounded CDC protocol-2 snapshot. Configuration uses the production Cyphal/CAN contract (`canProtocol=3`) with identity, boot, writer, request and epoch validation. Older private CAN-v2 inventory remains readable. Local gateways receive the same handshake checks as USB serial gateways; on macOS/Linux they advertise in the current user's private `/tmp/opensimgear-<user-id>/gateways/` directory. Local streams do not create OS USB/HID controllers.

## Run and build

Install the [Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/), Node.js 20.19+ or 22.12+, and the pnpm version declared in `package.json`. Rust 1.90.0 is pinned in `rust-toolchain.toml`.

From this repository:

```sh
pnpm install
pnpm dev
```

The desktop uses Rust IPC and its own frontend assets. Development serves the UI on `127.0.0.1:1420`. The installed application needs no Node runtime, Swift compiler, firmware checkout or simulation executable.

```sh
pnpm check
pnpm transport:test
pnpm native:test
pnpm build
```

`pnpm build` packages the application under `src-tauri/target/release/bundle/`. Local macOS builds can use ad-hoc signing; distribution signing/notarization remains future work.

For browser development through the same normal Rust adapters:

```sh
CARGO_TARGET_DIR="$PWD/.e2e/build/cockpit" cargo build \
  --manifest-path src-tauri/Cargo.toml --bin osg-gateway-service
pnpm web:dev
```

Open `http://127.0.0.1:5173/`. The development HTTP bridge starts only the native gateway service. `OSG_GATEWAY_SERVICE` can select another absolute service executable. `pnpm web:native` is an alias for this same transport. Avoid concurrent desktop/browser access to the same gateway. `pnpm ui:build` builds frontend assets; `pnpm preview` previews those static assets without a gateway service.

## Independent simulations and end-to-end tests

Start ordinary Cockpit, then launch one or more gateways with distinct identities from their own repository and JSON-configured devices from theirs. Point each device at its intended gateway identity. Cockpit discovers all gateways and displays each device under its gateway as it joins. Configuration, registration and layout Apply use the production protocol. Closing Cockpit leaves independent gateway and device processes running. See the simulation guides linked above for executable and device JSON commands.

Cockpit's integration runner provisions all required repositories itself:

```sh
pnpm e2e
```

It clones `device-gateway` and `device-template`, initializes their dependencies, builds the simulations through their owning repositories, then exercises the normal Cockpit adapter. Checkouts, build products, state and logs live in this repository's ignored `.e2e/` directory. No adjacent repositories or parent-workspace setup are required. See [the E2E guide](tests/e2e/README.md) for revisions, development source overrides and scenarios.

The [simultaneous gateway acceptance record](docs/verification/2026-10-13-multiple-gateways.md) covers concurrent connections, same-identity device isolation and four gateways in the native UI. The [repository-owned acceptance record](docs/verification/2026-10-13-repository-owned-simulation.md) documents the automated and visible native UI checks. The native gateway/device path exercises production configuration, persistence, descriptors and CAN handlers. Physical STM32 USB/HID enumeration and hardware CAN electrical behavior remain separate hardware acceptance work.

## Repository layout

- `src/`: Svelte/TypeScript interface and frontend tests.
- [src-tauri/](src-tauri/README.md): normal Rust gateway adapters, IPC permissions and protocol tests.
- `scripts/native-gateway-bridge.mjs`: ordinary local browser development transport.
- `tests/e2e/`: repository-owned dependency bootstrap and integration scenarios.
- [integration-profiles/](integration-profiles/README.md): future SimHub integration profiles.
- `docs/verification/`: historical software and hardware acceptance records. Older reports describe the implementation at the time of their run.

## Remaining scope

Direct USB/CAN inventory merging, output sessions, firmware update/recovery UI and SimHub profiles remain future work. Physical Windows/Linux serial/HID acceptance, distribution signing/notarization, production USB allocation and release licensing are pending. Everything remains work in progress; this migration introduces no release versions.

## Related repositories

- [Device gateway](https://github.com/opensimgear/device-gateway): STM32 USB/CAN gateway firmware, electronics and gateway simulation.
- [Device template](https://github.com/opensimgear/device-template): generic module firmware/electronics and JSON-configured device simulation.
- [Firmware common](https://github.com/opensimgear/firmware-common): shared acquisition, profiles, USB support and dependency pins.
- [Protocol](https://github.com/opensimgear/protocol): Cyphal DSDL, transport, CDC contracts and conformance checks.
- [Website](https://github.com/opensimgear/opensimgear.github.io): architecture, guides, equipment database and calculators.

## Licensing

OpenSimGear license selection remains pending; no license grant is implied for newly authored application or firmware code. Upstream components retain their own licenses.
