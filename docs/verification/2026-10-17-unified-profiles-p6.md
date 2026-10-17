# Unified profiles P6 authoring verification — 17 October 2026

This records the combined P2/P3/P6 list, editor, canvas and application
navigation checks for [#86](https://github.com/opensimgear/cockpit/issues/86).
The actual App entry exposes Profiles and retains an opened editor only for
its game-settings round-trip. Runtime and hardware qualification remain
separate acceptance work.

| Check | Result |
| --- | --- |
| Existing profile, inventory and compiler clients, P3 provider helpers/UI and P6 editor/canvas/pager/App SSR tests | 98 passed |
| Existing transport regression suite | 182 passed |
| Editor and navigation guard state tests | 18 passed |
| Canvas geometry/typed display tests | 5 passed |
| Large provider catalogue paging tests | 3 passed |
| Actual Svelte SSR workflow/inspector and App handler tests | 11 passed |
| Main and browser Svelte checks | 0 errors, 0 warnings |
| Existing browser and desktop production UI builds | Passed |
| Production App entry including Profiles list/editor/canvas | Passed; 219 modules |

The production App build now includes the real list, editor, canvas and native
client adapters. Actual App SSR verifies that Profiles is available in the
sidebar without eagerly mounting a never-opened editor. Handler tests exercise
the real navigation functions with controlled browser history and no native I/O.
CI runs the workflow tests through `pnpm profile-workspace:test`, independently
from each pure helper coverage gate.

| Production helper | Lines | Branches | Functions |
| --- | ---: | ---: | ---: |
| `profile-editor.ts` | 87.19% | 92.86% | 96.43% |
| `profile-canvas.ts` | 94.12% | 97.75% | 100% |
| `profile-port-browser.ts` | 81.08% | 100% | 100% |

All retain the repository's 71% thresholds. Local detailed output is retained
under `.research/p6-integrated-frontend.log`,
`p6-integrated-profile-editor-coverage.log`,
`p6-integrated-profile-canvas-coverage.log`,
`p6-integrated-profile-port-browser-coverage.log`,
`p6-integrated-game-provider-coverage.log`, `p6-integrated-check.log` and
`p6-integrated-build.log`. The dedicated workflow and transport runs are in
`p6-integrated-workspace-test.log` and `p6-integrated-transport.log`.

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
Sidebar departure offers Save, Discard or Keep editing through the same guard
as Back. Guarded Save retains newer edits and cancels its pending departure
when the user keeps editing or the editor epoch changes. Failed saves keep the
original destination pending for an explicit retry.

App handler tests cover settings return by button and browser Back, changing
games during the same visit, retained viewport/draft references, and guarded
sidebar departure from settings. Ordinary Games navigation creates no editor
return target. Opening, saving and returning from settings leave runtime
selection and the captured applied snapshot unchanged.

Interactive screenshot review was unavailable: the computer-use inventory
reported no connected browsers or native app surfaces. No browser interaction,
driver submission, device output or simulator qualification is claimed by this
evidence. P7 connection authoring/live inspection and P4/P9 runtime acceptance
remain separate from this authoring/navigation handoff.
