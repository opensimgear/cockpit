# Collapsed parallel connections experiment

Extended by [shared input and output trunks](2026-10-21-shared-wire-trunks.md).

Settings → Styling → Arrows now has **Collapse parallel connections**, enabled
by default. It is an app-local view preference; toggling it does not edit a
profile, add history entries, or change the signal graph.

Wires sharing both physical attachment points on the same directed node pair
draw as one line with a count badge. Logical ports sharing a resource-card footer
can therefore collapse together. Named ports on different transform rows remain
separate. Connections with different source blocks or destination blocks do not
merge just because the blocks happen to overlap.

Click a shared wire or its badge to choose a logical signal by its exact ports.
The selected signal draws separately with its own reconnect, segment and reroute
controls, while the other signals remain collapsed. Close the connection actions
or clear selection to restore the collapsed view. Delete on a shared wire opens
the chooser instead of deleting an arbitrary signal. Dropping a transformation
on a shared wire also requires choosing the target signal. Escape or blank canvas
cancels the chooser.

The experiment uses the existing validated route geometry and drag previews;
it does not change saved routes or run ELK during dragging. Badges remain readable
at different zoom levels and follow the rendered path midpoint.

The running native dev Cockpit passed the DCS Feature Lab scenario in both arrow
styles: **73 Base signals draw as 60 wires**, including bundles of 7 and 8 signals.
Every logical signal is represented exactly once. Individual selection,
disconnect/Undo, Escape/background dismissal, preference persistence and live
block dragging passed. All physical anchors were checked during dragging. The
complete saved document retained its 87 bindings, including vehicle tabs, and
the original demo was unchanged by the test.

Native evidence: `.research/profile-wire-bundles-native/1792616365351/`.
The existing native construction workflow also passed in
`.research/profile-wiring-native/1792616374102/`. The final checks passed 127
canvas tests, 266 profile-workspace tests, both type checks and both production
builds. The real demo is left open with collapse enabled, and its saved document
was verified unchanged.

```powershell
$env:OSG_NATIVE_CDP_URL='http://127.0.0.1:9227'
pnpm profiles:wire-bundles:native
```

The existing routing and graph-construction native suites explicitly select the
individual-wire view and restore the previous styling afterward, preserving
their full per-wire geometry and editing coverage.
