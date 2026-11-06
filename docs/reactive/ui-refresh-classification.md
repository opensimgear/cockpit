# Recurring frontend refresh classification

This is the frontend interval subset of the [R0 timing inventory](refresh-sites.json).
It was checked against the Cockpit tree at `3d836a2`. The rows describe calls
inside interval callbacks. An interval may have more than one responsibility.

| Site | Current work | Required disposition |
| --- | --- | --- |
| Cockpit `CompanionConnections.svelte` | Poll companion state and update the pairing countdown | Subscribe to committed companion and pairing changes. Keep countdown rendering local. |
| Cockpit `CompanionListenerSettings.svelte` | Refresh listener settings and runtime status | Subscribe to committed settings and listener lifecycle. |
| Cockpit `CompiledCompanionDesigner.svelte` | Refresh designer and companion state | Subscribe to committed designer and companion changes. |
| Cockpit `DebugControls.svelte` | Request module events and action metadata or state | Consume source input and action state subscriptions with boot and catalogue fencing. Preserve blur and exit releases. |
| Cockpit `DeviceVisibility.svelte` | Refresh visibility state | Subscribe to native visibility and ownership changes. Keep explicit visibility operations. |
| Cockpit `Esp32Companions.svelte` | Refresh serial and companion state | Subscribe to serial inventory and companion lifecycle, including installation and reconnect. |
| Cockpit `GameTelemetryInspector.svelte` | Refresh the selected paged telemetry query | Subscribe to updates for the selected game. Preserve query identity, inactive disposal and the explicit refresh action. |
| Cockpit `ProfileDebugControllerEditor.svelte` | Refresh debug controller state | Subscribe to native debug controller changes while the editor is active. |
| Cockpit `ProfileDebugView.svelte` | Read desktop status once and refresh expanded companion details | Separate the initial desktop read from the recurring companion read. Subscribe to companion lifecycle while details are expanded. |
| Cockpit `ProfileRetiredRecovery.svelte` | Refresh output and provisioning recovery records | Subscribe to committed recovery state and preserve explicit inspection and recovery commands. |
| Cockpit `SystemDevices.svelte` | Refresh the composite inventory while the document is visible | Publish changes from every contributing provider before removing the fallback. Controller catalogue changes already arrive through a subscription. |
| Browser companion `App.svelte`, preparation heartbeat | Renew ownership during panel preparation | Keep preparation lease renewal with its existing lifecycle. |
| Browser companion `App.svelte`, owner poll | Update presence and assignments, recover connectivity, renew active panel ownership | Separate assignment and presence observations from lease renewal and reconnect. Preserve focus and stale-owner fencing. |
| Browser companion `connection-worker.ts` | Renew the panel lease and release held inputs if the page stalls | Keep the watchdog and heartbeat with the connection owner. Qualify feedback delivery separately. |
| Native companion `App.svelte` | Poll native status and renew the active panel lease on separate timers | Replace the status timer with native notifications. Keep the lease timer with the current ownership checks. |
| Cockpit `ProfileDesktopControls.svelte` | Former desktop output status interval | Removed with the unreferenced component. Settings and Debug subscribe to committed desktop state. |
| Cockpit `ProfileInputLearning.svelte` | Refresh capture status in an unreferenced component | No live caller found. The native learning commands remain separate from this dormant UI. |

The wider R0 inventory still requires confirming the remaining native and
firmware timer owners and purposes.

Run `node scripts/reactive/audit.mjs --check-ui-timers` to check direct
`setInterval`, `setTimeout` and `requestAnimationFrame` calls in Cockpit, the
companion apps and shared frontend packages. This includes direct
self-scheduling timeouts and frame callbacks. The guard
requires review when a timer's callback or delay changes and rejects production
references to the two dormant timer components. The reviewed timeouts cover
connection retries and protocol deadlines, local QR detection, authored actions,
feedback expiry, compiler yields, user-initiated firmware workflows, debounce,
dismissal and resource disposal. The static check covers direct calls. Runtime
traces and the timing inventory cover indirect calls, callback behavior, frame
loops and acquisition below the frontend.

`CompiledWidget.svelte` schedules frames only during active playback. Authored
actions and feedback expiry share one timeout for the earliest pending deadline.
The native widget preview test measures these scheduler calls directly because
the idle network trace cannot detect a local render loop. ProfileCanvas
coalesces frames during pointer gestures, and its camera stops scheduling
when navigation finishes or is cancelled.

## Cockpit disposition through `5cc98117`

The table above records the original audit. The current Cockpit entry points
use the following sources.

| Original site | Current source or timing purpose |
| --- | --- |
| Companion connections, listener settings, designer and expanded Debug details | Companion presence and listener subscriptions. Firmware has its own observation. The pairing interval updates the displayed countdown. |
| Pico debug | Exact CAN input where available, committed gateway output readback, and coalesced fallback reads after accepted debug commands. Opening a route and the explicit Refresh readback action can read native state. |
| Device Visibility and Devices inventory | Native topology, visibility and ownership invalidations plus existing provider subscriptions. External HidHide or vJoy changes can be inspected explicitly. |
| Telemetry inspector | Selected integration observation and exact paged reads when the selection or observation changes. |
| Applied debug controls | Exact runtime owner and committed debug-input revision. |
| Physical input learning | Active editor subscribes to the native learning revision. The native learner wakes on reports, catalogue changes and its expiry. |
| Retained recovery | Native recovery revision, including provisioning job transitions. |
| `ProfileDesktopControls.svelte` | Retired. The active desktop views use separate settings and runtime observations. |

Browser companion assignments arrive over a subscription. Reconnect uses
transport events and a single retry deadline. The worker owns steady-state
lease renewal and unassigned pings with one timer for the next deadline.
The page sends local activity for the release watchdog only while a panel
session exists. In direct mode it wakes for the next session heartbeat or
unassigned ping. Panel preparation uses a separate cancellable heartbeat
deadline until the neutral baseline.
The shared panel canvas schedules the next pulse or feedback freshness
deadline. The portable companion renderer schedules lease renewals only while
its remote session is armed. The native transport retains its own watchdog.
The browser companion connection timers require separate lifecycle qualification.

Run the idle trace against the running native Cockpit with
`node --test tests/ui/reactive-idle-native.mjs`. By default it visits each main
view sequentially. Set `OSG_IDLE_VIEWS` to a comma-separated list of view names
for a focused run. The trace records gateway commands and observation
transport requests separately. For browser development, start `pnpm web:dev`
with the native gateway service and set
`OSG_IDLE_BROWSER_URL=http://127.0.0.1:5173/`. The browser trace verifies the
workspace observation baseline and allows startup discovery to settle for
45 seconds by default. Set `OSG_IDLE_BROWSER_WARMUP_MS` to override that wait.
Companion applications, modal workflows and physical deadlines need their
own evidence.

An initial 60,104 ms run in the Windows development Cockpit recorded zero
`system_inventory` reads, 23 `companion_inventory` reads, 23
`ui_preferences_save` calls, and 23 observation acknowledgements. The companion
reads arrived in pairs about five seconds apart. A separate response trace
confirmed that a single paired browser client repeatedly switched between
connected and disconnected with no assigned panel, pending pairing or USB
device. The trace preserves those transitions and the strict idle check still
fails. The connection cadence matches the native five-second WebSocket idle
limit. The reason the deployed browser missed its idle keepalive still needs
verification.

The server now probes idle authenticated browser sockets with WebSocket
control frames. A matching Pong renews socket liveness. The separate panel
lease still requires application traffic. After restarting the rebuilt
Windows Cockpit, the first trace included startup discovery and one companion
inventory read. Once settled, Devices completed 60,105 ms with zero gateway
and zero observation requests. The paired browser's network socket was still
established afterward and was older than the entire sample.

Two follow-up runs sampled all eight main views in the same settled Windows
development Cockpit. Each view recorded zero gateway requests and zero
observation requests.

| View | Sample |
| --- | ---: |
| Overview | 60,109 ms |
| Devices | 60,112 ms |
| Profiles | 60,114 ms |
| Games | 60,111 ms |
| Companions | 60,115 ms |
| Activity | 60,105 ms |
| Debug | 60,115 ms |
| Settings | 60,108 ms |

The same eight main views were sampled through the browser development
bridge on Windows using Edge and an isolated Cockpit database. The first
Overview sample overlapped cold discovery and recorded 361 gateway requests
and 41 observation requests, with the last gateway call at 36,915 ms.
Devices, Profiles, Games, Companions, Activity, Debug and Settings each
recorded zero requests after startup. A second Overview sample confirmed
the workspace observation baseline and recorded zero requests in 60,112 ms
against the warmed gateway service.

## Cockpit source and development bridge timing sites

These locations use the ledger pinned to Cockpit `56b6d1ec`. A timer type
annotation or injected scheduler declaration is listed with the call it supports.
The companion app and firmware rows are tracked separately.

| Ledger site | Owner and trigger | Disposition and verification |
| --- | --- | --- |
| `scripts/native-gateway-bridge.mjs:85` | Development bridge, one deadline per submitted request | Keep the bounded reply deadline. Bridge tests cover process failure and reply handling. |
| `activation-progress-presenter.ts:64,69` | Activation presenter, confirmation deadline after a pending operation settles | Keep the cancellable presentation deadline. `activation-progress-presenter.test.mjs` covers confirmation and stale operations. |
| `activity/ActivityPanel.svelte:28` | Activity search, user changes the filter | Keep the cancelled-on-change debounce. Activity UI tests cover query changes. |
| `companion-settings.ts:61,87` | Companion settings writer, one pending edit deadline | Keep user-edit autosave and ordered writes. `companion-settings.test.mjs` covers save coalescing. |
| `CompanionConnections.svelte:192` | Open pairing dialog, expiry countdown | Keep local countdown rendering. The browser and native idle traces verify no steady-state requests. |
| `CompanionConnections.svelte:213` | User-started flash, wait for serial re-enumeration | Keep the one-time settle delay pending physical reconnect qualification in #199. |
| `CompiledCompanionDesigner.svelte:164`, `GenericWidgetDesigner.svelte:194`, `ProfileLibraryTools.svelte:17` | Each export handler, after the user downloads a blob | Keep one-time object URL revocation. Export flow is covered separately from data observation. |
| `Esp32Companions.svelte:28,103,121` | Former ESP32 component, request timeout, flash settle delay and status refresh | Removed. The active companion view receives firmware progress from its own observation and retains explicit operation deadlines. |
| `game-read-models/test-fixture.mjs:37` | Test helper, wait for authored observation settlement | Test-only. Keep it out of production imports. |
| `gateway-workspace.ts:65,66` | Workspace client, renew an existing editing claim | Keep the deadline while a claim exists. `gateway-workspace.test.mjs` covers renewal and retired claims. |
| `notifications.ts:15,29` | Notification owner, dismissal for a visible notification | Keep the dismissal deadline. `notifications.test.mjs` covers pause and resume. |
| `profile-elk-runtime.ts:41,46` | Layout worker, after a requested layout | Keep the layout timeout and worker disposal. `profile-elk-layout.test.mjs` covers the layout path. |
| `profile-vehicle-conflicts.ts:14` | Profile conflict evaluator, debounce a changed editor draft | Keep the cancellable edit debounce. `profile-vehicle-conflicts.test.mjs` covers superseded compile results. |
| `ProfileCanvas.svelte:59,398` | Canvas camera animation and active pointer gesture | Keep dirty frame scheduling. Camera and canvas tests cover animation completion and gesture updates. |
| `ProfileCanvas.svelte:69,248` | Connection gesture, distinguish a single click from a double click | Keep the one-time selection delay. Canvas tests cover connection selection. |
| `ProfileDesktopControls.svelte:11` | Former desktop component, status interval | Removed. The frontend timer guard permits no replacement interval. |

## Companion application and shared widget timing sites

These locations use the same pinned ledger.

| Ledger site | Owner and trigger | Disposition and verification |
| --- | --- | --- |
| `apps/browser-companion/src/App.svelte:40,94` | Browser connection owner, retry deadline after failure | Keep bounded reconnect backoff. Reconnect and focus lifecycle still need live browser qualification. |
| `apps/browser-companion/src/App.svelte:41,267` | Direct browser connection, next lease or unassigned ping | Keep only while the direct connection is active. Transport tests cover the deadlines. |
| `apps/browser-companion/src/App.svelte:44,150` | Preparing panel owner, renew until the initial baseline or cancellation | Keep the preparation lease deadline. Qualify its minimum margin on actual clients. |
| `apps/browser-companion/src/connection-worker.ts:8,19` | Shared worker, next maintenance deadline for its connection | Keep one deadline per worker. `connection-worker.test.mjs` covers scheduling and cancellation. |
| `apps/browser-companion/src/connection-worker.ts:41` | Shared worker startup, retry after a failed connection | Keep one failure backoff per worker. Closing clears the pending retry and repeated connect messages share the same attempt. The worker regression covers both. |
| `apps/browser-companion/src/socket-transport.ts:31,41,65` | Socket owner, connection and outstanding reply deadlines | Keep protocol timeouts and close on failure. Transport tests cover cleanup. |
| `apps/browser-companion/src/transport.ts:32,63` | Transport bootstrap, stalled SharedWorker startup | Keep the one-time direct fallback. `worker-transport.test.mjs` covers fallback. |
| `apps/companion/src-tauri/src/status_events.rs:32,35,37,40` | Test-only receive bounds for status observation | No production timer. The fixture verifies one outstanding notification and acknowledgment. |
| `apps/companion/src/lib/lease-heartbeat.ts:10,12` | Native companion owner, renew an armed session | Keep the deadline with ownership and Stop. `lease-heartbeat.test.mjs` covers renewal and cleanup. |
| `apps/companion/src/lib/PairingForm.svelte:10,46` | User-opened QR scanner, read the next camera image | Keep the bounded local camera scan while explicitly open. Verify cancellation and hidden-page cleanup on actual clients. |
| `packages/companion-ui/src/CompiledWidget.svelte:22,60,69` | Shared widget renderer, active playback and next authored or freshness deadline | Keep dirty animation frames and a single deadline. The native preview fixture checks that idle rendering stays quiet. |
| `packages/companion-ui/src/PanelCanvas.svelte:59,66` | Panel controller, next action deadline and feedback expiry | Keep deadline wakes. Verify both return the affected widget to its safe state without further input. |
| `packages/widget-compiler/src/compiler.ts:87` | Active compiler, yield after a rasterized frame | Keep the cooperative yield and cancellation check. `compiler.test.mjs` covers compilation and bounds. |
| `apps/companion-esp32/boards/sunton-3248s035r/board.c:71,81`, `apps/companion-esp32/boards/waveshare-lcd7/board.c:160,162,164,166` | Board initialization, reset and display or touch readiness | Keep the one-time hardware settling delays. Physical display bring-up remains in #199. |
| `apps/companion-esp32/main/app_main.c:27` | Former common 20 ms touch read | Waveshare wakes on GT911 INT, with separate contact, fault, follow-up and health deadlines. Sunton and failed IRQ setup retain the timed path. Physical touch cadence and release qualification remain open. |
| `apps/companion-esp32/main/app_main.c:88` | Former unconditional 20 ms display wake | Replaced with task notifications for candidate and dirty state. Only visible feedback expiry and failed-render retry schedule deadlines. Verify display damage, expiry and recovery on hardware. |
| `apps/companion-esp32/network/serial_wire.c:82` | Former one-millisecond retry after an empty stdin read | Direct requests wait on UART console readiness and their absolute deadline. The UART event queue wakes provisioning on supported boards. Tests cover retained wakes, framing, interrupted waits and receive errors. Physical reconnect qualification remains open. |

At the pinned sibling revisions, the scanner found no direct timing calls in
`device-gateway`, `device-template`, or `firmware-common`. The gateway main
loop still passes the board clock to protocol and CAN deadline handlers.
`device-gateway/firmware/tests/gateway_test.c` and
`device-gateway/firmware/shared-cyphal/tests/transport_test.c` cover protocol
expiry and wraparound. Physical CAN retry and recovery timing and MCU
scheduling latency remain open in #199.
