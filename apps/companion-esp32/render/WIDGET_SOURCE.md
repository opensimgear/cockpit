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

This decoder is compiled into firmware but is not yet connected to live
publication adoption. The existing schema-2 decoder, resource negotiation,
feedback/input path, image cache and raster main loop are unchanged. Integration
must still apply full-document/source byte limits, bind logical IDs/assets,
preflight supported font glyphs, select board-specific memory limits and connect
dynamic state without changing ownership, neutral baselines or leases. Browser
and Android source authoring limits remain independent of these device budgets.
