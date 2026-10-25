# DCS panel and widgets in the development seed

Fresh debug databases now receive the DCS WIN Companion designer workspace in
the same transaction as the example profiles and widget library. It contains
the generic Toggle, five DCS widget sources, the 2400 × 1440 panel with eleven
placements and its compiled preview. Pairing, destination selection and profile
activation remain explicit actions. Existing initialized databases retain their
edits and deletions.

The generated `examples/dcs-bios/win-companion/workspace.json` is rebuilt by
`scripts/dev-companion-seed.mjs`. Both `widgets:check` and Nx shared fixture
checks recompile and compare it; native affected inputs include example assets.
The seed uses the normal designer validator before committing any content.

Validation on 25 October 2026:

- `pnpm widgets:check` passed, including the generated seed freshness check.
- `pnpm widgets:coverage` passed all 134 tests and coverage thresholds.
- Frontend Svelte checks reported zero errors and warnings; `pnpm ui:build`
  passed for both browser companion and Cockpit.
- Nx plugin and test-selection regressions passed all 18 tests.
- Full native library suite passed: 1,204 tests, zero failures, eight ignored.
- Focused native coverage passed: development seed 100%, widget storage 98.44%,
  designer workspace 98.75%. Evidence:
  `.research/dev-companion-seed-focused-coverage/`.
- `tests/ui/dcs-demo-native.mjs` passed both checks in the running dev Cockpit:
  the complete DCS profile, exact seeded workspace and widget inventory, panel
  editor and compiled Try preview. Screenshot:
  `.research/dcs-demo-native/seeded-panel.png`.

The running dev database was backed up and replaced to exercise fresh startup.
Native regressions also cover workspace/library atomicity, refusing an existing
unmarked workspace and preserving removed panel content across restart.
These checks verify authoring, persistence and local preview; live aircraft and
destination delivery are outside this seed change.
