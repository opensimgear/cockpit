# Cockpit read-model sources

This matrix starts at the native producer boundary. It covers the fixed MCP observation catalogue and the dynamic topics admitted by the native bridge. An invalidation can require an exact follow-up read. Its receipt alone does not prove that the underlying provider is event-driven or that every consumer has been exercised in the running app.

| Topic | Committed producer | Portal consumer or read boundary |
| --- | --- | --- |
| `workspace.state` | Gateway workspace worker publishes topology, connection, configuration and claim changes from its owned caches | App and gateway workspace. Editing renews an existing claim separately. |
| `controllers.inventory`, `controllers.input.<id>` | Controller registry inventory and typed capture publishers. Input preview is independent of ordered routing. | Devices and assigned profile controller inspectors. |
| `usb.inventory`, `usb.inputs.<route>`, `usb.metadata.<route>` | Module USB fleet inventory and per-route capture and metadata publishers | USB devices, input panels and setup details. |
| `can.inputs.<gateway>.<module>` | STM32 panel input capture through the exact gateway and module observation bridge | CAN panel input preview for supported streaming firmware. |
| `system.inventory` | Supplemental system inventory invalidation through `system_inventory.rs` and gateway topology commits | Devices and Settings Devices read the affected inventory. External driver changes have explicit inspection paths. |
| `profiles.library`, `profiles.runtime` | Native profile read models publish saved document and applied owner changes separately | Profile list, editor, activation controls and shell. The clean editor follows MCP commits. A dirty editor keeps its draft and the revision guard rejects stale Save. |
| `profiles.resources` | Resource observation owner publishes committed availability and route changes | Profile resource selection and live resource resolution. |
| `profiles.activation`, `profiles.learning` | Activation progress and native input learning publish their own lifecycle revisions | Activation window and learning controls. |
| `profiles.desktop`, `settings.desktop` | Runtime readiness from profile read models and committed output settings from the desktop settings owner | Profile Debug and Settings Devices respectively. |
| `profiles.debug-inputs`, `profiles.samples.<owner-node>` | Gateway debug input publication and the exact applied profile owner and node sample publisher | Authored input controls and live profile block previews. |
| `profiles.retired-recovery` | Retained recovery owner publishes recovery and provisioning transitions | Recovery controls and status. |
| `games.state`, `games.detail.<id>` | Game read-model worker captures prepared caches after source revisions | Game library and scoped game details. Immutable page reads follow the exact projected references. |
| `games.jobs`, `games.telemetry` | Game read-model worker publishes job changes and telemetry invalidation | Discovery and artwork progress, and selected telemetry inspection. |
| `companion.listener`, `companion.presence`, `companion.firmware` | Companion host publishes listener lifecycle, pairing and presence, and USB firmware progress separately | Companion connections, listener settings, designer and firmware workflows. |
| `activity.log` | Activity log revision published after log and source updates | Activity log reads and filters. |
| `storage.backups` | Persistence backup owner publishes operation and inventory state | Native Settings backup controls. |
| `preferences.<key>` | UI persistence owner publishes committed `theme`, `devices-mode`, `canvas-navigation`, `canvas-styling`, `tray` and `progress` values | Shared preference consumers on native and browser-dev. |
| `preferences.remembered` | Native, browser-dev and MCP `ui_preferences_save` commit the `inventory/remembered` SQLite record. Its observation carries only the committed revision, so the record may exceed the change-frame budget. | The shared preference adapter fetches the single record after its revision advances. Mounted Devices reprojects it with cached provider state without saving it back. Live native and browser-dev fixtures verified that another client can add and remove a remembered row without extra record revisions. |

The native portal also observes `settings.mcp`. The MCP server publishes a local-only invalidation when its status changes. Credential metadata is fetched through the local settings command, and the observation payload carries no issued tokens. The MCP settings and credential tools remain separate from this portal stream. Game artwork uses its own asset demand and immutable page lifecycle.

The native artwork regression changes, deletes and restores an owned file and verifies all four image transitions in the exact `games.detail.<id>` subscriber, without fixture gateway reads between mutations. The Activity fixture runs against native Cockpit with
`OSG_ACTIVITY_OBSERVATION_NATIVE=1` or against the browser development gateway
with `OSG_ACTIVITY_OBSERVATION_BROWSER_URL` set. It traces the real observation
and gateway requests. Both runs cover a visible commit, a commit while the
view is closed, one committed-only read on each of three reopens, and bounded
search requests. The native AI settings fixture covers committed edits and a
quiet idle interval.

The matrix still needs proof for other external device and filesystem changes, nested consumer teardown and browser parity on native-only settings surfaces. Windows process acquisition retains the documented access-denied fallback. The browser and native companion apps and firmware lifecycles need separate qualification. The [timing dispositions](native-timing-dispositions.md) and [remaining acceptance](remaining-acceptance.md) track those boundaries.
