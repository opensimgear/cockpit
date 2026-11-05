# Companion peer I/O and direct cancellation

The server no longer wakes every 100 ms to check an idle WebSocket. Each peer
owns its original nonblocking Mio stream, Poll and cancellation Waker. Native
client I/O retains its existing separate path; its read/status loops remain
open #195 work.

Authenticated browsers retain a two-second Ping opportunity and five-second
idle deadline. Native/unauthenticated connections retain five-second idle.
Matching Pongs prove socket liveness only; panel input authority still requires
its application lease. Incomplete TLS/WebSocket frames do not renew deadlines.
Raw reads retain the same absolute deadline through partial progress, readiness
and interruption. A completed message is checked against idle expiry before
dispatch. Subscription handoff clears the read deadline for event-only feedback.

Mio interest follows the current read/write direction. Keeping writable interest
armed while waiting for input can repeatedly wake a Windows socket. Tests count
idle polls to catch that failure. Cancellation stores its flag before waking
Poll and returns terminal ConnectionAborted; Interrupted would be retried inside
rustls. Only the peer owner drops the socket.

The 16-entry cancellation registry closes admission and takes its controls under
one mutex, then cancels outside it. Registration racing Stop is cancelled too.
Worker cleanup retires Hub authority and admission capacity on normal return or
unwind. Hub disconnect wakes feedback/assignment readers parked off-socket.

The absolute three-second handshake deadline remains. Event writes retain a
500 ms drain deadline through partial writes until successful transport flush.
That is a TLS drain bound, not a whole WebSocket-message delivery guarantee.
Read expiry is independent of writes. Slow application/network qualification
and physical/simulator timing remain explicit original acceptance work.

Local evidence, 2026-11-05:

- All 217 companion tests pass. New coverage exercises read/write cancellation,
  register/Stop races, late admission, independent fresh owners, bounded control
  handles, quiet read without periodic timeout, direction changes without idle
  spin, backpressured writes, partial-frame expiry and event writes after read
  expiry. Existing browser matching-Pong and real TLS/WSS tests pass.
- The stalled-handshake Stop regression is tightened from six seconds to one
  second and passes. The actual rebuilt Cockpit disables 16 simultaneously
  stalled TLS peers in **26.7 ms** in one recorded local run; all 16 close and
  the connection count returns to zero. This is software Stop evidence, not a
  percentile or physical input/output latency measurement.
- All 36 companion production coverage gates pass, minimum 73.61%. Cancellation
  registry and deadline wrapper: 100%; socket owner: 93.75%; listener: 91.57%;
  server: 83.75%. The gate now uses the shared parser to exclude test-only poll
  counters and include production fragments. All 17 parser regressions pass.
- All 2,289 native tests pass with 17 existing explicit ignores. Companion serial
  passes 46 tests; companion platform passes one. The locked Windows production
  build passes.
- Five actual-Cockpit checks pass: listener settings/retry/idle/disable;
  16 stalled peers; compiled-browser WSS input/feedback/reload/Stop; delayed
  physical preview recovery; physical reader replacement/quiet liveness/Stop.
- Final cleanup: listener disabled, zero active connections, no retained test
  companions/assignments, profile stopped with no owner. All 1,162 archived
  native source/SQL/manifest/lock records match HEAD except intended edits;
  all 12 owned source/test/config files match the candidate. All 108 unrelated
  modified files retain their original hashes. #199 remains first in Progress.
- Independent concurrency review and focused architecture review completed.
  The 247-site disposition checker passes; full inventory/migration acceptance
  remains open.

The first 213-test run passed 211 and failed direct blocking-read cancellation
and stalled-TLS Stop. A focused original-handle shutdown experiment also failed
on this Windows host. Both findings led to the event-backed owner; the final
Stop bound is unchanged at one second. The final full 217-test run and the later
production-parser coverage run pass. Failed attempts remain in the artifacts.

App SHA-256:
`02f72e8df56921748c09205d3fbc74d1a5cac594bd51fce2f64d1af3a2fb4044`.
Artifacts: `.research/reactive/companion-connection-*`; the 16-peer receipt is
under `.research/reactive/companion-stop-native/`. The existing dev frontend is
used; unrelated frontend edits are excluded. Other operating systems, boards
and actual simulators remain unqualified. CI stays deferred until implementation
completion as requested. [Remaining acceptance](../reactive/remaining-acceptance.md)
still records 13/55 verified criteria and 42 unchecked, not an effort estimate.
