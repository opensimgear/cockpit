# Percentage deadzone transform

Typed deadzone authoring now uses `centerPercent` and `deadzonePercent`, each
bounded to 0–100. Center is a position across the declared input range; deadzone
is the neutral band's full width. At center 50%, a 10% deadzone covers 45–55% of
travel. The native evaluator derives the center and radius from the connected
numeric contract and rescales values outside the band to retain full travel.
Integer signals retain explicit rounding. Degenerate input ranges are rejected.

The editor, advanced settings, summaries, native validator and MCP operation
catalogue share these parameters. Pending transforms preserve percentages when
connecting a differently scaled source. The separate builtin controller-axis
rule and hardware calibration deadzones are unchanged. No compatibility layer
or schema version was added.

Validation on 25 October 2026:

- Svelte checks: zero errors and warnings. Both production UI bundles built.
- `pnpm test:affected`: all eight check, test and build tasks passed.
- Frontend profile and processing coverage gates passed (95.07% and 98.28%
  production lines). Workspace regressions: 282 passed. Desktop coverage passed.
- Portable native mapping: 131 tests passed. Operation validation reached 94.71%
  and typed engine 93.62% production line coverage; every mapping module passed.
- Native profiles: 139 tests passed; every profile module passed its line gate.
- Native topology summary coverage: 88.06%.
- Full native library regression suite: 1,204 passed, zero failures, eight
  environment-dependent checks ignored.
- Final seed/workspace coverage report: 100% and 98.75%, respectively.
- Running Cockpit signal-overview checks passed both scenarios, including inline
  percentage edits, saving, undo and redo.
- The saved VKB Rudder Q/E profile now uses one 10% deadzone at center 50%, with
  its inverted directions preserved. Native preflight is ready. Recording replay
  confirmed no events inside the neutral band, E down/up for the low side and Q
  down/up for the high side, and no native submission. The actual inspector
  displays both percentage fields. Screenshot:
  `.research/deadzone-percent-rudder.png`.
  A subsequent native status read confirmed the profile running with no faults
  or degradation, alongside all six widget sources and the saved DCS panel.

The development database was backed up before updating its saved deadzone
parameters through the native persistence API. Profiles, assignments and the
designer workspace were retained. Local evidence lives in
`.research/deadzone-percent-*.log`, `.research/deadzone-percent-topology-coverage/`,
`.research/deadzone-percent-final-coverage/`, `.research/mapping-coverage/` and
`.research/profiles-coverage/`.
