# P7 graph authoring verification — 17 October 2026

This records the authoring portion of [#87](https://github.com/opensimgear/cockpit/issues/87)
on `main`. Runtime controls, live source/value inspection, input learning and
actual driver/device/game acceptance remain separate integration work.

| Check | Result |
| --- | --- |
| Related profile/inventory/compiler clients, P3 provider UI and P6/P7 state/graph/UI tests | 141 passed |
| Actual Svelte workspace/routing SSR and real-handler tests | 19 passed |
| Main and browser Svelte checks | 0 errors, 0 warnings |
| Browser and desktop production UI build | Passed; desktop App 233 modules |
| Fixed authoring fixture | Native parse and compiler acceptance: 29 nodes, 66 exact edges |

The fixed fixture is `tests/fixtures/profile-authoring/visible-presets.json`.
Frontend tests compare every generated preset and invocation tuple with it.
Native compiler tests validate the complete graph independently. Its resources
are unassigned and optional: an overall Ready report still has no actual delivery
tokens and its branches remain unavailable. No hardware readiness is inferred.

| Production helper | Lines | Branches | Functions |
| --- | ---: | ---: | ---: |
| `profile-editor.ts` | 87.32% | 92.89% | 94.74% |
| `profile-graph.ts` | 99.40% | 97.18% | 95.65% |
| `profile-graph-history.ts` | 100% | 98.75% | 100% |
| `profile-processing.ts` | 95.14% | 87.27% | 92.45% |
| `profile-canvas.ts` | 94.12% | 97.75% | 100% |
| `profile-port-browser.ts` | 81.08% | 100% | 100% |

All independent production helper gates retain 71% line, branch and function
thresholds. CI runs the actual component tests through `profile-workspace:test`.
Detailed local output is in `.research/p7-authoring-frontend.log`,
`p7-authoring-workspace.log`, `p7-authoring-check.log`, `p7-authoring-build.log`
and `p7-authoring-*-coverage.log`.

The real component handlers author third-party A+B into one virtual controller
and separate CAN controls into another. They author an event-triggered DCS
command independently from DCS altitude → ft/m conversion → hysteresis → ordered
setter invocation → companion indicator and physical USB action. No whole-block
mapping, trigger, argument or transform is inferred. Incompatible edges fail
without graph changes. Actual rendering exposes all routes in the bindings
table, typed arguments/ports in the inspector and transformation parameters on
visible nodes. Large provider catalogues remain paged and only selected ports
enter the graph and local pins.

Actual store/history tests cover semantic, layout and local-assignment undo/redo,
saved revision rebasing, newer edits during Save, editor replacement and applied
snapshot isolation. Review regressions prevent Discard during an in-flight Save
and mark captured preflight results stale after graph or inventory changes,
including a Ready fixture followed by disconnect/contract/setup/route changes.
Stale preflight diagnostics cannot decorate current blocks/edges. Diagnostic
pages advance by their actual length.

These are Svelte SSR, real-handler and native contract/compiler checks. They do
not simulate browser pointer capture or establish interactive native focus,
driver submission or simulator output acceptance. No additional native app or
hardware-owning browser instance was started for these checks. Live values,
learning, Apply/Pause/Stop/recovery controls and running-revision inspection must
consume the actual P4/P5 APIs before the remaining #87 acceptance is checked.
