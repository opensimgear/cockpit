# Profiles list and resource canvas (P6)

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
**Back to Profiles** explicitly offers Save, Discard or Keep editing when
changes are unsaved. These operations do not select, apply, start or pause
runtime processing. Default retains its protected fixed name and identity.

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
round-trip; no draft is saved or profile activated by navigation. Provider
connection settings are not duplicated in the graph inspector.

`ProfileEditorStore` accepts only list, exact document/assignment reads and
atomic editor-save capabilities. The canvas's provider client exposes only
inspection. Neither component invokes selection, Apply or action dispatch.
Exact connection authoring and live execution controls are subsequent scopes.

## Validation

The pure editor, canvas geometry, catalogue paging and actual Svelte SSR
rendering tests cover list entry, Default, Add/Copy/Save/Cancel/Back, exact
opening, complete category seeding, capacity errors, large catalogues, offline
retention, transport attributes and preserved layouts/session snapshots.
Frontend coverage gates retain the repository's 71% thresholds.

These tests establish authoring/navigation behavior. They do not establish
real driver submission, device ownership or simulator qualification.
