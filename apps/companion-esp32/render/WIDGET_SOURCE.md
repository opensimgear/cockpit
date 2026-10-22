# Portable widget source admission

`widget_source.c` admits one shared `WidgetDefinition` and its instance property
overrides into private C runtime data. It is not a new publication format. The
caller supplies the already checked document asset descriptors, the destination
arena and explicit limits for total layers, components and arena bytes.

The decoder validates strict field shapes, typed properties/references, port and
interaction contracts, hit areas, typography, images, gauges and all eight state
appearances. State paint resolution uses base → normal → selected state, matching
the shared TypeScript model. Each instance owns its resolved geometry, strings,
properties, port contracts and appearances. Freeing the parsed JSON cannot change
the admitted instance. Input and feedback logical IDs are mapped by the outer
publication admission layer, not inferred from definition port names.

The arena has no hidden allocation. Its storage must be aligned for `max_align_t`.
Failed admission restores its counters and returns no candidate; prior objects
remain byte-for-byte unchanged. Diagnostics identify the definition, layer and
field. Resource diagnostics also report the required and available capacity.
Only an immutable candidate may later be handed to the display task. This module
does not render, sample feedback, grant input authority or perform transport I/O.

The portable oracle in `scripts/widget-source-fixtures.mjs` derives 15 positive
cases and 5,965 mutations from the actual shared catalog and source validator.
`test/widget_source.py` compares target admission decisions and every resolved
state/layer with that oracle under ASan/UBSan. It also verifies rejected-candidate
rollback, duplicate fields, malformed UTF-8, owned strings after JSON destruction
and independent instances of one definition. Local coverage is 100% lines and
functions, 89.30% branches. The 15 examples occupy 34,753 bytes for 52 layers in
the 64-bit native harness; that is not an MCU resource measurement.

`widget_state.c` projects admitted port samples into the same visual state,
enable gate, confirmed indicators and gauge fractions as browser/Android. It
retains simulator origin independently of provenance, combines source age with
elapsed receive time without clock arithmetic overflow, and preserves the shared
disconnected/disabled/warning/pressed/unknown/stale/active precedence. Its enable
gate is only a necessary input condition: it cannot arm a session or emit input.
The caller must supply an immutable snapshot of feedback after logical identity,
type, unit and range validation. `test/widget_state.py` passes 1,274 comparisons
against the shared projection plus invalid-number and clock-boundary checks,
with ASan/UBSan and 100% line/function, 88.97% branch coverage.

`widget_draw.c` now consumes those admitted definitions and sample snapshots
directly. It implements ordered rectangle/ellipse/line/text/image/indicator and
linear/vertical/radial gauge layers, independent x/y scaling, clipping, inherited
state paint and group opacity. Rounded corners and radial gauges are evaluated
in authored coordinates, so stretching a widget also stretches its decoration.
The existing RGB565-plus-alpha image-cache format is accepted through a pinned
bitmap lookup callback. Missing assets, unsupported text and strip/resource
limits fail with the affected layer; they are never silently omitted.

Alpha layers use one partial ARGB8888 strip, limited to eight rows and 1,024
columns, with an explicit memory check. That strip is drained before the next
allocation. TinyTTF bitmaps likewise remain owned until their queued image tasks
finish: LVGL's no-OS `wait_for_finish` alone does not drain queued tasks. Empty
glyph bitmaps retain their text advance without becoming invalid image draws.
The pinned single synchronous software draw unit is enforced at compile time.

Live numeric text uses exact binary64 rounding at 0–4 decimal places. The 1,353
shared-code text fixtures include decimal ties, negative zero, subnormals and
large magnitudes. Values with magnitude at least 1e21 fail explicitly, as do
overlong strings or missing font glyphs. This is a target display limit, not a
restriction on portable browser/Android authoring.

The real-LVGL harness now admits twelve source instances and executes nine
feedback/interaction scenarios, rather than consuming pre-resolved drawing
commands. All 108 appearances match actual WidgetView captures in the running
dev Cockpit within two physical pixels and one RGB565 channel step for solid
colors. Edge checks allow rasterizer alpha blends and require at least 95% of
each significant color footprint in both directions. Source layer regions avoid
mistaking glyph AA shades for unrelated instrument paint. The comparison models
the catalog's alpha bezel over its canvas and rejects changed compositing until
the fixture is updated. Negative controls reject missing, displaced or recolored
features. This proves the catalog fixture, not every possible authored design.
Renderer tests include prepared-image alpha, vertical gauges, allocation budget
rejection, unsupported glyphs, board failure and recovery. ASan/UBSan pass, with
100% line/function and 85.61% branch coverage for `widget_draw.c`.

`core/widget.h` now separates resolved runtime types from JSON and LVGL. A
component binding pins its definition and maps each output ID/channel to one of
32 bounded feedback slots. Multiple ports no longer depend on widget indices.
Samples own the shared display prefix of up to 120 UTF-16 units of UTF-8 and retain receive validity and
simulator origin independently of provenance. The existing versioned feedback
envelope maps those ports and validates a whole immutable frame before a second
bounded pass commits it. This removes runtime-sized stack copies while keeping
failed frames atomic, duplicate counters inert and unbound aliases untouched.
Component text validates the entire host-supported 256-unit wire string before
retaining that prefix. Browser/Android and device projection stop before
splitting a Unicode code point at the display boundary. Unit metadata validates
the full shared 120-unit bound; existing fixed-widget text rules remain.

The existing interaction/lease queue now carries boolean and numeric values.
Momentary, toggle and selector interactions share its ordering, neutral baseline,
epochs and failure handling. Pointer cancellation releases the current component
owner; unrelated latched values remain. Fresh enable-gate loss neutralizes that
widget, and Stop, page/publication changes and source loss neutralize the source.
Local page navigation grants no gameplay input authority. Rendering does not
enqueue gameplay events.

The actual shared browser/Android controller generates 51 deterministic trace
steps, with 24 ordered changes, consumed by `test/component_test.c`. Tests also
exercise bipolar numeric queue entries, invalid port bindings, full-frame
rollback, origin checks, duplicate/delta snapshots, UTF-8 limits and independent
freshness damage. Fatal ASan/UBSan checks pass. Core runtime coverage is 100%
lines/functions and 78.86% branches; portable widget contracts are 100%
lines/functions and 75.90% branches. A native runtime snapshot is 17,456 bytes;
this is not an MCU heap or task-stack qualification.

`network/panel_source.c` now admits complete schema-3 publication candidates.
It verifies the exact canonical source hash, strict source shape, all definition
revisions (including unused ones), pinned instance properties, primitive arity
and types, one-to-one logical target bindings, normalized page items, groups,
safe insets, viewport regions and asset digests. Legacy widgets and portable
components can share a candidate. Group IDs/labels/page membership are retained
in owned metadata; prepared image references retain the existing hash identity.
The returned scene owns its entire image/model in caller-provided aligned memory.
On failure counters rewind and earlier admitted bytes remain unchanged.

The five panel cases contain 46 instances, including mixed legacy/components,
independent controls, editorial metadata, prepared artwork references and a
display-only panel. All 4,239 mutations are checked against the actual shared
TypeScript document validator. Valid source outside this target's supported
features is explicitly rejected; the evidence log names each such exception.
The 51-step control trace now uses this full authenticated-source decoder,
instead of manually constructing the runtime bindings. ASan/UBSan pass; panel
admission coverage is 100% lines/functions and 86.35% branches. The catalog
candidate occupies 35,777 native arena bytes, including owned metadata/model;
this does not qualify MCU memory or task stacks.

JSON heap/reply/node limits can be configured only with no outstanding JSON.
The original default limits remain until a caller opts in. Rejection reports
the exact field, required and available document/arena/layer/component/image or
JSON resource count. Syntax and publication-identity failures remain distinct.

Candidate admission, the component model and renderer are compiled into firmware
but are not yet connected to live schema-3 adoption. The active schema-2 delivery
path, resource negotiation, image cache and raster main loop remain. Integration
must still fence/reuse candidate banks, invoke bitmap preparation for their
admitted bindings, preflight
supported font glyphs, select board-specific memory limits and connect dynamic
state without changing ownership, neutral baselines or leases. Group drawing and
page-background support also remain to be integrated; page backgrounds currently
fail explicitly. Browser and Android source authoring limits remain independent
of these device budgets.

Portable layer artwork now uses the existing authenticated `image-chunk` path
with an optional `layerId`. The host resolves only that layer in the instance's
pinned definition revision, using its resolved property overrides. Replies echo
the layer ID; the firmware rejects a mismatched echo before writing the cache.
The session must still be current and disarmed. No asset selection or request
creates input authority. Existing legacy image requests retain their shape.

The device preparation code collects component layers and legacy images into one
bounded list. Content hash plus exact prepared dimensions remain the cache key,
so aliases across layers/instances share pixels and differently sized uses stay
distinct. Existing cached candidate images are pinned before allocating misses;
the displayed owner's pins remain until its renderer fence permits release.
Tests cover wrong-layer replies, cancellation, count/dimension/total-byte limits,
immutable active pixels and progress totals. LVGL can look up prepared pixels
only in the supplied instance binding, with exact asset/dimension matching.
All 116 native host tests, device client/cache regressions, full-panel admission
and both real LVGL sanitizer harnesses pass. This supplies the preparation path;
it does not yet enable live schema-3 adoption or qualify physical hardware.
