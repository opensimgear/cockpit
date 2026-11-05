# Ordered desktop dispatch runs outside profile input reduction

Native desktop output now has one ordered owner for its Coordinator, journal
and IPC. Profile polling admits a bounded batch, consumes completion records and
refreshes authorization through short memory operations. The owner wakes the
native profile worker after completion. Recording composition uses the same
executor inline for deterministic reducer tests.

Only one batch may remain outstanding until its completion is consumed. The
4096-delivery and 1 MiB admission bounds fail explicitly; byte counting stops
without allocating an oversized serialized copy. Adjacent pointer vectors keep
their ordering, grouping and transport-size limits. Queue and dispatch residence
count against original deadlines. A skipped held press suppresses its later
queued taps before another batch is admitted.

Current producer tokens and dispatch epochs revoke unsent work. The Coordinator
checks the gate again after durable preparation, immediately before IPC;
revocation expires the request so the worker returns a truthful skipped ACK.
Cleanup remains eligible. Pause/Stop have reserved priority admission, and Stop
requires native cleanup plus the final journal commit. Failed final commits
cannot become success on a repeated Stop. Journal ownership retires only after
proved Stop so immediate exact-scope replacement can acquire the same lock.
Destruction signals the owner without joining or performing IPC on the profile
worker. Renewal is coalesced, requires fresh upstream authorization and cannot
continue indefinitely after upstream stalls. No periodic owner polling is added.

Local evidence, 2026-11-05:

- All 83 desktop-output, 138 shared-host and 2,286 native tests pass; the native
  suite retains 17 existing explicit ignores. Tests cover blocked IPC,
  completion wakes, queue age, ordered pointer barriers, source revocation,
  saturation, priority cleanup, stale authorization, worker panic, failed
  native/commit results and journal-lock retirement.
- The actual Desktop poll integration continues canonical input reduction
  while Arm is blocked, accepts source loss, then prevents the revoked press
  from reaching the native adapter after release.
- All 17 permanent shared-host coverage gates pass, now including every new
  owner module and production after interleaved test-only items. Owner facade,
  worker and executor line coverage is 97.86%, 99.15% and 100%; Coordinator is
  92.09%. Shared desktop/lifecycle/dispatch are 72.22%/85.48%/88.89%; native
  wrapper/activation are 81.72%/83.33%. All 17 coverage-parser tests pass.
- Five final running-Cockpit checks pass: controlled native tap/Stop; delayed
  physical preview receipt catch-up; physical activation/replacement/quiet
  liveness/Stop; shared passive preview/reopen; actual vJoy Raw Input plus
  independent real MCP reader/Stop checks. Temporary profiles/readers retire,
  the vJoy slot returns free, receiver and desktop child processes exit.
- The companion isolation test now waits for a readable (`busy:false`)
  observation before asserting `running:true`. Its API intentionally returns
  `running:false,busy:true` while the worker lock is held, already covered by a
  separate contract test. This removes a test race without waiting for the
  success being asserted or changing production companion behavior. Earlier
  failures did not retain that busy field.

Initial owner compilation exposed integration mistakes, and the first pointer
fixture used an invalid oversized action ID; these were corrected before the
final full runs. Coverage-helper fixture writes initially hit sandbox temporary
directory ACLs; all helpers passed with a workspace temporary directory. Clippy
is unavailable in the pinned installed toolchain. CI remains deferred until
implementation is complete, as requested.

The isolated candidate's 1,159 tracked Rust/manifests/locks differ only in the ten
intended edited native files; all eight new Rust files match. The coverage
harness also matches. Unrelated worktree edits remain excluded. Final native
SHA-256: `ce090ecf876b2614d385691bfc4b15434a347d850395f61cd086363366d7388d`.
Logs, coverage, runtime identity and source proof are under
`.research/reactive/desktop-owner-*`. Final receive-only evidence is
`.research/reactive/desktop-deadlines-native/1793865963553/evidence.json`.

This removes direct journal/IPC waits from profile polling. Durable writes still
delay desktop injection; shared persistence contention is not qualified away.
The receiver recorded 15 ms between events for a requested 1 ms tap, so native
desktop timing remains software work under #193. Focus/child notifications,
remaining adapters and full integrated qualification remain #188/#193/#198.
Physical raw-to-preview/output timing and simulator acceptance remain #199,
verified first in Progress. No complete acceptance checkbox is claimed here.
