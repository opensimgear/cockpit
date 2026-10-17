# Profiles list, resource canvas and graph authoring (P6/P7)

Profiles opens to the saved profile list, including the editable built-in
Default and **Add profile**. Profile rows are native buttons, so pointer,
Enter and Space open the exact saved identity. The runtime selection and
captured applied revision are shown separately from the document being edited.

**Add profile** creates a local draft containing every inventory resource:
physical controllers, OSG peripherals, companions and game integrations,
including integrations whose games are closed. Each block starts without
selected graph ports. Provider catalogues remain available through paged
inspection; a 20,000-port catalogue does not become a 20,000-port portable graph.
No routes, transformations, virtual controllers or hiding policies are created.

The profile limit is 64 resource blocks. When the complete inventory exceeds
that limit, the creation flow requires an explicit selection of at most 64;
it never silently keeps the first 64. Newly discovered resources can be added
explicitly from inventory. Existing saved blocks and their bindings remain
when resources are absent, offline or unavailable.

The new-draft editor offers blank and copy choices. A copy retains the portable
graph and layout with a fresh identity. It excludes machine assignments and
shows copied resources as Unassigned until they are explicitly assigned.
Changing an edited draft's source requires confirmation.

**Save profile** commits the document and local assignments together through
`profile_editor_save`. New drafts use `expectedRevision: null`; existing
documents use their captured saved revision. Save validates the matching
committed profile summary and hash before advancing the editor baseline.
Newer in-flight edits remain local and receive the new optimistic base.
Late replies cannot replace a different editor.

**Cancel creation** discards the local draft without a storage request.
**Back to Profiles** and sidebar navigation explicitly offer Save, Discard or
Keep editing when changes are unsaved. A failed Save keeps the draft open; edits
made during Save must also be saved or discarded before continuing. Keeping
the editor open cancels the pending departure, including while a Save reply is
in flight. These operations do not select, apply, start or pause runtime
processing. Default retains its protected fixed name and identity.

## Canvas and ownership

Resource, processing and virtual-output blocks have distinct categories.
CAN, USB, gateway attachment, availability, route capabilities and ownership
metadata appear on blocks or in the inspector. They never create physical
transport wires. Each displayed line corresponds to an existing directed
signal binding in the document.

The canvas supports bounded pan, anchored zoom, Fit all, search, selection,
pointer and keyboard block movement, and auto-layout. Positions and viewport
are editor metadata and do not alter signal semantics. Opening a saved or
copied graph preserves its existing layout and viewport. Port groups expose
complete selected contracts, units, state freshness/fallbacks and ordered
action arguments. Provider browsing uses actual page lengths and rejects a
catalogue that changes revision or capability contract between pages.

Game integration inspectors link to the owning game's settings. The callback
includes the current editor store and selection as opaque local navigation
state. The application retains the editor instance for the actual settings
round-trip, including browser Back and changing the selected game. Returning
restores the same draft, canvas viewport and selection. The editor is mounted
when Profiles is opened and retained only for that actual settings visit.
Ordinary Games navigation creates no editor return target. Sidebar departure
from a settings visit returns to the retained editor before handling unsaved
changes. No draft is saved or profile activated by navigation. Provider
connection settings are not duplicated in the graph inspector.

`ProfileEditorStore` accepts only list, exact document/assignment reads and
atomic editor-save capabilities. The canvas's provider client exposes only
inspection. Editor navigation, graph edits and Save do not invoke selection,
Apply or action dispatch.

## Exact routing and visible processing

Select a resource and choose **Use this exact port** in its paged provider
catalogue. This saves one typed logical declaration and its exact machine-local
port/hash expectation. Cached contracts can be authored while offline; native
preflight determines their current availability. Copied resources require an
explicit inventory choice and a matching provider port for every saved logical
port before assigning the block.

Drag **Outputs** onto another block's **Inputs / actions**, use those buttons
with the keyboard, or open **Connect exact ports**. Choose a concrete source
output and destination input. Different types, units, numeric ranges and
state/event delivery require an explicit conversion node. An input has one
writer; OR and priority are visible nodes for intentional fan-in. Reverse
feedback is another directed route.

**Add virtual controller** creates a logical output block. Its inspector edits
stable button, axis and hat IDs, required policy, native axis ranges and neutral
fallbacks. The graph supports eight native targets and up to 128 total controls
per controller, including at most 16 axes and four hats. Removing a bound
control first requires removing its route. Raw source ranges are preserved;
normalization requires a visible range mapping.

Transformations include unit/range conversion, scaling, deadzones, curves,
comparisons/hysteresis, gates, boolean logic, explicit priority, state/event
conversion, selection, formatting, fallback, timing and declared-port scripts.
Their parameters are edited on the same visible graph objects. The bindings
table exposes every exact edge, supports retargeting and inserting a compatible
transformation, and provides a keyboard alternative to the canvas. Compatible
linear chains can be reordered; removing a single-input/output transformation
can join its route only when the resulting direct contracts fit.

An action receives a complete ordered invocation. **Action pack** has a separate
event-trigger input and a state input for each native argument. **Set on change**
is limited to setter actions. Sources and visible Constant nodes supply argument
values through explicit edges; no trigger, argument mapping or output is
inferred. Script nodes declare their typed ports and parameters, use the
[typed helper contract](typed-script-helper.md), and start with empty outputs.

Undo/redo covers graph semantics, exact local pins, names, policies and canvas
layout. A Save rebases history to the new saved revision while retaining newer
edits and keeping the applied session snapshot separate. Opening another editor
resets its history. Discard cannot race a Save already in flight.

## Native preflight

**Inspect draft preflight** captures the unsaved document and its current local
assignments in a read-only request. It does not save, select or Apply the profile.
Native diagnostics identify exact blocks and edges. A changed graph, changed
inventory metadata or an inventory refresh marks the captured result stale;
stale diagnostics no longer decorate the current canvas. Inventory invalidation
is conservative and also covers unrelated resource changes. An overall Ready
result with optional unavailable branches does not grant those branches delivery
tokens. Apply must independently capture current native authority.

## Native runtime controls

The runtime card shows the native session state, applied profile/revision,
current run generation and game context, degradation and recovery faults.
These remain separate from the profile draft. Read-only status refreshes do not
replace the editor or reset its history. Polling pauses while game settings hide
the retained editor and while the document is hidden.

**Apply saved revision** sends only the exact saved profile identity/revision
and an explicitly selected optional game context. Unsaved documents and local
assignment changes cannot be applied. Native performs a fresh admission check;
a successful read-only preflight never grants activation authority. Stop and
Pause remain available during an in-flight Apply. Delayed replies from an older
Apply or status read cannot replace a newer Stop/Pause result.

**Pause** persists the pause latch and stops the run. **Clear pause** clears that
latch and permits Automatic selection to resume. In Manual mode, starting again
requires an explicit Apply. **Recover outputs** uses native recovery and reports
its actual result. Failed effect requests are
never retried automatically. Leaving the editor does not stop an applied run.

## Applied port samples

The applied inspector reads cached native reducer samples for the exact current
run owner and profile revision. It does not acquire inputs, advance timers or
submit effects. Each page carries its own native capture time; large results
are paged by the actual returned count. Selecting a canvas block can filter the
applied ports when the editor and run have the same profile identity. If the
draft differs, the inspector labels its samples as belonging to the applied
revision and excludes draft changes.

Scalar values retain their raw units/ranges. Unknown, stale and unavailable
remain distinct from known false/zero. Actions retain the complete ordered
argument tuple. Input/sink samples reflect graph requests and do not confirm
hardware or simulator acceptance. Event samples are the last observed value;
polling is not an event history or input-learning mechanism. Stop or a changed
native owner clears the selected capture, and old replies cannot revive it.

Input learning remains a separate native capture-only workflow. The applied
inspector does not start it or manufacture candidate inputs from polled values.

## Validation

The pure editor, canvas geometry, catalogue paging and actual Svelte SSR
rendering tests cover list entry, Default, Add/Copy/Save/Cancel/Back, exact
opening, complete category seeding, capacity errors, large catalogues, offline
retention, transport attributes, preserved layouts/session snapshots, actual
App navigation, browser-history settings return, Save/continue races, exact
route authoring, typed action arguments, visible transformations and undo/redo.
Frontend coverage gates retain the repository's 71% thresholds.

These tests establish authoring, navigation and runtime-control request/status
behavior. They do not establish real driver submission, device ownership or
simulator qualification.
