# Activity lifecycle wakeups

Activity lifecycle batches already had their own nonblocking channel, but the
writer waited on the metadata channel for up to 750 ms. Lifecycle transitions
and observations released by abandoned actions could therefore sit available
without waking the writer.

The writer now observes a shared notification epoch before draining both
channels. Successful metadata/lifecycle enqueue and action abandonment notify
it directly. Abandonment releases the reconciliation lock before notification.
The main channel retains its 2,048-message bound; after a full 256-message drain
the writer continues rather than parking over queued work. Notifications run
no consumer callback or storage operation on the coordinator thread.

Aggregate metadata still uses its existing 750 ms fallback, and retention still
has an hourly deadline. This is a bounded #194 delivery, not completion of the
Activity producer migration or a full-app latency claim. The existing unbounded
lifecycle channel and potentially blocking aggregate snapshot remain unchanged.

Local verification on 2026-11-05:

- The full isolated instrumented native suite passes: 2,289 tests, zero failures,
  17 existing explicit ignores. New behavioral tests read committed storage
  without Flush: brief activation/degradation/recovery and abandoned-action
  deferred observations commit within the 500 ms regression bound, before the
  old 750 ms acquisition opportunity. This is not physical latency evidence.
- All changed production line coverage gates pass: Activity runtime 95.79%,
  writer 92.75%, lifecycle observer 100%; current floor 71%.
- The production native build passes. The actual rebuilt Cockpit passes Activity
  committed-change/idle/navigation checks, delayed physical controller preview
  recovery, and physical input reader replacement/quiet liveness/Stop.
- The initial Activity idle check failed during real startup discovery.
  Committed events show discovery progress and completion. The fixture now waits
  for discovery to finish before the unchanged 6.5-second zero-refresh assertion;
  the final three-test run passes after that precondition correction.
- All 1,156 archived native source/SQL/manifest/lock records match HEAD except
  the intended owned edits. All six owned Rust files match the tested candidate.
  All 108 unrelated modified files retain their original hashes.
- #199 is verified first in the Plan project's Progress lane. No hardware,
  simulator, Windows permission or board-flashing acceptance is claimed.

App SHA-256:
`cda0e24c5847cc4bbefee0aa2bac89edd505d2ceb33bf8d688fcff570aa0821a`.
Artifacts are under `.research/reactive/activity-wake-*`, including coverage,
initial/final running-Cockpit logs, committed-event investigation and source
proof. The final UI fixture uses the existing dev frontend; unrelated frontend
edits are excluded from this commit. CI remains deferred until implementation
completion, as requested.

The [remaining acceptance ledger](../reactive/remaining-acceptance.md) separates
software work from user-operated qualification. The accepted Assetto exception
is one native reader per active game, only at shared-memory telemetry acquisition.
Controller preview/raw-to-virtual timing and real Assetto CPU/detection-delay
measurements remain separately unqualified.
