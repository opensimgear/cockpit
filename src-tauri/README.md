# Cockpit native gateway services

Cockpit is a Tauri 2 desktop application. `pnpm dev` launches the desktop; `pnpm build` packages it. Rust 1.90.0 is pinned in the repository. The installed app communicates directly with gateway transports and does not depend on firmware source, simulation executables or external development tools.

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

The global `dcs_status`, `dcs_configure`, `dcs_start` and `dcs_stop` operations
manage one saved profile through `gateway_request`. Configure takes `dcsConfig`;
responses expose `state.dcsBios` independently of gateway inventory. This global
state must be merged without replacing a selected gateway's state. Schema 1 Start
requires a connected, online CAN module with settled layout, HID disabled and
unpublished, no report ID and no active direct USB input. It validates a mapped
non-encoder button and a readable boolean setter or board status LED mode setter.
The manager protects the bound LED from manual invocations while running.

The legacy Rust worker receives DCS-BIOS UDP export, decodes bounded synchronized
little-endian writes, verifies advancing update counters and resolves
`UFC_MASTER_CAUTION`/`MASTER_CAUTION` from the installed `doc/json` catalogue.
Aircraft name and fresh feedback gate command delivery. It drains ordered panel
events independently of frontend status polling. A baseline held button must
release before arming. Disarm/Stop sends one release for a previously forwarded
press, without replaying presses. Retention overwrites and actual cursor gaps
have separate diagnostic counters. Missing/stale export, unsupported aircraft
and explicit Stop request LED off; output failure is visible and recovery reads
applied state before setting it again. Ambiguous simulator sends do not replay.

Configuration is stored as `dcs-bios.json` under the platform's Cockpit settings
directory. `OSG_DCS_BIOS_CONFIG` overrides that path for isolated tests.
Application startup loads settings without starting UDP or event processing;
Start is explicit, settings changes require Stop, and shutdown stops the worker.
Global Stop remains accessible without a present module. The local
[DCS-BIOS peer and synthetic catalogue](../simulation/dcs-bios/README.md) exercise
this path without a simulator installation. Current-board firmware exposes the
same button/event/LED contract, but physical hardware and live DCS acceptance are
deferred.

CI's `scripts/dcs-bios-coverage.py` measures instrumented native tests and gates
the production sections of `dcs_bios.rs`, `dcs_runtime.rs`, `panel_events.rs`,
`dcs_profile.rs` and `dcs_profile_runtime.rs`
individually above 70% line coverage. Frontend profile helpers have a separate
line/branch/function gates through `pnpm dcs:coverage` and `pnpm dcs-profile:coverage`.

Schema2 generalizes the saved profile to named catalogue bindings: momentary
buttons, maintained switches, fixed/variable-step encoders and calibrated axes,
plus readable LED and bounded text/numeric display setters. `dcs_catalogue`
returns bounded filtered pages from the installed A-10C catalogue, with raw
output indexes and supported interfaces, without exposing wire addresses to the
editor. Existing schema1 profiles still use the legacy worker until explicitly
saved as schema2.

`dcs_profile_runtime` keeps UDP decoding independent of one I/O worker per
gateway. It commits feedback after complete advancing-counter frames, retains
one latest pending value per output and prioritizes ordered event pages before
the next setter. Output-only panels may keep HID enabled; input-bound panels
retain the HID/direct-USB exclusion above. Switches and available axes send
their current absolute state on a fresh baseline. Momentary held startup needs
release, and encoder deltas never enter the coalescing queue. Slow reads
recheck export freshness and physical capability metadata before commands.

Missing/stale/wrong-aircraft feedback applies configured fallback text or LED
off. Route/boot/stream changes and event gaps rebaseline current input and
reconcile current output. Readable state is checked before any new setter after
an uncertain acknowledgement; one-shot actions cannot be feedback bindings.
Boolean enum outputs must advertise both on and off before Start. Stop discards
new commands from outstanding input reads and pending feedback setters; known
forwarded button presses still receive their neutral cleanup release, and output
fallbacks remain allowed.
Gateway diagnostics include bounded pending depth/high-water, coalescing,
latency mean/p95/max, throughput and maximum input polling interval. Local IPC
allows 64 KiB profiles and 128 KiB replies; CDC/CAN budgets are unchanged. See the
[Milestone 3 verification record](../docs/verification/2026-10-14-dcs-bios-panels.md).

The Pico adapter probes the selected endpoint only on Connect and chooses the legacy JSON protocol-1 service or the shared production CDC3 service for the new Pico debug role. The latter validates serial/UID, epoch and bounded debug metadata before providing configuration, lifecycle, actions, events and guarded input injection. Debug mutations validate arguments and current device boot and are sent once. DCS schema-2 targets accept Pico identities; schema-1 saved profiles retain compatibility. Debug definitions hydrate truthful device labels/kinds. Legacy images retain RAM-only settings, setup leases, liveness/neutral recovery and macOS IOKit interface names. New board firmware persists gateway/device journals and retains the existing board serial.

## Local gateway transport

Normal macOS/Linux discovery also reads bounded gateway advertisements from `/tmp/opensimgear-<user-id>/gateways/`. Directories and files must belong to the current user with private permissions; stale process advertisements and invalid paths are ignored. Valid metadata selects a loopback TCP endpoint or a private Unix stream. Opening revalidates discovery, then applies the same CDC identity and epoch checks as a USB serial connection.

The gateway owns its advertisement and lifetime. Device inventory is obtained exclusively through the gateway protocol. Strictly validated protocol-3 Pico debug advertisements can also use this transport; input controls are provided only by the connected firmware's debug capability. The transport starts no simulation processes. For isolated debug-build tests, `OSG_LOCAL_GATEWAYS_ONLY=1` excludes physical serial ports; release builds ignore that flag. Set `OSG_DCS_BIOS_CONFIG` to a scratch file when testing saved profiles.

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
pnpm dcs:coverage
pnpm native:test
pnpm e2e
```

Frontend tests cover native IPC, browser transport, discovery, ownership, error reconciliation and configuration validation. Rust tests cover fleet routing, same-backend and cross-backend isolation, duplicate module IDs, identity mismatches, concurrent operations and shutdown, plus transport boundaries, protocol validation, setup ownership, reconnect, stale replies, layout recovery and local discovery. Browser bridge tests verify out-of-order response correlation and rejection of unmatched responses. C formatter [fixtures](tests/fixtures/README.md) verify production wire compatibility without requiring another checkout for unit tests. Repository-owned E2E bootstrapping builds the production gateway/device simulations in `.e2e/`; it requires no workspace parent setup. Local stream tests do not establish physical USB/HID enumeration or hardware CAN behavior.

## Provider-neutral integration service

`integration_runtime` validates/saves portable role profiles and local assignments, then starts the shared bounded `dcs_profile_runtime` gateway workers. `integration_profile` validates named identities and formats. `game_telemetry` decodes bounded ACR byte pages and tracks producer progression; `acr_windows` provides read-only Windows shared-memory acquisition. Synthetic snapshots use the identical parser on every platform. `output_leases` validates the additive endpoint claim protocol without returning secret capabilities in status. Global DCS/integration requests share an admission guard; only one run starts at a time.

IPC commands are `integration_get_state`, `integration_catalogue`, `integration_validate`, `integration_save`, `integration_start`, `integration_stop`, `integration_override` and `integration_simulate`. Device routing adds `action_claim`, `action_renew`, `action_release`, `action_ownership` and `action_invoke_owned`. See [the integration guide](../docs/simulator-integrations.md). Acquisition/parser tests and simulated firmware acceptance are distinct from deferred Windows live-game/hardware qualification.
