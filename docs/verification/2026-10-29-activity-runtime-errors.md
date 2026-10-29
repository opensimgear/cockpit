# Activity runtime error investigation — 2026-10-29

The retained Activity history contained 1,680 warnings/errors at inspection,
including 1,302 standalone `aux-panel feedback: Feedback rate exceeds30Hz.`
warnings and further occurrences combined with other degradation reasons.
The active DCS Feature Lab repeatedly alternated between running and degraded.

Feedback scheduling used the reducer's microsecond clock while the companion
Hub enforced its 30 Hz bound against monotonic elapsed time. The profile adapter
now uses `Instant` for its 34 ms interval. Consuming a current worker completion
restarts that interval, so slow validation followed by fast validation cannot
compress two publications together. Failed admission, failed jobs and expired
samples also back off. Revocation and superseded-completion fences remain in
place, as does the Hub rate limit. The diagnostic now reads
`Feedback rate exceeds 30 Hz.`

The process observer also conflated lock contention with missing/stale evidence.
Its cached read now reports those outcomes separately. A busy observer leaves
the accepted desktop scope alone without adding a degradation warning; genuinely
unavailable evidence still reports its existing warning, and a confirmed process
exit still clears the scope. The native desktop worker continues to validate
the exact target before submitting input.

Other history included real companion/controller disconnects, aircraft changes,
unavailable telemetry and Windows input rejection for a higher-integrity
foreground application. The asynchronous game-identity validator's 100 ms
freshness bound was retained; this investigation did not relax identity checks.

## Verification

- Companion service: 189 tests passed.
- Focused native feedback and activation regressions passed, including slow
  validation, reducer-time jumps, expiry/failure retry backoff, resumed publication,
  and deterministic busy/missing/stale/confirmed process observations.
- Native coverage passed the 71% gates: feedback 94.69%, companion adapter 83.89%,
  process observer 98.55%, observer runtime 79.61%.
- Frontend type/fixture checks passed with zero Svelte errors or warnings.
- Full native library suite: 1,639 passed, one unrelated topology fixture failed
  because it still uses the removed `hold` transform, 11 ignored.
- Broad frontend suite: 1,596 passed, 16 failed in existing editor/canvas changes.
  These files were outside this fix and remain uncommitted.
- The native application was rebuilt and the actual DCS Feature Lab revision 9
  restored with its paired browser companion. A 120-second live soak recorded
  553 running samples, zero degradation reasons and zero new Activity warnings;
  the companion stayed connected and armed. Evidence is saved under
  `.research/activity-errors/native-soak.json`. Repeat with
  `OSG_FEEDBACK_RATE_NATIVE=1 node --test tests/ui/profile-feedback-rate-native.mjs`
  against the active native DCS session with CDP enabled on port 9222.

The source change is limited to feedback timing, cached observer outcomes,
diagnostic spacing and their regressions. Existing workspace changes are excluded
from the commit, including the unrelated peripheral-policy edit in
`profile_session_host.rs`.

## Remaining runtime errors

The follow-up found five identity-validation warnings, a virtual-output helper
deadline failure, and repeated unavailable-sample warnings during baseline
acquisition. Windows also reported one access failure and one ambiguous process
match around a DCS restart. A 25-sample production inventory benchmark against
both configured DCS runtime paths found exactly one live process each time, with
no access errors: 17.039 ms median, 19.072 ms p95 and 20.072 ms maximum.

The identity worker previously scheduled another full process scan as soon as
each scan completed. Its 100 ms freshness window also began when work was queued,
before periodic catalogue parsing and native build validation. Live timing traces
measured 20–28 ms of catalogue work followed by a 12–27 ms process observation;
combined work and heartbeat jitter intermittently expired otherwise healthy
process evidence. Process and catalogue verification now have independent,
reader-owned workers. Process evidence refreshes after 25 ms and expires at
100 ms. Catalogue metadata checks refresh after 125 ms and retain the existing
250 ms metadata horizon and two-second content-check policy. They force real
file checks and timestamp before I/O. A slow catalogue check cannot starve native
process checks; failure or expired evidence in either worker still withdraws
the affected branches. Both workers are cancelled when the reader is retired.
The process timestamp is captured immediately before native observation. Setup,
catalogue, vehicle, preparation, installation and active-owner fences are checked
again before the result is accepted. Library names and artwork do not revoke
process evidence.

Input health now retains unavailable state samples between telemetry packets.
An idle heartbeat cannot report recovery while the source is still unknown;
valid samples clear the warning. Expected optional absence and quiet event ports
remain healthy. The mapping reducer's output revocation behavior is unchanged.

Virtual-output renewal now measures its interval from request start instead of
acknowledgement receipt. This preserves lease margin when acknowledgements take
time. The 100 ms IPC deadline, 250 ms worker lease and 50 ms renewal interval are
unchanged. Supervisor diagnostics distinguish missing replies, closed readers,
and acknowledgements consumed after their deadline, and include bounded request,
sequence, elapsed time and retained process status before termination.

The old live session hit its heartbeat watchdog during heavy native testing and
required desktop recovery before restart. Explicit recovery completed. Genuine
process ambiguity, access failures, disconnects and uncertain cleanup still stop
or withdraw affected work; these checks are not suppressed.

Follow-up validation:

- Native library: 1,652 passed, one existing `hold` topology-fixture failure,
  12 ignored; all new regressions passed.
- Virtual controller: all 105 tests passed. Changed-module coverage is 97.41%
  for the supervisor and 92.68% for the session; every virtual-output gate passed.
- Input health coverage: 99.17%. Proof workers: 98.18%. Native validation:
  100%. Context entry point including its validation fragment: 89.41%.
  Game reader including preparation, polling and delivery: 91.11%.
- Coverage tooling: six tests passed. Frontend type/fixture checks passed with
  zero Svelte errors or warnings.
- Strict live acceptance uses `OSG_RUNTIME_HEALTH_NATIVE=1 node --test
  tests/ui/profile-feedback-rate-native.mjs` against the rebuilt DCS session.
  It passed over five minutes with 1,377 running samples, no degradation reasons
  and zero new Activity warnings or errors. The paired companion remained
  connected and armed. The result is saved in
  `.research/activity-errors/runtime-health-soak.json`.
