# Profile deadline scheduling foundation — 2026-11-02

R4 #193 / parent #188 remain in progress. This milestone exposes actual reducer
deadlines and removes avoidable empty-batch graph work. It does not complete the
producer, service, routing or UI cutover.

## Changes

`Phase::next_deadline_us()` shares the exact minimum of source freshness,
built-in transitions and script timers with `advance()`. Deadline lookup cannot
diverge from execution. Stop/fault removes schedulable deadlines; expiry still
revokes dependent work before a same-time timer, without removing later work
from independent branches.

Empty active batches return before cloning source state, traversing dependencies
or canceling graph work. Bounds and stopped-state validation remain enforced.
Queued reports/effects, script callbacks, provenance, time and pending timers
remain unchanged. Native health also skips empty admission.

The current active owner exposes an absolute host wake. The profile worker parks
for the earlier of that deadline and its remaining adapter service interval.
Time spent in host work does not restart the deadline. Clock sampling before the
native registry lock makes any contention estimate conservative. Existing
epoch/unpark handling preserves reports arriving during service or before park.
Stopped, faulted and launch-held owners cannot supply a routing deadline.

Health now reads the controller journal's current monotonic admission clock,
rather than reconstructing it from an independently sampled activation Instant.
Liveness remains distinct from sample capture provenance. The journal admission
clock is not an OS/HID hardware capture timestamp and cannot establish physical
input latency.

## Verification

- Isolated instrumented mapping crate: **143 passed**, zero failed. All 15
  production module line gates pass the 71% floor; typed reducer 93.66%, new
  deadline selector 100%. Seven focused deadline/empty-batch regressions cover
  minimum selection, inclusive expiry, liveness/provenance, cancellation, stop,
  independent future timers and queued/in-flight scripts.
- Isolated instrumented native suite: **1,803 passed**, zero failed, 17 explicit
  opt-in/helper checks ignored; 94.67 seconds with four workers.
- Scheduler production coverage: **100%**. Existing coordinator 92.62%, native
  host 79.34%, gateway 83.95%. Coverage harness regressions: nine passed.
- Native production executable builds. SHA-256:
  `4c96c5aaa49dbaa8925f4acaa61c2e537f24a3aa81bb983a6129d9538be8d225`.
- Actual running Windows Cockpit, 21:06 UTC: supported create/save/apply of an
  authored debug button and 50 ms pulse. Observed high, then expired low with no
  further input or visible input preview; a held level did not retrigger it.
  Stop rejected a late old-owner input. Fixture profile was removed afterward.
  No simulator, virtual destination or physical output was driven.

Run the opt-in native fixture against the running dev Cockpit:

```powershell
$env:OSG_PROFILE_DEADLINES_NATIVE = '1'
node --test tests/ui/profile-deadlines-native.mjs
```

The 50 ms fixture proves autonomous expiration and ownership fencing. It does
not independently establish sub-5 ms wake precision or physical input latency.

## Bounded CPU diagnostic

Isolated release builds compared base `255bf13` with the staged implementation,
using 137 controller state controls, 137 identity nodes and 137 sinks. After one
baseline, only empty `accept_batch` calls were measured with Windows thread CPU
accounting. Mean cost was **183.787 microseconds/call before**, versus
**6.761 nanoseconds/call after**. The optimized path returns without graph work.

These are averages over batches, not individual-call latency percentiles. There
were 101 batches; baseline used 1,000 calls/batch and optimized used 10,000,000
to span coarse Windows CPU accounting ticks. The workload validates unchanged
clock, reports and effects. This does not measure full health-loop cost, IPC,
preview painting, raw-input delivery or virtual destination receipt.

Local source snapshots, hashes, results and the reproducer are retained in
ignored `.research/reactive/r4-empty-batch-cost/README.md`. Native evidence is
under `.research/reactive/profile-deadlines-native/1793653582177/`; coverage and
logs are under `.research/reactive/r4-deadline-*`.

## Remaining work

The 5 ms active / 100 ms inactive service fallback remains for adapters and
foreground/visibility checks without notifications, script completions and
output lease maintenance. It is not full reactive qualification. Future work
must replace those service polls with source/completion notifications and actual
lease deadlines, preserving loss-before-timer ordering and independent Stop.

Exact controller preview and raw-to-virtual latency remain unqualified. Actual
simulator/hardware acceptance stays separate in #199, first in progress. The
Assetto shared-memory acquisition boundary awaits an explicit R0 decision; no
sampling exception is assumed by this scheduling change.
