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

## Windows path check probe

A standalone Windows probe opened an existing regular file using
`NtOpenFile` with `OBJ_DONT_REPARSE` and validated the returned file's
attributes. Both absolute and directory-relative opens rejected traversal
through a disposable NTFS junction. They also rejected a directory in
place of the file and distinguished a missing leaf from a missing parent.
The absolute open without the flag followed the junction in the control run.

An optimized Rust 1.90 comparison used the real path of a closed, disposable
Cockpit database and the current `check_live_path` as its baseline. Each
row covers two passes of 2,001 observations after warm-up. The probe ran in
current, native main, native all, native all, native main, current order.

| Path check | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| Current ancestor, main and sidecar checks | 644–651 µs | 719–808 µs | 838–951 µs |
| Native main open, current sidecar checks | 167–169 µs | 189–190 µs | 210–224 µs |
| Native opens for main and sidecars | 162–164 µs | 182–186 µs | 223–233 µs |

Most of the measured gain came from replacing the main path's repeated
ancestor opens. The last row is a cost probe with all sidecars absent.
It skips the existing rollback journal deletion-race handling. These
isolated path timings exclude SQLite commit, queueing and desktop dispatch.

The generic Windows attributes contract describes refusal when a name
is reparsed. The user-mode file-open documentation lists zero and
`OBJ_CASE_INSENSITIVE` for file object attributes. A passing junction case
leaves open equivalent treatment of all reparse tags, redirected and
network storage, access-denied paths and replacement during a live write.
[Windows object attributes](https://learn.microsoft.com/en-us/windows/win32/api/ntdef/ns-ntdef-_object_attributes)
· [Native file-open parameters](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile)

Local source and results are under `.research/c193-reconnect/nt-path-proof/`.
All 27 desktop latency analyzer and fixture contract tests passed.

## Mixed pointer preflight

The running dev Cockpit built from `d2eeb7da` retried the mixed keyboard and
pointer scenario. The exact receiver had keyboard foreground, but the
pointer target belonged to Windows `PickerHost.exe` (PID 17540, HWND
24449610). Every proposed position failed the unobstructed-area check.
The fixture stopped before Apply, sent zero input, and closed the receiver
with no cleanup errors. This run produced no mixed latency samples.
Its logs are under
`.research/c193-reconnect/required-journal-leaf/mixed-current-04348dd2/`.
