# OpenSimGear Cockpit

## Development status and disclaimer

This project is heavily under development and is not ready for use. Use it entirely at your own risk. To the fullest extent permitted by applicable law, the authors and contributors disclaim all liability for any damage, loss, or other issues arising from its use.

X-Plane 11 and 12 support is available as an unqualified RREF telemetry provider.
See [setup, channels and qualification limits](docs/x-plane.md) before use.

Work-in-progress desktop application for discovering OpenSimGear gateways and supported games, configuring devices and composing complete rigs in **Profiles**. A saved graph connects physical inputs, virtual controllers, companions, game commands, telemetry and device feedback. Built with Tauri 2, Rust, Svelte 5, TypeScript and Vite.

Cockpit connects to gateways through its normal USB serial and local stream transports. Gateway and device simulations are maintained independently in [device-gateway/simulation](https://github.com/opensimgear/device-gateway/tree/main/simulation) and [device-template/simulation](https://github.com/opensimgear/device-template/tree/main/simulation). Cockpit neither creates simulated devices nor manages their processes.

## Current behavior

**Profiles** opens to a list with Default and **Add profile**. Click a profile to
open its editor. Saving a graph and choosing an editor do not change the applied
run. Explicit Apply captures a saved revision; Stop remains visible. The graph
owns temporary routing, companion publications, visibility intent and eligible
game branches. Read the [Profiles guide](docs/profiles-guide.md) for the complete
workflow and [contracts](docs/unified-profiles.md) for storage and admission.

**Games** discovers DCS World, Assetto Corsa Rally, X-Plane 11 and X-Plane 12.
Found installations appear as tiles; **Show all supported** reveals unavailable
games. Details own installation paths, provider settings, setup and automatic
profile selection rules. Launch uses the saved unified profile selection,
prepares hardware neutral and waits for the exact native process before admitting
game branches. Setup readiness and live telemetry are separate facts. DCS setup
installs checksum-pinned DCS-BIOS while preserving existing exports; ACR setup
checks the exact runtime build. See the [Games guide](docs/games.md) for setup,
launch, selection and explicit recovery.

**Devices** shows known third-party controllers, OSG USB/CAN modules and gateways, virtual devices, paired companions and integrations with active applied bindings in shared **List | Topology** modes. Integration visibility follows bindings rather than telemetry reception. Assigned panel names appear on their companion; separate panels and unpaired setup/pairing records stay in Companions. Hidden, offline, unavailable and unknown hardware remain discoverable. Both modes share search, state/type/transport filters, selection, details and actions. Topology labels physical and software connections, retained attachments and unconfirmed configured endpoints. Profile authoring and routing visualization are deferred from this inventory.

There are no separate Gateways or Topology navigation sections. Select a gateway in Devices to connect/disconnect it, inspect its modules or open existing configuration and controller-management controls. A deliberate Disconnect pauses that gateway's automatic reconnection until Connect is chosen. Other gateways continue independently. Verified USB/CAN sightings of one device share one physical identity; ambiguous sightings and identical module IDs on separate gateways retain their own routes and drafts. See the [Devices guide and inventory contract](docs/system-inventory.md) and [verification evidence](docs/verification/2026-10-18-unified-devices.md).

The current STM32 CAN gateway remembers up to 32 devices, each with up to 128 buttons, four hats and eight axes. New identities enable and append to the persisted USB controller order automatically. Enabled offline devices appear ghosted and retain neutral HID collections. Disable removes a collection while remembering the device; continuing announcements and gateway restarts do not re-enable it. Explicit Enable appends it after surviving controllers. Remove forgets an offline identity. Disable and removal of a published controller warn that remaining game bindings may need updating. Online disabled devices remain configurable. Direct device USB takes input ownership while its enabled gateway controller remains neutral.

A setup session edits module names, expansion banks, buttons, hats, encoders, selectors and calibrated axes. Apply activates a validated draft; Apply & save journals it on the device. Existing USB control profiles remain frozen across descriptor additions. A changed enabled profile stays neutral until an explicit Update profile, with a reconnect/binding warning. The firmware batches, persists and publishes controller changes itself; Cockpit observes and reconnects by gateway UID without replaying an ambiguous mutation. USB descriptor changes can still affect Windows/game identity or require a game restart; actual binding retention remains hardware qualification work.

The Pico USB adapter discovers both legacy protocol-1 images and the current protocol-3 debug gateway. The debug firmware hosts eight virtual devices with production configuration/calibration, automatic controller lifecycle, ordered events, LED/display actions and persistent journals. Cockpit's gateway card adds presets and manages virtual online/reboot state; device Inputs/Setup views provide raw axes, held buttons, switches, encoders, selectors, hats, reset, direct-USB and expansion-bank controls. Unified Profiles uses named DCS-BIOS catalogue ports and advertised device actions for panel input and feedback. Older images retain RAM-only settings and their existing USB reconnection/name helper. See the [Pico guide](https://github.com/opensimgear/device-gateway/tree/main/simulation/pico) for build, flash, host fixture and cross-process DCS acceptance. Upgrading the older multi-interface fixture changes USB descriptors and may require game rebinding.

STM32 discovery verifies manufacturer/product metadata, a 24-hex UID and bounded CDC protocol-3 automatic-lifecycle snapshots. Explicit report IDs and descriptor positions are validated independently. Capacity and admission failures remain visible. Configuration uses the production Cyphal/CAN contract (`canProtocol=3`) with identity, boot, writer, request and epoch validation. After Apply, readback follows the device's renewed lease with bounded read retries; a device reboot stops reconciliation, and writes are never replayed. CDC-v2 gateways remain readable with their older manual registration/layout Apply controls; those adapters do not claim automatic Enable/Disable/Remove support. Local gateways receive the same handshake checks as USB serial gateways; on macOS/Linux they advertise in the current user's private `/tmp/opensimgear-<user-id>/gateways/` directory. Local streams do not create OS USB/HID controllers.

## AI-assisted configuration

Open **Settings → AI integration** in the desktop app to enable its local MCP
server and create a Read only or Configure credential for an assistant. The
assistant shares the running Cockpit's resources and ownership rules: prepare
and validate changes, save a draft, then explicitly apply the saved revision.
See the [MCP setup and workflow guide](docs/mcp.md) for client configuration,
supported tools, conflict handling, and verification limits.

## Keyboard and mouse mappings (development acceptance)

Unified Profiles can author keyboard and mouse actions with recording previews.
Windows live output has independent local enablement and remains behind the
desktop acceptance gate; vJoy and HidHide are not required for desktop-only
profiles. See the [keyboard and mouse guide](docs/keyboard-mouse.md) and
[behavior and qualification contract](docs/desktop-output-contract.md).

## Host controller discovery (experimental)

On Windows, Devices List and Topology modes also show joystick, gamepad and multi-axis HID collections, including published OpenSimGear HID inputs. Background Raw Input capture provides paged live buttons, axes, hats and capability diagnostics independently of gateway setup sessions. Names and roles are local metadata. Serial/collection evidence can follow a controller across ports; ambiguous devices require explicit reassignment. Unplugging clears values, while stationary controls retain their last valid report. vJoy is excluded from physical input discovery.

This discovery slice does not activate virtual outputs or hiding. Linux/macOS capture adapters are implemented; native device qualification remains open. Device/game qualification gaps remain open; see the [controller guide](docs/host-controllers.md) and [M1 verification record](docs/verification/2026-10-14-controller-discovery-m1.md).

## Controller workflow

The Windows x64 desktop app includes optional **vJoy 2.2.2.0** and
**HidHide 1.5.230.0** installers. Open **Devices → Host controllers → Optional
Windows dependencies**, then choose **Set up vJoy** or **Set up HidHide**.
Setup is also available beside profile output and local visibility controls.
Each action opens the upstream setup wizard with Windows administrator approval;
complete any requested restart, then inspect the dependency. Neither is required
for ordinary controller input. See the [platform setup guide](docs/controller-mapping-platforms.md).

The automatic controller workflow is recorded in the [USB HID architecture](https://github.com/opensimgear/opensimgear.github.io/blob/main/src/content/docs/gear/architecture/usb-hid.md) and [Cockpit #5](https://github.com/opensimgear/cockpit/issues/5). Enabled devices have dynamic collections. Offline ghosts retain them temporarily; Disable or Remove deletes them. A replacement identity appends independently and keeps its own module settings/calibration. This implementation does not provide permanent empty collections or transfer another device’s calibration.

## Run and build

The repository is an Nx workspace. Run `pnpm test:affected` for affected checks,
tests with coverage, and builds; `pnpm test:all` runs all portable targets.
Use `pnpm nx graph` to inspect the projects and their dependencies.
Native and full integration checks run in the daily workflow when commits have
changed their inputs since its last successful run. See
[test selection and CI](docs/nx-testing.md) for local commands, cache setup, and
the daily baseline behavior.

Install the [Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/), Node.js 20.19+ or 22.12+, and the pnpm version declared in `package.json`. Rust 1.90.0 is pinned in `rust-toolchain.toml`.

From this repository:

```sh
pnpm install
pnpm dev
```

The desktop uses Rust IPC and its own frontend assets. Development serves the UI on `127.0.0.1:1420`. The installed application needs no Node runtime, Swift compiler, firmware checkout or simulation executable.

For an explicit Windows native-profile acceptance run, stop the existing dev app,
then rebuild that same app from PowerShell with
`$env:OSG_DEV_NATIVE_PROFILES='1'; pnpm dev`. Its readiness labels say
**Development acceptance mode**. This build-time opt-in enables the existing
vJoy output and HidHide adapters only in Windows debug builds; explicit settings,
profile Apply, ownership, freshness and recovery checks still apply. Scripts,
layout provisioning and other platforms remain gated. Remove the environment
variable and rebuild to return to the default disabled state. Release builds
remain disabled even when the variable is set; this mode does not graduate
release qualification.

```sh
pnpm check
pnpm transport:test
pnpm dcs:coverage
pnpm dcs-profile:coverage
pnpm integration:coverage
pnpm games:coverage
pnpm native:test
pnpm build
```

`pnpm build` packages the application under `src-tauri/target/release/bundle/`. Local macOS desktop builds can use ad-hoc signing; that does not authorize the separately gated virtual-HID helper.

GitHub Actions runs frontend type checks, transport tests and the UI build on every push and pull request. Native Windows, Linux and macOS jobs compile/test the desktop and mapping libraries, gate per-module coverage, and run native service startup and unified profile lifecycle checks without optional output dependencies. The Linux job also checks dependency bootstrap and independent gateway/device acceptance with sanitizers. Coverage, startup and integration evidence is uploaded as CI artifacts; physical hardware/game qualification remains separate.

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

Unified Profiles compiles visible typed processing and explicit assignments for
the captured run. [Custom Rhai logic](docs/mapping-scripts.md) retains bounded
execution and its separate live-output qualification gate. The portable mapping
engine remains available for deterministic library/CLI replay; its old standalone
desktop editor and activation commands are retired. Existing output, script and
visibility qualification gates remain enforced.

## Repository layout

The [unified profile contracts](docs/unified-profiles.md) document schema 2,
fresh storage, local assignment and applied-session boundaries. Old game,
mapping, integration and companion-feedback configuration is not migrated.
Retained ownership records are read only for explicit cleanup.

- `src/`: Svelte/TypeScript interface and frontend tests.
- [src-tauri/](src-tauri/README.md): normal Rust gateway adapters, IPC permissions and protocol tests.
- `scripts/native-gateway-bridge.mjs`: ordinary local browser development transport.
- `tests/e2e/`: repository-owned dependency bootstrap and integration scenarios.
- `docs/verification/`: historical software and hardware acceptance records. Older reports describe the implementation at the time of their run.

Implementation and qualification work is tracked in the [GitHub project](https://github.com/orgs/opensimgear/projects/2).

## Manual device actions

Each online compatible device can advertise a bounded action catalogue independently of
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
on separate gateways and lost-reply recovery. The master-caution binding below
uses this same output interface.
The [device actions acceptance record](docs/verification/2026-10-13-device-actions.md)
documents the production process checks and measured coverage; `pnpm
actions:coverage` enforces the frontend action helper's coverage threshold. CI
also runs `scripts/action-coverage.py` to gate Rust production action coverage
above 70%.

## Simulator commands and feedback

Set up each provider in **Games**, then add its logical game resource in
**Profiles**. The editor exposes named supported DCS-BIOS catalogue ports and
typed ACR/X-Plane telemetry. Wire physical/companion inputs to commands and fresh
telemetry through visible conversion, condition and formatting nodes to
advertised device or companion outputs. Local assignments select exact native
resources; saved graph content contains no live PID, pairing secret or OS path.

Native execution binds each game branch to the immutable applied owner and a
matching verified process context. Process exit, replacement, provider edits and
stale input revoke affected contributions. Endpoint leases, current capability
tokens, fresh baselines and exact captured restoration protect hardware state.
A lost write reply is not replayed. Game discovery and synthetic fixtures do not
establish live simulator or physical device qualification.

The old DCS-BIOS, Integrations, Mappings, per-game hardware-profile and companion
feedback editors no longer start independent runs. Existing recovery journals
remain available for explicit cleanup; they cannot resume those retired graphs.
Historical [DCS panel](docs/verification/2026-10-14-dcs-bios-panels.md),
[integration](docs/verification/2026-10-14-simulator-integrations.md) and
[mapping qualification](docs/verification/2026-10-15-mapping-qualification-m7.md)
records describe their dated checks. Current user flows are in the
[Profiles guide](docs/profiles-guide.md) and [Games guide](docs/games.md).

## Related repositories

- [Device gateway](https://github.com/opensimgear/device-gateway): STM32 USB/CAN gateway firmware, electronics and gateway simulation.
- [Device template](https://github.com/opensimgear/device-template): generic module firmware/electronics and JSON-configured device simulation.
- [Firmware common](https://github.com/opensimgear/firmware-common): shared acquisition, profiles, USB support and dependency pins.
- [Protocol](https://github.com/opensimgear/protocol): Cyphal DSDL, transport, CDC contracts and conformance checks.
- [Website](https://github.com/opensimgear/opensimgear.github.io): architecture, guides, equipment database and calculators.

## Licensing

Copyright © Ivan Gregurić Ortolan. All rights reserved for original OpenSimGear material while licensing is under review. The [temporary licensing policy](LICENSE) defines the current scope. Creative external contributions are suspended during review; [CONTRIBUTING.md](CONTRIBUTING.md) explains the policy for factual bug reports and questions. Upstream components retain their own licenses and notices.

Cockpit saves settings and durable content in one SQLite database per configuration
namespace. Development starts from a fresh database and seeds inert examples once;
release builds do not seed examples. There is no old-data migration. Settings →
General → Backups provides online snapshots and a stopped restore/restart workflow.
See [SQLite operations](docs/sqlite-persistence.md) for paths, overrides, reset,
backup assets and credential invalidation after restore.
