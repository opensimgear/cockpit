# OpenSimGear Cockpit

Work-in-progress desktop application for discovering OpenSimGear gateways, configuring devices, managing automatically enabled USB controllers, viewing live inputs and connecting panel controls, LEDs and displays to DCS-BIOS and game telemetry, starting with Assetto Corsa Rally. Built with Tauri 2, Rust, Svelte 5, TypeScript and Vite.

Cockpit connects to gateways through its normal USB serial and local stream transports. Gateway and device simulations are maintained independently in [device-gateway/simulation](https://github.com/opensimgear/device-gateway/tree/main/simulation) and [device-template/simulation](https://github.com/opensimgear/device-template/tree/main/simulation). Cockpit neither creates simulated devices nor manages their processes.

## Current behavior

Every compatible gateway connects automatically and stays connected independently, including multiple gateways of the same type. The Gateways view shows a card for each gateway and its devices; Devices combines their inventories, and Topology shows one computer branching to every gateway and its device chain. Devices appear as they join after Cockpit starts. Configuration, controller lifecycle changes, live inputs and reconnects are routed by gateway identity. A busy, failed or restarting gateway does not interrupt the others, and identical module identities on separate gateways keep separate state and drafts. Device names come from normal runtime configuration. Previously connected gateways retain their last-known devices while offline.

The current STM32 CAN gateway remembers up to 32 devices, each with up to 128 buttons, four hats and eight axes. New identities enable and append to the persisted USB controller order automatically. Enabled offline devices appear ghosted and retain neutral HID collections. Disable removes a collection while remembering the device; continuing announcements and gateway restarts do not re-enable it. Explicit Enable appends it after surviving controllers. Remove forgets an offline identity. Disable and removal of a published controller warn that remaining game bindings may need updating. Online disabled devices remain configurable. Direct device USB takes input ownership while its enabled gateway controller remains neutral.

A setup session edits module names, expansion banks, buttons, hats, encoders, selectors and calibrated axes. Apply activates a validated draft; Apply & save journals it on the device. Existing USB control profiles remain frozen across descriptor additions. A changed enabled profile stays neutral until an explicit Update profile, with a reconnect/binding warning. The firmware batches, persists and publishes controller changes itself; Cockpit observes and reconnects by gateway UID without replaying an ambiguous mutation. USB descriptor changes can still affect Windows/game identity or require a game restart; actual binding retention remains hardware qualification work.

The Pico USB adapter discovers both legacy protocol-1 images and the current protocol-3 debug gateway. The debug firmware hosts eight virtual devices with production configuration/calibration, automatic controller lifecycle, ordered events, LED/display actions and persistent journals. Cockpit's gateway card adds presets and manages virtual online/reboot state; device Inputs/Setup views provide raw axes, held buttons, switches, encoders, selectors, hats, reset, direct-USB and expansion-bank controls. The DCS-BIOS editor offers catalogue-validated A-10C master-caution/radio test presets and output readback. Older images retain RAM-only settings and their existing USB reconnection/name helper. See the [Pico guide](https://github.com/opensimgear/device-gateway/tree/main/simulation/pico) for build, flash, host fixture and cross-process DCS acceptance. Upgrading the older multi-interface fixture changes USB descriptors and may require game rebinding.

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
pnpm dcs:coverage
pnpm dcs-profile:coverage
pnpm integration:coverage
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

## DCS-BIOS panel profiles

Milestone 3 extends the saved A-10C master-caution path into one configurable
profile with up to 16 input bindings and 16 output bindings, covering eight known
modules on four gateways. Each binding retains its gateway and module identity;
the same module identity on another gateway is a separate target. The original
master-caution preset remains available. An unchanged saved Milestone 2 profile
can start as before; **Save settings** explicitly upgrades its two displayed
bindings to the new profile format. Loading the view does not rewrite settings.

Select named controls from the installed DCS-BIOS `doc/json` catalogue. Cockpit
queries supported command interfaces and feedback types from those files and
resolves their binary addresses in the native service. The UI does not accept
arbitrary command text or hard-coded feedback addresses. UDP defaults remain
command `127.0.0.1:7778`, export listen `0.0.0.0:5010`, multicast `239.255.50.10`
on interface `127.0.0.1`, and feedback freshness 1500 ms. An empty multicast group
selects unicast reception.

1. Open **DCS-BIOS** from the sidebar, a gateway card or a device workspace. Enter
   the installed catalogue directory and use **Search catalogue** to find named
   controls. The shared editor retains an unsaved draft when changing views.
2. Choose a device for each binding and refresh its capabilities. Inputs offer
   advertised momentary buttons, maintained switches, encoder ordinals and
   calibrated axes. Selector positions are not offered as switch bindings.
3. For input devices, use **Disable controller**, accept the existing USB
   reconnect/binding warning and wait for publication to finish. Disconnect active
   direct USB input and finish setup sessions before Start. Output-only targets
   can keep their HID controller enabled.
4. Choose a supported command interface and values. Buttons and switches send
   `set_state`; encoders send ordered `INC`/`DEC` or signed variable steps. Axes
   use independently calibrated 0–65535 samples, scale to the catalogue range and
   apply the chosen change threshold in target units.
5. Add LED or display feedback. Choose a named catalogue output and compatible
   advertised device action. Text requires string feedback; integer and fixed
   decimal formats convert numeric feedback into a string display value.
6. **Save settings**, then **Start DCS-BIOS**. Saving persists one profile in
   Cockpit's application settings and never starts it. Restarting Cockpit also
   requires explicit Start. Stop the running profile before editing.

Readable boolean setters and the current board's `led.status.set_mode` support
on/off feedback. Displays use readable bounded string setters. Their advertised
columns, rows, ASCII or UTF-8 encoding and byte limit determine text fit. Numeric
formatting supports scale, decimal places, prefix and suffix; optional alignment
pads the advertised display cells. Overflow either truncates whole characters or
reports an error. Unknown and stale text are explicit settings, blank by default;
unknown or stale LED feedback requests Off. The UI shows native desired and last
applied values rather than duplicating the formatter.

The Rust background service processes complete DCS export frames atomically and
coalesces pending absolute output writes to the newest value. Ordered button
edges and encoder detents remain ordered commands. UI polling only displays
status. Device events and calibrated axis samples remain available with HID
disabled. Startup, route/boot/mapping changes, export recovery and event gaps
establish a current baseline instead of replaying historical commands. Held
momentary buttons must release before a fresh press; maintained switches and
available axes synchronize their current absolute state at a fresh baseline; disarming sends one neutral release
only for a press this profile previously forwarded. Uncertain command sends are
never replayed. Readable output actions reconcile uncertain replies through
readback. See the [ordered panel input contract](https://github.com/opensimgear/protocol/blob/main/serial/panel-input-events.md).

The shared editor displays aircraft, feedback age, per-binding readiness/errors,
command counts, desired/applied outputs and per-gateway queue, write, coalescing,
latency and polling metrics. Retained event overwrites count device ring evictions;
recovered event gaps count cursor discontinuities, not lost simulator commands.
The running profile manages only its selected output actions; other manual actions
remain available. Global **Stop DCS-BIOS** remains available when a target is
offline; an unreachable device cannot confirm its fallback until it reconnects.

For development, [simulation/dcs-bios/peer.mjs](simulation/dcs-bios/peer.mjs) uses
an authored [synthetic catalogue](simulation/dcs-bios/catalogue/) with the native
panel simulations. These sample addresses are for that peer, not a real aircraft.
Gateway and device processes come from their owning repositories and use normal
production contracts. See the [simulation guide](simulation/dcs-bios/README.md),
[master-caution verification record](docs/verification/2026-10-13-dcs-bios-master-caution.md)
and [integration profile notes](integration-profiles/README.md). Real DCS and
assembled STM32 inputs, displays, LED, USB and CAN qualification remain deferred.
Simulated display endpoints do not establish physical display hardware support.

`pnpm dcs:coverage` and `pnpm dcs-profile:coverage` enforce frontend helper coverage
above 70% for lines, branches and functions. CI also runs
`scripts/dcs-bios-coverage.py` for native DCS processing. Browser development uses
bounded 64 KiB requests and 128 KiB replies for profiles, catalogue pages and
status; device CDC/CAN child-command limits remain separate.

The combined device-action and DCS-BIOS roadmap is tracked by these milestone stories:

1. [Manual device actions](https://github.com/opensimgear/cockpit/issues/13): implemented advertised actions, gateway routing and Cockpit controls.
2. [First DCS-BIOS button and LED path](https://github.com/opensimgear/cockpit/issues/14): implemented simulated master-caution commands, ordered button events and LED feedback.
3. [Displays and encoders](https://github.com/opensimgear/cockpit/issues/15): implemented configurable typed inputs, bounded display formatting and gateway traffic measurements; see the [simulation verification](docs/verification/2026-10-14-dcs-bios-panels.md).
4. [Simulator profiles, output ownership and recovery](https://github.com/opensimgear/cockpit/issues/16): portable roles, broader installed DCS catalogues, endpoint leases, output-only devices and Rally telemetry; live game/hardware qualification deferred.

## Simulator integrations

Open **Integrations** for portable profile import/export, explicit local device assignments, catalogue migration diagnostics and device-enforced output ownership. DCS-BIOS uses installed aircraft modules and named supported controls. The Assetto Corsa Rally starter reads Windows shared memory and offers a synthetic byte-snapshot source on every platform. Bind RPM/rev-ratio thresholds or RGB bands to advertised LEDs, including the current board status LED, and speed/gear values to bounded displays. Temporary overrides retain ownership and resume the latest fresh telemetry. Output-only modules enroll without HID input profiles.

See [the integration guide](docs/simulator-integrations.md) for setup, lease/fallback behavior, source provenance and validation limits, and [the software verification record](docs/verification/2026-10-14-simulator-integrations.md) for tests and measured coverage. Hardware and live-game validation are deferred.

## Related repositories

- [Device gateway](https://github.com/opensimgear/device-gateway): STM32 USB/CAN gateway firmware, electronics and gateway simulation.
- [Device template](https://github.com/opensimgear/device-template): generic module firmware/electronics and JSON-configured device simulation.
- [Firmware common](https://github.com/opensimgear/firmware-common): shared acquisition, profiles, USB support and dependency pins.
- [Protocol](https://github.com/opensimgear/protocol): Cyphal DSDL, transport, CDC contracts and conformance checks.
- [Website](https://github.com/opensimgear/opensimgear.github.io): architecture, guides, equipment database and calculators.

## Licensing

No OpenSimGear license grant is implied for newly authored application or firmware code. Upstream components retain their own licenses.
