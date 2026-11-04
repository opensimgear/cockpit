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
