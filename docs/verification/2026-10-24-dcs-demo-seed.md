# Restored DCS Feature Lab development example

Recovered **DCS Feature Lab - AH-6J + F/A-18C** from the saved revision 11 in
the local `overview-native-data/profiles-v2/library.json` archive. The checked-in
`examples/dcs-bios/feature-lab.profile.json` matches every archived document field
except revision, reset to 1. It preserves 7 resources, 44 processing nodes,
87 bindings, both aircraft tabs, output declarations and 54 canvas positions.

Fresh debug databases seed the document in the existing atomic initialization
transaction. Existing databases remain unchanged by startup. The current dev
Cockpit received the same document through native `profile_import`, which creates
a fresh profile ID. No local hardware assignments or runtime ownership were
restored. Companion publication bytes and game setup are outside this profile.

Validation on 24 October 2026:

- Full native library suite: 1,203 passed, 8 ignored, zero failures.
- Development seed regressions: 4 passed, including restart preservation and
  failed-write atomicity.
- Integration coverage gate: development seed 90/90 production lines (100%);
  namespace paths 37/52 (71.15%). Local evidence:
  `.research/dcs-demo-coverage-gate/`.
- `pnpm profiles:coverage` and all 281 `profile-workspace:test` checks passed.
- `tests/ui/dcs-demo-native.mjs` passed against the running dev Cockpit: saved
  graph equality, empty hardware assignments, stopped runtime, Base canvas and
  both AH-6J and F/A-18C canvases. Screenshot:
  `.research/dcs-demo-native/base.png`.

These checks verify authoring and persistence. They do not qualify live aircraft,
physical controllers, virtual outputs or companion delivery.
