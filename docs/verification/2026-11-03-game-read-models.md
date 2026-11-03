# Games subscriptions — 3 November 2026

This delivers a bounded games read-model slice of #188, R5 #194 and R7 #196.
Those issues remain open for the other application domains and their acceptance.

## Behavior

`games.state` publishes catalogue summaries from prepared native caches.
`games.detail.<gameId>` publishes selected-game details; each summary names the
exact detail hash, so a late response cannot qualify another revision or game.
`games.jobs` preserves ordered lifecycle transitions. Discovery Ready remains
actionable while awaiting Apply. Completed job history is bounded; eviction
explicitly resynchronizes consumers and does not exhaust the lifetime source.

Native source hooks cover committed library changes, discovery, prepared artwork,
setup jobs, vehicle/context changes, permission state and persistence errors.
The publisher wakes for invalidations, subscription lifecycle and actual expiry
deadlines. Renewals can reschedule expiry without rebuilding an unchanged view.
Expired independent sources cannot be hidden by another future deadline. Worker
panics retire topics, clear pages and release bootstrap/detail waiters.

Large collections and text use recursively encoded, hash-verified immutable pages.
Images remain opaque until visible or explicitly selected for editing. Immutable
pages are shared across consumers; unchanged hashes preserve object identity.
Last-subscriber disposal cancels page work and releases transport registrations.
The native and MCP read-only `game_read_model_page` command reads issued cache
identities, never paths or a source-acquisition endpoint.

App, Games, profile game selection and the games inventory adapter use shared
subscriptions. Their periodic `game_get_state` calls are removed. Command receipts
cannot overwrite an actively observed catalogue. Stale details remain labelled
and cannot authorize editing. Inventory retains catalogue error diagnostics.

## Bounds

- Catalogue/discovery records retain their existing source limits. Recursive
  paging supports reviews beyond 1,024 rows and objects above the wire frame cap.
- Native page blobs: 16 MiB each, 1 GiB/65,536 cached blobs, 8 KiB UTF-8 page reads;
  current roots pin their transitive pages. Capacity failure is explicit.
- At most 64 selected detail scopes; preparation leases bridge registration and
  subscription, and the last reader retires its exact publisher.
- Jobs: 16 retained keys, 256 queued transitions/512 KiB. Messages are bounded to
  1,024 UTF-8 bytes with an explicit truncation flag; overload requires resync.
- Client: 128 MiB/4,096 cached entries, eight concurrent loads, bounded tree depth
  and logical node count. Digests, declared bytes and collection shape are checked.

## Local evidence

The isolated candidate preserves pre-existing workspace edits. Final staged
native/frontend/config sources are compared with that candidate before commit.
Local evidence is under `.research/reactive/game-stream-*` and is not committed.

- Full instrumented native suite: **2,050 passed**, zero failures, 17 existing
  explicit ignores; **all 166 production-module coverage gates pass** (minimum
  75.94%, required 71%), with unchanged native source/test stamps.
- Persistence: **58 passed**, one existing ignore; all 13 coverage gates pass,
  including observation notifications at 96.45% production lines.
- Games frontend delivery group: **141 passed**. The broader affected consumer
  run passed 174 tests before the final null-receipt regression; the final stream
  suite passes 30 tests with 95.12% lines, 87.57% branches, 86.27% functions.
- Svelte checking: zero errors/warnings. Production frontend/native builds pass.
  All 12 ownership checks and 20 coverage-helper checks pass.
- Actual rebuilt Cockpit: two summary consumers observe create/edit/remove;
  exact selected details follow the summary hash, lazy artwork resolves through
  actual native pages, and removal withdraws details. No game is launched and
  the profile remains stopped. The first run exposed a native `null` add receipt
  rejected by the client; the corrected validator and regression now pass.
- Existing native setup/artwork/preview/baseline/source-wake checks pass. The
  first pulse fixture failed with `Cannot inspect persistence path`; both tests
  in its focused rerun passed. This first failure is retained as evidence rather
  than described as an uninterrupted green run.
- Authored DCS ownship/BIOS fixture passes, including independent expiry/recovery
  and process exit; fixture cleanup reports no errors.
- All four authored Assetto page fixtures pass processing, frozen expiry and
  recovery. These are software fixtures, not live simulator confirmation.

Executable SHA-256:
`6B43297C7194DEF79D67D82DBC54DD1BE4739B14B62E4916931F81121E037C6E`.

## Remaining scope

Existing upstream vehicle/context/process/setup acquisition cadences are not
claimed migrated by replacing their downstream UI reads. Workspace, devices,
companions, settings/activity, complete MCP subscription acceptance and full-app
zero-refresh/soak qualification remain in the original issues. This milestone
does not qualify physical controller latency or simulator/board effects. #199
holds user-operated acceptance and stays first in progress. Assetto sampling is
still unapproved; measured CPU/detection-delay options precede that decision.
CI remains deferred until all features are implemented, as requested. Historical
overall percentage estimates were withdrawn because they lacked a defensible
requirement-based calculation.
