# Applied profile path activity â€” 17 October 2026

This P7 increment adds a source â†’ visible transformation â†’ requested destination
view to Profiles. It reads the immutable native applied document rather than
reconstructing a running route from the current editor. A newer or different
draft can remain open without changing the graph, owner or values being shown.

The passive `profile_runtime_topology` command accepts an exact native RunOwner
and a bounded nodes/edges page. Every page returns that owner, applied snapshot,
program hash and native capture time. Its native projection exposes only logical
block IDs, labels, kinds, readable operation summaries and exact directed
bindings. It returns no script code, parameters, resource tokens, provider polls
or output acknowledgements. Reads cannot advance or drain the reducer.

The frontend loads every topology page with consistency checks and independent
frozen copies. Connection expansion crosses processing blocks only: a simulator
command input never implies a connection to its independent telemetry output.
The view orders transformations by their actual graph dependencies and retains
every mapping as an exact source/destination port pair. A route over 32 blocks or
64 connections reports its size and offers single-connection inspection; nothing
is silently omitted. Native graph limits remain 336 blocks and 512 bindings.

Samples come from the existing exact-owner applied-port inspector. Every node
and page keeps its own capture time, including short native pages. Missing,
unknown, stale and unavailable values are distinct. Known false and zero remain
known neutral samples. Requested action arguments retain their ordered IDs and
values. Highlighting means only that a producer has a known graph sample; it is
not evidence that a physical device, virtual controller or simulator accepted
the destination value. Events retain their last observed sample rather than
being fabricated as a live pulse. No end-to-end atomic capture is claimed.

Opening, changing selection, polling, hiding or closing this view never starts,
pauses or stops a profile and never saves a draft. Polling is read-only and runs
only while the panel is active and the document is visible. Owner replacement,
selection changes and disposal fence late successes and errors. Stale samples
lose the live connection highlight. Explicitly choosing another applied
connection suspends editor following; restoring follow selects the exact
applied block again. The existing port table remains the detailed alternative.

## Verification

| Check | Result |
| --- | --- |
| Native topology projection/parser tests | 4 passed |
| Topology/path client tests | 4 passed |
| Fenced activity controller tests | 7 passed |
| Actual Svelte activity SSR/handler tests | 7 passed |
| Complete profile UI SSR/handler suite | 59 passed |
| Gateway queue/transport tests | 32 passed |
| Main and browser Svelte checks | 0 errors, 0 warnings |
| Browser companion production build | Passed, 128 modules |
| Cockpit production frontend build | Passed, 258 modules |

| Independent production helper | Lines | Branches | Functions |
| --- | ---: | ---: | ---: |
| `profile-activity.ts` | 85.96% | 100% | 100% |
| `profile-activity-controller.ts` | 100% | 97.12% | 100% |

The package and CI run both helpers with the unchanged 71% line, branch and
function thresholds. `profile-workspace:test` includes the seven new real
component tests. Native metadata tests use the compiled 29-transformation,
66-binding authoring fixture and verify stable exact pages, owner/document
fences, bounded summaries, script-code exclusion and unchanged phase clock,
token and pending effects. The source-stamped native gate passes 141/170
production lines (82.94%) for the topology module; its original 71% threshold
is unchanged. Evidence is retained under `.research/profile-composed-hosts-coverage`.

The earlier `tests/ui/unified-profiles.mjs` browser run used Enter for Add,
Save and reopening an exact saved profile; Back, dirty Keep/Discard and Cancel
were mouse actions. That run did not prove keyboard access to those remaining
controls.

The current P9 navigation check closes that evidence gap with actual Tab presses
followed by native Enter/Space activation. Its 13 recorded activations cover
Add, Save, exact-profile reopening, clean and dirty Back, dirty Keep editing,
Discard and continue, Save and continue, and Cancel creation after editing a new
draft. The Tab search is bounded by visible DOM focus stops (within the native
modal when open); neither a locator focus method nor JavaScript moves focus.
The test also checks that list/Add/Stop are visible before runtime details and
that opening inspection is explicit. It does not claim a full application or
assistive-technology accessibility audit.

The accepted Edge `155.0.4283.62` run passed **1 browser test** in 3.66 seconds
against the existing frontend at `127.0.0.1:1420`, with product sources held at
`f55b623279abe1dce18bd50c3dc81b80e85e6061`. Evidence is retained in
`.research/p9-keyboard-navigation/verified-run-3`: the exact driver copy,
`keyboard-evidence.json`, intercepted requests, final screenshot and a source
stability record. The driver, ProfilesPanel, editor store and App hashes stayed
unchanged during the run. Driver SHA-256 is
`f80a2233c42341d95c8a4d9d1cb889d956752cc702e25954880eccc9e3d0dc68`;
keyboard evidence SHA-256 is
`e18dbfd60e7d0b68668d03b8618bb90562e95080eddd4a9cb386ef6a4516bd1c`.
Node syntax validation passed, and no browser page error or unexpected profile
command was recorded.

Exactly one save has occurred after creation, Keep/Discard and Cancel; a separate
dirty Save-and-continue path adds exactly one revisioned save, with expected
revisions `[null, 1]`. The saved list contains only Default and the exact new
profile; cancellation creates no third entry. Runtime selection, owner, applied
Default revision and stored Default remain unchanged. Every native IPC is
intercepted and HTTP gateway fallback is forbidden; no actual service, driver,
device, game or native profile effect is used.

The two preceding attempts remain in `verified-run-1` and `verified-run-2`.
They exposed stale fixture assumptions: counting every row button as a profile
(current rows also contain Export/Delete), and rejecting the current passive
retired-cleanup status request. The test now counts actual profile list entries
and exact editor rows, and supplies only the existing clean/unqualified status
reply. Its strict unexpected-effect, saved-ID/revision and runtime-isolation
assertions remain. No application implementation was changed.

This evidence covers SSR, actual component handlers, native projection contracts,
frontend builds and the scoped browser navigation fixture. No hardware output
was started; assembled physical/native acceptance remains open in #87.
