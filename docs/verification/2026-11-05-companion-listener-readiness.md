# Companion listener readiness and expiry

The companion listener no longer probes `accept` and Hub expiry every 20 ms.
One Mio Poll owns socket readiness and a shared Waker for Stop, earlier committed
deadlines and completed peer workers. With no connection or expiry work it waits
without a timer. The Waker remains alive for the owner's lifetime, following
the [Mio ownership contract](https://docs.rs/mio/1.2.4/mio/struct.Waker.html).

Hub deadlines cover pairing offers, short-code offers, pending approvals and
session leases. Preparing panels retain their 30-second lease; ordinary armed
sessions retain 1.5 seconds. Mutation guards inspect the committed minimum under
the existing Hub lock and wake only for a new or earlier minimum. A pure input
lease extension leaves the previously armed earlier deadline to recompute once;
it does not add an OS wake per input report. Observer registration is bounded
to 16 weak leases and rejects capacity exhaustion explicitly.

Socket readiness drains to WouldBlock, yielding after each 16 accepts to process
Stop and expiry during a connection flood. Peer admission remains capped at 16.
Completion wakes reaping and releases admission capacity even during unwinding.
A terminal listener I/O/spawn failure records an error, disconnects authority
and wakes feedback readers before joining workers. Native peer I/O retains its
existing handshake/write/idle bounds.

This removes listener acquisition polling. It does not remove the native
pairing-status loop, the per-connection read-timeout loop, legacy companion
routing polling or physical companion firmware loops. Those remain #195 scope.
The accept change concerns connection setup and idle work; it is not a measured
reduction in physical controller-to-output latency.

Local verification, 2026-11-05:

- All 206 companion service tests pass. Real TLS/WSS tests cover pairing,
  independent multi-panel routing, publication transfer, slow consumers,
  malformed peers, reconnect and bounded shutdown. New tests cover quiet
  indefinite waits/direct interruption, multiple accepted sockets, exact minimum
  expiry, bounded observer admission, lease-extension wake suppression, and
  actual listener-owned shortened-lease expiry without status or manual ticks.
- All 33 companion coverage gates pass, minimum 73.61%. Changed production
  modules: listener 93.33%, deadlines 98.53%, server 85.11%, Hub 93.37%.
- All 2,289 native Cockpit tests pass with 17 existing explicit ignores.
  Companion serial passes 46 tests; companion platform passes its one test.
  The locked offline Windows production build passes.
- The actual rebuilt Cockpit passes listener settings/occupied-port retry/idle
  observation/disable, real compiled-browser WSS input and returned feedback,
  browser reload recovery and Profile Stop. Delayed physical controller preview
  recovery and physical input replacement/quiet liveness/Stop also pass.
- Final cleanup confirms listener/configured state disabled, zero active
  companion connections and a stopped profile with no owner. Test publication
  assignment/pairing/profile cleanup completed.
- All 1,158 archived native source/SQL/manifest/lock records match HEAD except
  the intended edits. All 13 owned source/config/test files match the tested
  candidate; all 108 unrelated modified files retain their original hashes.
- Independent concurrency review found no actionable issues. #199 remains first
  in Progress. Stale project statuses for #197/#198 are corrected to In progress;
  #190 is already completed and closed.
- The disposition checker passes: all 247 pinned timing sites retain exactly
  one classification. This checks ledger accounting, not full migration completion.

The initial 205-test run failed a new test fixture that recreated a second
Waker for the same Poll. Each test Waker now has its own Poll; the full corrected
206-test run passes. Unfiltered offline metadata inspection required an uncached
dependency; Windows-filtered resolution and every locked Windows build pass.
Other operating systems and actual boards/simulators remain unqualified.

App SHA-256:
`14ab7321e263a5c136440936af014d891bee76837208bea4cc90644acf33b988`.
Artifacts: `.research/reactive/companion-readiness-*`, including coverage,
native/consumer/build logs, four actual-Cockpit checks, source proof and cleanup.
The existing dev frontend is used; unrelated frontend edits are excluded.
CI remains deferred until feature implementation is complete, as requested.

The [remaining acceptance ledger](../reactive/remaining-acceptance.md) keeps all
other original software requirements and user-operated qualification explicit.
13/55 criteria are verified; the 42 unchecked criteria are not an effort estimate.
