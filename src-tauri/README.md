# Cockpit native gateway services

## Development status and disclaimer

This project is heavily under development and is not ready for use. Use it entirely at your own risk. To the fullest extent permitted by applicable law, the authors and contributors disclaim all liability for any damage, loss, or other issues arising from its use.

Cockpit is a Tauri 2 desktop application. `pnpm dev` launches the desktop; `pnpm build` packages it. Rust 1.90.0 is pinned in the repository. The installed app communicates directly with gateway transports and does not depend on firmware source, simulation executables or external development tools.

The shared `gateway_request` transport serves fresh schema-2 `profile_*`
authoring and one native ProfileSession coordinator. Games owns installations,
provider setup and verified process observations; it does not save separate
hardware profiles. Manual/Automatic selection captures a saved unified graph.
Launch prepares hardware neutral, spawns once and waits for the exact independent
native watcher identity before admitting game-dependent execution. Status and
emergency Stop remain independent of slow preparation. See the
[Profiles contracts](../docs/unified-profiles.md) and [Games guide](../docs/games.md).

## Adapters and lifecycle

The main window invokes `gateway_request` with typed JSON request/state messages. Its shared `GatewayManager` keeps an independent adapter worker and connection for every connected gateway, including multiple gateways using the same backend. Each worker bounds requests, correlates replies and validates gateway identity before accepting state. One gateway's configuration, inputs, claims, errors and reconnect never replace another gateway's session.

Global `ports` reads metadata from all compatible endpoints without opening or disconnecting them. Every port has a stable `gatewayId`: `stm32:<lowercase UID>` or `pico:<USB serial>`. Requests for a gateway include that ID; responses repeat it in `state.gatewayId`. Connecting verifies that the requested identity matches both the selected port metadata and the protocol handshake. Legacy unscoped operations are accepted only when exactly one session exists; ambiguous operations fail before reaching any adapter. Module identities and setup ownership are scoped to the gateway session, so two gateways can report the same device ID independently.

The manager only holds its session map while finding a worker. CDC operations and periodic polling run independently, so a slow or unplugged gateway does not block the others. Application shutdown cancels and closes every adapter, releasing its setup access.

The STM32 adapter implements CDC protocol 3 automatic controller lifecycle plus compatible CDC-v2 manual gateways. It validates the gateway UID, protocol, role, capabilities, epoch, enabled/published state, descriptor order and explicit report IDs, with a bounded declared capacity up to 32 devices. New devices are enabled by firmware. Enable, Disable, offline Forget and explicit Update profile are sent once using the current epoch, then reconciled through read-only snapshots. Firmware performs persistence and USB reconnection. The adapter follows the same UID, VID and PID across changed serial paths and lost acknowledgements without replaying a mutation; a different board cannot take over. Pending layouts, admission failures and persistence/publication failures remain visible. Offline enabled devices remain neutral ghosts; disabled devices remain remembered and configurable while online. Closing the connection leaves gateway/device input delivery running independently.

CDC-v2 and protocol-1 Pico images retain their older registration/layout workflows and reject remembered Enable/Disable/Forget operations. Protocol-3 Pico debug firmware uses the automatic lifecycle through the shared adapter. Preserved descriptor order supports binding continuity, but host/controller/game identity after dynamic USB changes still needs physical Windows and simulator qualification.

Runtime configuration routes bounded commands to the device using its exclusive writer, draft and Apply/Save/Discard operations. Claims bind to gateway epoch, device identity, boot and route. Readback confirms activation and reconciles lost commit replies; CDC v3 readback follows the same gateway across changed CDC paths and verifies the module boot. Read-only configuration reads wait for the renewed CAN route after an automatic publication; a configuration mutation is never replayed. Cached configuration binds to the device boot and configuration generation. Fresh request-counter seeds distinguish new client sessions from a running device's completed-request cache.

`action_describe`, `action_state` and `action_invoke` route the device's generic
action handler through `cockpit <id> <epoch> action <identity> <boot> <child>`.
The outer `targetAction` reply is independent of `targetConfig`. Discovery follows
bounded pages (up to four actions per page, sixteen total) and validates IDs,
parameter constraints, revision consistency and duplicate descriptors before
exposing controls. Boolean, signed integer, enum, RGB and bounded UTF-8 string
arguments are validated and encoded without allowing serial command injection.
The decorated `moduleActions` state contains the complete catalogue, applied
readback, the epoch/node/lease/boot route token and explicit ready/unsupported/error/uncertain status.
Display string setters additionally validate declared columns/rows, encoding,
logical scalar-cell capacity and byte budget before invocation or readback.

Action cache entries bind to this gateway's epoch and module identity/node/lease/
boot; stale or offline routes invalidate them. Actions do not require a settings
writer or HID registration/input ownership. Describe/state may wait for a fresh
route after a layout change, while invokes are sent once. A timeout or failed
application acknowledgement requires readback and never replays the invocation.
Unsupported firmware remains connected and usable for its existing inputs.

## Ordered events and DCS-BIOS

`events_state` and `events_read` use the production module event handler through
`cockpit <id> <epoch> events <identity> <boot> <child>` and its separate
`targetEvents` reply. `moduleEvents` validates schema, mapped button bounds,
boot/stream, consecutive sequences, cursor and route before exposure. The device
retains 64 debounced transitions and returns nonconsuming pages of up to 16.
Input acquisition continues with HID disabled. Startup, reconnect, stream
changes and gaps establish a fresh head/held baseline; historical events are not
sent to the simulator. The canonical
[event contract](https://github.com/opensimgear/protocol/blob/main/serial/panel-input-events.md)
also specifies ring overflow and encoder pairs.

The DCS decoder, named-control catalogue and ordered panel events feed the
unified profile compiler and native adapters. Provider settings belong to DCS
game details. Actual native process/context, applied owner, resource capabilities
and freshness authorize each branch; browser process labels do not. Commands are
never replayed after an uncertain send. Physical outputs use endpoint leases,
atomic capture-at-claim and compare-if-unchanged restoration.

Independent `dcs_*`, `integration_*`, `mapping_*`, per-game profile/capture and
standalone visibility Apply/launch commands are rejected before selection,
learning or output effects. Their old runtime modules are not constructed or
registered. Retained journal codecs exist only for explicit ownership cleanup;
cleanup cannot configure/start an old runtime, resume mappings/scripts or reapply
hiding. `config_paths.rs` preserves the shared path anchor without reading an old
configuration file. `OSG_DCS_BIOS_CONFIG` overrides this anchor in isolated tests.

CI keeps per-module production coverage for the retained DCS decoder, catalogue
and events above 70%, and integration/profile modules at least 71%. The native
coverage runners pin production/test source hashes and reject concurrent edits.
Historical [Milestone 3 verification](../docs/verification/2026-10-14-dcs-bios-panels.md)
describes the retired standalone workflow; current graph authoring is documented
in the [Profiles guide](../docs/profiles-guide.md).

The Pico adapter probes the selected endpoint only on Connect and chooses the legacy JSON protocol-1 service or the shared production CDC3 service for the new Pico debug role. The latter validates serial/UID, epoch and bounded debug metadata before providing configuration, lifecycle, actions, events and guarded input injection. Debug mutations validate arguments and current device boot and are sent once. Unified profile assignments accept verified Pico routes. Debug definitions hydrate truthful device labels/kinds. Legacy images retain RAM-only settings, setup leases, liveness/neutral recovery and macOS IOKit interface names. New board firmware persists gateway/device journals and retains the existing board serial.

## Local gateway transport

Normal macOS/Linux discovery also reads bounded gateway advertisements from `/tmp/opensimgear-<user-id>/gateways/`. Directories and files must belong to the current user with private permissions; stale process advertisements and invalid paths are ignored. Valid metadata selects a loopback TCP endpoint or a private Unix stream. Opening revalidates discovery, then applies the same CDC identity and epoch checks as a USB serial connection.

The gateway owns its advertisement and lifetime. Device inventory is obtained exclusively through the gateway protocol. Strictly validated protocol-3 Pico debug advertisements can also use this transport; input controls are provided only by the connected firmware's debug capability. The transport starts no simulation processes. For isolated debug-build tests, `OSG_LOCAL_GATEWAYS_ONLY=1` excludes physical serial ports; release builds ignore that flag. Set `OSG_DCS_BIOS_CONFIG` to a scratch anchor when testing profile storage.

For isolated local simulation runs, set `OSG_SIMULATION_NAMESPACE` to the same
value on Cockpit/service, gateway and device processes. Names use 1–31 ASCII
letters, digits, dots, underscores or hyphens; discovery then uses
`/tmp/opensimgear-<user-id>/gateways-<namespace>`. Unset preserves ordinary
discovery. Invalid values disable local discovery/opening without falling back
to the ordinary directory. Production process acceptance creates a fresh private
namespace per scenario, so an already running desktop cannot attach to its CDC
endpoints.

## Browser development

The debug `osg-gateway-service` binary exposes the same `GatewayManager` as newline-delimited JSON over standard input/output. Vite's local `/api/gateway/` bridge uses it for browser development and integration. The bridge requires a same-origin loopback request, `X-OSG-Client: 1`, POST and bounded JSON bodies. It uses `{requestId, request}` envelopes with correlated `{requestId, response}` replies, allowing different gateways to respond independently. The service reserves 1 KiB for the envelope around the manager's 64 KiB request budget; oversized payloads within that framing limit return a correlated rejection. At most 32 service requests run concurrently. Raw request/response lines remain available for sequential CLI consumers. The desktop uses IPC and does not enable the HTTP bridge.

```sh
CARGO_TARGET_DIR="$PWD/.e2e/build/cockpit" cargo build \
  --manifest-path src-tauri/Cargo.toml --bin osg-gateway-service
pnpm web:dev
```

The bridge defaults to `.e2e/build/cockpit/debug/osg-gateway-service` within this repository; `OSG_GATEWAY_SERVICE` accepts an absolute executable override. Gateway and device processes are managed by their owners, outside this service.

## Permissions and validation

Only the main local window receives gateway command permissions. External links use the opener with explicitly permitted documentation URLs. Packaged content uses a restrictive content security policy. Firmware building and flashing instructions belong to the firmware repositories.

```sh
pnpm check
pnpm transport:test
pnpm dcs:coverage
pnpm native:test
pnpm e2e
```

Frontend tests cover native IPC, browser transport, discovery, ownership, error reconciliation and configuration validation. Rust tests cover fleet routing, same-backend and cross-backend isolation, duplicate module IDs, identity mismatches, concurrent operations and shutdown, plus transport boundaries, protocol validation, setup ownership, reconnect, stale replies, layout recovery and local discovery. The native service process test checks full-size correlated requests, larger authored telemetry pages and recovery after rejecting an oversized payload. Browser bridge tests verify out-of-order response correlation and rejection of unmatched responses. C formatter [fixtures](tests/fixtures/README.md) verify production wire compatibility without requiring another checkout for unit tests. Repository-owned E2E bootstrapping builds the production gateway/device simulations in `.e2e/`; it requires no workspace parent setup. Local stream tests do not establish physical USB/HID enumeration or hardware CAN behavior.

## Provider-neutral integration execution

`profile_session_games`, the native context broker and resource adapters execute
only branches owned by the captured applied profile. `game_telemetry` decodes
bounded ACR pages; `acr_windows` observes native shared memory. X-Plane uses its
typed read-only provider. Synthetic snapshots exercise parsers without granting
native readiness. `output_leases` validates exact device endpoint claims; secret
capabilities never appear in passive status. Configuration changes revoke stale
contexts; ordinary hardware routes can remain useful with game branches dormant.

Device routing retains `action_claim`, `action_renew`, `action_release`,
`action_ownership` and `action_invoke_owned` for actual guarded device operations.
Profile ownership guards independent writes to participating resources. Hardware,
driver and live-game qualification remain separate from software fixtures.

## Windows download releases

[Windows release CI](../.github/workflows/cockpit-windows-release.yml) builds and tests
Cockpit on native Windows x64, then publishes a development prerelease with an NSIS
installer, portable ZIP and SHA-256 checksums. A push to `main` automatically builds
and publishes `cockpit-windows-build-<run-number>` when desktop source, browser
companion, shared UI/native dependencies, bundled resources, dependency/toolchain
configuration, examples/test fixtures, or Windows packaging files change. Mobile-only
app/platform changes, ESP32-only firmware changes (apart from the embedded firmware
version header), and unrelated documentation do not trigger a Windows release.

Manual dispatch from `main` also publishes a build. Push `cockpit-v<version>` matching
the app's version to publish that tag; explicit tag pushes bypass path filters.
Manual runs on other branches upload CI artifacts only. The workflow does not
replace the latest stable release.

The portable package uses the existing dependency hash/signature checks in
`package-windows-mapping.ps1` and retains development qualification metadata. No
Cockpit signing certificate is configured, and driver installation remains explicit.
See [the release notes](../docs/windows-release-notes.md) for package contents and
current limitations. This workflow does not run for mobile-only changes.
