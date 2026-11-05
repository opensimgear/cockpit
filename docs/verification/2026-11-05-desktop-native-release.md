# Native tap release runs independently of parent persistence

This delivers worker-owned tap/click release deadlines under #193. It keeps the
host scheduler, ordered up operations and durable successful acknowledgements.
It does not complete the broader routing or timing acceptance milestone.

## Ownership and recovery

The compiled desktop KeyTap/MouseClick action supplies its existing down dwell.
Only its nonempty Held delivery receives a native release guard. Holds, Repeat,
pointer movement, ordinary up and cleanup keep their existing dispatch meaning.
The parent retains its epoch/current-producer gate and durable possible-hold
record before acquisition. The native worker checks the exact run and scope,
submits down, forms its truthful ACK, then anchors the release on a fresh native
clock sample. The final parent ACK/owner transaction remains mandatory.

At expiry the worker can only clear the matching producer/action/down-operation
contribution. It never advances that owner's operation counter or manufactures
a press. Shared holds survive, and the original scheduled up remains valid.
Arm, replacement, Pause, Stop, focus/lease loss and newer operations cancel the
corresponding guard. Short/error release faults and attempts bounded cleanup.

Subsequent ACKs report released guard IDs using the contiguous channel sequence.
The parent validates the entire bounded list before changing a staged reducer.
Pointer checks and submit/batch/Arm/Pause/Renew projections start from that
reconciled state; stale copies cannot restore released ownership. Unknown,
duplicate or oversized receipts fail with conservative recovery evidence.
Lost ACKs and failed final writes cannot become successful cleanup or replay.
Empty receipt lists avoid an extra reducer copy. Ordinary deliveries reuse their
validated projection unless a release receipt or worker-side expiry changes it.

## Native waits

The Windows worker waits on an owned queue event and high-resolution one-shot
timer. Sends, EOF, overflow and writer failure wake it directly. Reset/check/wait
races preserve queued frames; failure is rechecked before processing input.
The timer services the earlier release deadline or existing 5 ms focus/lease
watchdog. No process-wide timer-resolution change or spin loop is added.
Initialization and wait errors fail explicitly before further acquisition.

Microsoft documents high-resolution timer support from Windows 10 version 1803
and resetting timer state when rearming. The implementation uses relative
100 ns intervals and rechecks the native authority clock after wakes.
[CreateWaitableTimerExW](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-createwaitabletimerexw),
[SetWaitableTimerEx](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-setwaitabletimerex).
Portable recording retains its non-Windows channel implementation.

## Final verification

- 126 desktop-output and 142 shared-host tests pass. New cases cover exact
  release identity, shared holds, original up, native anchoring, replay,
  cancellation, malformed/capacity/forged receipts, mixed pointer/batch/lifecycle
  reconciliation, failed persistence and recovery. Six real Windows wait tests
  include 100 reset/send races, timer rearm, coalescing and failure wakes. Three
  recording protocol tests cover ordered messages and EOF/error cleanup.
- All 32 permanent desktop/host production coverage gates exceed 71%.
  Release timer: 100%; parent receipts: 91.11%; delivery projection: 100%;
  native wait: 86.67%; protocol serve: 87.39%. All 17 coverage-parser tests and
  247 pinned timing dispositions pass.
- All 2,289 native Cockpit tests pass, with 17 existing explicit ignores.
  Locked offline Windows app and receiver builds pass.
- The audited actual-Cockpit one-update tap/Stop passes. With auditing disabled,
  five final actual-Cockpit checks pass: tap/Stop, delayed preview catch-up,
  physical reader replacement/quiet liveness/Stop, shared preview/reopen and
  real vJoy Raw Input with independent MCP observation and Stop.

In the final controlled run, native call returns span **1.5959 ms** for an authored
1 ms dwell. The category-7 autonomous release occurs during the parent's down
ACK journal transaction, before that transaction finishes. The ordinary host up
then acknowledges without another native release edge. The earlier release-only
parent path measured 6.8949 ms at the same native-call boundary in one run.

The receiver's message-handling interval is **573 us**. That is a different
boundary affected by when its thread processes messages; it does not establish
native dwell or physical input latency. These individual measurements are not
percentiles or a strict 1 ms guarantee. First acquisition, burst/up-gap timing,
focus/child notification cutover and full load/fault qualification remain open.

Parent checkpoint and worker footer are valid/complete: 35/35 parent and 17/17
worker records, with zero drops, evidence faults or I/O errors. Artifacts are
under `.research/reactive/desktop-guard-*`; final audit/summary use the
`desktop-guard-highres-*` prefix. Receiver evidence is
`desktop-deadlines-native/1793885634795/evidence.json` under `.research/reactive`.

App SHA-256:
`2db5b322af06e45bc7a8f55488f62ee58d58318c38915345d277314f9d55d2ec`.
Receiver SHA-256:
`c35bb6820e7a775c790dfc2196b40e07e9228ee2b809d18e25510d70f79e3798`.
Audited Cockpit PID: 37464; final audit-disabled PID: 36092. Temporary profiles,
readers, receiver and private worker retire, with stopped ownership, no held
controls/uncertainty and free virtual slots.

## Corrected checks and remaining scope

The first library run exposed an old Pause write-count expectation: its owner
projection now shares the mandatory final ACK transaction instead of making a
third write. Permanent receipt coverage initially failed; live unexpired guard
cancellation and independent-owner cases were added. A duration-map test needed
its OutputControl import, and the parser regression now includes the separately
gated delivery module. All final checks above pass.
Moving the new duration-map regression into a small test module initially moved
adjacent shared fixtures too; those helpers were restored to their original
scope before the final 142-test host/coverage pass.

The initial full native run passed 2,288/2,289 and failed a watch quietness check.
That test drained only already-queued notifications before making a new write;
an earlier callback can still be in flight. It now uses its existing settling
helper before testing the empty configuration, and failures report caller and
notification details. The original log identifies only the shared helper, so
the exact earlier quiet boundary remains unknown. Final full native runs pass.

Timed channel waits produced 3.2443 and 5.4851 ms native-call spans before the
event/high-resolution-timer cutover. Those intermediate builds are not the
delivered binary. The final wait has its own coverage and actual native evidence.

Physical raw-input-to-preview/virtual measurements and simulator effects remain
in #199, first in Progress. Software prerequisites and incomplete matrices stay
in the original issues. CI remains deferred until implementation is complete.
