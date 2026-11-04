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
| Cockpit `ProfileDesktopControls.svelte` | Refresh desktop output status in an unreferenced component | No live caller found. Count it as dormant source until removed or wired to a live route. |
| Cockpit `ProfileInputLearning.svelte` | Refresh capture status in an unreferenced component | No live caller found. The native learning commands remain separate from this dormant UI. |

The wider R0 inventory still requires confirming the remaining native and
firmware timer owners and purposes.

Run `node scripts/reactive/audit.mjs --check-ui-timers` to check direct
`setInterval` and `setTimeout` calls in Cockpit, the companion apps and shared
frontend packages. This includes direct self-scheduling timeouts. The guard
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
the idle network trace cannot detect a local render loop.

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
| `Esp32Companions.svelte` and `ProfileDesktopControls.svelte` | No production imports. Their intervals are dormant. The latter still has a direct component test. |

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
