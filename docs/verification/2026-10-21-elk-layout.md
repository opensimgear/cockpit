# ELK Layered canvas trial

The explicit **Auto-layout** action now uses elkjs 0.12.0 and ELK Layered.
The engine runs lazily in a same-origin worker; layout is never run from a
pointer-move handler. The existing cached router and lightweight previews keep
dragging responsive. Both smooth cubic and rounded perpendicular styling remain
available.

The graph adapter sends actual block dimensions and fixed east/west attachment
coordinates. Signals sharing a card footer use the same physical port; named
transform ports keep their individual offsets. Complete results are validated
for node identity, dimensions, bounds, orthogonality, physical port leads and
block clearance before a single history edit applies positions and corridors.
Pending requests cannot overlap; errors leave the graph untouched. Results are
discarded after a document edit, vehicle-tab change or component destruction.

Generated corridors use optional `editor.wireRoutes[].automatic` metadata.
They remain automatic during rendering and temporary obstacle repair. Reshaping
a segment or adding a reroute handle makes that route authored. Native save and
reopen retain generated corridors without changing profile schema version 2 or
the compiled signal graph. This first integration uses a flat graph and retains
the existing fixed group frames and membership reconciliation.

ELK can bundle signals sharing physical ports. Automatic fan normalization and
bounded lane refinement separate their approaches. A final parallel-only pass
prioritizes a usable interval between long trunks, while preserving authored and
retained geometry. Both rendered styles are checked for an individually
selectable region on every signal, outside neighboring 18px hit strokes.
The extra overlap priority applies to ELK-generated wires, including corridors
replanned after a duplicate or obstruction; the final pass also covers distinct
named-port approaches. Existing same-port fan refinement keeps its previous scoring.

The running native dev Cockpit was tested with a disposable copy of
**DCS Feature Lab - AH-6J + F/A-18C**: 42 blocks and 73 Base-tab connections.
ELK completed the actual UI action in 1,751 ms, including worker startup, layout,
validation and rendering. One Undo restored the entire arrangement, Redo restored
ELK, and native Save/reopen preserved all 73 generated corridors. Hidden vehicle
positions and every binding remained unchanged.

| Rendered measurement | Prior recorded layout | ELK layout |
| --- | ---: | ---: |
| Smooth crossings | 415 | 72 |
| Perpendicular crossings | 266 | 44 |
| Smooth shared length | 1,479 px | 635 px |
| Perpendicular shared length | 13,509 px | 18,490 px |
| Smooth total wire length | 109,108 px | 64,647 px |
| Perpendicular total wire length | 121,245 px | 73,560 px |

These are sampled actual SVG measurements, excluding the 24px shared port lead.
Changing node placement makes the overlap measurement a layout tradeoff rather
than a fixed-coordinate routing regression: ELK's perpendicular paths share
more stems, but all 73 signals have an independently selectable region in both
styles. Fewer crossings and shorter total wire length remain acceptance gates.

Both close-up screenshots were visually inspected. The 11 inputs on Virtual
auxiliary have no self-intersections, retracing, approach reversals or excursions
after entering the obstacle-free terminal strip. Every wire clears every block
and meets its physical port center. Drag-update p95 was 24.7 ms for smooth and
20.7 ms for perpendicular; drop-to-render was 55 ms and 62 ms respectively.
Reroute insertion, dragging, cancellation, Undo/Redo, native Save/reopen, deletion
and Reset bends also passed for both styles.

The complete native graph-construction scenario also passed, including backward
connections into distinct named inputs, automatic layout, exact port selection,
transform insertion and global styling. Its evidence is under
`.research/profile-wiring-native/1792611965262/`. The 100-wire routing regression
retains 100 distinct paths in both styles and completes within its 2.5 s budget.
Canvas test files run serially so their unchanged wall-clock performance limits
measure routing without competing TypeScript loaders or ELK tests.
The buried-port performance case times both router calls before running its
unchanged SVG-sampling and clearance assertions.
The final checks passed: 114 canvas tests, 262 profile-workspace tests, native
editor-metadata roundtrips, both Svelte type checks and both production builds.

Live DCS evidence is under `.research/profile-routing-native/1792611920975/`.
The existing arrangement also passed unchanged at the recorded 415/266 crossings
and 1,479/13,509 px shared lengths for smooth/perpendicular styles, in
`.research/profile-routing-native/1792611876923/`.
Run against the already-running native dev Cockpit:

```powershell
$env:OSG_NATIVE_CDP_URL='http://127.0.0.1:9227'
$env:OSG_ROUTING_AUTO_LAYOUT='elk'
$env:OSG_ROUTING_START_GEOMETRY_PATH='tests/fixtures/profiles/feature-lab-geometry.json'
pnpm profiles:routing:native
```

The optional starting geometry is applied only to the disposable test copy; the
native demo's actual declarations and all 73 Base bindings must match the fixture.

ELK reference: https://eclipse.dev/elk/reference/algorithms/org-eclipse-elk-layered.html
and https://github.com/kieler/elkjs. The worker is a separate production asset;
the API and adapter are loaded only when Auto-layout is requested.
