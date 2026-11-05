# Desktop host wakes and foreground identity

## Delivered behavior

Native desktop profiles with qualified HID/internal input sources can now omit
the host's 5 ms adapter-service interval. The policy checks both saved and
executable documents, exact run ownership, retained input wakes and healthy native
desktop scope subscriptions. Desktop and virtual-controller sinks are supported;
game/vehicle, companion, physical action, visibility and other unqualified
adapter combinations retain their explicit service fallback.

Input capture, parent scope changes, helper acknowledgements and output
completions retain direct wakes. Reducer freshness, isolated desktop shadow
freshness, tap/repeat/pointer transitions and authorization retain exact
deadlines. The read-only desktop runtime reports `serviceIntervalMs`; null means
the active scheduler uses wakes/deadlines without an adapter-service interval.
Portal and MCP use the same gateway state.

The ordered output owner now schedules its actual 100 ms renewal deadline even
when no parent authorization is pending. It requests a newly evaluated parent
decision once, then waits for a response. A newly stale response permits one
replacement request. Freshness remains 5 ms and a consumed authorization cannot
renew again. Stop/fault retire renewal requests; healthy paused registrations
retain renewal without rearming. A stalled parent cannot keep the helper alive
through autonomous renewals. Quiet helper failure is still detected at the next
bounded renewal request; this is not a dedicated transport EOF wake.

The recent-application chooser no longer calls foreground acquisition on every
profile-worker iteration. A separate bounded `ForegroundWatch` shares the
existing native WinEvent/WTS owner. Atomic subscription and sequenced snapshots
fence older initial seeds. The small cache preserves the last external process
identity when Cockpit takes focus and reports observer failure explicitly when
capturing an application. It computes Cockpit's executable once at startup.
Scope-revocation callbacks precede optional chooser callbacks. Chooser callback
panic or sequence exhaustion does not poison scope watchers. The profile-worker
closure owns the watcher and retires it during shutdown.

Terminal notifications wake the profile worker to retire that watch and attempt
one replacement for the failed generation. A failed startup leaves an explicit
error without a timer retry. Retained diagnostic leases cannot pin a completed
native owner: replacement waits outside the registry lock for native cleanup.
Retirement also fences shutdown posts before native resource teardown so a
reused Windows handle cannot receive an old owner's shutdown message.

## Verification

An isolated main fa5ecac2 candidate contains only this milestone's changes.
Desktop-output tests and coverage include initial identity/None/error updates,
bounded subscriptions, teardown, sequence exhaustion, callback priority and
actual shared native observer lifetime. Mapping-host tests cover missing, early,
expired and consumed authorization, exactly one request per response, paused
renewal and terminal request retirement. Native scheduling tests reject missing
subscriptions while preserving the unrelated-adapter fallback.

Final local checks pass: 191 desktop-output tests, 165 mapping-host tests and
2,294 native tests (zero failures, 17 existing native ignores). All 54 affected
production line coverage gates pass: 29 desktop, 21 host and four native.
Foreground subscription coverage is 91.3%, native lifetime 100%, native registry
96.97%, host renewal worker 99.33%, scheduling policy 87.88% and application
foreground cache/startup 76.74%. All 17 coverage-parser regressions and all 247
original timing dispositions pass. The full native suite includes the actual
foreground-observer startup smoke test; dedicated native tests also exercise
retained terminal leases, replacement after cleanup and reused-handle fencing.

The production Cockpit build passes. The actual audit-disabled runtime is
PID 51112, executable SHA-256
`5a1692faa0c27279f63a52f54e37982f06cff6a9919b41538157154aa9d007f3`.

The running-Cockpit lifecycle check observes `serviceIntervalMs: null`, a live
helper receipt, and no held inputs. It then makes no gateway reads and authors
no input for 1.1 seconds, longer than four helper lease periods. The same helper
registration/epoch remains live and uncertainty stays false. Replacement uses a
new scope subscription; Stop removes it and cleans ownership. The shared native
controller observer/reopen check and the separately enabled passive physical
source activation/replacement/Stop check also pass. The final profile is stopped,
held/possible-held lists are empty, uncertainty is false and all virtual slots
are free. These checks qualify software lifecycle and quiet renewal, not
physical end-to-end input timing.

The controlled two-window focus test and report-dependent preview/Raw Input
checks subsequently pass after Windows is unlocked; see [updated evidence](2026-11-05-unlocked-native-checks.md).
Sustained latency qualification is interrupted by WSL foreground takeover.
First-dispatch/burst timing and mixed fault/load matrices remain in #193/#198. User-operated physical
preview/raw-to-virtual and simulator effect qualification remain in #199, first
in Progress. This milestone alone closes no broad original acceptance criterion.

CI remains deferred until feature implementation is complete.
