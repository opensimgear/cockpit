# Cockpit native gateway services

Cockpit is a Tauri 2 desktop application. `pnpm dev` launches the desktop; `pnpm build` packages it. Rust 1.90.0 is pinned in the repository. The installed app communicates directly with gateway transports and does not depend on firmware source, simulation executables or external development tools.

## Adapters and lifecycle

The main window invokes `gateway_request` with typed JSON request/state messages. Its shared `GatewayManager` keeps an independent adapter worker and connection for every connected gateway, including multiple gateways using the same backend. Each worker bounds requests, correlates replies and validates gateway identity before accepting state. One gateway's configuration, inputs, claims, errors and reconnect never replace another gateway's session.

Global `ports` reads metadata from all compatible endpoints without opening or disconnecting them. Every port has a stable `gatewayId`: `stm32:<lowercase UID>` or `pico:<USB serial>`. Requests for a gateway include that ID; responses repeat it in `state.gatewayId`. Connecting verifies that the requested identity matches both the selected port metadata and the protocol handshake. Legacy unscoped operations are accepted only when exactly one session exists; ambiguous operations fail before reaching any adapter. Module identities and setup ownership are scoped to the gateway session, so two gateways can report the same device ID independently.

The manager only holds its session map while finding a worker. CDC operations and periodic polling run independently, so a slow or unplugged gateway does not block the others. Application shutdown cancels and closes every adapter, releasing its setup access.

The STM32 adapter implements CDC protocol 3 automatic controller lifecycle plus compatible CDC-v2 manual gateways. It validates the gateway UID, protocol, role, capabilities, epoch, enabled/published state, descriptor order and explicit report IDs, with a bounded declared capacity up to 32 devices. New devices are enabled by firmware. Enable, Disable, offline Forget and explicit Update profile are sent once using the current epoch, then reconciled through read-only snapshots. Firmware performs persistence and USB reconnection. The adapter follows the same UID, VID and PID across changed serial paths and lost acknowledgements without replaying a mutation; a different board cannot take over. Pending layouts, admission failures and persistence/publication failures remain visible. Offline enabled devices remain neutral ghosts; disabled devices remain remembered and configurable while online. Closing the connection leaves gateway/device input delivery running independently.

CDC-v2 uses its existing registration/layout Apply workflow; Pico retains its older simulator lifecycle. Those adapters reject the new remembered Enable/Disable/Forget operations. Preserved descriptor order supports binding continuity, but host/controller/game identity behavior after dynamic USB changes still needs physical Windows and simulator qualification.

Runtime configuration routes bounded commands to the device using its exclusive writer, draft and Apply/Save/Discard operations. Claims bind to gateway epoch, device identity, boot and route. Readback confirms activation and reconciles lost commit replies; CDC v3 readback follows the same gateway across changed CDC paths and verifies the module boot. Read-only configuration reads wait for the renewed CAN route after an automatic publication; a configuration mutation is never replayed. Cached configuration binds to the device boot and configuration generation. Fresh request-counter seeds distinguish new client sessions from a running device's completed-request cache.

`action_describe`, `action_state` and `action_invoke` route the device's generic
action handler through `cockpit <id> <epoch> action <identity> <boot> <child>`.
The outer `targetAction` reply is independent of `targetConfig`. Discovery follows
bounded pages (up to four actions per page, sixteen total) and validates IDs,
parameter constraints, revision consistency and duplicate descriptors before
exposing controls. Boolean, signed integer, enum, RGB and bounded UTF-8 string
arguments are validated and encoded without allowing serial command injection.
The decorated `moduleActions` state contains the complete catalogue, applied
readback and explicit ready/unsupported/error/uncertain status.

Action cache entries bind to this gateway's epoch and module identity/node/lease/
boot; stale or offline routes invalidate them. Actions do not require a settings
writer or HID registration/input ownership. Describe/state may wait for a fresh
route after a layout change, while invokes are sent once. A timeout or failed
application acknowledgement requires readback and never replays the invocation.
Unsupported firmware remains connected and usable for its existing inputs.

The Pico adapter owns normal USB serial discovery, protocol checks, control lines, UI liveness, setup leases and reconnect to the same board. It reads live inputs and configures settings; it rejects input/inventory injection commands. Stop/disconnect and app exit release access and neutralize this older firmware's reports. Settings remain in board RAM. On macOS the native IOKit helper updates individual joystick service names after matching VID/PID, serial, usage and interface.

## Local gateway transport

Normal macOS/Linux discovery also reads bounded gateway advertisements from `/tmp/opensimgear-<user-id>/gateways/`. Directories and files must belong to the current user with private permissions; stale process advertisements and invalid paths are ignored. Valid metadata selects a loopback TCP endpoint or a private Unix stream. Opening revalidates discovery, then applies the same CDC identity and epoch checks as a USB serial connection.

The gateway owns its advertisement and lifetime. Device inventory is obtained exclusively through the gateway protocol. This transport is independent of how a gateway is implemented and contains no simulation factory or administrative input controls.

For isolated local simulation runs, set `OSG_SIMULATION_NAMESPACE` to the same
value on Cockpit/service, gateway and device processes. Names use 1–31 ASCII
letters, digits, dots, underscores or hyphens; discovery then uses
`/tmp/opensimgear-<user-id>/gateways-<namespace>`. Unset preserves ordinary
discovery. Invalid values disable local discovery/opening without falling back
to the ordinary directory. Production process acceptance creates a fresh private
namespace per scenario, so an already running desktop cannot attach to its CDC
endpoints.

## Browser development

The debug `osg-gateway-service` binary exposes the same `GatewayManager` as newline-delimited JSON over standard input/output. Vite's local `/api/gateway/` bridge uses it for browser development and integration. The bridge requires a same-origin loopback request, `X-OSG-Client: 1`, POST and bounded JSON bodies. It uses `{requestId, request}` envelopes with correlated `{requestId, response}` replies, allowing different gateways to respond independently. At most 32 service requests run concurrently. Raw request/response lines remain available for sequential CLI consumers. The desktop uses IPC and does not enable the HTTP bridge.

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
pnpm native:test
pnpm e2e
```

Frontend tests cover native IPC, browser transport, discovery, ownership, error reconciliation and configuration validation. Rust tests cover fleet routing, same-backend and cross-backend isolation, duplicate module IDs, identity mismatches, concurrent operations and shutdown, plus transport boundaries, protocol validation, setup ownership, reconnect, stale replies, layout recovery and local discovery. Browser bridge tests verify out-of-order response correlation and rejection of unmatched responses. C formatter [fixtures](tests/fixtures/README.md) verify production wire compatibility without requiring another checkout for unit tests. Repository-owned E2E bootstrapping builds the production gateway/device simulations in `.e2e/`; it requires no workspace parent setup. Local stream tests do not establish physical USB/HID enumeration or hardware CAN behavior.
