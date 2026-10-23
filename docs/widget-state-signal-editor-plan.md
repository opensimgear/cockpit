# State and signal-flow editor — implementation plan

Date: 23 October 2026. Status: proposed; this feature is not implemented.

## Outcome and scope

Extend the current generic widget designer with graphical state and signal-flow
editing. **Keep the current layer designer** for creating and editing artwork.
Use the **capsule Toggle as the only product pilot**. This supersedes the proposed
import-first replacement in [the SVG creator plan](svg-widget-creator-plan.md).

The author should be able to select **Off**, place the thumb on the left, select
**On**, place it on the right, connect activation to the requested output, and
configure animation and independent game confirmation without writing expressions.
The resulting widget must still use the existing compiled-pixel delivery path.

Included:

- State regions, transitions, guards, actions and state-specific appearances.
- Typed interaction inputs, profile inputs/outputs, transformations and wiring.
- The current primitives, layer tree, groups, reparenting, selection, dragging,
  resizing, imported raster assets, parameters and advanced bindings.
- Optional motion and sound, compiled Try, trace inspection and browser delivery.
- Save/reopen/import/export and an explicit conversion of the current capsule pilot.

Not part of this feature:

- Replacing the layer designer, SVG import, or rebuilding the old widget catalog.
- ESP32 implementation, pairing or hardware qualification. These remain in the
  existing [ESP32 backlog feature](https://github.com/opensimgear/cockpit/issues/161).
- A complete slider interaction implementation or a shipped RPM widget. Keep the
  numeric contracts needed for them; qualify generality with fixtures.
- A second runtime graph interpreter, arbitrary scripts or vector rendering on devices.

## Existing foundation

Use [the current implementation guide](generic-widget-designer.md) as the baseline.

| Existing area | Reuse | Required extension |
| --- | --- | --- |
| `src/lib/GenericWidgetDesigner.svelte`, `GenericSceneNode.svelte` | Layer artwork, expressions, parameters and canvas interaction | Explicit base/state edit context and graph workspaces |
| `src/lib/generic-widget-editor.ts` | Immutable history, geometry helpers and compilation fences | Atomic graph/appearance commands and reference maintenance |
| `packages/widget-runtime/src/behavior.ts` | Typed values, regions, events, guards/actions, timers, feedback quality and traces | Preserve runtime semantics; no graph execution engine |
| `packages/widget-compiler/src/{types,source,compiler}.ts` | Validation, finite sampling, rasterization, tile deduplication and optional media | Authoring schema, lowering, overrides and source diagnostics |
| `packages/companion-ui/src/CompiledWidget.svelte` | Compiled Try and player lifecycle | Trace/value inspection; future slider pointer mapping is separate |
| `src/lib/ProfileCanvas.svelte` and canvas routing/navigation helpers | Dotted canvas, node/port treatment, selection, wires and inspector conventions | Extract narrowly reusable parts without importing profile-specific semantics |
| `src/lib/compiled-companion-library.ts` and native source admission | Revisioned source, compilation identity and publication validation | New source admission and pilot conversion |

The existing reducer evaluates guards and actions against one event snapshot;
conflicting writes fail atomically. Generated behavior must retain this rule.
Existing numeric sampling, provenance checks, freshness, neutral baselines and
compile cancellation must continue working.

## Authoring architecture

### One authoritative document

Introduce a versioned **authoring source format 3**. Retain the existing
`VisualNode` layer representation and parameter/asset definitions. Add:

- Statechart definitions with stable region, state and transition IDs.
- A typed signal graph: nodes, named ports, connections and configuration.
- Explicit links from graph values/events to guards, actions, appearance inputs,
  published ports and media cues.
- Appearance overrides keyed by stable layer ID and state/selector ID.
- Transition effect settings referencing stable transition and layer IDs.

State and signal workspaces edit this same document through commands. The generated
runtime behavior and conditional scene are compiler outputs, not a second editable
copy. Existing expression editors remain an advanced way to edit graph expression
nodes; they must not silently diverge from the graphical representation.

Keep node positions, wire bends, collapsed groups, pan and zoom in a separate
editor-layout record keyed by source identity. Layout-only changes do not alter
compiled source identity or require rasterization. Semantic edits still invalidate
the compiled result immediately. Undo a graph gesture as one operation, including
its associated layout changes.

### Compiler bridge

```text
Authoring v3: existing layers + statechart + signal graph + appearance overrides
  -> validate types, references, ownership, conflicts and bounded work
  -> lower to existing executable source/behavior representation
  -> resolve state appearances and numeric sample domains
  -> preflight target costs
  -> existing raster compiler and optional media compiler
  -> existing compiled artifact and browser player
```

Keep the existing executable source v2 representation as an internal compilation
target and supported import format. Add a `lowerWidgetDesign` entry point with
diagnostic source maps from generated expressions/actions to authored nodes/edges.
Validate the lowered result with existing validators before rasterization.

The artifact's source identity must bind the original semantic v3 document, not
just its temporary lowered v2 result. Include the lowering/compiler version in
cache identity. Update TypeScript and independent native source admission,
canonical hashing, saved-draft checks and fixture generation together. Preserve
the deployed raster artifact format unless qualification proves a necessary change.

Never ship authoring layers, graph layouts, SVG or draw commands. Static states and
intermediate animation frames are fully composed off-device. Tile deduplication
continues to share finished pixels across appearances.

### State ownership and graph evaluation

Every stateful value has one owner. For the pilot, the **Requested** region owns
Off/On; the signal-flow node represents that region rather than maintaining a
second toggle latch. A state-to-value mapping exposes `requested` as false/true.
If existing port contracts require a local signal, generate its mirror and enforce
the initial/reset/transition invariant in lowering. Do not let authors wire another
writer into it.

- Event ports trigger transitions or effects; value ports carry typed values.
  Connections between these domains require an explicit operation.
- Pure transformation subgraphs are acyclic. State feedback passes through an
  explicit state owner; reject combinational cycles and competing writers.
- Lower pure subgraphs into bounded existing expressions. Do not execute nodes
  in screen order or introduce a render-frame-driven evaluation loop.
- Inputs are applied using the existing event-update/feedback rules. Guards and
  action expressions observe the reducer's single snapshot. Emit a transition's
  known target value directly; never depend on an earlier `set` action being visible.
- Define initial values, evaluation triggers, output neutral values and emission
  rules explicitly. Pure reads and feedback updates must not emit control commands.
- Preserve raw numeric values for behavior; only presentation selects sampled frames.

For the pilot, expose state ownership, boolean operations, comparison, constants,
typed signal references, availability guards and confirmation classification.
Retain numeric arithmetic/clamp and advanced expressions from the existing feature
set. Add a bounded map-range node and a numeric fixture to prove the graph is not
toggle-specific. Slider gesture handling and advanced numeric operations remain later work.

## State-aware layer editing

Add a persistent edit-context selector to **Design**:

- **Base** edits shared geometry and styling.
- **Requested / Off** or **Requested / On** edits that state's overrides.
- Other independent regions, such as Availability, can be selected separately;
  preview-only settings choose the rest of the active state combination.

The canvas always renders the resolved appearance. Selecting a state in the graph
offers **Edit appearance**, opening the same existing layer designer in that context.
Selecting a state for authoring must not dispatch a live event or send a game command.

Property rows identify **Inherited**, **Overridden** or **Bound** values. Provide
Reset override and Apply to base actions. A drag, resize or property edit writes
only to the selected context. Base changes propagate to states that still inherit
the property. Bound properties retain their binding plus a numeric offset unless
the author explicitly replaces the binding.

Rules:

1. Layers, IDs and tree topology are shared. Add/delete/duplicate/reparent are
   base-level operations, visibly labeled as affecting all appearances.
2. State-specific additions use shared layers with visibility overrides. Do not
   create separate layer trees per state in the pilot.
3. Reparenting a group preserves its subtree and state references. Duplication
   allocates new IDs and copies associated overrides; deletion reports affected
   bindings/motion tracks and removes or repairs them in the same undoable command.
4. Resolve base values first, then active state overrides. Different regions may
   override different properties. Two simultaneous overrides of the same layer
   property are a validation error unless the author adds an explicit combination
   appearance resolving that conflict; there is no hidden last-writer ordering.
5. Group transforms and state transforms follow the same coordinate conventions
   as the current canvas. Selection remains on release; tree-selected groups or
   single layers remain draggable through overlapping artwork.
6. Source previews, thumbnails, compiler output and motion endpoint generation
   use the same appearance resolver. Do not implement separate merge logic per view.

## Editor workspaces and visual conventions

Retain **Design**, **Motion**, **Sound** and **Try**. Evolve **Data** into
**Signal flow** and **Behavior** into **States**, retaining advanced inspectors for
current capabilities. Do not remove a capability because it lacks a new node preset.

Use the profile editor's application tokens, dotted canvas, compact node headers,
typed ports, orthogonal connections, purple transformation styling, selection ring,
zoom/fit and inspector patterns. Extract shared presentational/navigation helpers
only where useful; do not turn the widget graph into a `ProfileDocument`.

**States** supports creating/renaming/removing states and regions, initial-state
selection, drawing transitions, selecting transitions, guards, priorities and
actions. Show appearance thumbnails, separate selected/live markers and parallel
region boundaries. Show enter/exit semantics only where the runtime supports them;
the initial implementation uses existing explicit transition actions.

**Signal flow** shows interaction events, incoming profile values, state-owner
values, transformations, outgoing commands and presentation consumers. Connect by
dragging typed ports; provide a keyboard-accessible Connect action. Connection and
deletion diagnostics identify the exact pin and reason. Value probes and event
counts are driven by the simulator, with last-update and feedback-quality information.

Use widget-relative direction labels: **From profile**, **To profile**, and
**Interaction**. Existing `WidgetSource.inputs/outputs` names follow controller
semantics and are counterintuitive from the widget's perspective. Implement an
explicit boundary adapter and test it; do not reverse serialized endpoints or
invalidate existing profile mappings just to improve the UI terminology.

## Capsule toggle contract

| Area | Required behavior |
| --- | --- |
| Requested state | Off initially; activation changes Off ↔ On when available |
| Outgoing control | Exactly one target boolean emitted per accepted activation |
| Presentation | Thumb, halo and highlight move immediately toward the requested position |
| Availability | Incoming `available`; disabled blocks new activation and clears pressed feedback, retaining requested state |
| Press lifecycle | Press, release and cancel keep the existing pressed appearance and cancellation behavior |
| Confirmation | Incoming game value and quality; never overwrites requested state |
| Confirmation status | Fresh + matching = confirmed; fresh + differing = pending; unknown/stale/disconnected retain distinct labels |
| Provenance | Host submission/acceptance does not become game confirmation |
| Motion | Optional Off ↔ On transition, deriving endpoints from resolved state appearances; interruption preserves the existing reverse policy |
| Sound | Optional click cue using existing WAV/cooldown/unlock policies; no asset means no sound |
| Reset | Initial state and neutral output, cleared press/timers; no replayed sounds or stale commands |

Availability is an input-owned region; Requested is an interaction-owned region.
Generated availability transitions and guards must agree after every input event.
Confirmation is a separate presentation dependency, not a state that holds the
requested thumb position until the game replies.

Preserve the pilot's public port IDs, neutral values, parameters and hit regions.
Retain current user-edited layer IDs, groups and geometry when converting the pilot.
For conditional position expressions, split supported Off/On branches into overrides
while retaining unrelated parameter bindings and offsets. Never replace an unknown
expression with its currently evaluated number.

## Milestones and acceptance gates

### M1 — Authoring contract and deterministic compiler bridge

Deliver:

- Versioned v3 types/validators, typed graph registry, stable references and editor
  layout record; canonical source and compile identity rules.
- Lowering to the existing behavior/scene model, with reference/type/cycle/write
  diagnostics and limits on expansion into expression trees.
- A hand-authored capsule v3 fixture, generated runtime behavior and parity traces.
- v2/v3 library admission, native validation updates and immutable revision support.

Gate: headless compilation produces valid capsule artifacts; old v2 sources still
validate; generated toggle emissions, freshness, reset and neutral behavior match
the baseline. Malformed graphs are rejected before compilation or persistence.

### M2 — State appearances in the existing layer designer

Deliver:

- Base/state context selection, shared resolver, overrides and inherited/bound
  property affordances; shared tree operations and atomic undo/redo.
- State thumbnails and preview-combination controls using the common resolver.
- Compiler application of overrides before numeric sampling/rasterization.

Gate in the running Cockpit: move the capsule thumb in On without changing Off;
change the shared track color in Base and see both states update; move a whole
group; drag the bound thumb; resize all eight handles; undo/redo; compile and compare
the source preview against finished pixels. No second drawing editor is introduced.

### M3 — Graphical state editor

Deliver:

- Profile-style graph shell, regions, states, transition wiring and inspector.
- Initial state, event, guard, priority, target and action editing; diagnostic links.
- Edit appearance navigation, selected/live distinction, layout persistence and
  keyboard alternatives to pointer-only operations.

Gate: author the capsule Off/On and Ready/Disabled regions through the UI; remove
and restore a transition with undo; save/reopen; simulate activation and disable/
reenable. Ambiguous priorities, missing targets and invalid initial states prevent
compile with a focused diagnostic. Node arrangement does not invalidate pixels.

### M4 — Typed signal-flow editor

Deliver:

- Input/event, state-owner, transformation, command-output and presentation nodes.
- Connection creation/removal/reconnection, port inspectors, named domains and
  boundary-direction adapter; shared command history with the state editor.
- Capsule command routing and quality-aware confirmation path; numeric map/clamp
  and existing advanced expression capabilities.

Gate: recreate the capsule's complete signal path in the UI and preserve endpoint
IDs. Each activation emits once; feedback emits no command; disabled activation
emits nothing. Invalid types/ranges, cycles and competing writers are rejected.
A numeric test fixture demonstrates range mapping and bounded sampling without
adding a second product pilot or claiming working slider gestures.

### M5 — Effects, compiled simulation and trace inspection

Deliver:

- Transition inspector links to existing Motion/Sound features; automatic numeric
  endpoint tracks from appearances, plus explicit keyframes for advanced edits.
- Keep existing custom tracks. Detect an explicit track that conflicts with an
  automatically generated track and require one owner. Unsupported automatic
  interpolation snaps with a clear label; do not promise arbitrary path morphing.
- Compiled Try with input/feedback injection, live states, values, chosen transitions
  and emitted outputs; errors navigate back to authored nodes/edges.
- No-motion/no-audio compile profiles and resource/fallback reporting.

Gate: rapid toggles reverse motion while logical outputs remain immediate; changed
On artwork regenerates the endpoint; missing audio, mute, reduced motion, stale
feedback and disabled availability behave correctly. Try runs the same lowered
behavior and compiled player as publication, not a mock evaluator.

### M6 — Pilot conversion, browser qualification and release

Deliver:

- Explicit **Convert to state editor** for the capsule, producing a new revision;
  retain the prior revision for rollback and switch panel references deliberately.
- Recognize supported capsule bindings by validated structure, not only layer names.
  Preserve unrecognized expressions as advanced nodes; if lossless conversion is
  impossible, retain the v2 editor and show a concrete conversion diagnostic.
- Normal save/reopen, export/import, panel placement/parameters and publication.
- Current guide, authoring examples and recorded verification evidence.

Gate: create and edit the capsule in the running dev Cockpit, compile it into a
panel, assign it to a browser companion and operate it there. Confirm artwork,
aspect ratio, hit regions, input emissions, feedback provenance and independent
confirmation. Reload both ends and repeat; verify rejected candidate updates leave
the active publication intact. Do not require or enable an ESP32 target.

Dependencies: M1 → M2 → M3 → M4 → M5 → M6. Keep each milestone reviewable and
executable; use atomic commits within the larger feature branch. Do not merge a
partial source-version rollout that the native host or source importer rejects.

## Compilation, capabilities and cost control

- Preflight the full required visual selector product, including regions, signal
  samples and motion intermediates. State thumbnails are previews, not an exemption
  from publication budgets. Identify which dimension caused an over-budget result.
- Do not double-enumerate a generated state mirror and its authoritative region;
  resolve visual dependencies to the owner. Preserve selector coverage checks.
- Keep existing frame/asset/memory/manifest limits and pixel-error sampling checks.
  Reject an oversized build rather than silently dropping required appearances.
- No motion support: choose the current static frame immediately; live numeric
  values still update. No supported audio: omit cues/assets and report the fallback.
- Preserve static compiled output as the mandatory baseline. Test synthetic capability
  profiles now; defer actual ESP32 codec, transport, budgets and hardware testing.
- Source and effect edits cancel/fence compilation. A stale result cannot enable
  Save or replace a newer publication. Layout-only edits use separate persistence.

## Verification plan

| Layer | Required coverage |
| --- | --- |
| Authoring model | Type/domain inference, event/value separation, missing references, graph cycles, ownership, initial/reset invariants, bounds and deterministic lowering |
| Appearance resolver | Base inheritance, state overrides, explicit combinations, conflicts, group transforms, bound offsets, topology edits and reference cleanup |
| Compiler | Real raster goldens, state combinations, motion endpoints, deduplicated tiles, capability fallbacks, limits, cancellation and cache/source identity |
| Behavior | Event snapshot semantics, rapid activation, disabled input, press/cancel, exactly-once emission, unknown/stale/disconnected feedback and provenance |
| Persistence | v2/v3 admission, new revision conversion, old revision rollback, layout isolation, round trips and source/artifact mismatch rejection |
| Editor | Release selection, overlapping layers, individual thumb/group dragging, resize handles, graph wiring, keyboard connection, diagnostics, undo/redo and save tooltips |
| Browser | Published pixels/hit regions, output/feedback direction, reload/reconnect, candidate replacement, mute/unlock and reduced motion |
| Profile editor | Regression tests for shared canvas helpers, connections, navigation and inspector behavior after extraction |

For each relevant milestone run `pnpm check`, `pnpm widgets:coverage`,
`pnpm browser:test` and `pnpm ui:build`. Add the new authoring/lowering modules to
the coverage include list and maintain the repository's 71% gates; do not count
diagram snapshots as semantic coverage. Run affected native source/service tests
and generated parity fixtures whenever source admission changes.

Before release also run `pnpm companion:test`, the affected native coverage path
(`scripts/companion-coverage.py`), the full applicable native test suite, and
`pnpm widgets:native:ui` against the **running dev Cockpit** and a real browser
companion. Extend that UI suite for state/flow authoring rather than keeping the
pilot checks only in temporary scripts. Record commands, coverage, screenshots,
pixel comparisons, traces and target resource reports. Handle certificate warnings
through the user; do not disable certificate verification in qualification.

The feature is complete when the capsule can be authored, compiled, saved,
reopened and used in the browser through these views, while all current layer
editing and behavior/media capabilities remain available.

## Follow-on backlog

- Slider interaction: pointer-to-value mapping, capture/cancel, keyboard controls,
  axis/direction, step snapping and value-change output. Preserve requested versus
  confirmed values. Its continuous position is a numeric signal, not hundreds of
  manually authored states.
- Linear/radial RPM and three-state authoring examples; threshold/hysteresis presets
  and numeric keypoints, using the same graph and sampled presentation model.
- Further graph presets and editing ergonomics after capsule usability review.
- ESP32 capability negotiation, asset delivery, runtime and hardware qualification
  under its separate backlog feature; no pairing changes in this feature.
