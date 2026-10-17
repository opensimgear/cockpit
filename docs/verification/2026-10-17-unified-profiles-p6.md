# Unified profiles P6 authoring verification — 17 October 2026

This records the list/editor/canvas component handoff for
[#86](https://github.com/opensimgear/cockpit/issues/86). The application navigation
entry and settings return integration are verified separately after combining
the P2/P3/P6 branches. These results do not declare the milestone or hardware
qualification complete.

| Check | Result |
| --- | --- |
| Existing profile, inventory and compiler clients plus new editor/canvas/pager/actual Svelte SSR tests | 74 passed |
| New editor state tests | 13 passed |
| Canvas geometry/typed display tests | 5 passed |
| Large provider catalogue paging tests | 3 passed |
| Actual Svelte SSR workflow/inspector tests | 7 passed |
| Main and browser Svelte checks | 0 errors, 0 warnings |
| Existing browser and desktop production UI builds | Passed |
| Isolated production client build with `ProfilesPanel.svelte` as the actual entry | Passed; 118 modules |

The isolated build matters because the existing App entry does not yet import
the new component in this branch. It compiles the list, editor, canvas and native
client adapters as a real client bundle, in addition to the SSR rendering tests.

| Production helper | Lines | Branches | Functions |
| --- | ---: | ---: | ---: |
| `profile-editor.ts` | 90.57% | 92.31% | 97.83% |
| `profile-canvas.ts` | 94.12% | 97.75% | 100% |
| `profile-port-browser.ts` | 83.78% | 100% | 100% |

All retain the repository's 71% thresholds. Local detailed output is retained
under `.research/p6-frontend-final.log`, `p6-editor-coverage.log`,
`p6-canvas-coverage.log`, `p6-ui-check.log` and `p6-component-build.log`.

The tests cover Default-only entry, exact profile opening with native button
keyboard semantics, local Add/Copy/Cancel, atomic Save, explicit dirty Back,
complete multi-resource category seeding, no initial routes or effects, and
independent copied graphs/layouts. Failed inventory reads retain known metadata
but block blank creation until the next complete inventory snapshot; a 65-row
inventory cannot be silently truncated. A 20,000-port catalogue remains paged
instead of being serialized into graph ports. Short pages advance by their
actual size and changed catalogue contracts require refresh.

Canvas tests retain offline identities, selected ports, graph routes and saved
positions. Actual component rendering shows CAN and USB as metadata, grouped
typed source/destination ports, dormant game requirements and game-owned
settings links. Saved graph bindings alone produce drawn edges. Store save
races preserve newer edits, reject mismatched committed summaries and cannot
replace an editor opened later. The applied session snapshot remains distinct.

Interactive screenshot review was unavailable: the computer-use inventory
reported no connected browsers or native app surfaces. No browser interaction,
driver submission, device output or simulator qualification is claimed by this
evidence. P7 connection authoring/live inspection and P4/P9 runtime acceptance
remain separate from this component handoff.
