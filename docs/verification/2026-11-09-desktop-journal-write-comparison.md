# Desktop journal write comparison

The native audit joins each immediate KeyHold change to its exact output
request. Two retained-connection runs recorded 806 key transitions. All
237 binding-to-request delays above 5 ms overlapped the previous request's
post-acknowledgement journal write on the same coordinator channel. The
overlap accounted for 49.2% of those delays in aggregate.

| Retained-connection run | Next edge binding and request | Wait | Previous request's final journal write |
| --- | --- | ---: | ---: |
| b1 | Parent events 616 and 624, operation 28 | 21.131 ms | Request 42, event 622, 6.304 ms |
| b2 | Parent events 4020 and 4028, operation 2 | 20.415 ms | Request 6, event 4026, 14.923 ms |

The previous request reached its parent outcome before the next request
issued in both examples. The trace uses the same 10 MHz QPC clock across
the parent and helper. Earlier audit data is under
`.research/reactive/r4-desktop-timing/journal-ab-27110a00/`.
The overlap report joins on the exact channel and preceding request
sequence, and counts only writes after that request's dequeue and before
its parent outcome.

## Cached SQLite update experiment

The candidate replaced separate statements for existing evidence with a
cached update conditional on the exact prior payload, revision and database
epoch. The checked connection, FULL synchronization, pre-dispatch
intent and post-acknowledgement writes remained in the test. The Windows
persistence suite passed 68 tests with one existing ignore. LLVM reported
98.18% lines and 100% functions for `evidence.rs` and 98.40% lines and
100% functions for `evidence/live.rs`. Branch instrumentation was unavailable.

Four alternating runs of the native dev Cockpit used the same two
authenticated WSS peers and receive-only keyboard fixture. All 1,614
marked edges arrived in order. Every run confirmed cleanup. Timings run
from WSS admission to successful `SendInput` return.

| Run | Build | Sustained idle p95 / p99 | Sustained Profiles p95 / p99 | First press in Profiles, slowest source |
| --- | --- | ---: | ---: | ---: |
| A1 | Baseline | 14.14 / 15.57 ms | 13.72 / 14.72 ms | 13.57 ms |
| B1 | Cached update | 16.86 / 20.91 ms | 16.58 / 17.38 ms | 11.42 ms |
| B2 | Cached update | 18.69 / 22.41 ms | 12.56 / 18.05 ms | 11.32 ms |
| A2 | Baseline | 14.57 / 18.22 ms | 14.19 / 16.13 ms | 12.22 ms |

The candidate worsened sustained idle p95 and Profiles p99 in both paired
comparisons. The baseline writer remains in production. The strict
desktop timing and mixed pointer checks stay open. These measurements
cover the desktop-key path. Physical preview and virtual-output timing
remain in #199.

The audited runs, local candidate patch, and overlap joins are under
`.research/c193-journal-cas/`. Two earlier candidate launches failed preflight
because the development audit was disabled, before any candidate injection.
The audited baseline executable SHA-256 is
`b1eb29dd1c65e128d4084d9bb9584e74b042c33b50c7727dea99f93ba4635966`.
The audited candidate SHA-256 is
`3d4fadd3ffa75a11c92b845a1179ff58958b635580c4f5f1737e73583d1e4b37`.
