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

The decoder and renderer are compiled into firmware but are not yet connected to live
publication adoption. The existing schema-2 decoder, resource negotiation,
feedback/input path, image cache and raster main loop are unchanged. Integration
must still apply full-document/source byte limits, bind logical IDs/assets,
preflight supported font glyphs, select board-specific memory limits and connect
dynamic state without changing ownership, neutral baselines or leases. Browser
and Android source authoring limits remain independent of these device budgets.
