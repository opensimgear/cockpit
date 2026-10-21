# Coordinated canvas routing and minimum segments

The running native dev Cockpit was tested with a disposable copy of
**DCS Feature Lab - AH-6J + F/A-18C**. Its Base canvas contains 42 blocks and
73 connections. The original saved profile was preserved.

Both fully smooth Bezier and rounded perpendicular paths were sampled every
two world pixels against the actual block bounds. All 73 connections remained
visible, avoided blocks and met their physical port centers. Mouse drags kept
unrelated wire paths unchanged. Moving blocks and reroute points, canceling,
Undo/Redo, native Save, reopening, deleting points and resetting manual paths
were exercised through the running UI.
Final drag-update p95 was 13.9 ms for smooth and 12.7 ms for perpendicular
arrows; drop-to-render was about 55 ms and 33 ms respectively on this machine.

The follow-up close-up checks cover all 11 wires entering Virtual auxiliary.
Automatic fans now use their actual approach corridors, erase retracing and
self-crossing loops, and merge toward the port without overshooting its height.
Both ends use this normalization. Authored route points remain separate from
automatic cleanup.

Smooth routing tries one obstacle-clear cubic first, then merges consecutive
pieces with a bounded minimum-piece search. Surviving joins retain their exact
tangents. Parallel wires retain a separation station, and an entire-chain
self-intersection check rejects unsafe merges. The live Base canvas now uses
116 cubic pieces, compared with 277 in the previous committed rendering;
48 of its 73 wires use a single cubic. The isolated obstacle fixture needs two
pieces instead of four. Perpendicular routing removes redundant legs with
whole-path clearance checks while preserving physical leads and fan gates.
Isolated aligned, offset and centrally obstructed ports use one, three and
five legs respectively. Group drag translates committed cubic geometry exactly;
single-endpoint previews retain direct curves and distinct parallel lanes.

Native acceptance physically pans and zooms to the input at more than 120%,
captures each style, and samples each complete incoming SVG path every 0.5 world
pixels. A spatial grid checks near-port self-intersection and retracing. The
obstacle-free terminal strip is derived from actual block bounds; monotonicity
starts at first entry and includes any later excursions. All 11 incoming wires
have zero horizontal reversal, vertical movement away from the input, or exits
from that strip. This catches the larger detours missed by checking only the
final few pixels. Both close-up screenshots were also visually inspected.

Actual SVG readability measurements use the same ten-pixel sampling before and
after, excluding the first and last 24 pixels shared at port attachments:

| Style | Crossings before / after | Shared length before / after |
| --- | --- | --- |
| Smooth | 617 / 415 | 3,850 / 1,479 px |
| Perpendicular | 345 / 266 | 17,186 / 13,509 px |

These sampled rendering metrics differ from the exact orthogonal routing-channel
metrics. Counts describe this profile and geometry, not a guarantee that arbitrary
manually placed diagrams can avoid every crossing.

The table compares the original routing baseline with the final rendering.
Automated coverage includes deterministic fan ordering, a dense 100-signal fan,
manual route recovery after obstruction, stable incremental routing, and moving
an obstacle onto a broad spline away from its orthogonal routing guides.
Adding and reconnecting parallel signals also rebalances peers sharing physical
ports so existing arrows remain selectable. The complete native graph-construction
and wire-editing acceptance test passed.

Checks passed: 94 canvas tests, 254 profile-workspace tests,
Cockpit and browser-companion type checks, and both production UI builds.

Run live acceptance against an already-running native dev Cockpit:

```powershell
$env:OSG_NATIVE_CDP_URL='http://127.0.0.1:9227'
pnpm profiles:routing:native
```

Optional `OSG_ROUTING_BASELINE_PATH` points to a JSON capture containing the old
rendered paths; when provided, acceptance also prevents readability regressions.
Final local screenshots and detailed timing evidence are in
`.research/profile-routing-native/1792607968544/`. Native construction and editing
evidence is in `.research/profile-wiring-native/1792608026579/`.

The native profile crate has one pre-existing failing test:
`complete_rig_retains_offline_blocks_and_changed_contracts_but_cannot_reuse_delivery_tokens`.
It fails with `Unknown persistent resource identity` at `complete_rig.rs:298`,
identically in an isolated copy of the unchanged HEAD sources. The remaining
153 native profile tests, including all eight editor-metadata tests, pass.
