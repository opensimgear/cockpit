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

The rest of R0 still requires classifying the one-shot frontend, native and
firmware sites and confirming each retained timer's owner and purpose.

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

The browser companion still has a periodic connection task. Assignments
arrive over a subscription. Its recurring work includes lease renewal,
unassigned connection pings, retry and recovery after a suspended renderer.
The shared panel canvas also has a 20 ms presentation and armed safety tick.
These companion timers require separate lifecycle qualification.

Run the Devices idle trace against the running native Cockpit with
`node --test tests/ui/reactive-idle-native.mjs`. The trace records gateway
commands and observation transport requests separately. It covers the
Devices view and its mounted providers. Other views, browser development,
companion applications and physical deadlines need their own evidence.

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
