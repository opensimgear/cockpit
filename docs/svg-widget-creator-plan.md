# Imported SVG widget creator — implementation plan

Date: 23 October 2026. Status: deferred by the user; implementation has not started.

The current direction keeps the existing layer designer. See the
[state and signal-flow editor plan](widget-state-signal-editor-plan.md).
The import-first replacement below is retained as a historical proposal.

## Product decision

Replace the freeform drawing workspace with an artwork-driven widget creator.
Users draw in their preferred design application, import an SVG, identify named
parts, and configure how those parts respond to signals and states. Cockpit owns
behavior, feedback, interaction regions, optional motion/sound, compilation and
delivery. The capsule **Toggle** is the only product pilot. Fresh radial RPM,
linear RPM and three-state examples prove generality; they are test/review fixtures.

The creator must make these tasks straightforward:

- Move a toggle thumb immediately when a control is requested, while showing game
  confirmation independently.
- Rotate a gauge needle around a pivot, or reveal a linear fill, from bounded RPM.
- Map a three-value signal to three presentations and optional transitions.
- Import separate complete images for state presentations when part bindings are
  unnecessary; PNG/WebP support can use the existing raster asset pipeline.
- Disable optional effects according to author preference and target capabilities.

The preview accompanying this plan is an interactive interface concept using
bundled examples. It demonstrates parts, mappings, behavior, effects and synthetic
capability fallbacks. It does not run the production importer/compiler, import
files, play sounds, save widgets, or demonstrate real ESP32 support.

## Architecture boundary

SVG is an authoring asset. Source preview may manipulate sanitized SVG parts.
**Compiled preview, Try and every production companion display finished raster
frames.** Keep this boundary for browser delivery as well as future ESP32 delivery.
This refines the earlier optional suggestion of direct browser SVG playback and
preserves one production rendering contract.

```text
Original SVG + named parts + bindings + optional states/effects
                              |
                  validated, normalized source v3
                              |
      resolve parameters / target size / capabilities / quality
                              |
     preflight -> sample -> compose full SVG -> pinned rasterizer
                              |
     finished frames -> deduplicated tiles + selectors + media
                              |
          compiled preview / browser companion / future ESP32
```

There is no layer tree, SVG, font layout, arbitrary script, or runtime vector
composition in the delivered artifact. Tile deduplication stores finished pixels;
tiles are not authoring layers. A companion still composites complete widget
surfaces into its panel through the existing panel renderer.

## User experience

### Entry and artwork

`Companions → Widgets → New` opens an import-first flow. Import SVG, choose the
Toggle starter, or start from complete state images. Show the intrinsic dimensions,
viewBox, aspect ratio and import diagnostics before creating the draft.

Use four workspaces, with the preview available throughout:

| Workspace | Main tasks | Layout |
| --- | --- | --- |
| Artwork | Import/reimport, expose parts, bind properties, configure instance parameters | Read-only parts list, canvas, focused binding inspector |
| Behavior | Signals/ports, optional state regions, events, transitions, hit regions | Simple mapping first; transition table and state diagram when needed |
| Effects | Optional transition tracks, easing, interruption, sound cues | Selected transition, timeline and preview |
| Test & build | Stimulate signals and feedback, compile, inspect results, publish | Exact compiled preview, simulator controls, resource/fallback report |

Parts are selected from the list or by clicking the artwork. A dashed outline
identifies the selected part. Keep pan/zoom, fit, undo/redo and pivot placement.
Remove primitive creation, path editing, freeform resizing, layer ordering,
reparenting, duplication and deletion. Imported hierarchy is read-only. A user may
give an exposed part a friendly alias without changing its SVG geometry.

The inspector offers only supported bindings. It includes a signal picker, an
inline “New” action for a typed signal, source domain, output mapping, and units.
A rotating needle exposes a pivot. A fill exposes a reveal direction and range.
A binding preview shows the minimum, midpoint and maximum without requiring a
state machine. Advanced bounded expressions remain available behind an explicit
advanced control.

Keep artifact size/aspect ratio distinct from per-part behavior. Default to
contain with preserved aspect ratio. Letterboxing and input coordinates use the
same transform; changing widget size invalidates compilation.

Separate **Save draft** from **Build / Publish**. A draft may contain unresolved
bindings and validation diagnostics; it lives in a draft store and cannot be
mistaken for an admitted deployable source. Publication requires a successful
current compilation. Disabled actions expose their reason on hover and focus.
Buttons have icons and short labels appropriate to their section.

### State and interaction design

States are optional. A gauge can be a direct numeric binding. A static image needs
neither data nor behavior. A control adds typed inputs/outputs and events; authors
add state regions only when the interaction requires them. Internally, the creator
may generate the existing minimal behavior definition for these simple cases.

Reuse independent regions, guards, priorities, bounded timers and actions. Provide
a form/transition-table editor first, with a diagram as an alternate view of the
same data. Do not implement two independent state editors or a scripting language.

Every valid visual state combination has a resulting static presentation. State
rules override named part properties or choose a complete artwork variant. A
default presentation covers the initial condition. Flag conflicting or missing
rules before compilation; do not rely on incidental DOM order.

For Toggle, keep separate values/regions:

| Concern | Example | Required behavior |
| --- | --- | --- |
| Requested | OFF / ON | Update and emit immediately on activation |
| Confirmed value | OFF / ON | Update only from simulator feedback; its meaning depends on feedback quality |
| Feedback quality | Fresh / unknown / stale / disconnected | Preserve the existing feedback contract; never present unavailable feedback as confirmed |
| Confirmation status | Unknown / pending / matched / mismatch / timed-out | Derive from requested versus confirmed value, feedback quality and timers; show separately from thumb position |
| Presentation progress | Current animation frame | Never determines input authority or delays an output |

The capsule's `thumb` follows requested state. Its `confirmation` part follows
derived confirmation status, with explicit stale/disconnected presentation. Test rapid reversal, late
feedback, stale data, timeouts and disconnection explicitly. A feedback change does
not replay the user's click sound.

V1 interaction regions remain bounded rectangles in source coordinates, matching
the current artifact contract. Start with the whole widget or a selected part's
bounding rectangle; permit explicit padding/coordinates. Bind press, release,
activate and cancel to named events. Drawing a hit region is behavior configuration,
not a general drawing mode. Arbitrary SVG-path hit testing is deferred.

## Source v3 and property semantics

The current source v2 schema requires a primitive `scene`. Introduce an explicit
source v3 rather than injecting SVG fields into that exact-key schema.

| Field | Contents |
| --- | --- |
| Identity | Format version, widget ID/revision, title and logical size |
| Artwork | Asset reference/hash, import version, canonical normalized representation, viewBox, import report; original bytes retained as opaque editable source |
| Parts | Stable logical part IDs mapped to unique SVG element/group IDs; display aliases |
| Bindings | Part, allowed property, typed source/expression, bounded mapping and local coordinate policy |
| Presentations | Default presentation and state-dependent overrides or whole-asset variants |
| Parameters | Baked instance values, such as a finite label, color or range |
| Behavior/ports | Existing typed signals, events, regions, guards, actions and input/feedback ports |
| Visual domains | Finite values or bounded numeric sampling and accepted error |
| Interaction | Existing rectangular hit regions and event mapping |
| Effects/assets | Optional transitions, sound cues and local asset closure |

Illustrative authoring data, not a finalized wire schema:

```json
{
  "formatVersion": 3,
  "artwork": { "assetId": "capsule-svg", "importVersion": 1 },
  "parts": [{ "id": "thumb", "svgId": "toggle-thumb" }],
  "bindings": [{
    "part": "thumb",
    "property": "translateX",
    "source": { "signal": "requested" },
    "mapping": { "false": 0, "true": 120 },
    "space": "part-local"
  }]
}
```

Start with these supported part properties:

- Visibility, opacity, fill and stroke color.
- Translation, rotation with explicit local pivot, and scale about a local pivot.
- Rectangular reveal clipping for linear gauges, with defined direction and range.
- Selection of a complete static asset/presentation.

Preserve the imported transform matrix. Apply behavior in a dedicated transform
wrapper with a documented local-coordinate order; never replace an existing
nested SVG transform string. Normalize nested viewports and verify non-zero
viewBox origins. Parent and child bindings compose in hierarchy order.

Define precedence as imported appearance → baked instance parameters → resolved
binding/state presentation → temporary transition interpolation. A property has
one resolved owner at each level. Conflicting rules are errors unless their
mutual exclusivity is explicit and validated. Reimport or parameter changes
invalidate any compiled result.

Continuous values have declared units, minimum/maximum, clamping behavior and
sampling accuracy. RPM motion smoothing is distinct from a transition between
discrete states. A numeric value can keep updating when motion is disabled.

Static text should be outlined in the exporting tool for the first SVG release.
Retain the existing pinned-font path for an explicitly introduced later text
binding; only baked parameters or bounded finite string domains can be compiled.
Arbitrary live strings and exact unlimited numeric labels are outside this model.

## SVG import and reimport

The importer is the largest new subsystem. Define a supported SVG profile and
publish export guidance; do not promise arbitrary SVG fidelity.

V1 imports paths/basic shapes, groups, transforms, solid paints, gradients,
supported clips/masks, and bounded embedded PNG/JPEG/WebP images. Normalize
supported inherited/presentation styles into a canonical representation. Accept
a small documented CSS subset; diagnose unsupported rules rather than silently
changing appearance. Resolve `currentColor` deterministically.

Expand local `use`/`symbol` instances with bounded expansion and stable instance
identity. Part bindings address the resulting declared instance IDs, not every
copy of a shared definition. Preserve paint/clip/mask references after ID rewriting.
Reject duplicate IDs, reference cycles, unresolved references and ambiguous parts.
Unidentified elements can remain static; expose only deliberately named parts.
Generated IDs may help initial inspection but must not be assumed stable on reimport.

Parse before displaying. Reject scripts, event handlers, external resource URLs,
external styles/fonts, `foreignObject`, embedded SMIL/CSS animation, DTD/entity
expansion and unsupported features such as filters in v1. Impose limits on input
bytes, elements, nesting, paths, reference expansion, image pixels and processing
time. Malformed or over-budget imports return per-element diagnostics; they never
reach the live preview DOM. Retain original bytes as inert source for export.

This boundary is necessary because SVG includes structural references and nested
coordinate systems: see the W3C specifications for [document structure](https://www.w3.org/TR/SVG/struct.html)
and [coordinates and transforms](https://www.w3.org/TR/SVG/coords.html). The import
profile and limits above are proposed product decisions, not claims about full SVG support.

Reimport is transactional:

1. Parse the replacement into a candidate and show diagnostics.
2. Match existing unique SVG IDs and retain logical part aliases/bindings.
3. Show added, removed, renamed and structurally changed bound parts, plus viewBox changes.
4. Require reassignment of broken references; never guess based on list position.
5. Show before/after previews. Accept as one undoable source edit, or cancel intact.
6. Revalidate domains/pivots/interaction geometry and invalidate the prior build.

## Compilation and resource control

Extend the existing compiler frontend; reuse rasterization and artifact packaging:

1. Validate source v3, importer version, normalized artwork, part references and
   property ownership in both browser-side and native admission paths.
2. Resolve baked parameters, exact target size, capabilities and quality policy.
3. Enumerate finite visual combinations and optional transition samples before
   rendering. Show a conservative upper bound and identify each contributing domain.
4. Apply resolved properties to normalized artwork and compose a complete SVG for
   each required state/sample. This stage happens in Cockpit, never on a companion.
5. Rasterize with the pinned resvg/fonts/codecs. Use the same rasterizer for final
   visual qualification. Authoring preview is explicitly labeled as source preview.
6. Reuse RGB565+A8 tile packing/deduplication, frame selectors, motion tables,
   optional PCM WAV assets, hashes and exact resource reporting.
7. Verify the complete asset closure in TypeScript and Rust. Try consumes that
   exact artifact, then publication uses the existing authenticated channel.

Use a worker/cancellable compilation job so imports and builds do not freeze the
UI. Cancel and discard stale results after any source/target change. Cache keys
include artwork bytes, normalization version, bindings, resolved parameters,
compiler/rasterizer identity, target capabilities, dimensions and quality.

Current source/compiler bounds include 4,096 visual combinations and 600 frames
per motion. Current browser admission has 2,048-pixel dimensions, 4,096 total frames,
2 MiB per asset, 8 MiB aggregate binary assets, and 64 MiB active/candidate resident
accounting. These are existing constraints, not performance guarantees or proposed
ESP32 limits. Admission checks must account for the combined static and motion set.

A 121-sample gauge plus several independent state regions can multiply quickly.
Use dependency analysis to avoid nonvisual domains where proven safe, then dedupe
identical images; do not rely on deduplication to bypass a preflight explosion.
Never silently lower visual accuracy. Offer explicit remedies: smaller dimensions,
fewer optional transition frames, a larger accepted error, or fewer independent
presentation dimensions. Whole-widget flattened frames remain the v1 boundary.

For sampling, compute conservative displacement in target pixels from binding
geometry when possible, including rotation radius, transform scale and reveal
length. Reject unsupported/unbounded mappings or require an explicit validated
bound; do not present an arbitrary sample count as a quality guarantee. Show the
resulting error and verify rendered samples in tests.

## Optional capabilities and fallbacks

Effective support is the intersection of authored preference, advertised target
capabilities and runtime availability. Keep the behavior identical across profiles.

| Condition | Compilation | Playback |
| --- | --- | --- |
| Browser supports motion/audio | Include chosen bounded frame sequences/cues | Normal playback; audio still needs user gesture |
| Motion disabled or unsupported | Omit optional motion frames/smoothing | Snap to current result; keep live signal updates |
| Reduced-motion preference | Retain a valid static endpoint path | Suppress motion without altering control semantics |
| Audio disabled, unsupported or wrong format | Omit audio and report reason | Visual behavior remains complete |
| Muted, locked, hidden or disconnected browser | Artifact may still contain supported cues | Drop cues; do not replay them on unlock/reconnect |
| Visual/memory/transfer budget exceeded | Fail with exact resource/quality diagnostics | Keep previous admitted publication active |
| Capability profile changes | Recompile the affected variant and revalidate | Switch only after complete candidate admission |

ESP32 remains in [feature #161, Backlog](https://github.com/opensimgear/cockpit/issues/161).
This feature adds synthetic silent/static/low-budget tests and target-contract
readiness. It does not enable pairing, assign widgets to ESP32, or claim physical
device support. ESP32 work separately qualifies actual advertised formats, display
sizes, RAM/flash, decode throughput, frame rate and audio hardware, then runs the
same compiled-asset scenarios on physical screens. “Powerful enough” must become
measured capability/budget values, not a check for the device name.

## Existing implementation: reuse and change

| Component | Action |
| --- | --- |
| `packages/widget-compiler/src/types.ts`, `source.ts` | Source v3 and strict validation; move SVG-specific parsing/normalization into new modules |
| New compiler modules: `svg-import.ts`, `svg-parts.ts`, `bindings.ts` | Bounded import, part identity, property resolution, normalized SVG generation |
| `packages/widget-compiler/src/compiler.ts` | Replace primitive-scene SVG generation; retain resvg and tile/media packaging |
| `packages/widget-runtime/src/behavior.ts`, `player.ts`, `panel.ts` | Reuse typed behavior, frame playback, smoothing, audio policy and panel composition |
| `src/lib/GenericWidgetDesigner.svelte` | Replace monolith with creator shell and smaller artwork/bindings/behavior/effects/test components |
| `src/lib/GenericSceneNode.svelte`, drawing helpers | Retire obsolete primitive editor; preserve useful history/fence/selection patterns |
| `src/lib/compiled-companion-library.ts` | Version source/library contracts; preserve immutable revisions and exact-build identity |
| `crates/companion-service/src/publication/widget_source.rs` | Native source v3 admission and TS/Rust parity fixtures |
| `packages/widget-runtime/src/artifact.ts` and `crates/companion-service/src/publication/compiled/artifact.rs` | Both currently pin compiler/rasterizer identities; coordinate version and cache admission changes |
| `src-tauri/src/widget_library.rs` | Update native/MCP authoring contract and examples |
| `src-tauri/src/widget_cutover.rs` | Versioned, idempotent replacement of obsolete widget sources/assignments |

Keep `osg-raster-widget-1` if deployed structure and semantics remain compatible.
Bump compiler identity for the new frontend and record importer version in source
identity. Do not mutate old content-addressed assets or reuse a stale compile hash.

## Milestones and acceptance gates

Milestones are sequential delivery gates; review each in the running dev Cockpit.
Estimate calendar dates after M0 proves importer fidelity and complexity. Do not
merge a partial creator as the default before the capsule end-to-end gate.

| Milestone | Deliverables | Exit gate |
| --- | --- | --- |
| **M0 — Contract and compatibility spike** | Source v3 ADR/schema, supported SVG/export profile, limits, ID/transform ownership, text policy, representative export corpus | Nested transform rotation, gradient/clip and capsule render deterministically; preview/compiled differences understood; importer dependency/version chosen |
| **M1 — Import and parts** | Parser/normalizer, dimensions, diagnostic report, read-only hierarchy, canvas selection, aliases, transactional reimport | Supported assets import faithfully; unsafe/unsupported cases fail clearly; same-ID reimport keeps mapping; removed parts require repair |
| **M2 — Bindings and capsule vertical slice** | Signal mapping UI, transform/visibility/color/reveal semantics, hit regions, source v3 compilation, imported Toggle pilot | Import → configure requested/confirmed → compile → Try → save → panel → running browser works; thumb changes immediately and confirmation stays independent |
| **M3 — States and effects** | Simple/advanced behavior UI, state presentations, transitions, timing/easing, interruption, sound-cue controls | Reversal/cancel/reset/disconnect preserve immediate outputs; every valid state has a static result; disabled effects produce equivalent behavior |
| **M4 — Gauges, quality and generality** | Fresh radial/linear RPM and three-state fixtures, continuous sampling, target-pixel error bounds, budget preflight, final build report | All fixtures use the same binding/compiler path; no type-specific rendering; over-budget families fail before raster work; actual compiled pixels meet accepted quality |
| **M5 — Reimport and authoring durability** | Draft autosave, undo/redo, replace/diff/remap, versioned export/import, cancellation/cache fences, finite parameter variants | Reimport and restart preserve source/configuration; broken references cannot publish; old compilation cannot become current after an edit |
| **M6 — Cutover and release** | New default creator, source/library reset, old editor removal, native/MCP docs, parity fixtures and release acceptance | Clean install and upgrade work; one Toggle product seed; all tests/coverage/build pass; running Cockpit and browser verified at deployed dimensions |

M0 defines shared contracts; importer tests and creator-shell UI can proceed in
parallel afterward. M2 is the first reviewable product slice. M3/M4 build on it.
Reimport semantics start in M1; M5 qualifies the whole persistence lifecycle.
ESP32 milestone(s) are separate backlog work after the browser creator is stable.

## Cutover and migration

Do not migrate the existing widget catalog or preserve primitive-editing workflows.
Create the capsule artwork as a checked-in SVG with stable IDs and recreate only
its behavior/bindings. A one-time development export can help obtain its artwork;
it is not a supported source-v2 conversion feature.

Add an idempotent library/schema cutover that resets obsolete widget sources,
drafts, compiled publications and dependent widget assignments. Retire stale
references without deleting unrelated profile data, pairing, trusted certificates
or listener configuration. Test clean install, upgrade, repeated startup and
interruption/recovery. Reject unsupported old imports with an explicit version
message. Retain a Git rollback point and a local pre-cutover store snapshot for
development recovery; do not build a permanent compatibility layer.

Remove obsolete drawing UI and tests after new coverage replaces their relevant
behavior. Update documentation and MCP tools together so neither continues to
teach primitive construction. Existing uncommitted ESP32 work is outside this plan.

## Testing and release evidence

| Layer | Required evidence |
| --- | --- |
| Import | Real editor exports; malformed XML; entities/scripts/URLs/events; duplicate/missing IDs; cycles; expansion/path/image limits; supported-style fidelity |
| Geometry | Non-zero viewBox, nested transforms, pivots, rotated/scaled parents, reveal clips, masks, gradients, alpha, fit/letterboxing and hit-coordinate alignment |
| Bindings/behavior | Type/domain validation, property conflicts, every state, requested/confirmed disagreement, stale/unknown feedback, timers, rapid reversal, neutral outputs |
| Compilation | Deterministic golden pixels, exact hashes, complete asset closure, static/motion combination bounds, target-pixel sampling error, cancellation and stale-cache rejection |
| Reimport/storage | Same IDs, rename/delete/remap, group restructuring, dimensions changed, undo/cancel, autosave/restart, version rejection and idempotent cutover |
| Capability | Full browser, synthetic no-motion/no-audio/low-memory, reduced motion, browser audio lock/mute, over-budget refusal and active-publication retention |
| Actual Cockpit/browser | Import → configure → compile → Try → save → assign → browser, plus edit/reimport/recompile; compare deployed pixels at native target dimensions |
| Native contracts | TS/Rust validation parity, malformed source/artifact corpus, compiler identity and generated fixture checks, native/MCP import/export |
| Future ESP32 backlog | Real device capability report, transfer/reconnect, flash/RAM/scratch/frame timing, orientation/alpha/color, absent-audio and disabled-motion behavior |

Run relevant importer/compiler/runtime tests and coverage locally before each
atomic push. At release run `pnpm widgets:coverage`, `pnpm check`,
`pnpm browser:test`, relevant companion/native admission tests and production
`pnpm ui:build`, plus the adapted live-native designer acceptance suite. Preserve
existing coverage gates; measure new import/binding modules explicitly so unrelated
coverage cannot hide gaps. Documentation-only changes use link/schema/example
checks; no product coverage claim follows from validating this concept preview.

A release is done when the capsule and three generalization fixtures complete the
actual browser workflow; supported imports survive reimport; every advertised
fallback is exercised; the old drawing workspace is absent; and ESP32 remains
explicitly unavailable until its separate backlog acceptance is completed.

## Decisions to resolve during M0

- Exact parser/normalizer dependency and bounded CSS/use expansion strategy.
- Initial supported SVG export corpus and documented fidelity limits.
- Per-document element/path/image limits based on that corpus and measured import cost.
- Whether masks enter v1 or receive an explicit deferred-feature diagnostic if
  deterministic preview parity cannot be demonstrated.
- Exact draft-store/source-library version numbers and coordinated native admission.
- Quality-bound method for complex nonlinear expressions; narrow the allowed
  mapping set if a meaningful conservative error cannot be computed.

These are implementation decisions with explicit tests, not reasons to expand the
product back into a general SVG drawing or animation editor.
