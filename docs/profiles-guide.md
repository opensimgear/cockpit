# Profiles

A profile describes how your rig's inputs, transformations and outputs work
together. Open **Profiles** to see Default and your saved profiles. Open a profile
to edit it. Cockpit has either **No profile active** or one active profile, with
**Deactivate** available. Opening an editor leaves the active profile alone.

## Vehicle tabs

When a profile has a game context, its canvas has a **Base** tab and a **+**
button for vehicle tabs. Base applies to every vehicle. Add a tab, select an
aircraft or car, and add components and routes to that vehicle's canvas. Its
configuration applies together with Base only while native telemetry identifies
that vehicle. Unknown or stale vehicle identity applies Base alone.
Each canvas has its own game integration block for telemetry and simulator
commands. Connection settings, freshness and the installed catalogue directory
remain in the game's provider settings; vehicle tabs do not change those settings
while editing.

For DCS, Base exposes **CommonData** shared flight telemetry. Each aircraft tab
exposes that aircraft's DCS-BIOS controls and telemetry, resolved through the
installed aircraft aliases. Opening a tab loads its catalogue even while the game
is closed. It leaves the loaded aircraft and active module alone. During play,
the fresh native aircraft identity selects the correct DCS-BIOS module
automatically; Games shows the active module. There is no manual module selector.

Existing aircraft-tab ports keep their routes when every saved port has the exact
matching contract in the aircraft's catalogue. A mismatch leaves the configuration
intact and reports the control that needs explicit reassignment or removal.
To configure a different aircraft when a tab already contains its controls,
add another aircraft tab or remove those controls before changing the selection.

The picker includes installed DCS-BIOS aircraft, installed X-Plane aircraft and
previously observed Rally cars. Load a car once to discover its exact telemetry
identity, or use **Enter exact vehicle ID** when that identity is already known.
Saved vehicle names remain available while the game is closed.

Removing a vehicle tab removes all of its components, routes and local resource
assignments. Undo restores the complete edit. Changing or clearing the game
context removes the old game's vehicle tabs in the same undoable edit.
Conflict checks compare Base with each vehicle separately; different vehicle
tabs never execute together. Vehicle changes switch configurations automatically
in both Manual and Automatic profile mode.

## Create, save and activate

Choose **Add profile** to go straight to an empty editor. Name the profile, then
use the **Components** palette beside the canvas to add controllers, OSG devices,
paired companions and outputs. Drag a component onto the
canvas to preview its position, then drop it to place the block there. Clicking a
palette component also adds it. Additions keep the current selection; select the
new block to open its settings. Offline paired companions and closed games remain
available. Adding a resource creates no routes. A profile supports 64 resource
blocks. Palette sections collapse independently. Choose **Game context** in the
editor header to save the profile's game association and automatically add its
integration to the canvas. **None** removes that association and integration.
Changing context replaces the integration and its routes in one Undo edit.
There is no separate integration palette section.

Blocks automatically discover and show their available inputs and outputs.
Drag between their input and output groups to choose the exact ports in a side
panel, connect the route, and choose **Save profile**. Saving commits the graph and local resource assignments
together and shows a notification. Saving an inactive profile leaves the active
profile alone. Choose **Activate** on its tile when ready; it uses the saved game
context. A game can be closed while hardware remains active and game routes wait.
Tiles show a badge for their saved game context.

Game settings list profiles with a matching saved context. Launch activates the
only match automatically. Multiple matches show a chooser with **Remember my
choice**; game settings can change or remove that preference. With no matches,
launch uses Default for hardware only. Automatic mode also considers saved
contexts when a game is detected. An unresolved multiple match leaves the
current profile active until a choice is made.

Saving the active profile applies the saved changes
automatically, including its saved game context, and keeps Manual/Automatic mode.
If the changes cannot be applied, they remain saved and Cockpit reports the
failure; deactivate and activate the saved profile to try again. Typing and graph edits
remain local until Save.

A new draft can also copy a saved profile. The copy keeps its graph and layout
with a fresh identity. Select its local resources and match their ports
explicitly; copies do not inherit the source's device assignments.

**Cancel creation** discards an unsaved new profile. **Back to Profiles** and
sidebar navigation offer Save, Discard or Keep editing for unsaved changes.
A failed Save keeps the editor open. Default is editable, has a fixed name and
cannot be deleted. Fresh startup in Manual mode has no active profile and
acquires no output.

Choosing **Activate** for another profile asks whether to switch. **Cancel**
leaves the active profile alone. **Switch profile** deactivates it and activates
the requested profile. Opening, saving or copying a profile never selects it as
a separate runtime state.

## Set up this computer

Open **Settings → Controller and desktop output** for vJoy setup, device hiding
enablement and keyboard/mouse enablement. These settings belong to this computer.
The profile defines mappings, per-device hiding intent and keyboard/mouse scope.
Game installation paths and provider connection settings remain in the owning
game's details; a game block can open them and return to the same draft.

## Connect the rig

Each canvas line is one directed signal route between exact ports. USB, CAN and
gateway attachment appear as resource metadata. A device observed over USB and
CAN can retain both connections while Cockpit chooses its current input source.

Drag **Outputs** onto **Inputs / actions**, or start from an input and drag to
an output. The side panel selects both blocks and lets you search their discovered
ports by name or ID. Choose the output and input, then create the route. Ports
are saved automatically with that connection. Cancel or Escape leaves the graph
unchanged. You can also click a port group or use **Connect exact ports**. The bindings table provides
a keyboard alternative and lets you edit ports or remove routes. Click a
connection line to insert a compatible transformation; transformations are not
palette components. Different types, units, numeric ranges or state/event delivery
need an explicit transformation. For those ports, choose **Connect with transformation**
in the exact connection editor to create both routes together. Reverse feedback
is another directed route.

Transformations include range/unit conversion, scaling, deadzones, curves,
comparisons and hysteresis, gates, boolean logic, input priority, state/event
conversion, formatting, fallback, toggles, pulses, gestures, macros and typed
scripts. Select a transformation to edit its parameters. A raw numeric provider
can use **Interpret raw quantity** before **Convert units**; see the
[quantity guide](profile-quantities.md).

For a game action with arguments, add **Action pack**, connect its event trigger
and every ordered argument explicitly, then route the invocation to the action.
**Set on change** is available for setter actions. Constants can supply argument
values through their own visible edges.

**Add virtual controller** creates a logical controller output. Configure its
buttons, axes, hats and neutral axis fallbacks, then connect selected inputs.
The native plan supports eight controllers, with at most 128 controls per
controller, 16 axes and four hats.

Choose **Keyboard and mouse** in the palette to add an endpoint, then configure
its actions. In that block choose **Context scoped** to send output only while the saved
context game is in front, or **Global** to use it across applications. Scope is
saved with the profile. See [keyboard and mouse
mappings](keyboard-mouse.md).

Companions appear in the palette by their paired device names, including devices
without a panel. Panels are chosen after adding a companion: select its published
panel in the block settings, then select feedback inputs or interactive
control outputs. Replacing a panel or assigning a copied companion requires
explicit port reconciliation. Browsing panels leaves its current runtime alone.

For physical devices, **Game visibility intent** can hide the device while the
profile is active. **Controller publication policy** separately keeps its
configured setting or enables/disables USB controller publication while active.
Temporary publication changes require an advertised gateway CAN capability.
They leave saved flash settings alone, but can reconnect USB collections and
require game rebinding. Direct USB and Pico publication overrides remain
unsupported.

Direct OSG-to-DCS command routes require every HID publisher for their input
devices to be disabled. Device hiding is separate. Unsupported or unknown
publishers keep those commands blocked. A publication change while active
withdraws affected commands until the profile is activated again; unrelated
controller routes and read-only telemetry can continue.

Closing a game disables its integration routes and context scoped bindings,
while the active profile and independent hardware remain. When the matching
game returns, Cockpit verifies the new process and refreshes the integration
baseline. Controls must be released or centered before scoped bindings rearm.

## Arrange and inspect

The canvas supports pan, zoom, Fit all, search, auto-layout and mouse/keyboard
block movement. The palette's **Groups** section lists named frames and provides
Add, Focus, Rename and Remove controls. Drag components into a frame to join it;
drag them outside to leave it. Each block belongs to at most one of the 32 groups.
Frames can move and resize but cannot overlap. Removing a frame keeps its
components and routes. Output, input and action sections can be collapsed.
Layout and grouping follow Save, copy, import/export
and Undo/Redo. Disconnected resources keep their blocks and routes.

Connections automatically avoid blocks and use ordered lanes to separate nearby
wires. Moving a block updates the affected connections while keeping unrelated
routes stable. Choose **Settings → Styling → Arrows** for fully smooth curves or
perpendicular segments with rounded corners.

Double-click a connection to add a reroute point, then drag the point to reshape
the connection. Selected perpendicular connections also expose draggable segments.
Manual routing follows Save, import/export and Undo/Redo. Use **Reset bends** on
the selected connection to return it to automatic routing.

Open **Debug** from the list or editor to reach **Profile debug**. It contains
draft/saved-profile preflight, applied port samples, path activity, keyboard/mouse
recording preview, native runtime details and retired-output recovery.
**Inspect draft preflight** checks the draft without saving or activating it.
Graph or inventory changes make a captured check stale. A ready check does not
activate output; activation independently rechecks current resources and ownership.

Live inspection shows the active graph's captured values, transformations and
destinations. Unsaved editor changes remain separate. Unknown, stale and
unavailable differ from ordinary zero/false values. Destination samples describe
requested values; they do not acknowledge device or simulator acceptance.

## Deactivation, recovery and games

Required unavailable resources block activation. Optional unavailable routes can
degrade while independent routes continue. Wrong-game and absent-game routes
stay dormant. Lost or stale inputs release their affected contributions.
Reconnection needs fresh input baselines; held inputs and old events are not
replayed.

**Deactivate** cancels pending activation/launches and releases owned output.
Cockpit then shows **No profile active**. Choose **Activate** to use a saved
profile again, or explicitly select Automatic to resume automatic selection.
**Recover** restores
retained ownership without restarting mappings or commands. Retired ownership
cleanup is available in Profile debug.

Manual mode keeps profile activation under your control. Automatic uses enabled game rules
and the unique highest priority among verified running games; no match uses
Default. A priority tie holds selection. Deactivation, recovery and faults inhibit
automatic activation until resolved. Saving an active profile preserves this
selection mode; saving another profile does not switch to it. Enabling Automatic
authorizes its rule-driven switches without a confirmation on every game change.

Games **Launch** uses the active profile in Manual mode, or Default if no profile
is active. Automatic mode uses its matching rule/default. Launch prepares
neutral hardware, starts the game and waits for its verified process before
activating the graph. **Deactivate** remains available while waiting. Deactivating a profile
does not close the game. Provider setup is needed only by routes using it.

## Import, export and delete

Use **Import profile** to paste JSON or choose a file up to 512 KiB. Import creates
an independent profile and requires explicit local resource assignments; it
does not start output. The library holds up to 32 profiles including Default.

**Export saved profile** exports saved content, leaving unsaved edits out. It
offers selectable JSON and a download, excluding local device assignments,
credentials, backend slots and native application identities. An application
reference from another computer must be captured locally before use.

**Delete** confirms removal of a named profile and local assignments. An active
profile continues until deactivated. A concurrent saved edit requires refreshing
before deletion. Import and Delete from a dirty editor first offer Save, Discard
or Keep editing.

See [editor details](unified-profile-editor.md), [portable storage
contracts](unified-profiles.md) and [game setup](games.md).
