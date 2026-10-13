# OpenSimGear Cockpit

Work-in-progress desktop application for discovering OpenSimGear gateways, configuring devices, managing automatically enabled USB controllers and viewing live inputs. Built with Tauri 2, Rust, Svelte 5, TypeScript and Vite.

Cockpit connects to gateways through its normal USB serial and local stream transports. Gateway and device simulations are maintained independently in [device-gateway/simulation](https://github.com/opensimgear/device-gateway/tree/main/simulation) and [device-template/simulation](https://github.com/opensimgear/device-template/tree/main/simulation). Cockpit neither creates simulated devices nor manages their processes.

## Current behavior

Every compatible gateway connects automatically and stays connected independently, including multiple gateways of the same type. The Gateways view shows a card for each gateway and its devices; Devices combines their inventories, and Topology shows one computer branching to every gateway and its device chain. Devices appear as they join after Cockpit starts. Configuration, controller lifecycle changes, live inputs and reconnects are routed by gateway identity. A busy, failed or restarting gateway does not interrupt the others, and identical module identities on separate gateways keep separate state and drafts. Device names come from normal runtime configuration. Previously connected gateways retain their last-known devices while offline.

The current STM32 CAN gateway remembers up to 32 devices, each with up to 128 buttons, four hats and eight axes. New identities enable and append to the persisted USB controller order automatically. Enabled offline devices appear ghosted and retain neutral HID collections. Disable removes a collection while remembering the device; continuing announcements and gateway restarts do not re-enable it. Explicit Enable appends it after surviving controllers. Remove forgets an offline identity. Disable and removal of a published controller warn that remaining game bindings may need updating. Online disabled devices remain configurable. Direct device USB takes input ownership while its enabled gateway controller remains neutral.

A setup session edits module names, expansion banks, buttons, hats, encoders, selectors and calibrated axes. Apply activates a validated draft; Apply & save journals it on the device. Existing USB control profiles remain frozen across descriptor additions. A changed enabled profile stays neutral until an explicit Update profile, with a reconnect/binding warning. The firmware batches, persists and publishes controller changes itself; Cockpit observes and reconnects by gateway UID without replaying an ambiguous mutation. USB descriptor changes can still affect Windows/game identity or require a game restart; actual binding retention remains hardware qualification work.

The Pico USB adapter retains normal discovery, setup claims, settings and USB reconnection. Settings remain in board RAM until reset. On macOS, Rust applies individual controller names through IOKit. Its firmware and input-generation tools live in the [gateway repository's Pico directory](https://github.com/opensimgear/device-gateway/tree/main/simulation/pico).

STM32 discovery verifies manufacturer/product metadata, a 24-hex UID and bounded CDC protocol-3 automatic-lifecycle snapshots. Explicit report IDs and descriptor positions are validated independently. Capacity and admission failures remain visible. Configuration uses the production Cyphal/CAN contract (`canProtocol=3`) with identity, boot, writer, request and epoch validation. After Apply, readback follows the device's renewed lease with bounded read retries; a device reboot stops reconciliation, and writes are never replayed. CDC-v2 gateways remain readable with their older manual registration/layout Apply controls; those adapters do not claim automatic Enable/Disable/Remove support. Local gateways receive the same handshake checks as USB serial gateways; on macOS/Linux they advertise in the current user's private `/tmp/opensimgear-<user-id>/gateways/` directory. Local streams do not create OS USB/HID controllers.

## Controller workflow

The automatic controller workflow is recorded in the [USB HID architecture](https://github.com/opensimgear/opensimgear.github.io/blob/main/src/content/docs/gear/architecture/usb-hid.md) and [Cockpit #5](https://github.com/opensimgear/cockpit/issues/5). Enabled devices have dynamic collections. Offline ghosts retain them temporarily; Disable or Remove deletes them. A replacement identity appends independently and keeps its own module settings/calibration. This implementation does not provide permanent empty collections or transfer another device’s calibration.

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

`pnpm build` packages the application under `src-tauri/target/release/bundle/`. Local macOS builds can use ad-hoc signing.

GitHub Actions runs frontend type checks, transport tests and the UI build on every push and pull request. A Linux native job compiles/tests the Rust application, checks dependency bootstrap behavior, and runs the independent gateway/device acceptance scenarios with sanitizers. Integration dependency manifests and run reports are uploaded as CI artifacts.

For browser development through the same normal Rust adapters:

```sh
CARGO_TARGET_DIR="$PWD/.e2e/build/cockpit" cargo build \
  --manifest-path src-tauri/Cargo.toml --bin osg-gateway-service
pnpm web:dev
```

Open `http://127.0.0.1:5173/`. The development HTTP bridge starts only the native gateway service. `OSG_GATEWAY_SERVICE` can select another absolute service executable. `pnpm web:native` is an alias for this same transport. Avoid concurrent desktop/browser access to the same gateway. `pnpm ui:build` builds frontend assets; `pnpm preview` previews those static assets without a gateway service.

## Independent simulations and end-to-end tests

Start ordinary Cockpit, then launch one or more gateways with distinct identities from their own repository and JSON-configured devices from theirs. Point each device at its intended gateway identity. Cockpit discovers all gateways and displays each device under its gateway as it joins. Configuration and automatic controller lifecycle use the production protocol; older firmware retains registration and layout Apply. Closing Cockpit leaves independent gateway and device processes running. See the simulation guides linked above for executable and device JSON commands.

Cockpit's integration runner provisions all required repositories itself:

```sh
pnpm e2e
```

It clones `device-gateway` and `device-template`, initializes their dependencies, builds the simulations through their owning repositories, then exercises the normal Cockpit adapter. Checkouts, build products, state and logs live in this repository's ignored `.e2e/` directory. No adjacent repositories or parent-workspace setup are required. See [the E2E guide](tests/e2e/README.md) for revisions, development source overrides and scenarios.

The [automatic lifecycle acceptance record](docs/verification/2026-10-13-automatic-controller-lifecycle.md) covers current enrollment, ghosts, Disable/Enable/Remove, zero controllers, reconnect recovery and UI warnings. The [simultaneous gateway acceptance record](docs/verification/2026-10-13-multiple-gateways.md) covers concurrent connections, same-identity device isolation and four gateways in the native UI. The [repository-owned acceptance record](docs/verification/2026-10-13-repository-owned-simulation.md) documents the earlier automated and visible native UI checks. The native gateway/device path exercises production configuration, persistence, descriptors and CAN handlers. Physical STM32 USB/HID enumeration and hardware CAN electrical behavior remain separate hardware acceptance work.

## Repository layout

- `src/`: Svelte/TypeScript interface and frontend tests.
- [src-tauri/](src-tauri/README.md): normal Rust gateway adapters, IPC permissions and protocol tests.
- `scripts/native-gateway-bridge.mjs`: ordinary local browser development transport.
- `tests/e2e/`: repository-owned dependency bootstrap and integration scenarios.
- [integration-profiles/](integration-profiles/README.md): integration profile directory.
- `docs/verification/`: historical software and hardware acceptance records. Older reports describe the implementation at the time of their run.

Implementation and qualification work is tracked in the [GitHub project](https://github.com/orgs/opensimgear/projects/2).

## Manual device actions

Each online CAN device can advertise a bounded action catalogue independently of
its HID controls. Cockpit discovers the catalogue once per current route, then
renders manual controls for booleans, enums, bounded integers, RGB colors, bounded
UTF-8 strings and actions without arguments. The device workspace's **Device
actions** panel provides discovery, Apply/Run and applied-state readback. Controls
remain available when a remembered device's HID controller is disabled or direct
USB owns its inputs. Older firmware continues to provide inputs without actions.

The JSON device simulation advertises RGB LED 1 and boolean LED 2. The current
board advertises its physical status LED's `auto`, `on` and `off` modes. `auto`
restores firmware status indication; LED overrides reset on device reboot. These
temporary actions do not acquire configuration claims, save flash or change HID
descriptors. Applied state confirms the device callback, rather than electrical
verification of the LED.

Cockpit binds cached descriptors/readback to gateway identity, epoch, device
identity, boot and route, and validates arguments against the catalogue revision
before transmitting. A lost invoke reply leaves state uncertain and is never
replayed automatically. Reconnect and use **Refresh state** to reconcile it;
catalogue changes require **Discover again**. See the
[action protocol](https://github.com/opensimgear/protocol/blob/main/serial/device-actions.md).

`pnpm transport:test` includes argument, routing and state-label tests. Native
tests cover discovery, stale revisions/boots, unsupported firmware, read retries
and uncertain replies. `pnpm e2e` also exercises actual production firmware
processes for RGB/boolean outputs, disabled HID, direct USB, same-identity devices
on separate gateways and lost-reply recovery. DCS-BIOS bindings are subsequent
milestones; manual actions establish their reusable output interface.
The [device actions acceptance record](docs/verification/2026-10-13-device-actions.md)
documents the production process checks and measured coverage; `pnpm
actions:coverage` enforces the frontend action helper's coverage threshold. CI
also runs `scripts/action-coverage.py` to gate Rust production action coverage
above 70%.

The combined device-action and DCS-BIOS roadmap is tracked by these milestone stories:

1. [Manual device actions](https://github.com/opensimgear/cockpit/issues/13): advertised actions, gateway routing and Cockpit controls.
2. [First DCS-BIOS button and LED path](https://github.com/opensimgear/cockpit/issues/14): master-caution commands and feedback bindings.
3. [Displays and encoders](https://github.com/opensimgear/cockpit/issues/15): bounded text, formatting, ordered input events and traffic measurements.
4. [Profiles and hardware qualification](https://github.com/opensimgear/cockpit/issues/16): wider aircraft coverage, ownership, reconnects and hardware validation.

## Related repositories

- [Device gateway](https://github.com/opensimgear/device-gateway): STM32 USB/CAN gateway firmware, electronics and gateway simulation.
- [Device template](https://github.com/opensimgear/device-template): generic module firmware/electronics and JSON-configured device simulation.
- [Firmware common](https://github.com/opensimgear/firmware-common): shared acquisition, profiles, USB support and dependency pins.
- [Protocol](https://github.com/opensimgear/protocol): Cyphal DSDL, transport, CDC contracts and conformance checks.
- [Website](https://github.com/opensimgear/opensimgear.github.io): architecture, guides, equipment database and calculators.

## Licensing

No OpenSimGear license grant is implied for newly authored application or firmware code. Upstream components retain their own licenses.
