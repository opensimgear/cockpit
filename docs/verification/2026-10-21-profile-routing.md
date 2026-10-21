# Coordinated canvas routing

The running native dev Cockpit was tested with a disposable copy of
**DCS Feature Lab - AH-6J + F/A-18C**. Its Base canvas contains 42 blocks and
73 connections. The original saved profile was preserved.

Both fully smooth Bezier and rounded perpendicular paths were sampled every
two world pixels against the actual block bounds. All 73 connections remained
visible, avoided blocks and met their physical port centers. Mouse drags kept
unrelated wire paths unchanged. Moving blocks and reroute points, canceling,
Undo/Redo, native Save, reopening, deleting points and resetting manual paths
were exercised through the running UI.
Final drag-update p95 was 12.7 ms for smooth and 12.9 ms for perpendicular
arrows; drop-to-render was about 50 ms and 30 ms respectively on this machine.

Actual SVG readability measurements use the same ten-pixel sampling before and
after, excluding the first and last 24 pixels shared at port attachments:

| Style | Crossings before / after | Shared length before / after |
| --- | --- | --- |
| Smooth | 617 / 340 | 3,850 / 1,691 px |
| Perpendicular | 345 / 265 | 17,186 / 15,519 px |

These sampled rendering metrics differ from the exact orthogonal routing-channel
metrics. Counts describe this profile and geometry, not a guarantee that arbitrary
manually placed diagrams can avoid every crossing.

Automated coverage includes deterministic fan ordering, a dense 100-signal fan,
manual route recovery after obstruction, stable incremental routing, and moving
an obstacle onto a broad spline away from its orthogonal routing guides.
Adding and reconnecting parallel signals also rebalances peers sharing physical
ports so existing arrows remain selectable. The complete native graph-construction
and wire-editing acceptance test passed.

Checks passed: 39 geometry/preview/layout tests, 254 profile-workspace tests,
Cockpit and browser-companion type checks, and both production UI builds.

Run live acceptance against an already-running native dev Cockpit:

```powershell
$env:OSG_NATIVE_CDP_URL='http://127.0.0.1:9227'
pnpm profiles:routing:native
```

Optional `OSG_ROUTING_BASELINE_PATH` points to a JSON capture containing the old
rendered paths; when provided, acceptance also prevents readability regressions.
Local screenshots and detailed timing evidence are in
`.research/profile-routing-native/`.

The native profile crate has one pre-existing failing test:
`complete_rig_retains_offline_blocks_and_changed_contracts_but_cannot_reuse_delivery_tokens`.
It fails with `Unknown persistent resource identity` at `complete_rig.rs:298`,
identically in an isolated copy of the unchanged HEAD sources. The remaining
153 native profile tests, including all eight editor-metadata tests, pass.
