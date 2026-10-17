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

The additional `tests/ui/unified-profiles.mjs` browser fixture verifies keyboard
Add, Save, Back, reopening, dirty Keep/Discard and clean Cancel, with an unchanged
applied owner. It also checks that list/Add/Stop are visible before runtime
details and that opening inspection is explicit. Every native IPC is intercepted;
no actual service, driver, device or game is used.

This evidence covers SSR, actual component handlers, native projection contracts,
frontend builds and the scoped browser navigation fixture. No hardware output
was started; assembled physical/native acceptance remains open in #87.
