# Always-collapsed arrows

Removed the collapse checkbox, store setter, saved preference, native validator
field and MCP schema field. Canvas styling now contains only `arrowStyle` and
`cornerRadius`. Wire bundling has no disabled mode. Selecting an individual
signal still exposes its controls.

Verified against the running native dev Cockpit on 2026-10-26. In both smooth
and orthogonal styles, the DCS Feature Lab overview displayed 9 arrows. Opening
a parallel group exposed its 18 distinct signal lanes. Settings had no collapse
checkbox, SQLite acknowledged the exact two-field styling value, and the saved
profile document stayed unchanged. Evidence is in
`.research/always-collapsed-acceptance-current.log` and
`.research/always-collapsed-{smooth,orthogonal}.png`.

Passed checks:

- Shell and profile frontend test groups, including their coverage gates.
- Wire bundle/trunk coverage: 98.13% lines, 87.27% branches, 100% functions.
- Canvas styling coverage: 100% lines and branches, 92.31% functions.
- Svelte check: no errors or warnings; production UI build passed.
- Native library: 1,204 passed, 8 ignored, no failures with native socket/process access.

The legacy wire-bundle native suite cannot complete against the current UI: it
expects the removed detailed-graph switch and its per-binding paths. The current
overview verification above covers the actual user-facing canvas. Sandbox-only
native test failures were resolved by rerunning with native socket/process access.
