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
