# Applied profile port inspection — 2026-10-17

The frontend inspector consumes only `profile_runtime_inspect`, scoped to the
exact native applied owner (session, generation, profile ID/revision and game
context). Requests contain only that identity and a bounded node/page selection.
They cannot contain browser samples, reducer instructions or activation flags.

| Integration check | Result |
| --- | --- |
| Workspace/routing/runtime/inspection Svelte SSR and handler tests | 30 passed |
| Main and browser Svelte checks | 0 errors, 0 warnings |
| Browser and desktop UI production builds | Passed; desktop 241 modules |

The independent 71% frontend coverage gates pass:

| Production helper | Tests | Lines | Branches | Functions |
| --- | ---: | ---: | ---: | ---: |
| `profile-inspection.ts` | 6 | 90% | 98.13% | 100% |
| `profile-inspection-controller.ts` | 4 | 93.55% | 96% | 100% |

Client tests preserve immutable typed samples, raw units, full unsigned 64-bit
timestamp strings, explicit unknown states, ordered action arguments and display
constraints. Wrong owner/revision, malformed contracts/samples, unsafe counters,
duplicate endpoints, empty nonfinal pages and oversized replies are rejected.
Controller tests fence late owner/node responses and errors, advance short pages
by their actual size, keep stale failures explicit and never replay a read or
submit an effect during disposal.

Four actual Svelte SSR/handler/lifecycle tests cover passive initial/stopped
views, raw-unit/unknown/action-tuple rendering, applied-revision versus draft
labels, exact selected-block filtering, active/visible polling and cleanup, and
stale-owner errors. A stopped owner hides old values. Paging is explicitly one
native capture per page; sink samples are labelled upstream/requested values,
without claiming device or simulator acknowledgement.

Native tests now cover strict request bounds, real cached source/processing/sink
samples, timestamps, stale transitions, stable paging, owner/document rejection
and preserved effect queues. The actual Manager test applies revision 2, saves a
changed revision 3, verifies that inspection still returns revision 2's actual
values, and rejects the owner after Stop. Ten relevant native tests pass.

The source-stamped native inspector coverage gate passes at 143/149 production
lines (95.97%). All 89 mapping tests and all thirteen mapping module gates pass;
the typed engine is 96.22%. Evidence is in
`.research/profile-inspection-native-coverage` and
`.research/profile-inspection-mapping-coverage`.

The frontend tests use actual Svelte SSR and instrumented production handlers.
Live physical/game output qualification and input learning remain separate
acceptance work.
