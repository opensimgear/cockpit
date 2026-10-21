# Shared input and output trunks

The existing **Collapse parallel connections** preference now also shares trunks
between connections with a common physical output or input point. Named ports on
different rows remain separate. Logical signals and saved route metadata are
unchanged.

The renderer places a common stem and a vertical spine in a clear corridor, then
taps branches where their existing guides can reach the other blocks. Source and
destination buses can coexist on one connection. Reconnection is bounded and
uses existing guides rather than running ELK or a growing grid search. If a safe
shared path cannot be formed, the original route remains visible. Authored paths
and the selected individual signal keep their own geometry.

Both arrow styles compose exact shared SVG spans: smooth uses cubic segments;
horizontal/vertical uses line segments and the configured corner radius. Full
paths are checked for block clearance, loops, retracing and endpoint direction.
Backward central fits must match both local guide tangents to avoid a cusp.
Dragging uses a lightweight preview, with full validation on release.

Shared sections have their own hit targets and exact membership. Clicking a
trunk, its count, or dropping a transformation onto it opens the signal chooser.
The chooser includes both block names and exact ports. Selecting one signal
exposes its individual editing controls above the trunk hit targets. Delete on a
shared section requires choosing a signal. One count badge per common endpoint
keeps the spine from accumulating a badge at every tap.

The running native dev Cockpit was checked with the real DCS Feature Lab graph
in a disposable native copy: 42 Base blocks, 73 Base signals and 87 saved bindings
including vehicle tabs. Both styles produced 60 full branch paths and shared
source/destination trunk sections, including the dense Virtual auxiliary input.
Actual SVG samples at 2 px intervals verified that trunk spans coincide with
every advertised member and clear all blocks. Selection, individual deletion and
Undo, view-toggle persistence, source-profile immutability and real block dragging
passed. Existing graph-construction and same-endpoint bundle behavior are checked
separately. Both styles produced 39 shared trunk spans. Observed native drag
samples ranged from 4.2–23.5 ms for smooth and 7.5–33.5 ms for horizontal/vertical.

Final verification passed 138 canvas tests, 267 profile-workspace tests, both
Svelte type checks with zero errors/warnings, and both production builds. Native
evidence is under:

- `.research/profile-wire-trunks-native/1792617566876/`
- `.research/profile-wire-bundles-native/1792617579733/`
- `.research/profile-wiring-native/1792617703078/`

The construction harness explicitly foregrounds the native window and verifies
keyboard focus before its existing Settings-tab arrow-key check. The real DCS
demo was left open with trunks enabled and its saved document verified unchanged.

```powershell
$env:OSG_NATIVE_CDP_URL='http://127.0.0.1:9227'
pnpm profiles:wire-trunks:native
```

Pure coverage includes both endpoint directions, coexisting buses, named ports,
obstacle fallback, selected/authored route preservation, deterministic immutable
results, backward tangent continuity and a bounded 100-branch case.
