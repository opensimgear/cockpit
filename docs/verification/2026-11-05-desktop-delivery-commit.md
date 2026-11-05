# Desktop delivery and owners commit together

Successful desktop delivery now installs its validated reducer/owner projection
inside the existing acknowledgement transaction. Hold-changing delivery uses
two commits instead of three: the conservative possible-hold record before
dispatch, followed by the acknowledged controls and complete owner projection.
Worker-skipped Held delivery resolves cancellation from the original reducer
before that final commit.

The callback runs only after acknowledgement identity, counts, held controls
and success/uncertainty validation. Callback or final-write failure fences
further delivery with uncertainty and incomplete cleanup. The last durable
record remains sufficient for explicit recovery. Pointer operation counters
remain volatile, and unchanged pointer owners produce no successful writes.
Arm keeps its single final commit; other lifecycle boundaries are unchanged.

Local evidence, 2026-11-05:

- All 71 isolated desktop-output tests pass, including atomic down/up/shared
  owner records, pre-dispatch possible holds, skipped delivery, callback
  rejection/failure and failed-commit recovery. Existing pointer tests pass.
- All 10 focused native tests and the complete native suite pass: 2,286 passed,
  zero failures, 17 existing explicit ignores. Coordinator production line
  coverage is 91.16%; unchanged journal coverage is 79.32%.
- One initial shared-host run passed 124/125 and failed the existing companion
  isolation test while the full native suite was also running. Its healthy
  route reported `running: false` without further state. The assertion now
  prints that state. A focused run passed, then all 125 shared-host tests passed.
  The root cause is unconfirmed; these reruns are not a defect fix or a waiver
  of combined-load qualification under #198.
- The rebuilt Cockpit's exact receive-only window receives down/up from one
  authored update, and Stop proves complete, certain cleanup. Native audit
  verifies one Arm commit and two commits per actual down/up delivery. Parent
  checkpoint and worker footer are valid/complete with zero evidence/I/O faults
  or dropped audit records.
- One trace recorded a 50.03 ms pre-dispatch commit. Worker QPC call-return
  timestamps for down and up were 11.07 ms apart for a requested 1 ms tap. The
  receiver's coarse timestamps reported zero. These are not physical latency
  percentiles or a low-latency pass; synchronous pre-dispatch I/O remains work.
- The prior isolated configuration was restored with auditing disabled.
  Physical VKB activation/replacement/quiet liveness/Stop and actual vJoy Raw
  Input delivery pass. Independent real MCP readers, including one left unread
  after baseline, receive changes/retirement and do not prevent Stop. Temporary
  receiver processes exit and the vJoy slot returns free.
- The isolated candidate's 1,157 tracked Rust/manifests/locks differ from HEAD
  only in the four intended edited files. The new delivery test file matches
  it. Unrelated worktree changes remain excluded. Final native SHA-256:
  `ac7879fe7a7be6a816317b5b81ad30afecafbf80553b55df9a00988663eb1b97`.

Artifacts are under `.research/reactive/desktop-delivery-*`. Audit records are
in `desktop-delivery-audit`, with the checkpoint, journal count summary and
native span in their adjacent JSON files. Receive-only evidence is
`.research/reactive/desktop-deadlines-native/1793860787352/evidence.json`.
Nonblocking dispatch, focus/child notifications, remaining adapters and full
qualification stay open under #188/#193/#198. Physical latency and simulator
acceptance remain [#199](https://github.com/opensimgear/cockpit/issues/199).
CI remains deferred until implementation is complete, as requested.
