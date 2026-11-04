# Profile completion and lease scheduling — 2026-11-02

R4 #193 and parent #188 remain in progress. This extends the deadline scheduling
foundation; it does not qualify controller preview or raw-to-virtual latency.

## Delivered behavior

Typed script workers publish their reply or terminal transport failure before
waking the native profile owner. The bounded transport retains one reply and a
terminal failure slot. The response timeout is an absolute deadline from queue
admission, rather than a timeout restarted by polling. Idle EOF is observable
even while the child process remains alive. Wake callbacks carry no host state
and are detached on termination.

Native health preflights script failure and overdue responses before admitting
source batches or advancing timers. Successful replies are staged with their
original ticket; intervening loss or replacement still fences them. A failed
script cannot fire a same-service timer, while an independent branch survives.

The next native wake includes routing deadlines, script response deadlines and
the virtual controller's next lease renewal. Lease accounting starts at the
Start/Renew send time, so a slow acknowledgement does not extend the lease.

Active profiles can omit the periodic service interval only when both captured
source and executable documents are entirely internal, without game/launch
context, assignments, external sinks, controllers or adapter work. They park
until an explicit wake or real deadline. Other profiles retain the existing
service interval. Unassigned global controller reports may still wake an
internal profile unnecessarily; this is not full idle-zero-work qualification.

Sinkless internal scripts may run through the supported native profile API.
This allowance requires the original captured candidate, no game or launch
context, and no compiled sinks or controllers. The external script qualification
flag remains false; this does not authorize or qualify script-driven hardware.

Actual Cockpit testing exposed an unchanged-constant propagation bug: evaluating
an unrelated branch or accepting a script reply marked the constant dirty,
manufacturing another callback and indefinitely refreshing script output.
Unchanged constants now preserve their value and capture provenance without
dirtying dependants. Real observations, including equal-valued source reports,
still propagate. Regression tests cover exact expiry and independent timers.

## Verification

- Isolated instrumented mapping: **145 passed**, no failures; all 15 module
  gates above 71%. Typed reducer **93.67%**, deadline selector **100%**.
- Isolated logic: **28 passed**, including real helper process checks; all 11
  module gates pass. Typed worker **87.05%**, transport **98.44%**.
- Isolated Windows virtual controller: **107 passed**, no failures; all module
  gates pass. Session **92.84%**.
- Coverage harness regressions: **nine passed**.
- Isolated source-stamped native suite: **1,816 passed**, no failures,
  17 explicit helper/opt-in checks ignored; 95.38 seconds with four workers.
  Scheduler production coverage **100%**, script host **80.16%**, coordinator
  **92.03%**, native host **80.21%**, gateway **84.05%**, launch host **87.50%**.
- Native production executable builds. SHA-256:
  `aa3e58c44ce39c06d152bc0863e17639f184513b41b4b025b08e131af2aebcfc`.
- Actual Windows dev Cockpit, **22:08 UTC**: both opt-in fixtures in
  `tests/ui/profile-deadlines-native.mjs` pass. An authored 50 ms pulse expires
  without further input and rejects late input after Stop. A real isolated
  script completes, then its output becomes stale without further input,
  inspection or a periodic profile service interval. Fixtures use supported
  create/save/apply/inspect/stop/delete APIs and remove their own profiles.

Local artifacts: `.research/reactive/r4-completion-*`, mapping coverage under
`.research/reactive/r4-deadline-mapping-coverage/`, logic coverage under
`.research/script-completion-wakes-coverage/`, and native fixture evidence under
`.research/reactive/profile-deadlines-native/1793657322403/` and
`.research/reactive/profile-completion-native/1793657322890/`.

## Remaining qualification

The fixtures establish autonomous completion, expiration and owner fencing.
They do not measure wake precision, UI paint, native raw capture, virtual
destination receipt, physical hardware or simulator-confirmed effects. No
physical output was driven. Controller preview and raw-to-virtual latency
remain open, alongside the remaining producer, service and UI cutovers.

User-operated hardware/simulator acceptance remains separate in #199, first in
the progress lane. The Assetto acquisition boundary still awaits an explicit R0
decision; no sampling exception is inferred here.

## vJoy feeder deadline verification, 2026-11-04

The isolated vJoy worker waits for pipe input or its current lease deadline.
A failed output pipe wakes an inactive worker even if stdin is stalled.
Both transport failure flags are checked again after the worker wakes and
before it admits a message. Reports still take the existing ordered pipe path.

The Windows virtual-controller suite passed all 108 tests and its coverage
gates. Production line coverage was 98.21% for the feeder and 96.83% for the
worker. The Cockpit suite passed 2,224 tests with 17 ignores and one known
topology fixture excluded.

The rebuilt dev Cockpit applied a temporary profile to free vJoy slot 1 after
matching its complete layout. It stayed active past two lease periods, accepted
two authored input commands, and released the slot on Stop. An independent
WinMM read did not see the button press even while the mapped virtual input
sample reported true. This verifies the feeder lifecycle and mapped input
sample. Game-visible button output and raw-input-to-vJoy latency remain open.
