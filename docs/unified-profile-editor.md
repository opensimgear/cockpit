# Profiles editor

Profiles opens to the saved profile list, including editable Default and
**Add profile**. Rows open with pointer, Enter or Space. The active profile and
editing document are separate: opening or navigating an editor does not switch
the active profile. There is no separate selected-for-runtime state.

## Creating and saving

**Add profile** immediately opens an empty draft editor. The searchable
**Components** palette adds individual resource blocks, virtual controllers and
keyboard/mouse outputs. Drag a component onto the canvas to preview its placement
and drop it at that position, or click its palette entry to add it. Neither method
changes the current selection; select the new block when ready to configure it.
Resource additions create no routes, transformations or device policies. Offline
paired resources and closed-game integrations remain available, with up to 64
resource blocks. Sections collapse independently, with **Groups** first. The
header's **Game context** is saved with the profile and automatically places
that game's integration. **None** removes it. Changing context replaces the
integration and its routes, with Undo available. Integrations are managed by the
context rather than offered in the palette.
Existing blocks and bindings remain when resources disconnect.

The new editor also offers **Copy saved profile**. A copy retains its graph and
layout with a fresh identity, excludes local device assignments and shows those
resources as Unassigned until explicitly assigned. Replacing an edited new
draft with a blank graph or copy requires confirmation.

**Save profile** commits the graph and local assignments together. If that
profile is active, Save also prepares and applies the saved changes using the
newly saved game context and existing selection mode. If another profile is active, or no
profile is active, Save only stores the edited profile. Successful saves appear
as notifications, without a persistent saved-feedback banner. **Activate** on a
profile tile activates an inactive saved profile using its saved context; the
editor has no activation card. Game settings list matching profiles. Launch
chooses the sole match or asks among multiple matches, with a remembered choice
that can be changed or removed in game settings. No matches use Default for
hardware only. Game exit suspends game routes and context scoped bindings; a
fresh verified game session can resume them without deactivating the profile.

A failed storage write keeps the draft open. A successful Save with a failed
runtime update reports that the profile was saved and its changes could not be
applied; deactivating and activating the saved profile retries it. New edits made during Save
remain unsaved, and a late reply cannot replace a different editor.

**Cancel creation** discards the new draft. **Back to Profiles** and sidebar
departure offer Save, Discard or Keep editing when dirty. Keeping the editor
open cancels the pending departure. Default keeps its fixed identity and name.

## Canvas and resources

Resources, processing, virtual-controller outputs and keyboard/mouse outputs
have distinct block categories. USB, CAN and gateway connections appear as
metadata. Every solid canvas line is an actual directed signal binding.

The canvas supports pan, anchored zoom, Fit all, search, selection, pointer and
keyboard movement and auto-layout. Positions and viewport are editor metadata.
Opening or copying a graph preserves its saved layout. Create, rename, focus or
remove groups from the palette. Group frames have saved bounds and cannot overlap.
Dropping a component into a frame joins it; dragging it outside removes its
membership. Deleting a group keeps components and routes. Move a frame by its
heading and resize it by its corner. Collapsible source/input/action sections
help organize ports. These preferences follow Save, import/export and Undo/Redo,
whose icon buttons sit at the top of the canvas workspace.

Adding an assigned resource automatically discovers its inputs and outputs across
all provider pages. Blocks show counts and a preview of the available ports.
Discovery does not create routes or fill the saved graph with unused ports.
Companions discover the panel selected in the profile, including its prospective
ports when the companion is offline or currently runs a different panel.
The connection side panel searches the full catalogue by name or ID; only the
ports used by a new route are declared and pinned in that same Undo edit.
Saved ports remain available when a resource disconnects. The inspector's paged
provider catalogue also supports explicit selection of cached contracts for
offline authoring. Assigning a copied resource requires an explicit inventory
choice and a matching provider port for every saved logical port.

Required resources must be available to activate. Optional unavailable routes can
remain degraded while independent routes remain active. Block names, required policy,
game visibility intent and supported temporary publication policies are edited
in the selected resource's inspector.

Game blocks open their owning game's installation/provider settings and return
to the same draft, viewport and selection, including browser Back. Ordinary
Games navigation creates no editor return target. Driver setup and output
enablement live in **Settings → Controller and desktop output**.

## Routes, actions and processing

Drag **Outputs** onto **Inputs / actions**, or drag in the opposite direction,
to open the connection side panel with both blocks selected. Clicking either
port group, activating it with the keyboard, or choosing **Connect exact ports**
also opens the panel. Select a source output and destination input, then create
the route. Cancel or Escape closes the panel without adding ports or routes.
Different types, units, ranges and state/event delivery need
an explicit conversion node. An input has one writer; OR or input-priority nodes
make intentional fan-in visible. Reverse feedback is a separate route.

Click a canvas connection line, or its keyboard control, to insert a compatible
transformation. Transformations are excluded from the component palette. The
bindings table also exposes exact edges and their editing controls. When two
ports require conversion, **Connect with transformation** adds the transformation
and both routes in one edit. Compatible linear chains can be
reordered. Removing a single-input/output transformation can join its route
only when the resulting direct contracts fit.

Transformations cover unit/range conversion, raw-quantity interpretation,
scaling, deadzones, curves, comparisons/hysteresis, gates, boolean logic,
priority, state/event conversion, selection, formatting, fallback, constants,
timing and scripts. Edit parameters on the visible block. Built-in summaries
show timing, polarity, thresholds, curve points or macro steps as applicable.

An action receives a complete ordered invocation. **Action pack** has an event
trigger and a state input for each native argument. **Set on change** is limited
to setters. Connect argument values through explicit edges, including constants.
Scripts declare typed inputs, outputs and parameters, use the
[typed helper contract](typed-script-helper.md), and start with empty outputs.

**Add virtual controller** creates a logical output block. Its inspector edits
button/axis/hat counts, required policy, native axis ranges and neutral fallbacks.
There are eight native targets and up to 128 controls per controller, including
at most 16 axes and four hats. Remove a control's route before removing the
control. Use visible range mapping to normalize raw source ranges.

The palette adds one keyboard/mouse endpoint with a configurable starter action
and no implicit routes or processing. Its saved **Output scope** chooses the selected game, a captured
application, or desktop-wide navigation. Captured application references resolve
through machine-local native identities. Choosing scope or capturing an
application updates the draft; Save persists the choice with that profile.
Each profile's choice remains independent of other profiles. See
[keyboard and mouse mappings](keyboard-mouse.md).

The palette lists paired companions by device identity, not panel instances.
Adding one captures no panel. Its inspector then selects a retained immutable panel
publication. Feedback inputs and control outputs are chosen explicitly from that
panel's prospective ports. Copies and panel replacements require explicit
instance/port reconciliation. Authoring leaves the live companion panel alone.

Undo/Redo covers graph semantics, exact local assignments, names, policies and
canvas layout. Save establishes the new saved baseline while retaining newer
edits. Opening another editor resets its history.

## Profile debug

The **Debug** button on the list or editor opens **Profile debug**. The main
editor focuses on authoring; diagnostics live in this separate view:

- **Inspect draft preflight** checks the current draft or a selected saved
  profile and its local assignments without saving or activating it. Diagnostics
  identify exact blocks and edges. Changes to graph or inventory make the
  captured result stale; Refresh preflight captures current inputs.
- **Applied port samples** and **Path activity** inspect the active graph, including
  exact ports, values, transformations, freshness and requested destinations.
  Unsaved draft changes remain separate from the active graph.
- **Recording preview** checks keyboard/mouse operations and timing using sample
  controls. It never submits OS input.
- **Native runtime and desktop output details** expose technical state for
  diagnosis. Retired-output recovery is also available here.

Preflight readiness never grants output ownership. Activating or updating an active
profile performs fresh native admission. Destination samples describe requests,
not device/simulator acknowledgements. Unknown, stale and unavailable remain
distinct from false or zero. Event samples show the last observed event and
are not an event history or input-learning mechanism.

## Runtime and concurrency details

The profile list action bar contains the Manual/Automatic toggle beside **Debug**
and **Add profile**. The active profile is first and carries an **Active** badge.
Other tiles are greyed out while remaining available to edit or activate; they
return to full contrast on hover or keyboard focus. Without an active profile,
all tiles have normal contrast and Default is first.
**Activate** and **Activate profile** request activation. When another
profile is active, a confirmation offers **Cancel** or **Switch
profile**. Cancelling changes nothing; confirming deactivates the active profile
before activating the requested profile.

**Deactivate** cancels pending activation or launches and releases output. It
leaves no active profile. Choose **Activate** to use a saved profile again, or
explicitly select Automatic to resume automatic selection.
**Recover** performs native cleanup without replaying mappings. Leaving
the editor leaves the active profile alone.

Saved documents retain internal optimistic revisions for concurrency. The
`profile_editor_save` request uses `expectedRevision: null` for creation and the
captured saved revision for replacement. Native commits graph/assignments
atomically, then reapplies only when that exact profile is still active.
The applied run captures immutable content; edits cannot mutate it in place.
Revisions are implementation details, not a user-managed profile workflow.

Tests cover editor entry, empty creation, copy/assignment boundaries, atomic
save, active-save updates, navigation races, port paging, typed graph authoring,
scope persistence, activation switching/cancellation, Undo/Redo and separate Debug rendering. Hardware, driver and
game behavior are additionally checked through the running development Cockpit.
