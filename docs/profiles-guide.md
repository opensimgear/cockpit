# Profiles

Open **Profiles** to see Default, saved profiles and **Add profile**. Click a row,
or focus it and press Enter or Space, to open that profile's editor. The runtime
selection and applied revision appear separately, with **Stop** always available.
Opening an editor leaves the running profile alone.

## Create and save

Choose **Add profile** for a blank graph seeded with the current inventory's
resource blocks. Closed game integrations and offline resources remain usable
for authoring. If the inventory exceeds the 64-block graph limit, explicitly
choose the resources to include. Creation adds no signal routes or output policy.

Name the profile and select exact ports from each block's provider catalogue.
Resources copied from another profile need explicit machine assignments; copies
retain the portable graph without acquiring the source's local identities.
Use **Save profile** to commit the graph and assignments together.

**Cancel creation** discards an unsaved draft. **Back to Profiles** and sidebar
navigation offer Save, Discard or Keep editing for unsaved changes. A failed Save
keeps the draft open. Saving, copying and navigating do not apply a profile.

Default has a fixed identity and cannot be deleted. Its graph can be edited.
Fresh startup selects Default without starting an output session. An untouched
Default works without custom profiles or optional hiding/output dependencies.

## Import, export and delete

Use **Import profile** from the list or editor to paste portable JSON or choose
a file up to 512 KiB. Import creates a new identity and needs explicit local
resource assignments. It never applies the graph. A library contains at most
32 profiles including Default.

**Export** reads the current saved revision, leaving unsaved editor edits alone.
The dialog offers selectable JSON and a download. It excludes local assignments,
credentials and backend slots. **Delete** is available for named profiles and
confirms the exact saved revision. A concurrent edit requires refreshing before
trying again. A captured applied revision remains independent of deletion.
Import and Delete from a dirty editor first offer Save, Discard or Keep editing.

## Connect the rig

Each graph line is a directed signal connection between exact declared ports.
USB, CAN and gateway attachment appear as resource attributes. They do not become
signal wires. A verified device observed through USB and CAN can retain both
connections while native route selection chooses its current input authority.

Expand **Editor groups** to name a frame and select its member blocks. Use the
group picker to rename it or change its members, **Delete group** to remove the
frame, and **Focus** to fit its blocks in the canvas. A profile supports 32 groups;
each block belongs to at most one group. Removing a block removes its membership
and removes an empty group.

The selected block's **Source outputs**, **Destination inputs** and **Destination
actions** sections each have a collapse button. Group membership and collapsed
sections follow Save, reopening, copies, portable import/export and Undo/Redo.
These editor preferences leave the profile's semantic and compiled program
identity unchanged.

For a physical device, **Controller publication policy** can keep its configured
setting or request Enable/Disable while the profile is active. This requires an
advertised nonpersistent gateway CAN capability; native Apply verifies the exact
device and ownership again. Offline policies remain saved without implying
readiness. Publication changes can reconnect USB controller collections and
require game rebinding. They leave saved device flash settings alone and are
separate from the **Game visibility intent** that hides a device through HidHide.
Direct USB and Pico publication overrides remain unsupported.

Select an output and an input/action using the canvas or **Connect exact ports**.
Different units, types, ranges or delivery kinds require a visible processing
node. Use the bindings table as a keyboard alternative. Its diagnostics identify
the exact resource, port or edge that needs attention.

For a combined rig:

1. Add a virtual controller, declare its logical buttons/axes/hats and neutral
   fallbacks, then connect selected ports from third-party A and B. Add a second
   virtual controller for selected OSG module inputs.
2. On third-party C, choose **Game visibility intent → Hidden while profile is
   active**. This is an owned runtime policy; it does not create a signal edge.
3. Select an OSG event output and the exact game command action. For actions with
   arguments, add **Action pack**, connect its event trigger and every ordered
   argument explicitly. Zero-argument commands need only their declared trigger.
4. Connect the game's altitude state through visible unit conversion,
   comparison/hysteresis and formatting nodes to declared companion indicator
   and text ports. A physical setter uses **Set on change** with its exact typed
   argument. Select the companion's retained panel publication explicitly.
5. Inspect draft preflight, resolve required-resource or contract errors, and
   save. Preflight is read-only and cannot grant output ownership.

Game/provider catalogues determine which ports are available. A directly
connected OSG device exposes only the controls/actions supported by its adapter.
Virtual output, physical writes, scripts and hiding still require their native
capabilities and qualification gates; a saved graph does not enable them.

## Apply and inspect

Expand **Apply, Pause and applied runtime inspection**. Apply requires a saved
revision and current local assignments. Choose an optional game context only
when needed. Native admission rechecks resources, contracts and ownership; a
failed read-only preparation leaves the previous healthy run intact.

The applied snapshot remains independent of later editor changes. Inspection
shows its exact ports and connections, source values, visible transformations
and requested destinations. Captured timestamps and stale/unknown/neutral state
are shown separately. A requested output value is not a device acknowledgement.

Required unavailable resources block activation. Optional unavailable branches
are reported as degraded while independent admitted branches can continue.
Wrong-game and absent-game branches stay dormant. Lost or stale input releases
its affected contributions; reconnect requires fresh baselines and current
native authority. Held input, old events and action requests are not replayed.

**Stop** cancels pending Apply/launch and releases owned output. **Pause** stops
the run and retains the pause latch. Clearing Pause does not restart processing;
use an explicit Apply. **Recover outputs** restores retained ownership journals
without restarting the graph, commands or hiding. Conflicting external edits or
offline hardware can leave recovery pending for explicit attention.

## Games and automatic selection

Installation paths and provider settings belong in the owning game's details.
The graph inspector can open those settings and return to the same draft and
viewport. DCS-BIOS settings are in DCS details. Installation discovery, provider
setup readiness, verified process identity and fresh telemetry are separate facts.

Manual mode retains an explicit saved profile pin. Automatic uses enabled rules
and a unique highest priority among verified running games; no match uses
Default. A priority tie reports a hold. Stop, Pause, recovery and faults inhibit
automatic activation until their explicit controls are resolved. Editing a saved
graph does not silently reapply it.

Games **Launch** uses the saved Manual pin or Automatic rule/default. It prepares
and owns neutral hardware before spawning, waits for the exact native process
off-lock, then starts the admitted graph. **Stop profile** remains available while
waiting. A game can stay open after Stop or a failed launch; the profile controls
do not terminate it. Optional provider setup is needed only for matching branches
that require it.

See [editor details](unified-profile-editor.md), [portable contracts and fresh
storage](unified-profiles.md), [game setup](games.md) and the [complete portable rig
verification](verification/2026-10-17-complete-profile-rig.md). Browser and portable
fixtures are distinct from actual Windows/game/controller/companion acceptance.
