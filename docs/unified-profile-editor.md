# Profiles editor

Profiles opens to the saved profile list, including editable Default and
**Add profile**. Rows open with pointer, Enter or Space. The active profile and
editing document are separate: opening or navigating an editor does not switch
the running profile.

## Creating and saving

**Add profile** immediately opens an empty draft editor. **Add from inventory**
lets the user select resource blocks explicitly, up to 64. It adds no routes,
transformations, virtual controllers or device policies. Offline resources and
game integrations whose games are closed remain available for authoring.
Existing blocks and bindings remain when resources disconnect.

The new editor also offers **Copy saved profile**. A copy retains its graph and
layout with a fresh identity, excludes local device assignments and shows those
resources as Unassigned until explicitly assigned. Replacing an edited new
draft with a blank graph or copy requires confirmation.

**Save profile** commits the graph and local assignments together. If that
profile is running, Save also prepares and applies the saved changes using the
same game context and selection mode. If another profile is running, or no
profile is running, Save only stores the edited profile. **Start profile** is
the explicit action for starting an inactive saved profile.

A failed storage write keeps the draft open. A successful Save with a failed
runtime update reports that the profile was saved and its changes could not be
applied; **Retry saved changes** remains available. New edits made during Save
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
Opening or copying a graph preserves its saved layout. Named editor groups,
Focus controls and collapsible source/input/action sections help organize large
graphs. These preferences follow Save, import/export and Undo/Redo.

Select a resource and choose **Use this exact port** from its paged provider
catalogue. Only selected typed ports enter the graph. Large provider catalogues
stay paged. Cached contracts support offline authoring; runtime admission checks
current availability. Assigning a copied resource requires an explicit inventory
choice and a matching provider port for every saved logical port.

Required resources must be available to start. Optional unavailable routes can
remain degraded while independent routes run. Block names, required policy,
game visibility intent and supported temporary publication policies are edited
in the selected resource's inspector.

Game blocks open their owning game's installation/provider settings and return
to the same draft, viewport and selection, including browser Back. Ordinary
Games navigation creates no editor return target. Driver setup and output
enablement live in **Settings → Controller and desktop output**.

## Routes, actions and processing

Drag **Outputs** onto **Inputs / actions**, activate those buttons with the
keyboard, or choose **Connect exact ports**. Select a concrete source output and
destination input. Different types, units, ranges and state/event delivery need
an explicit conversion node. An input has one writer; OR or input-priority nodes
make intentional fan-in visible. Reverse feedback is a separate route.

The bindings table exposes every exact edge. It supports retargeting, removal
and inserting a compatible transformation. Compatible linear chains can be
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

Keyboard/mouse presets add one desktop endpoint and any necessary visible
processing. Its saved **Output scope** chooses the selected game, a captured
application, or desktop-wide navigation. Captured application references resolve
through machine-local native identities. Choosing scope or capturing an
application updates the draft; Save persists the choice with that profile.
Each profile's choice remains independent of other profiles. See
[keyboard and mouse mappings](keyboard-mouse.md).

Companion blocks select a paired instance and capture a retained immutable panel
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
  profile and its local assignments without saving or starting it. Diagnostics
  identify exact blocks and edges. Changes to graph or inventory make the
  captured result stale; Refresh preflight captures current inputs.
- **Applied port samples** and **Path activity** inspect the active run, including
  exact ports, values, transformations, freshness and requested destinations.
  Unsaved draft changes remain separate from the running graph.
- **Recording preview** checks keyboard/mouse operations and timing using sample
  controls. It never submits OS input.
- **Native runtime and desktop output details** expose technical state for
  diagnosis. Retired-output recovery is also available here.

Preflight readiness never grants output ownership. Starting or updating a running
profile performs fresh native admission. Destination samples describe requests,
not device/simulator acknowledgements. Unknown, stale and unavailable remain
distinct from false or zero. Event samples show the last observed event and
are not an event history or input-learning mechanism.

## Runtime and concurrency details

The runtime card shows the active profile and state. Save running changes or
use **Start profile** for another saved profile. **Stop** cancels pending starts
or launches and releases output. Use **Start profile** to run again, or explicitly
select Automatic to resume automatic selection.
**Recover outputs** performs native cleanup without replaying mappings. Leaving
the editor leaves the running profile active.

Saved documents retain internal optimistic revisions for concurrency. The
`profile_editor_save` request uses `expectedRevision: null` for creation and the
captured saved revision for replacement. Native commits graph/assignments
atomically, then reapplies only when that exact profile is still running.
The applied run captures immutable content; edits cannot mutate it in place.
Revisions are implementation details, not a user-managed profile workflow.

Tests cover editor entry, empty creation, copy/assignment boundaries, atomic
save, active-save updates, navigation races, port paging, typed graph authoring,
scope persistence, Undo/Redo and separate Debug rendering. Hardware, driver and
game behavior are additionally checked through the running development Cockpit.
