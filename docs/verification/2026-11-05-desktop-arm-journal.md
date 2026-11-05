# Desktop Arm commits once

Arming replaces one desktop owner's contribution with an empty contribution.
It can release old controls, including the final contribution to a shared
control, but cannot acquire a control. The preceding durable held/possible-held
record therefore remains a conservative recovery superset until the final
owner/phase commit.

Arm now verifies the retained evidence before dispatch, omits its intermediate
pre-dispatch and successful-ACK writes, and retains the final owner/phase write.
Its ACK must report a subset of previously held controls; an unexpected acquired
control fails with uncertainty before entering the parent ledger. Failure,
rejection and uncertain-ACK persistence remain. Actual acquisition deliveries
retain their durable pre-dispatch, ACK and owner commits.

Local verification, 2026-11-05:

- All 67 isolated desktop-output tests pass. Five new regressions inspect the
  prior durable bytes at dispatch, prove a single final Arm commit, exercise
  real-worker release and shared ownership, reject unexpected acquired holds,
  and recover safely from a failed final write. Existing stale-evidence and
  actual-down commit-boundary tests pass. The initial external-target run needed
  the crate-local temporary directory used by its existing journal fixtures;
  that directory was prepared before the complete successful rerun.
- All 125 shared-host tests, 10 focused native tests and the full native suite
  pass: 2,286 native tests, zero failures, 17 existing explicit ignores.
- Changed production line gates pass: coordinator 90.61%, journal 79.32%.
- The rebuilt Cockpit passes the exact receive-only application's authored
  one-update key tap and clean Stop. Its audit parent checkpoint and worker
  footer are valid and complete, with zero queue drops, evidence faults or I/O
  errors. Native QPC evidence records **one Arm journal commit**, while both
  actual down/up deliveries retain **three commits each**.
- In this one audit, Arm's commit took 6.04 ms. Down pre-dispatch took 15.62 ms;
  release ACK persistence took 44.85 ms. The receiver observed a 16 ms down/up
  span for a requested 1 ms dwell. These observations identify remaining
  synchronous journal cost; they are not latency percentiles or a timing pass.
- The prior isolated Cockpit configuration was restored with auditing disabled.
  Physical VKB activation/replacement/quiet liveness/Stop and actual vJoy Raw
  Input delivery regressions pass. The receive-only process exited.
- All 1,156 tracked Rust/manifests/locks match HEAD in the isolated candidate
  except the four intended edited files. The new Arm test file matches it.
  Unrelated worktree changes remain excluded. Final native executable SHA-256:
  `170bdd6db8e10128d687980235894ce5e74fa6ad5359980fb147ee02f86015fa`.

Audit artifacts are in `.research/reactive/desktop-arm-audit`, with the parent
checkpoint in `desktop-arm-audit-checkpoint.json` and per-request timing summary
in `desktop-arm-journal-summary.json` beside it. Receive-only evidence is
`.research/reactive/desktop-deadlines-native/1793859735488/evidence.json`.
Logs and coverage are under `.research/reactive/desktop-arm-*`.

Further reduction of post-ACK/owner commits, nonblocking desktop dispatch,
focus/child notifications and full routing qualification remain software work
under [#193](https://github.com/opensimgear/cockpit/issues/193). Physical latency
and simulator acceptance remain in [#199](https://github.com/opensimgear/cockpit/issues/199).
CI remains deferred until feature implementation is complete, as requested.
