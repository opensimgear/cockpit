# Cockpit read-model sources

This matrix starts at the native producer boundary. It covers the fixed MCP observation catalogue and the dynamic topics admitted by the native bridge. An invalidation can require an exact follow-up read. Its receipt alone does not prove that the underlying provider is event-driven or that every consumer has been exercised in the running app.

| Topic | Committed producer | Portal consumer or read boundary |
| --- | --- | --- |
| `workspace.state` | Gateway workspace worker publishes topology, connection, configuration and claim changes from its owned caches | App and gateway workspace. Editing renews an existing claim separately. |
| `controllers.inventory`, `controllers.input.<id>` | Controller registry inventory and typed capture publishers. Input preview is independent of ordered routing. | Devices and assigned profile controller inspectors. |
| `controllers.visibility` | Successful local visibility preference commits publish a retained revision without driver inspection. | The open Device visibility card, Settings Devices and MCP subscribers can re-read visibility status. Settings Devices reads once on its visible baseline and on each committed invalidation. |
| `usb.inventory`, `usb.inputs.<route>`, `usb.metadata.<route>` | Module USB fleet inventory and per-route capture and metadata publishers | USB devices, input panels and setup details. |
| `can.inputs.<gateway>.<module>` | STM32 panel input capture through the exact gateway and module observation bridge | CAN panel input preview for supported streaming firmware. |
| `system.inventory` | Supplemental system inventory invalidation through `system_inventory.rs` and gateway topology commits | Devices and Settings Devices read the affected inventory. The explicit inventory command also includes current driver metadata. |
| `system.inventory.drivers` | The inventory worker watches the exact Windows vJoy and HidHide driver files and publishes changed installed status and file versions | Settings Devices renders the retained driver snapshot directly. Native, browser-dev and MCP receive the same metadata. Driver-only commits leave general inventory readers quiet. [Running verification](../verification/2026-11-07-optional-driver-observation.md) |
| `system.inventory.vjoy` | Visible status demand wakes the approved shared Windows vJoy status reader. Ownership changes publish through `system.inventory`. | Devices, Settings Devices and the profile slot picker keep demand only while they display vJoy status. [Profile picker verification](../verification/2026-11-08-profile-vjoy-owner-demand.md) |
| `profiles.library`, `profiles.runtime` | Native profile read models publish saved document and applied owner changes separately | Profile list, editor, activation controls and shell. The clean editor follows MCP commits. A dirty editor keeps its draft and the revision guard rejects stale Save. |
| `profiles.resources` | Resource observation owner publishes committed availability and route changes | Profile resource selection and live resource resolution. |
| `profiles.activation`, `profiles.learning` | Activation progress and native input learning publish their own lifecycle revisions | Activation window and MCP passive input learning. The profile editor no longer mounts the learning panel after the controller settings simplification in `2013ce62`. |
| `profiles.desktop`, `settings.desktop` | Runtime readiness from profile read models and committed output settings from the desktop settings owner | Profile Debug and Settings Devices respectively. |
| `settings.autostart` | Windows Run and StartupApproved events, macOS LaunchAgent file events, and Linux XDG registration file events wake the desktop auto-start owner before the retained status changes | General Settings reads the stream only while visible. MCP exposes the same snapshot from the native desktop. |
| `settings.window-state` | The native window persistence owner publishes its current restore, capture, save, or flush diagnostic and clears it on recovery | General Settings reads the error while visible. The read-scoped MCP resource carries the same diagnostic. |
| `profiles.debug-inputs`, `profiles.samples.<owner-node>` | Gateway debug input publication and the exact applied profile owner and node sample publisher | Authored input controls and live profile block previews. |
| `profiles.retired-recovery` | Retained recovery owner publishes recovery and provisioning transitions | Recovery controls and status. |
| `games.state`, `games.detail.<id>` | Game read-model worker captures prepared caches after source revisions | Game library and scoped game details. Immutable page reads follow the exact projected references. |
| `games.discovery` | Game read-model worker publishes the prepared scan review independently | The Games view and MCP discovery readers resolve scan result pages. Catalogue subscribers do not load them. |
| `games.jobs`, `games.telemetry` | Game read-model worker publishes job changes and telemetry invalidation | Discovery and artwork progress, and selected telemetry inspection. |
| `companion.listener`, `companion.presence`, `companion.firmware` | Companion host publishes listener lifecycle, pairing and presence, and USB firmware progress separately | Companion connections, listener settings, designer, firmware workflows and the expanded Debug companion details. The [Companions roster](../verification/2026-11-08-companion-roster-visibility.md) reads status only while visible. |
| `companion.firmware-releases` | The native release resolver publishes changed metadata after a catalog or installation result commits | Visible companion tiles, firmware details and expanded Debug companion details share a reader. Native, browser-dev and MCP readers receive the current release snapshot independently of USB flash progress. [Running verification](../verification/2026-11-07-companion-release-observation.md) |
| `designer.workspace` | Native designer storage publishes the workspace and library revisions after a successful workspace or widget-source commit. | The native companion library and MCP readers receive the revision notice. Browser-dev designer storage has a separate IndexedDB commit signal. Open editors preserve their drafts. |
| `activity.log` | Activity log revision published after log and source updates | Activity log reads and filters. |
| `settings.activity` | Activity retention publisher serializes committed retention changes | General Settings follows the committed value on native and browser-dev while preserving a local draft. |
| `storage.backups` | Persistence backup owner publishes operation and inventory state | Native Settings backup controls subscribe while visible. A queued restore stops the broker. On reopen, one status read recovers the durable restart requirement if the stream is unavailable. |
| `preferences.<key>` | UI persistence owner publishes committed `theme`, `devices-mode`, `canvas-navigation`, `canvas-styling`, `tray` and `progress` values | Shared preference consumers on native and browser-dev. |
| `preferences.remembered` | Native, browser-dev and MCP `ui_preferences_save` commit the `inventory/remembered` SQLite record. Its observation carries only the committed revision, so the record may exceed the change-frame budget. | The shared preference adapter fetches the single record after its revision advances. Mounted Devices reprojects it with cached provider state without saving it back. Live native and browser-dev fixtures verified that another client can add and remove a remembered row without extra record revisions. |

The dedicated native activation window consumes `profiles.activation-window`, a window-scoped, UI-only projection of the activation progress owner. The main native portal also observes `settings.mcp`. The MCP server publishes a local-only invalidation when its status changes. Credential metadata is fetched through the local settings command, and the observation payload carries no issued tokens. The MCP settings and credential tools remain separate from this portal stream. Game artwork uses its own asset demand and immutable page lifecycle.

The [workspace transition fixture](../../tests/ui/workspace-simulation-browser.mjs)
runs the browser-dev portal with the Linux gateway service and the production
C gateway simulator in a private namespace. It observes a gateway arriving in
Devices, a replacement while Overview and Devices are closed, and removal
after reopen. The shell count and Devices tiles follow the native workspace
snapshot with zero frontend discovery, connection or status requests. A new
browser shell opens on the already connected replacement. The portal display
withdraws connectivity when discovery confirms absence, even while the
adapter's cached state still reports connected. The Windows native
fixture separately verifies the shared idle baseline. Physical gateway
arrival and removal remain in [#199](https://github.com/opensimgear/cockpit/issues/199).

The `windows` persistence key applies to next-start geometry. Saving that key
through MCP has no current-session portal consumer. General Settings uses the
shared `tray` and `progress` preference revisions for saved values. It reads
native desktop status only while General and the document are visible and
discards replies from retired views. Auto-start updates use the same retained
topic on Windows, macOS, and Linux. Windows follows the registry. The file
watcher follows the registration paths used by the pinned plugin. [Registry verification](../verification/2026-11-07-desktop-autostart-observation.md) · [File watcher verification](../verification/2026-11-07-desktop-autostart-files.md)

Window geometry failures and recovery publish a separate retained diagnostic.
General retires that reader while hidden and opens on the current diagnostic.
The saved geometry itself applies on the next launch. [Running verification](../verification/2026-11-07-window-state-observation.md)

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

On November 7 the running native AI settings fixture reproduced an eager
status request before subscription, followed by a second request at the
opening baseline. The corrected pane reads once after the baseline, retires
its reader when the document hides, and fences any status reply from that
retired observation. An external credential revocation while hidden
triggered zero reads. Reopening obtained one current status and leaving the
tab retired the reopened reader. Edge browser-dev rendered the desktop-only
notice with a disabled switch and sent zero AI settings requests.

The navigation fixture also reproduced an editable Activity retention value
while the document was hidden. In native Cockpit and Edge browser-dev the
fixed pane withdrew the editable snapshot, retained its local draft, retired
its exact reader, and reopened on the retention saved while hidden. The same
navigation trace verifies the current Devices, driver metadata, firmware
release and Designer subscriptions. Raw logs are under
`.research/reactive/mcp-settings-lifetime/`.

The isolated frontend passed 1,700 tests, the Svelte check and the production
build. The MCP settings controller reached 97.61% line, 93.47% branch and
94.44% function coverage. The visibility helper reached 100% on all three
measures. The desktop and browser-dev executables used SHA-256
`8bc0f11da8f59b0648759cba6337dbdd7396f71113889ac60b036cf941c7978e`
and `0dfc6a0bd97fa9a9a994c5ab2d5b6b72bd13cd90585b0f12d3e7887791d1c4b4`.

The profile fixture uses two independent library readers and the runtime
reader on both transports. An external rename triggers one pinned document
read for a clean editor. A second rename updates both libraries while the
dirty editor retains its draft and performs no document read or save.
The fixture also checks that the former profile refresh commands stay quiet.

Profile Save takes runtime state from the retained `profiles.runtime`
stream. A trace of the previous editor captured one redundant
`profile_status` request per save. The running native WebView and Edge
browser-dev created and saved a profile with a debug controller, observed
a background rename while open, and reopened on a second committed rename.
Each applied the output-free profile, saved an axis-range change, and
observed the new applied revision and owner generation through the runtime
stream. Each made two editor saves and zero profile library, runtime status,
or automatic status refresh requests. The fixture used private databases.
Browser-dev activation exceeded the original 15-second fixture wait and
passed with a 30-second bound. Controller latency remains in #199.

The controller inventory fixture verifies an exact-source metadata rename and
restore through the native and browser-dev transports. Two inventory readers
see the same commit while the selected input reader remains ready. A closed
reader misses the restore and receives the current name immediately on reopen.

The mounted Devices sidebar shares the `profiles.resources` catalogue with
the Profiles editor for identity correlation and its live device count.
The resource reopen fixture renames an attached controller through the native
gateway with the editor open, then renames it again after closing the editor.
The palette follows the visible commit. The shared reader fetches the second
committed revision while the editor is closed, and the reopened palette shows
the current name without another subscription or catalogue read. The native
WebView and Edge browser-dev passed this check against separate private
databases. The fixture uses metadata only and restores the controller name
through the standard command before deleting its temporary profile.

The attached-controller visibility check verifies an external save while the
detail is open, then closes the detail and observes its reader retire. Restoring
the saved preference while the detail is closed produces no visibility status
read. Reopening uses a fresh reader and one opening read to show the restored
policy. The browser-dev fixture and native WebView trace both passed this check.

The companion listener fixture runs on both transports. It occupies a
loopback port, observes the waiting state, releases the port with Settings
hidden, and checks that reopen shows Listening. A committed disable updates
the pairing form while the hidden listener settings make no reads.

On November 7, the running Windows WebView and Edge browser-dev verified the
expanded Debug companion details. The presence and listener readers stayed
shared with the shell. The details opened firmware-progress and release readers
only while visible. All four baselines led to one `companion_status` read per
open. Hiding the document or closing the details retired their exact readers.

Debug's Runtime & recovery tab reads status after its observation baseline.
Hiding the document or switching tabs retires that reader. Reopening starts
with a new baseline before cached recovery actions can be used. The native
WebView and browser-dev fixtures held the real subscription request and
verified no status read before release, one after each of three opens,
and the exact unsubscribe on each close. Before the change both transports
made an early read. The fixture issues no recovery commands.

The Debug Profile inspection tab shares exact node sample readers between
its Applied port samples and Applied path activity views. The Windows WebView
and Edge browser-dev ran `tests/ui/profile-inspection-lifetime.mjs`. Both
showed an authored value in the first profile and retired both node readers
when the document hid. While hidden, a separate command stopped that owner,
applied a different profile and changed its input. The hidden tab opened no
readers and made no topology or inspection requests. Reopening read one
inspection page and two topology pages for the new owner, shared two exact
node readers, and showed only the new topology and current input. Switching
tabs retired both new readers. The two profiles had no hardware outputs.
Eleven focused UI tests and all three topology and inspection coverage gates
passed. The tested view, controllers and shared sample module matched main.

The profile editor's Debug controller inspector retires its input reader
when the Cockpit document becomes hidden. Its held buttons follow the existing
ordered release path. The controls stay disabled until the reopened reader
has a fresh baseline and the exact applied owner's status has been read.
`tests/ui/profile-debug-input-lifetime.mjs` reproduced a retained reader on the
previous native build. The fixed Windows WebView and Edge browser-dev both
retired two exact subscriptions, made zero status reads while hidden and made
one read on each baseline. The test held the second subscription request and
verified that the controls stayed disabled. A hat change submitted while
hidden appeared after reopen. The native trace sent one button release, and
the browser trace also sent the existing final neutral when the first release
was still in flight. The fixture used only authored input on a private profile.
The isolated frontend passed 1,697 tests with four workers. The default
parallel run hit one graph timing assertion that passed both in isolation
and in the four-worker run. Both debug controller coverage gates, Svelte
checks and the production frontend build passed.

The Companion library fixture opens a second Edge tab in the same browser
context. The second tab commits a draft to IndexedDB while the first tab
shows the library. The first tab receives both addition and removal. The
fixture repeats the two commits with the reader on Overview and verifies
each result when the library reopens. The writer is a separate page and
uses the production designer store. The Windows WebView fixture separately
passed the same visible and hidden mutations via native SQLite.

The open panel and widget editors now follow the same committed workspace.
An external source addition updates the panel palette. A change to the exact
saved panel or widget leaves the local draft in place and blocks Save until
the editor reopens. A change to a placed source invalidates the existing
compilation. The fixture reproduced the missing panel change on the previous
desktop build. The fixed Windows WebView and Edge browser-dev both preserved
locally edited titles, surfaced each conflict and showed the saved title on
reopen. The browser writer ran in a separate tab.

The panel editor also follows committed native assignments while visible.
Local checkbox edits overlay the host snapshot and merge with the latest
assignment set at Save. The [native and browser-dev fixture](../verification/2026-11-08-panel-assignment-reopen.md)
verifies a visible commit, a hidden commit, the saved selection and reopen.

Both transports rejected a stale writer after a second store committed.
Browser-dev checks and advances the IndexedDB revision in the same transaction
as the workspace write. A rejected write emits no commit notification. The
store also fences a write prepared from an older snapshot if a queued load
already superseded it. Twelve targeted tests passed with 96.97% lines,
93.51% branches and 94.12% functions for the library module.
The isolated frontend passed 1,699 tests with four workers. Svelte checks
and the production frontend build passed.

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

The profile's virtual slot picker follows two separate committed sources.
The exact profile's local version in `profiles.library` advances on a saved
slot pin even when its portable document revision stays the same. The picker
then reads that profile's current pin. `system.inventory` drives the choices
and their availability. The previous desktop picker missed a visible pin
commit. The fixed desktop and browser-dev runs each read the changed pin once
with no inventory read, reflected a second profile taking and releasing its
vJoy slot without re-reading the pin, and made zero pin reads while hidden.
The picker reopened with one pin read and the current value.

That fixture covers Cockpit-owned claims. Third-party feeders can acquire
or release a slot without a topology notification. Visible Devices views
share one status reader and receive changes through `system.inventory`.
Passive readers, including MCP, consume the same committed inventory. The
MCP inventory tool can explicitly inspect the driver when the Devices views
are closed. [vJoy acquisition](acquisition.md#external-vjoy-ownership)

On November 7, the desktop WebView and Edge browser-dev passed
`tests/ui/external-vjoy-ownership.mjs` against separate private databases and
the Windows vJoy driver. An independent process claimed and released free
slot 1. Devices showed busy and free. The virtual-device detail and Settings
Devices showed the exact owner PID and cleared it on release. Each transport
opened and retired four exact demand subscriptions. Releasing the slot while
Overview was open or while the document was hidden produced zero inventory
reads. Reopening showed the current status without `refresh:true`.

The native test also held two demand readers and verified one helper and one
query stream, with no further samples after the last reader retired. The
Windows suite passed 2,396 tests with 18 default ignores and two pinned DCS
smokes passed separately. The new owner measured 88.24% production line
coverage and 73.08% function coverage. The virtual-controller coverage gate
measured 81.11% production line coverage for the status helper. Including its
real Windows startup, queries and EOF shutdown, LLVM measured 72.22% function
coverage. The frontend
visibility helper reached 100% lines, branches and functions in the Devices
coverage gate.

The desktop executable SHA-256 was
`348aa36a85df3c1b6d0c65b282ace6498a38b98f7ef9c6cce80ba2bba8b033af`.
The newly linked browser-dev service SHA-256 was
`0c5f347b108a8b1562a74eadae63b3f5245ad3a6754b5234ee0c60e8b41cecc8`.
The local records are `live-detail-native.log` and `live-detail-browser.log`
under `.research/reactive/vjoy-status/`.

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

Games shows **View results** for the current scan while it runs, waits for
review, or has failed. Opening that review uses the existing discovery stream.
This also covers scans started from another client after first setup.
`tests/ui/game-discovery-reopen.mjs` completed first setup with no selected
changes in a private database, then started one scan while Games was visible
and a second while Overview was visible. The Windows WebView and Edge
browser-dev opened the current review in both cases. After reopen they showed
the second scan's entry and withdrew the first. Both runs made two authored
scan requests, zero extra scan requests and zero game state refresh reads.
The baseline Windows WebView reached Ready and lacked the review button.
The fixture executables contained text and were never launched.

Windows WebView and Edge browser-dev both delivered one header and shared
read-model transition for each authored process boundary without repeat UI
queries. A committed executable-path replacement withdrew the old identity
while it was still running, and its later exit left the new identity quiet.
The Windows native process acquisition fallback remains a separate boundary.
[Running verification](../verification/2026-11-07-windows-process-boundary.md#native-and-browser-dev-delivery-with-executable-replacement)

After the discovery stream split, the running Windows WebView and Edge
browser-dev navigation fixture each opened one `games.discovery` reader and
one `games.jobs` reader per Games visit. Both readers retired on leaving the
view in both cycles. Edge also passed the full review, apply, move and removal
workflow with no page errors. The real MCP client discovered `games.discovery`
through `resources/list`, resolved its snapshot through the immutable page
tool, and matched the two expected fixture entries to the exact ready scan.
The same MCP run applied and replayed its operation, then removed its fixture
game. The local MCP fixture waits for WebView readiness and any first-start
scan before starting its own scan. Logs are `parity-native.log`,
`parity-browser.log` and `parity-mcp.log` under
`.research/reactive/r0-baseline-clean/`.

DCS setup watches the configured Windows Saved Games path even when the
directory is missing. Its nearest existing ancestor wakes the native owner on
creation. The owner then registers the non-recursive child watch before
rebuilding the setup snapshot. The same path recovers after directory removal
and recreation. The filter admits matching DCS profile names and ignores
unrelated descendants.

The Windows native regression creates, removes and recreates a private watch
root and verifies matching child creation and removal in both generations.
The running dev Cockpit fixture separately creates and removes a private DCS
profile under the existing Saved Games root. The native suite passed 2,374
tests with 18 existing ignores. Line and function coverage reached 88.04% and
82.61% for the watch and 84.04% and 72.04% for DCS setup. Branches were not
instrumented.

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

The R5 producer audit covered the fixed native/MCP catalogue, admitted dynamic
topics, portal-only observation scopes, and direct native or gateway status
reads. The live delivery gate still needs coverage of other external device
and filesystem changes, nested consumer teardown, and browser parity for
native-only settings. Windows process acquisition retains the documented
access-denied fallback. The browser and native companion apps and firmware
lifecycles need separate qualification. The [timing dispositions](native-timing-dispositions.md)
and [remaining acceptance](remaining-acceptance.md) track those boundaries.
