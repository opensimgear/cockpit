# Desktop releases verify retained evidence before dispatch

This removes one measured desktop dispatch cost under #193. A validated
Held/Cleanup delivery whose projected combined controls are a subset of the
acknowledged held set cannot acquire a native hold. Its previous durable record
already covers every possible remaining hold, including shared owners.

The Coordinator verifies that exact retained evidence before dispatch instead
of writing an intermediate release record. The ACK must not report any new held
control. Successful ACK/owner persistence remains mandatory, as do failed,
rejected and uncertain delivery records. New control acquisition still writes
possible holds before dispatch and commits the validated ACK afterward. Parent
dispatch revocation and worker focus/lease checks remain on their existing path.

If the parent dies during release, the old record conservatively retains the
prior holds. Failed final persistence likewise leaves recoverable evidence and
fences later output; it cannot become successful cleanup. No input is replayed.

## Local verification

- All 91 desktop-output tests pass, including seven new tests covering captured
  durable bytes at dispatch, empty/subset/Cleanup releases, shared-owner changes,
  new acquisitions, lost/rejected/uncertain/forged ACKs, failed final persistence,
  stale evidence and explicit recovery. A real locked SQLite writer no longer
  delays native release, while its final persistence still fails truthfully.
- All ten production desktop coverage gates exceed 71%. Coordinator: 92.22%;
  journal: 79.32%. All 17 coverage-parser tests and 247 pinned dispositions pass.
- All 138 shared-host and 2,289 native Cockpit tests pass, with 17 existing native
  explicit ignores. Locked offline Windows app and receiver builds pass.
- The audited running-Cockpit tap/Stop check passes. With audit then disabled,
  five final native checks pass: tap/Stop, delayed preview catch-up, physical
  source replacement/quiet liveness/Stop, shared preview/reopen, and real vJoy
  Raw Input delivery with independent MCP observation and Stop.

The audit confirms **two commits for down and one for release**. The worker's
native-call-return span is **6.8949 ms**, and the receiver observes **5,743 us**
for the authored 1 ms tap. Earlier precise observations were 9.9087 ms and
8,729 us respectively; the fresh receive-only baseline before this change was
8,593 us. These are individual software observations with different timing
boundaries, not percentiles or physical latency qualification. The remaining
post-down ACK commit still delays release; **1 ms timing remains unqualified**.

Parent checkpoint and worker footer are valid and complete: 39/39 parent and
20/20 worker records, zero drops, evidence faults or I/O errors. Evidence is
under `.research/reactive/desktop-release-*`, with the audited receiver result
in `desktop-deadlines-native/1793881606473/evidence.json` beneath that directory's
parent. The audit summary records per-request commit counts and QPC spans.

App SHA-256:
`afdf7324299e6d064d55cb577d291feace081695a5ee26fecaf1956ee98ed461`.
Receiver SHA-256:
`50dfff2ba3318f3d5a997dcbdb649c78f944746db9fe16f79790a638b78070b3`.
Audited Cockpit PID: 12480; final audit-disabled PID: 35264. Both use the isolated
development runtime. Temporary profiles, readers, receivers and desktop helpers
retire; the virtual slot returns free and profile ownership is stopped.

## Corrected checks and remaining work

The first unit run passed 86/91. Four new test failures came from fixture state
expectations and attempting recovery before dropping the old journal owner.
One existing test expected release to wait before native dispatch; it now checks
the native release followed by a truthfully failed final write under contention.
The corrected full run passes without production changes after those failures.

Initial baseline attempts reached the WebView before initialization; setup now
waits for its native invoke bridge. The old binary had audit collection compiled
out, so its configured-but-disabled checkpoint is not counted as audit evidence.
The changed build enables the existing development audit flag; the final runtime
removes audit configuration. A summary initially used the wrong audit event name;
it now reads the actual `SubmitReturned` records and validates footer reports.

Disk exhaustion interrupted initial app/native compiler jobs. Only verified
rebuildable incremental compiler caches inside this workspace were removed,
after their compiler jobs became terminal. Source, unrelated work and evidence
were preserved. Final builds/tests ran with incremental generation disabled.

Further desktop timing reduction, focus/child notifications, remaining adapters,
fault/backpressure matrices and integrated qualification remain original software
work. Physical controller raw-to-preview/raw-to-virtual timing and simulator
acceptance remain #199, first in Progress. CI remains deferred until feature
implementation is complete. This delivery does not close an acceptance milestone.
