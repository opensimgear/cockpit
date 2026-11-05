# Desktop audit capacity notifications — 2026-11-05

This milestone removes the original audit barrier's 1 ms queue-capacity retry
poll under #188/#193/#198. It does not qualify physical controller latency or
complete the remaining acquisition migrations.

## Behavior

An audit checkpoint or finalization retains its original absolute deadline.
When the bounded writer queue is full, it waits on a capacity generation captured
under the same mutex used by dequeue notifications. A dequeue racing the Full
result cannot be lost between the queue check and sleep. Event producers retain
their bounded nonblocking try-send path; they do not wait on the new capacity
condition variable.

Writer I/O failure terminates the writer. An exit guard drops its receiver before
waking waiters, including during panic unwinding. A barrier already queued behind
a failed event sees reply disconnection immediately: its original reply sender
is dropped after enqueue rather than keeping its own receive channel alive.
Timeout, I/O failure and incomplete evidence remain distinct. Concurrent
finishers can enqueue only one footer; they are not all promised a completed
reply before that writer finishes.

The writer loop and native process adapter now live in separate collector
modules. Every changed production file is under 500 lines; the collector itself
is reduced from its inherited oversized implementation to 451 lines. Audit
records, schemas, bounds and native submission authority are unchanged.

## Local checks

The isolated candidate is HEAD `431141e5` plus this milestone's owned files.
It excludes the 108 unrelated modified files and the prepared, unfinished native
helper-scope storage files. Unrelated hashes remain preserved.

- **149 desktop-output tests pass**, including full-queue dequeue wake, original
  timeout, writer error/disconnect, queued finish reply loss after error/panic,
  and concurrent finalization with one footer.
- **149 mapping-host tests pass**.
- **2,289 native Cockpit tests pass**, with 17 existing ignores.
- All **41 permanent production coverage gates pass**: 22 desktop-output and
  19 mapping-host. Audit collector: 96.67%; capacity/termination: 98.21%; writer:
  97.65%; native file/process adapter: 84.88%.
- Isolated parent/worker process tests create actual local audit files, use real
  QPC timestamps, validate process linkage, checkpoints, final footers and alias
  privacy. The coverage script explicitly enables the debug audit build flag;
  the tests do not construct a native input injector or author input.

Review caught a queued-reply sender that would prevent disconnection from being
observed, and the regression now exercises that exact failure. Initial fixture
corrections made its blocked writer pause only the first of two queued events,
and used the actual gateway's nested desktop envelope. Final checks above pass.

## Actual running Cockpit

Locked/offline Windows build passes. SHA-256:
`5c9d67ce3fca4be3838deb08f9617fce347f8e607941173e49ae36a86032603d`.
Audited app PID: 36856; final audit-disabled PID: 47608.

The running application accepts 64 ordered numeric markers and returns complete,
valid checkpoints with zero drops, I/O errors, flush timeouts or evidence faults.
A neutral profile also creates, replaces and stops its actual private desktop
workers. Final parent evidence is a **140-record checkpoint prefix**; all **four
worker files have valid completed footers**, exact sequence continuity and the
correct parent PID, process instance and channel. This does not claim a parent
process-exit footer or actual queue saturation in the app: saturation and blocked
writer failures are exercised by the isolated unit fixtures.

Actual audit marker/checkpoint and neutral lifecycle fixtures pass with auditing
enabled. Neutral lifecycle passes again with auditing disabled. The final
checkpoint API correctly rejects an unconfigured audit. Profile ownership is
stopped, held/possible-held controls are empty, uncertainty is false, and all
three virtual slots are free. No non-neutral input is authored.

Artifacts: `.research/reactive/audit-capacity-*`, including final evidence,
audited and disabled runtime identities, local coverage logs and the original
native files under `audit-capacity-live-audit`.

## Remaining scope

Private helper focus notifications and immutable admission proof across scope
epochs are still pending; preparation is not recorded as delivery. Host service
cutover, first-dispatch/burst and full load/fault qualification remain in the
original issues. The previous controlled receiver cursor-startup failure and two
report-dependent live failures remain unresolved; this audit milestone does not
substitute for them.

Original acceptance remains 13/55 verified and 42 unchecked: 29 software or
qualification/governance, 13 user-operated. Physical acceptance stays in #199,
first in Progress. CI remains deferred until implementation is complete.
