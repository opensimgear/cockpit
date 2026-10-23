# Generic widget designer with compiled visuals

Status: proposed implementation plan. Date: 23 October 2026.

This is a deliberate replacement of the current widget system. The capsule
toggle is the only product pilot. Existing widgets, saved widget definitions,
and their old rendering contracts do not need compatibility or migration.
Linear/radial RPM gauges and a three-state control are later generalization
fixtures built from scratch, not a requirement to port the old catalog.

## Non-negotiable architecture

1. Layers, vector geometry, font layout, state artwork and design effects are
   authoring inputs. They remain in Cockpit; companion targets do not receive
   or render the authoring scene.
2. Compilation produces immutable, target-sized raster visuals plus bounded
   behavior/frame-selection tables, hit regions and optional media.
3. Browser and ESP32 production runtimes select and display compiled widget
   frames. Try mode and target preview use that same artifact. The Design canvas
   may render source layers interactively while an edit is in progress.
4. Requested controls, game-confirmed values and feedback quality are separate.
   Logical outputs change immediately. Animation and audio never grant input
   authority or delay game controls.
5. Device support comes from actual versioned capability reports and measured
   budgets. No audio support means no audio; no motion support means final-frame
   snap. Disabling smoothing must not disable live gauge updates.
6. Reuse current authentication, pairing, sessions, neutral baselines, semantic
   input transport, immutable publication identity and active/candidate fences.
   This is a raster-asset compiler, not a revival of the retired #77 vector scene
   ABI, expression ABI or separate render-prepare transport.

## Source and deployment contracts

```text
Editable source, stored in Cockpit
  parameters + scene/layers + typed signals + bindings
  optional state machines + motion definitions + sound cues
  bounded visual domains + quality/fallback policies
                         |
       compile for resolved instance parameters, pixel size,
       device capabilities, quality settings and compiler version
                         |
Immutable compiled artifact
  manifest + semantic tables + hit regions + visual lookup maps
  flattened raster frames encoded as deduplicated pixel tiles
  optional frame sequences + compiled sound assets + fallback report
                         |
Browser / ESP32 runtime
  input and feedback -> bounded behavior -> select frame -> blit pixels
  semantic cue -> optional audio playback
```

The artifact contains no layer tree, SVG, vector drawing commands, runtime font
layout or references back to authoring node geometry. Tiles are a storage and
pixel-copy encoding of finished frames: they do not become renamed design
layers. Panel composition may place complete widget surfaces over a compiled
background, with explicitly supported pixel alpha blending. Widget source
composition never runs on the target.

Every artifact binds its source hash, behavior fingerprint, compiler/rasterizer
version, resolved parameter hash, target capability/profile hash, exact pixel
geometry, quality policy, asset digests and complete asset closure. Label, color,
size or baked parameter changes invalidate the affected compiled variant.
Identical variants and pixel tiles can be shared across instances.

Behavior changes that affect emitted controls change semantic identity. Raster,
motion and sound updates remain presentation changes only when they do not
change input behavior. Artifact validity never authorizes gameplay input.

## Compilation policy and initial limits

The first implementation uses complete flattened widget frames, not runtime
needle/label/shape composition. The compiler:

1. Validates source, typed bindings, behavior and declared visual domains.
2. Resolves the actual instance parameters, widget pixel size and target profile.
3. Enumerates required discrete appearances and finite samples of continuous
   visual inputs. It reports conservative state/value combination counts before
   rasterization and deduplicates equivalent appearances afterward.
4. Rasterizes final appearances and, when enabled, transition frames using pinned
   fonts, shaping/rasterization settings, color handling and effect padding.
5. Packs/deduplicates fixed raster tiles, emits frame-to-tile maps and optionally
   bounded compressed chunks. V1 should start with a small, streaming-friendly
   tile format and RGB565 plus an explicitly negotiated alpha representation.
6. Compiles sound assets to target-supported formats, if enabled.
7. Emits resource/accuracy diagnostics and an immutable complete artifact.

A bounded structural source format and bounded semantic tables are permitted;
arbitrary scripts, a general drawing instruction stream and unbounded decoding
are not part of the deployed contract.

Finite assets cannot represent unlimited independent values. Continuous visual
inputs therefore declare a range, sampling policy and permitted visual error.
For RPM, a sample maps to a precomposed bar/needle frame. The target selects a
frame from the live value; it does not draw a gauge. The preview shows the exact
quantization and error accepted for the selected target. Threshold behavior uses
the original validated signal, not the rounded visual sample.

For scale: 128 full 320 x 96 RGB565 frames require 7,864,320 bytes (7.5 MiB)
before alpha and metadata. Deduplication may help substantially but is not a
guarantee. Current flash partitions, metadata overhead and active/candidate
pinning make a resource spike an early release gate.

V1 supports fixed labels, finite enum labels and declared finite value domains.
Unrestricted live text and unconstrained combinations of independent numeric
readouts are out of scope. Reject an unbounded domain or an artifact exceeding
its budget. Do not introduce a hidden glyph/layer renderer or host frame stream
to make it appear supported. Either future approach would require a separate,
explicitly approved capability and design decision.

## Milestones and dependencies

Each milestone is an independently reviewable change with checks and evidence.
Software completion and physical qualification are tracked separately.

### M0 — Contracts and resource feasibility

Dependencies: none.

- Specify versioned authoring, artifact, capability-report and semantic-event
  contracts. Choose a new format version; do not reinterpret old publications.
- Define machine initialization, event ordering, guards, reset behavior,
  presentation ownership and requested/confirmed/quality separation.
- Specify tile/frame encoding, dimensions, alpha, color order, compression limits,
  hashes, sampling rules and exact resource accounting.
- Prototype capsule OFF/ON, pressed and independent confirmation appearances
  at actual Sunton and Waveshare widget sizes. Include static and motion variants.
- Add synthetic linear-fill and radial-needle raster sweeps as compiler resource
  benchmarks, not additional product pilots or migrated widgets. Use initial
  visual-error targets of at most one native pixel at the linear endpoint and
  two native pixels at the radial needle tip at each tested widget size. Record
  the chosen size/domain and prove a useful static variant fits both targets.
  If it cannot, revisit encoding, authored presentation or product scope before
  fixing the artifact format; capsule-only feasibility is insufficient.
- Inventory actual partitions and cache limits from current source/firmware;
  reconcile stale documentation rather than assuming the older larger cache.
- Account for compressed flash, metadata/alignment, active plus candidate data,
  decode scratch, resident RAM, transfer bytes and worst-case changed pixels.
- Write the capability/fallback matrix below into executable contract fixtures.

Exit gate: static capsule and synthetic gauge benchmark artifacts fit each
target's available budget at the declared accuracy, the decoder can stream within
bounded scratch memory, and any required partition change is identified explicitly.
No full designer investment before this gate. Physical resource claims remain
provisional until M4/M8.

### M1 — Generic behavior and capsule source

Dependencies: M0.

- Replace fixed widget-state enums with bounded named state machines and typed
  signals/variables. Start with flat machines and independent regions; defer
  general nested/history semantics until a concrete fixture requires them.
- Define a small versioned expression/operator set with type/range validation,
  deterministic transition priority and maximum evaluation work.
- Build a pure reference evaluator with an injected clock and ordered events.
- Recreate the capsule from scratch: requested OFF/ON; independent confirmed
  OFF/ON/unknown/stale/disconnected; pressed/available state and hit region.
- Use generic bindings/state rules; no capsule IDs or special cases in runtime.
- Specify restore/reconnect behavior: requested input resets to neutral; feedback
  is unknown until fresh; restored snapshots suppress historical effect cues.

Exit gate: recorded traces prove immediate requested toggling, disagreement with
confirmation, stale/disconnected states, cancellation and neutral reset. Invalid
or cyclic unbounded behavior is rejected deterministically.

### M2 — Deterministic visual and media compiler

Dependencies: M0 and M1.

- Implement compilation outside the companion runtime, callable from Cockpit and
  headless tests. Keep a pure contract/model boundary separate from rasterization.
- Implement canonical inputs, pinned fonts/rasterizer, target-sized rasterization,
  clipping/effect extents, color conversion and alpha conventions.
- Produce complete frame families, tile dictionaries/maps, bounded decode data,
  visual lookup tables and content-addressed assets.
- Implement incremental cache keys and deduplication across states/instances.
- Implement explicit quality/fallback selection and diagnostics identifying the
  offending state, signal domain, asset, dimension or resource estimate.
- Add cancellation/progress. An edit while compiling invalidates stale results;
  old asynchronous completions cannot become the published current artifact.
- Define the sound compilation interface now; actual playback ships in M6.

Exit gate: repeat compilation produces identical artifacts; capsule frames match
golden pixels at both target resolutions; malformed/oversized inputs fail before
deployment; compiler reports active/candidate resource totals and selected
fallbacks. Packet inspection shows no source layers in the target artifact.

### M3 — Browser capsule vertical slice

Dependencies: M1 and M2.

- Add the compiled artifact player to the shared companion UI package. Browser
  production and Try mode use frame maps/pixels, not WidgetView source rendering.
- Integrate source edit -> compile -> save -> assign through the running dev
  Cockpit. Native service validates the artifact and complete asset closure.
- Reuse authenticated chunk delivery/cache machinery with versioned manifest and
  raster/media payload support. Do not create a second pairing/session protocol.
- Keep hit regions/input semantics separate from pixels. Resizing and parameter
  changes recompile; pixel assets cannot redefine control IDs or authority.
- Provide build status and an artifact inspector showing size, capabilities,
  fallback choices, source revision and compile identity.

Exit gate: author capsule, compile, assign to the actual browser companion and
click OFF/ON/OFF. Thumb responds immediately; game confirmation changes
independently. Cold/warm cache, reconnect, page hiding and replaced publications
pass without replayed input or stale compilation activation.

### M4 — Static compiled visuals on ESP32

Dependencies: M2 and M3. This is the embedded feasibility gate before rich effects.

- Implement strict artifact admission, C behavior/frame selection, tile lookup,
  bounded decode and complete-widget pixel blits using existing display adapters.
- Replace source-layer widget admission/rendering for this new format. Production
  playback must not parse authoring layers or invoke per-layer vector drawing.
- Reuse dirty regions, pixel caches, active/candidate banks, readers and display
  fences; validate the entire candidate before atomic activation.
- Complete durable artifact activation: persist manifest plus complete verified
  asset closure, pin active/candidate assets, and restore verified visuals after
  cold boot with inactive input/unknown feedback. Existing portable store tests
  are useful infrastructure, not proof that this integration already exists.
- Version capability reports to advertise actual supported subsets and limits.
  Device admission checks real available resources in addition to host estimates.
- Ship an explicit engine/firmware update for the new artifact contract. An old
  engine must reject the new format and offer the normal update path; it must not
  accept the package and silently use the former source-layer renderer. Subsequent
  compatible visual packages must work without reflashing.
- Measure capsule transfers, flash/cache occupancy, scratch/resident memory,
  display work and touch latency on both physical board types.

Exit gate: static capsule works on Sunton and Waveshare over USB and Wi-Fi/WSS;
new artwork activates with the same installed firmware binary. Partial/corrupt/
over-budget candidates retain the valid active artifact and its assets. Cold
boot restores pixels without restoring control authority. If physical boards are
unavailable, mark software complete and keep this hardware gate open.

### M5 — Generic designer and compiled Try mode

Dependencies: M3 and M4 software gates; physical M4 findings constrain defaults.

- Deliver Design, Data, Behavior, Motion, Sound and Try workspaces progressively.
  Only capsule is seeded in the product library at this stage.
- Design edits source layers/groups/parameters. Data declares ranges and visual
  bindings. Behavior edits named states/events/guards with diagnostics.
- Try runs the actual behavior evaluator and compiled player. Expose requested,
  confirmed, pressed, enabled, connected and feedback quality independently.
- Show target-specific sampling, memory/flash estimates, compilation progress,
  and full/static/silent/reduced presentation choices before assignment.
- Allow source preview during editing but label it distinctly; Try/export/assign
  require a current successful compilation. Never deploy a stale preview artifact.
- Follow existing UI rules: contextual labels such as New/Save and icons on
  buttons; keyboard operation, undo/redo and clear validation errors.

Exit gate: a user can rebuild the capsule visually without JSON or source edits,
simulate disagreement/staleness and deploy the exact compiled preview to browser
and ESP32. No extra widget-specific runtime code is added.

### M6 — Compiled animation and optional sound

Dependencies: M2-M5.

- Bake authored transition/timeline frames into the same asset pipeline. Devices
  play time-indexed frames; they do not interpolate authoring geometry.
- Declare interruption policies. Capsule uses reversible compiled movement:
  rapid reversal resumes from the nearest displayed progress frame. Other
  transitions require a compiled route or an explicitly authored snap policy.
- Define monotonic-clock sampling, bounded repeats/concurrency, dropped-frame
  behavior and stop-on-hidden/disconnect/replacement rules.
- Without animation, omit intermediate assets and snap to the correct endpoint.
  Semantic control events must be identical with and without motion.
- Compile/import sound assets with codec/duration/size limits. Attach cues to
  semantic edges, with cooldown, deduplication, voice limits and stop policy.
- Implement browser playback availability/mute handling and embedded audio only
  where an actual initialized driver/runtime advertises support. Unsupported
  audio produces no playback assets or calls; do not infer support from a board
  having a speaker connector.

Exit gate: browser capsule supports movement/click audio; static/silent profiles
produce identical logical output traces. ESP32 motion is enabled only after its
actual load test passes. An audio-capable embedded claim requires physical sound
testing; silent ESP32 qualification does not depend on adding an audio driver.

### M7 — Continuous and three-state generalization

Dependencies: M2, M5 and M6.

- Create new linear RPM, radial RPM and three-position control fixtures entirely
  through the generic designer. These are new capability proofs, not ports of
  the old widget families.
- Add bounded continuous visual sampling/frame lookup and publish the resulting
  error bound. Expose quantization and authored alternatives in target preview.
- Compile scalar gauge frame families. Smoothing advances through those existing
  samples over time and retargets from the currently displayed frame index; it
  does not interpolate geometry or bake every possible source/destination pair.
  Telemetry is transported as semantic values; frames are already local. No
  steady-state image streaming.
- Use original telemetry for redline thresholds/hysteresis and optional cues,
  independently of the selected raster sample.
- Exercise multiple independent regions so confirmation, warning and requested
  state do not require hand-authored Cartesian combinations. Compiler analysis
  still bounds and reports their actual flattened visual combinations.

Exit gate: all three fixtures have useful static variants running on browser,
Sunton and Waveshare without new runtime widget-type branches. Gauge endpoints,
full sweeps, declared native-pixel accuracy,
staleness and redline hysteresis pass. Static profiles still update every admitted
live value; an accuracy/storage conflict is rejected or explicitly resolved by
the author, never silently degraded.

### M8 — Destructive widget cutover and release qualification

Dependencies: M0-M7 and physical qualification below.

- Perform a one-time widget-library format reset. Discard old widget definitions,
  old panel widget instances/publications, old render assets and obsolete sample
  bundles. Do not write schema-2/schema-3 widget converters or preserve the catalog.
- Stop old widget sessions and neutralize their controls before reset. Remove or
  invalidate profile bindings to deleted widget endpoints and surface a concise
  migration report. Do not leave stale bindings connected to reused IDs.
- Preserve companion installations/pairings, listener trust material, non-widget
  profile resources, hardware drivers and unrelated application settings.
- On firmware/engine replacement, invalidate only incompatible content and
  sessions; preserve credentials/NVS/PHY and require a fresh neutral baseline.
- Seed only the new capsule. Use new source/control/artifact identities. Old
  widget imports/assignments are rejected clearly with instructions to recreate.
- Remove old fixed widget kinds/state enums, runtime layer renderers and their
  compatibility-only tests from browser and ESP32 production paths. Source-layer
  rendering stays editor-only. Remove temporary development switches/fallbacks.
- Update active docs/release matrices and mark superseded plans explicitly;
  preserve historical verification records as historical evidence.

Exit gate: clean-install and reset-install flows pass, old widgets cannot enter
the runtime, compiled artifacts remain independently verifiable, and all admitted
browser/physical ESP32 profiles meet the release gates. Hardware unavailable
means the physical gate remains open; a software client is not a substitute.

## Device capability and fallback contract

Capabilities are advertised by the actual engine/device and include versioned
behavior/expression support, raster formats/alpha/tile decoding, maximum asset
and frame dimensions/counts, flash/cache/decode budgets, playback concurrency
and update-work budgets, touch features, and optional audio format/voice limits.
Unknown support is unavailable. Limits apply to the aggregate panel and to
simultaneously pinned active/candidate artifacts, not only one widget.

| Capability situation | Compilation/admission outcome |
| --- | --- |
| Device lacks optional audio capability | Omit audio assets/cues; retain authored visual confirmation |
| Optional audio is muted or temporarily blocked at runtime | Silent operation, explicit playback status, no accumulated cue replay when restored |
| Animation unavailable or user requests reduced motion | Omit intermediate frames and use final frames immediately |
| Smoothing unavailable | Select current gauge sample directly; continue live updates |
| Motion budget supports a declared lower-rate variant | Compile that explicit variant and show the applied fallback |
| Sampling/resolution exceeds budget | Offer authored coarser/smaller variant within its declared tolerance, otherwise reject |
| Required behavior/visual format unsupported | Reject assignment and retain valid active content |
| Free memory/cache admission fails at activation | Reject candidate atomically; no partial replacement |

Audio support and playback availability are different contracts. Animation and
audio are optional throughout this release, not only in the pilot. Their absence
must not block required widget behavior. The compiler requires a usable static,
silent presentation; sound cannot be the only way to communicate an authored
condition. Never override user mute/browser policy or replay expired cues.

Performance capability values require measured conservative defaults. Neither
PSRAM presence nor an ESP32/ESP32-S3 model name establishes affordable animation.
Runtime overload may skip decorative frames or snap according to the declared
policy; it must preserve input servicing and existing health/lease limits.

## Testing and release evidence

### Shared model/compiler/player tests

- Deterministic source/parameter/size/profile -> artifact hashes and golden pixels.
- TypeScript reference and C runtime consume identical event/time/feedback traces
  and produce identical logical outputs, frame indexes and cue intents.
- State/event bounds, conflicting writes, loops, guard priority, timer overflow,
  sampling endpoints, non-finite/out-of-range inputs and quantization tolerance.
- Outline/effect padding, alpha edges, RGB565/color order, tile seams, dirty-region
  cleanup, frame maps, duplicate pixels and bounded compression/decompression.
- Corrupt manifests/assets/offsets/maps, missing closure, wrong profile/hash,
  allocation failure, cancellation and active/candidate cache fragmentation.
- ASan/UBSan for C admission, decoder, player, cache and replacement lifetimes.
- Same gameplay output trace with all optional effects enabled and disabled.

### Running dev Cockpit and actual browser

- Create/edit capsule -> compile -> inspect report -> assign -> OFF/ON/OFF.
- Separate requested/confirmed values, delayed/disagreeing confirmation and all
  unknown/stale/disconnected cases; no manufactured simulator confirmation.
- Recompile label/color/size, cancel a build, edit during build, replace a panel,
  reconnect with warm cache, reload from cold cache and reject stale completions.
- Rapid motion reversal, pointer cancellation/lost capture, keyboard activation,
  hidden/focus changes and freshly acknowledged neutral baselines.
- Audio locked before user gesture, mute, cue cooldown and no snapshot/reconnect
  replay; test a supported browser's real playback path, not only mocks.
- Compare target-preview pixels to compiled output at native physical dimensions.
  Confirm the production browser path does not fall back to source SVG/layers.

### ESP32 software and physical matrix

Run unit/native software tests throughout, cross-build both board profiles, then
qualify each physical combination independently:

| Board | Relevant constraints | Transport cases |
| --- | --- | --- |
| Sunton 480 x 320 resistive | No PSRAM, shared display/touch SPI, bounded scratch | USB and Wi-Fi/WSS |
| Waveshare LCD7 800 x 480 | PSRAM, RGB scanout/DMA fences, GT911 touch | USB and Wi-Fi/WSS |

For every combination record board, firmware, source, compiler, capability and
artifact identities. Capture photographs/video of capsule states, outline/alpha/
color quality and gauge sweeps. Verify corner/center touch, release/drag-out,
USB/Wi-Fi interruption, reconnect, cold cache, warm cache and same-firmware visual
updates. Power-interrupt manifest/assets/commit writes and retain either the old
complete artifact or the new complete artifact; never restore partial content.

Run at least a 30-minute loaded telemetry/touch/reconnect soak per combination,
including a maximum admitted panel and enabled optional motion where advertised.
Record maxima for pending-input age, render/present time, bus/flash bytes, input
queue/ACK age, heap/minimum/largest block and stack minima. Preserve existing
250 ms pending-input, 600 ms render-health and 1,000 ms ACK limits with measured
margin; do not invent an FPS pass threshold before the hardware baseline.

After activation, capture zero graphics/frame/image transfer during steady
operation: only semantic samples/events/heartbeats travel. Validate the silent,
no-animation and reduced variants with the same functional traces.

### Checks before each implementation push

Work on main for atomic changes; a multi-milestone implementation may use the
repository's documented big-feature exception. Use the actual running dev
Cockpit for behavior acceptance. Keep at least the current 71% per-metric,
per-module coverage floor and add focused correctness assertions for compiler,
admission and neutralization paths.

Current checks to extend/reconcile as legacy tests are replaced:

- `pnpm check`, `pnpm companion:check`, `pnpm companion:test`.
- `pnpm browser:test`, browser feedback coverage and `pnpm ui:build`.
- Rust companion-service/serial and impacted native Cockpit tests.
- ESP32 runners under `apps/companion-esp32/test/`, including `run.py`,
  `network.py`, `storage.py`, `lvgl.py`, transport and board suites.
- Native UI tests for widgets/browser/image cache, updated for compiled artifacts.
- New compiler/model/player contract, golden raster and target-resource suites.

Remove tests only when their old product contract is intentionally retired;
replace their relevant behavior coverage in the new system. Never claim a
physical gate from cross-builds, screenshots of software clients or a passing
host test alone.

## Implementation ownership

| Area | Existing boundary to evolve |
| --- | --- |
| Authoring schema, semantics, validation, revisions | `packages/companion-designer` |
| Raster/media compilation | New focused compiler module/package invoked by Cockpit and headless tests; no target layer dependency |
| Portable deployment/event/capability contracts | `packages/companion-contracts` plus Rust/C validation fixtures |
| Editor / compiled preview / browser player | `src/lib/WidgetDesigner.svelte`, `CompanionDesigner.svelte`, `packages/companion-ui`, `apps/browser-companion` |
| Host admission, publication and delivery | `crates/companion-service`, `src-tauri` companion integration |
| ESP32 decoder/player/admission | `apps/companion-esp32/network` and `render`; replace widget source/layer paths |
| Cache, durable activation and fences | Existing pixels/cache/content-store/scene-bank infrastructure, explicitly integrated and qualified |

The first end-to-end success criterion is deliberately narrow: one capsule
authored in Cockpit, compiled once per selected target variant, delivered as
finished pixels, and operated on browser and both ESP32 board types without any
authoring-layer execution on those targets.
