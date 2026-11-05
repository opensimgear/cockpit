# Desktop parent scope notifications — 2026-11-05

This is a software milestone under #188/#193/#198. It does not complete the
focus acquisition cutover or qualify physical input latency.

## Delivered behavior

Parent desktop profiles use one shared native observer per process with bounded
scoped subscriptions. Exact out-of-context foreground and desktop-switch hooks
retain original HWND records; an owned hidden window receives current-session
WTS changes. Registration precedes the initial snapshot. Ordered loss epochs
retain an away/return boundary even when the consumer reads only the final
eligible state. Same-process window changes and eligible global-scope changes
preserve current ownership.

Callbacks revoke queued delivery and Arm authority in memory, clear pending
authorization and schedule ordered Pause without performing IPC. Watch identity
and sequence fences reject retired callbacks. The profile consumes each retained
loss before admitting new work and requires fresh neutral observations. A
revoked Arm consumes its channel sequence, releases the run and acknowledges a
skip without authorizing its old baseline. Parent-authorized renewal preserves
healthy paused registrations beyond the 250 ms helper lease; it does not arm
input or authorize renewal from stale parent evidence.

Source overflow, callback panic and terminal owner failure fail closed. Transient
inspection errors remain an inspectable healthy scope pause. Native registrations
retire on their owner thread; lease retirement precedes bounded teardown. A
retiring observer cannot overlap a replacement native owner. The existing desktop
status, available through the manual gateway and MCP, includes bounded scope
notification identity, sequence, loss epoch, eligibility and health.

## Local verification

The isolated candidate is HEAD `2b4f4817` plus the owned changes. It excludes
108 unrelated modified files, whose hashes remain preserved.

- Desktop-output: **142 passed**. All **18** production coverage gates pass;
  native scope modules measure 96.55% (API/lifetime), 96.32% (state) and 82.56%
  (Windows owner).
- Mapping host: **149 passed**. All **19** production coverage gates pass;
  parent scope integration measures 98.51% and callback admission revocation
  98.33%. Real native subscriptions are exercised with a recording adapter that
  rejects any attempt to submit input.
- Full native Cockpit library: **2,289 passed**, **17 existing ignores**.
- Coverage parser regressions: **17 passed**. All **247** pinned timing sites
  still have exactly one disposition.
- Locked/offline Windows Cockpit build passes. CI remains deferred until feature
  implementation is complete, as requested.

The first host fixture attempted to replace an immutable captured application
scope and correctly failed. The fixture now replaces the watch for the same
scope. A subsequent assertion incorrectly required empty diagnostics while
Windows process inspection could truthfully report a paused scope; the final
test preserves the paused/no-input assertions and accepts those diagnostics.
Review also caught paused registrations losing renewal; the correction is
covered past the original lease by a controlled recording clock.

An initial coverage parser run used a restricted system temporary directory and
failed with access-denied errors. Both parser suites pass using a writable
workspace temporary directory; no production change was made for that failure.

## Actual running Cockpit

The rebuilt application runs from the isolated disposable dev runtime, PID
12252, with audit disabled. SHA-256:
`b5dc9068417f072e510c2532010c119ae20ca4713b54c4225e6a8e4096ba6101`.

`desktop-focus-lifecycle-native.mjs` passes against the actual native application.
A constant-neutral profile creates a healthy native scope subscription, replaces
the applied profile with a distinct watch ID, and retires it on Stop. Both runs
retain empty held state; final cleanup is complete without uncertainty or
possible holds. No non-neutral input is authored. Evidence:
`.research/reactive/desktop-focus-lifecycle-native/1793891091426/evidence.json`.

The source-reader activation/replacement/Stop and shared native observer/reopen
fixtures also pass. Two other live fixtures **fail** in this session: the preview
delayed-receipt check receives no advancing physical reports, and the vJoy Raw
Input receiver records no matching device reports. Their causes are unresolved;
earlier passes are not substituted for these results.

The controlled two-window focus/hold fixture is implemented but **blocked at
receiver startup**: Windows rejects its cursor-centering operation. Its initial
profile draft also used an invalid 10-second action contract; that fixture was
corrected to the required 1-second contract before the startup failure. A
separate read-only probe reports an eligible interactive foreground with no
scope error, so a locked desktop is not established as the cause. No controlled
focus-switch or physical latency acceptance is claimed.

Artifacts use `.research/reactive/desktop-focus-*`. The runtime remains stopped
at the profile level, and temporary profiles/preferences are cleaned up.

## Remaining original work

The private helper still samples focus on its 5 ms safety watchdog. Its native
notification owner and admission proof across parent/helper scope epochs remain
to be integrated. Host 5 ms service also remains; the new explicit parent
authorization deadline prepares its later removal. A callback revokes parent
admission immediately, while actual held cleanup still depends on the helper
watchdog or ordered Pause; this milestone does not make that cleanup independent
of parent journal stalls.

Controlled focus-switch qualification, full fault/load matrices, first-dispatch
and burst timing remain software work in #193/#198. Physical controller and
simulator qualification remains in #199, first in Progress. Acceptance accounting
is unchanged: 13 of 55 original criteria verified, 42 unchecked (29 software or
qualification/governance, 13 user-operated). This count is not an effort estimate.
