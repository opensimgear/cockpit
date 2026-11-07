# Cockpit read-model sources

This matrix starts at the native producer boundary. It covers the fixed MCP observation catalogue and the dynamic topics admitted by the native bridge. An invalidation can require an exact follow-up read. Its receipt alone does not prove that the underlying provider is event-driven or that every consumer has been exercised in the running app.

| Topic | Committed producer | Portal consumer or read boundary |
| --- | --- | --- |
| `workspace.state` | Gateway workspace worker publishes topology, connection, configuration and claim changes from its owned caches | App and gateway workspace. Editing renews an existing claim separately. |
| `controllers.inventory`, `controllers.input.<id>` | Controller registry inventory and typed capture publishers. Input preview is independent of ordered routing. | Devices and assigned profile controller inspectors. |
| `controllers.visibility` | Successful local visibility preference commits publish a retained revision without driver inspection. | The open Device visibility card and MCP subscribers can re-read the saved preference. |
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
| `designer.workspace` | Native designer storage publishes the workspace and library revisions after a successful workspace or widget-source commit. | The native companion library and MCP readers receive the revision notice. Browser-dev designer storage has a separate IndexedDB commit signal. Open editors preserve their drafts. |
| `activity.log` | Activity log revision published after log and source updates | Activity log reads and filters. |
| `settings.activity` | Activity retention publisher serializes committed retention changes | General Settings follows the committed value on native and browser-dev while preserving a local draft. |
| `storage.backups` | Persistence backup owner publishes operation and inventory state | Native Settings backup controls. |
| `preferences.<key>` | UI persistence owner publishes committed `theme`, `devices-mode`, `canvas-navigation`, `canvas-styling`, `tray` and `progress` values | Shared preference consumers on native and browser-dev. |
| `preferences.remembered` | Native, browser-dev and MCP `ui_preferences_save` commit the `inventory/remembered` SQLite record. Its observation carries only the committed revision, so the record may exceed the change-frame budget. | The shared preference adapter fetches the single record after its revision advances. Mounted Devices reprojects it with cached provider state without saving it back. Live native and browser-dev fixtures verified that another client can add and remove a remembered row without extra record revisions. |

The dedicated native activation window consumes `profiles.activation-window`, a window-scoped, UI-only projection of the activation progress owner. The main native portal also observes `settings.mcp`. The MCP server publishes a local-only invalidation when its status changes. Credential metadata is fetched through the local settings command, and the observation payload carries no issued tokens. The MCP settings and credential tools remain separate from this portal stream. Game artwork uses its own asset demand and immutable page lifecycle.

The artwork regression runs against the native gateway over both transports.
It changes, deletes and restores an owned file and checks every transition in
the exact `games.detail.<id>` subscriber without fixture gateway reads between
mutations. After the detail subscription closes, a summary subscription
confirms the committed file deletion and the first reopened detail snapshot
reflects it.

The Activity fixture runs against native Cockpit with
`OSG_ACTIVITY_OBSERVATION_NATIVE=1` or against the browser development gateway
with `OSG_ACTIVITY_OBSERVATION_BROWSER_URL` set. It traces the real observation
and gateway requests. Both runs cover a visible commit, a commit while the
view is closed, one committed-only read on each of three reopens, and bounded
search requests. The native AI settings fixture covers a visible credential
creation, a quiet idle interval and a revocation while the tab is hidden.
The hidden tab makes no settings reads and reopening shows the current
credential list. The navigation fixture also verifies that Activity retention
follows external commits, preserves a local draft and opens a fresh snapshot
after General was hidden.

The profile fixture uses two independent library readers and the runtime
reader on both transports. An external rename triggers one pinned document
read for a clean editor. A second rename updates both libraries while the
dirty editor retains its draft and performs no document read or save.
The fixture also checks that the former profile refresh commands stay quiet.

The controller inventory fixture verifies an exact-source metadata rename and
restore through the native and browser-dev transports. Two inventory readers
see the same commit while the selected input reader remains ready. A closed
reader misses the restore and receives the current name immediately on reopen.

The attached-controller visibility check verifies an external save while the
detail is open, then closes the detail and observes its reader retire. Restoring
the saved preference while the detail is closed produces no visibility status
read. Reopening uses a fresh reader and one opening read to show the restored
policy. The browser-dev fixture and native WebView trace both passed this check.

The companion listener fixture runs on both transports. It occupies a
loopback port, observes the waiting state, releases the port with Settings
hidden, and checks that reopen shows Listening. A committed disable updates
the pairing form while the hidden listener settings make no reads.

The navigation fixture verifies that native Settings releases its Backups
subscription when General is hidden. An external create commits while no
Backups reader is open. Returning to General opens a fresh baseline and
renders the backup, then an external removal clears the row. Browser-dev
displays the desktop-only management notice without opening the native
storage subscription.

Devices Settings uses the inventory subscription baseline for its initial
read and ignores superseded replies. The native and browser-dev navigation
fixtures count one initial inventory read on each reopen. The native vJoy
fixture also opens the view on a free slot, verifies that a profile lease
updates the visible row to busy, stops the profile while the view is closed,
and verifies the free slot on reopen.

On both transports, the navigation fixture commits desktop output enablement
through the guarded settings command while Devices Settings is open. It checks
the visible toggle and its observation acknowledgement, closes the tab, and
restores the original setting while no desktop-settings reader is open. A new
reader shows the restored value on reopen. No profile is applied by the fixture.

The discovery fixture runs against native Cockpit with
`OSG_GAME_DISCOVERY_NATIVE=1` or browser development with
`OSG_GAME_DISCOVERY_BROWSER_URL`. Both wait for the exact committed
`games.jobs` run before fetching review data. The same UI checks two copies
of a known title, a generic executable, same-volume relocation, and explicit
removal. The browser fixture also covers the first-start results banner.

On November 7, the navigation fixture passed in the running WebView and in
Edge through the isolated browser-dev gateway built from `65c2c653`. Two
navigation cycles across Devices, Profiles, Games, Companions, Debug and
Activity checked reader retirement and single baselines. Both transports
passed external desktop setting updates while visible and snapshot recovery
on reopen. Browser-dev also cancelled one in-flight subscription open and
verified its exact retirement. The initial WebView run reached DevTools before
the app mounted. The fixture now waits for the navigation control before
clicking. Logs are `matrix-native.log` and `matrix-browser.log` under
`.research/reactive/held-launch-schedule/`.

The matrix still needs proof for other external device and filesystem changes, nested consumer teardown and browser parity on native-only settings surfaces. Windows process acquisition retains the documented access-denied fallback. The browser and native companion apps and firmware lifecycles need separate qualification. The [timing dispositions](native-timing-dispositions.md) and [remaining acceptance](remaining-acceptance.md) track those boundaries.
