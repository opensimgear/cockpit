# Portal and companion timing sites

| Site | Category | Owner and trigger |
| --- | --- | --- |
| `apps/browser-companion/src/App.svelte:40,41,44` | Declaration | Handles for reconnect, active-session maintenance and preparation heartbeat cancellation. |
| `apps/browser-companion/src/App.svelte:94` | Reconnect | Retry a disconnected browser companion after its selected backoff. |
| `apps/browser-companion/src/App.svelte:150` | Lease deadline | Renew the exact preparing panel session 500 ms after its preceding heartbeat until preparation ends. |
| `apps/browser-companion/src/App.svelte:267` | Session deadlines | Schedule the next connected heartbeat, idle ping or feedback expiry while visible. |
| `apps/browser-companion/src/connection-worker.ts:8,9` | Declaration | Handles for the worker's next deadline and reconnect cancellation. |
| `apps/browser-companion/src/connection-worker.ts:20` | Session deadline | Wake the socket worker for the next local or caller-owned maintenance deadline. |
| `apps/browser-companion/src/connection-worker.ts:38` | Reconnect | Retry a failed worker WebSocket after one second while the worker still owns the connection. |
| `apps/browser-companion/src/socket-transport.ts:31` | Declaration | A pending request owns one reply and its timeout. |
| `apps/browser-companion/src/socket-transport.ts:41` | Operation deadline | Close the candidate WebSocket if connection establishment exceeds three seconds. |
| `apps/browser-companion/src/socket-transport.ts:65` | Operation deadline | Fail the pending request and close the socket after a four-second reply timeout. |
| `apps/browser-companion/src/transport.ts:32` | Declaration | Own the optional worker startup fallback handle. |
| `apps/browser-companion/src/transport.ts:63` | Startup deadline | Switch to the direct transport once if the worker fails its startup handshake. |
| `apps/companion-esp32/boards/sunton-3248s035r/board.c:75,85` | Hardware startup | Display power-on command settling delays in the board initialization sequence. |
| `apps/companion-esp32/boards/waveshare-lcd7/board.c:161,163,165,167` | Hardware startup | Display and touch pin/reset settling during board initialization. |
| `apps/companion-esp32/main/app_main.c:55` | Acquisition boundary | Touch IRQ can wake the task early. Held contact, early/fault cases and Sunton use 20 ms checks. Quiet healthy IRQ mode uses a 200 ms lease health read. Physical qualification and residual sampling disposition remain open. |
| `apps/companion-esp32/main/app_main.c:123` | Source wake | Display waits indefinitely for a committed notification when no drawing deadline is due. |
| `apps/companion-esp32/main/app_main.c:126` | Display deadline | The same display notification also wakes early during an exact feedback expiry or failed-render retry wait. |
| `apps/companion-esp32/network/client.c:944` | Network maintenance | The network owner waits for commands or the next state-dependent link, lease or retry deadline. Console paths without UART readiness still rely on network maintenance. |
| `apps/companion-esp32/network/serial_wire.c:98` | I/O readiness | Wait for console input readiness or the direct request's original deadline. |
| `apps/companion-esp32/network/serial.c:15` | I/O readiness | The UART event queue blocks until the driver signals receive readiness. |
| `apps/companion/src-tauri/src/status_events.rs:32,35,37,40` | Fixture | Bound committed-status notification and quiet-period assertions. |
| `apps/companion/src-tauri/src/status_events.rs:68` | Source wake | Native companion status owner blocks until a committed update or stop. |
| `apps/companion/src/lib/lease-heartbeat.ts:10` | Declaration | Handle for one native companion lease deadline. |
| `apps/companion/src/lib/lease-heartbeat.ts:12` | Lease deadline | Renew the current companion lease at its scheduled deadline. |
| `packages/companion-ui/src/CompiledWidget.svelte:22` | Declaration | Handle for one widget timeout. |
| `packages/companion-ui/src/CompiledWidget.svelte:60` | Presentation | Request a frame for active widget animation. |
| `packages/companion-ui/src/CompiledWidget.svelte:69` | Widget deadline | Wake at the next scheduled widget state or cue deadline. |
| `packages/companion-ui/src/PanelCanvas.svelte:59` | Widget deadline | Advance the panel controller at its next authored transition. |
| `packages/companion-ui/src/PanelCanvas.svelte:66` | Presentation | Reproject the panel at its visible feedback expiry boundary. |
| `packages/widget-compiler/src/compiler.ts:87` | Cooperative yield | Yield between authored raster frames so an explicit compile can report progress and honor cancellation. |
| `scripts/native-gateway-bridge.mjs:85` | Operation deadline | Fail a timed-out browser-dev gateway request and retire the stalled native service. |
| `src/lib/activation-progress-presenter.ts:64` | Declaration | Handle for the local progress confirmation deadline. |
| `src/lib/activation-progress-presenter.ts:69` | Presentation | Injected scheduler for the progress presenter's confirmation deadline. Native observation owns progress data. |
| `src/lib/activity/ActivityPanel.svelte:28` | User debounce | Apply the search text 250 ms after the last edit. |
| `src/lib/companion-settings.ts:61` | Declaration | Handle for the next companion settings autosave. |
| `src/lib/companion-settings.ts:87` | User debounce | Flush an edited settings draft at the selected save deadline. |
| `src/lib/CompanionConnections.svelte:195` | Presentation | Display the countdown against a pairing offer's existing expiry. Native owns approval and expiry. |
| `src/lib/CompanionConnections.svelte:218` | Command recovery | Wait once after a finished USB flash for the board to re-enumerate before requesting connection. |
| `src/lib/CompiledCompanionDesigner.svelte:200` | Resource cleanup | Revoke the export blob URL after its download click. |
| `src/lib/controller-observations.ts:14` | Declaration | Testable interface for a display-frame callback. |
| `src/lib/controller-observations.ts:222` | Presentation | Draw the latest continuous sample once when the capture makes the preview dirty. |
| `src/lib/game-read-models/test-fixture.mjs:37` | Fixture | Bound the test helper's wait for a read-model condition. |
| `src/lib/gateway-workspace.ts:65,66` | Lease deadline | Schedule or cancel renewal of editing claims already held by this client. Workspace data comes from the native observation. |
| `src/lib/GenericWidgetDesigner.svelte:194` | Resource cleanup | Revoke the compiled export blob URL after its download click. |
| `src/lib/notifications.ts:15` | Declaration | Handle and deadline for one dismissible notification. |
| `src/lib/notifications.ts:29` | Presentation | Dismiss the specific notification after its remaining visible duration. |
| `src/lib/profile-elk-runtime.ts:41` | Declaration | Handle for the current layout worker's deadline. |
| `src/lib/profile-elk-runtime.ts:46` | Operation deadline | Stop an explicit layout operation after 30 seconds. |
| `src/lib/profile-vehicle-conflicts.ts:14` | User debounce | Coalesce profile conflict validation after an authored edit. |
| `src/lib/ProfileCanvas.svelte:59` | Presentation | Animate an explicit canvas camera transition. |
| `src/lib/ProfileCanvas.svelte:69` | Declaration | Handle for deciding a single connection click. |
| `src/lib/ProfileCanvas.svelte:248` | User debounce | Distinguish a single connection click from a double click. |
| `src/lib/ProfileCanvas.svelte:398` | Presentation | Coalesce pointer movement to one frame while dragging or wiring. |
| `src/lib/ProfileLibraryTools.svelte:17` | Resource cleanup | Revoke the downloaded profile's blob URL after its click. |
