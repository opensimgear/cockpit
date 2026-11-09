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

## Live WAL/FULL comparison

Four alternating Windows dev Cockpit runs compared the current DELETE/FULL
build with an isolated WAL/FULL build. The candidate kept the existing path
checks and exact durable intent and receipt writes. Its SQLite connector
accepted only the disposable experiment tree. All connections to those
disposable databases used WAL. The SQLite file headers confirmed the
expected mode in each run.

The same two authenticated WSS sources drove two exact receive-only key
targets, first with the UI idle and then while navigating between Overview
and Profiles. The native QPC analyzer joined source admission to successful
`SendInput` return. All 1,614 first-dispatch and sustained key transitions
matched the audit and receiver counts, with clean Stop in every run.

| Run | Journal | Sustained idle p50 / p95 / p99 | Sustained Profiles p50 / p95 / p99 |
| --- | --- | ---: | ---: |
| A1 | DELETE | 4.64 / 9.87 / 10.68 ms | 4.48 / 9.22 / 10.16 ms |
| B1 | WAL | 3.83 / 7.43 / 8.12 ms | 3.80 / 7.23 / 7.89 ms |
| B2 | WAL | 3.60 / 7.29 / 7.95 ms | 3.65 / 6.98 / 7.55 ms |
| A2 | DELETE | 4.52 / 10.14 / 18.61 ms | 4.70 / 9.70 / 10.53 ms |

Under Profiles load, the durable intent segment fell from 4.00–4.14 ms
p95 to 2.56–2.71 ms. The measured overlap with the preceding request's
receipt write fell from 3.76–3.79 ms p95 to 2.38–2.40 ms. The candidate
met the sustained p99 budget in both phases but stayed above 5 ms p95.

The separate whole-app crash fixture passed on the same candidate. The
read-only observer found the exact held-key journal at revision 8 before
termination, while the app was down, and after restart. Explicit Recover
advanced it to revision 14 and produced the only KeyA up. The receiver
recorded one down and one up, and cleanup was clean. An earlier launch
through a second executable path failed the worker identity check before
termination. Stop cleared its held key. The successful run used one
executable path throughout.

This experiment establishes that storage can reduce both the current
intent delay and the next request's wait. Production integration still
needs a local-only storage design that preserves backup and restore
fencing, exact identity, redirected configuration support and sidecar
validation. Mixed pointer and physical-controller latency remain separate
qualification steps.

Local build, audits and crash evidence are under
`.research/c193-reconnect/wal-live/`. The candidate executable SHA-256 is
`87a61b594652d5e2b91f75b3d1a714bd8a1c72fe4d5cadb2072752661fe0d840`.
The successful crash `evidence.json` SHA-256 is
`799237517b50a7265a2c5aa143e0029f83cd30be938cace6091fd90c07e8297b`.

## Live WAL/FULL and native main-file check

Four more alternating runs compared the private WAL/FULL build with the
same mode plus the native main-file check. The experiment replaced the main
path check only at live evidence checkpoints. The current sidecar checks,
journal retry, SQLite transaction boundaries and exact evidence comparisons
were still in use. The build only accepted disposable database paths.

| Run | Main-file check | Sustained idle p50 / p95 / p99 | Sustained Profiles p50 / p95 / p99 |
| --- | --- | ---: | ---: |
| A1 | Current | 3.63 / 7.05 / 7.79 ms | 3.58 / 6.94 / 8.03 ms |
| B1 | Native | 2.96 / 5.41 / 5.73 ms | 2.93 / 5.41 / 17.10 ms |
| B2 | Native | 2.88 / 5.30 / 5.88 ms | 2.95 / 5.30 / 5.85 ms |
| A2 | Current | 2.98 / 4.42 / 7.52 ms | 3.83 / 7.39 / 9.50 ms |

All 1,614 key transitions matched the audited and observed counts, and
all four runs proved clean Stop. Under Profiles load, the intent segment
fell from 2.57–2.72 ms p95 to 1.67–1.73 ms. Loaded p95 improved in both
pairs but still missed the strict 5 ms budget. Idle p95 increased in the
second pair. In B1, a 15.319 ms durable write accounted for most of a
17.096 ms dispatch. The next delivery waited 16.338 ms before issue
and took 19.553 ms end to end. The trace does not identify which
operation inside the durable write accounted for that tail.

The combined build reached an acknowledged KeyA hold in the crash
fixture. The fixture then failed to pin exactly one private child and
stopped before termination. Stop released the key, the journal proved
clean ownership and the receiver exited. The earlier WAL-only build
passed full interruption and recovery. Combined-build crash recovery,
production path equivalence and strict timing still need qualification.

Local inputs and outputs are under `.research/c193-reconnect/wal-live/`
and `.research/c193-reconnect/wal-fast/`. The combined executable SHA-256 is
`82dc5483ecc7f9c1d8f1e447b35e05be935197812d220a37397cf0ef30f860cd`.

## Companion input admission

The combined experiment's native trace located most of the pre-conversion
latency between enqueue and dequeue. Under Profiles load, that interval
was 848–849 µs p95. Dequeue through typed alias conversion was 42–43 µs.
Typed alias through desktop effect was another 778–788 µs p95.

The companion adapter now computes the immutable contract hash once per
binding at preparation. Delivery checks compare that hash, route identity,
port and access directly against the borrowed token. Taking ownership of
a new binding still clones its token. Per-frame generation checks reuse
one encoding. Current inventory validation and reconnect fences still run
on the live path.

The instrumented Windows suite passed 52 focused companion tests. The
changed adapter reached 81.78% line and 90.82% function coverage. In the
full run, 2,487 tests passed and 18 were ignored. Five X-Plane bridge
tests hit the disk limit, and a game capture test missed an X-Plane packet.
All six passed when rerun serially on the same executable.

Four alternating dev Cockpit runs with the production DELETE/FULL
journal delivered all 1,605 audited key transitions and proved clean Stop.
The comparison removes repeated binding work but does not establish a
consistent gain in end-to-end latency.

| Run | Binding hash | Profiles dequeue through alias p95 | Profiles WSS admission through SendInput p95 / p99 |
| --- | --- | ---: | ---: |
| A1 | Recomputed | 44 µs | 9.62 / 11.95 ms |
| B1 | Prepared | 41 µs | 9.22 / 17.45 ms |
| B2 | Prepared | 43 µs | 9.70 / 15.75 ms |
| A2 | Recomputed | 43 µs | 9.76 / 10.33 ms |

The B1 p99 sample had a 15.452 ms durable write before the parent
transport enqueue. The B2 p99 sample waited 14.068 ms from request issue
to parent enqueue without a matching write interval. Both samples
reached the desktop binding within 1.1 ms of WSS admission. The audit
does not attribute the second stall inside the pre-enqueue boundary.
The strict desktop timing and mixed pointer criteria stay open.

Local coverage and the four native run artifacts are under
`.research/c193-reconnect/companion-contract-cache/`.

## Release verification and SQLite contention

The opt-in desktop audit now records each pre-dispatch evidence
verification with its exact channel, request sequence, QPC interval and
result. The analyzer reports successful verification only inside that
request's issue-to-enqueue window. Latency still ends at the native
submission receipt.

Two running Windows dev Cockpit workloads delivered all 805 expected key
transitions and proved clean Stop. The analyzer joined 400 read-only
verifications. Sustained verification p95 ranged from 0.82 to 0.95 ms,
with a maximum of 1.204 ms. The largest measured intent write was
32.571 ms. These runs did not reproduce the earlier 14 ms pre-enqueue
stall in the read-only step.

A separate contention probe used the pinned SQLite 3.53.2 Windows
VFS, rollback journal and a private database held by an exclusive writer.
The 5 ms busy timeout took 168.55–168.95 ms p95 to return busy. A custom
high-resolution busy callback reduced that to 61.26–62.68 ms p95. The
VFS retries acquiring its pending lock before calling the busy handler.
A precise callback alone therefore cannot enforce the read budget.
[SQLite Windows VFS source](https://raw.githubusercontent.com/sqlite/sqlite/version-3.53.2/src/os_win.c)

A prototype shared pending-byte lock on SQLite's own file handle refused
exclusive contention in at most 1.2 µs across 64 attempts. Ordinary reads
and reads under a reserved writer succeeded. With a hot rollback journal,
the query recovered the journal but the prototype's unlock failed with
Windows error 158. The extra lock cannot be relied on across SQLite's
recovery path, so this prototype was discarded.
[SQLite native file handle](https://sqlite.org/c3ref/c_fcntl_begin_atomic_write.html)
· [Windows byte-range unlock contract](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-unlockfileex)

All 202 Windows desktop-output tests and 29 analyzer and fixture tests
passed. Windows line coverage for the two changed coordinator modules
was 76.87% for the journal and 87.35% for the transaction. The audit enum
addition has no executable lines. Local probe sources, coverage and
native traces are under `.research/c193-reconnect/sqlite-read-wait/`.

## WAL/FULL on verified local Windows storage

The native startup lease now selects WAL/FULL for an ordinary fixed NTFS
drive whose device and volume serial can be verified. Routine database
opens retain the selected mode, and the post-restore handle is registered
before starting workers. Hard-linked databases and unqualified locations
use DELETE/FULL. An existing WAL database on an unqualified location is
refused with its sidecars preserved.

The first live route preflight rejected the database before Apply.
A lease-owned connection now retains the WAL sidecars between command
connections. It holds no transaction and closes before the namespace
lease releases. Both subsequent dev Cockpit route runs passed and matched
all 808 key transitions, including the first-dispatch samples.

| Run | Sustained idle p50 / p95 / p99 | Sustained Profiles p50 / p95 / p99 |
| --- | ---: | ---: |
| 1 | 3.53 / 7.11 / 18.93 ms | 2.91 / 3.84 / 4.32 ms |
| 2 | 3.54 / 7.17 / 7.79 ms | 3.60 / 6.79 / 7.66 ms |

The running backup fixture created a portable DELETE snapshot, restarted
Cockpit, restored the profile and invalidated historical MCP credentials.
The restored process reported WAL/FULL and a new database epoch. The
separate whole-app interruption fixture preserved the held-key row at
revision 8 through termination and restart. Explicit recovery produced
the only KeyA up and advanced the row to revision 14, with clean Stop.

Windows persistence tests passed 70 cases with one existing ignore.
Line coverage for journal mode selection and Windows eligibility was
94.67% and 93.41%, respectively. All 29 focused Cockpit persistence tests
passed, with 95.62% line coverage in backup startup and 94.33% in the
backup module. The hard-link and existing rollback
sidecar fallbacks ran locally. Redirected and mapped network locations
were not exercised live. The strict desktop timing gate and long-running
checkpoint behavior remain open.

Local artifacts are under `.research/c193-reconnect/local-wal-mode/`.
The final Windows dev executable SHA-256 is
`67fd7e5c07b2168812ecd3eb23b31f6461ca25e215e81b45a27bcd2b93edbd67`.

### Prior intent and receipt writes

The analyzer now includes a preceding request's write even when both
deliveries bound before that request issued. It intersects each exact
channel and sequence write with the next delivery's binding-to-issue
interval. A write completed before parent enqueue is classified as intent.
A write started after the native worker outcome is classified as receipt.

Replaying both WAL runs matched all 808 deliveries and reproduced all
eight original latency distributions. In the three sustained phases that
missed the target, each 198-edge phase had 50 deliveries overlapping the
preceding intent write and four to seven overlapping the preceding receipt
write. Among those 50 samples per phase, intent overlap p95 was 2.78–2.89 ms.
The sustained Profiles phase that met the target had one intent overlap.
The traces place the larger recurring wait in the previous press's durable
intent, including pairs that already share a receipt write.

All 30 analyzer and native fixture contract tests passed. Python trace
measured 91% executable line coverage in the analyzer and exercised every
line of the write-attribution function. The mixed pointer retry on the
same native build stopped before Apply or injection. The receiver had no
unobstructed pointer target because Windows PickerHost owned the hit-tested
window. The fixture confirmed a stopped profile, no input worker, no
cleanup errors, and a clean receiver exit.

The running Cockpit also passed the rollback fallback through a temporary
Windows `SUBST` alias to a private local directory. Both processes reported
DELETE/FULL at `S:\cockpit.sqlite3`. The portal/MCP backup survived restart,
restored the saved profile in a new epoch, and invalidated old credentials.
The fixture verified the backup's standalone rollback format and the
temporary drive alias was removed after the run. The evidence is under
`.research/c193-reconnect/local-wal-mode/backup-6896/`. A redirected or
network-backed path and long-running checkpoint behavior remain untested.

### Two queued presses in one native call

All 155 `JournalMerged` pairs from the two previous runs had both
deliveries bound before the first request issued. The new path accepts
two already queued and armed KeyHold presses only when both produce one
distinct native down in the same run. One durable intent contains the
exact pair, and one native call returns a shared receipt with separate
logical operation IDs. A declined batch with zero native records falls
back to the original individual checks. Later arrivals use the existing
ordered path.

Two running dev Cockpit trials matched all 808 expected transitions and
proved clean Stop. They submitted 87 and 93 native key pairs. The sustained
trials recorded these WSS admission-to-SendInput results.

| Run | Idle p50 / p95 / p99 | Profiles p50 / p95 / p99 |
| --- | ---: | ---: |
| WAL baseline, same session | 3.64 / 7.32 / 7.75 ms | 3.60 / 7.10 / 7.87 ms |
| 1 | 3.84 / 5.84 / 6.53 ms | 3.74 / 5.86 / 6.59 ms |
| 2 | 3.72 / 5.43 / 6.49 ms | 3.75 / 5.58 / 6.20 ms |

Each sustained phase previously showed 50 preceding-intent overlaps
when it missed the target. The new runs showed four to nine. First-phase
p95 ranged from 4.85 to 8.69 ms. Sustained and first-dispatch timing
therefore still need qualification against the strict 5 ms p95 gate.

The whole-app interruption fixture drove KeyA and KeyB from one WSS
button. Both downs shared helper request 6 and had distinct operation
IDs. The durable held record remained at revision 9 through restart.
Explicit recovery produced one up per key, cleared recovery at revision
15, and left no input worker. The original single-key scenario also
passed on this build.

All 205 Windows desktop-output tests, 184 mapping-host tests, and 31
Python audit and workload tests passed. LLVM measured 90.91% lines and
100% functions for the shared pair projection, 89.40% lines for the
coordinator delivery module, 83.79% lines for the helper worker, and
91.71% lines for mapping-host dispatch. Python measured 91% analyzer
lines. The first mapping-host run hit disk limits when it tried to build
a second script helper. It passed using the existing local helper.

Evidence is under `.research/c193-reconnect/native-key-pair/`.
The dev executable SHA-256 is
`929670b574560c10eee0571f82327a8f7a29e0f9615121bd5e9c46d2f6253896`.

### Exact Windows path checks on local WAL storage

The live evidence connection now checks an ordinary verified local WAL
path with a single no-reparse native open. It compares the opened file's
volume and file ID with the SQLite connection's retained Windows handle.
The path is checked at the same read and write boundaries as before.
Sidecar validation and FULL synchronization remain part of that path.
Rollback databases and unusual paths use the existing checks. A probe
using `GetFileAttributesW` separately for every ancestor produced no
meaningful improvement and was discarded.
[Windows object attributes](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wudfwdm/ns-wudfwdm-_object_attributes)
· [Windows file IDs](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info)
· [SQLite file control](https://www.sqlite.org/c3ref/c_fcntl_begin_atomic_write.html)

Two native trials matched all 803 candidate keyboard transitions and
proved clean Stop. Timings run from authenticated WSS admission to
successful `SendInput` return.

| Run | Sustained idle p50 / p95 / p99 | Sustained Profiles p50 / p95 / p99 | First idle / Profiles p95 |
| --- | ---: | ---: | ---: |
| Same-session baseline | 3.96 / 6.44 / 7.10 ms | 3.09 / 4.39 / 4.86 ms | 5.787 / 4.708 ms |
| Candidate 1 | 2.32 / 3.24 / 4.20 ms | 3.08 / 4.55 / 5.44 ms | 5.071 / 4.151 ms |
| Candidate 2 | 3.01 / 4.62 / 5.20 ms | 3.15 / 4.46 / 5.07 ms | 5.030 / 3.707 ms |

Sustained pre-enqueue intent p95 fell from 2.67–2.72 ms to
1.65–1.77 ms. Pre-enqueue evidence verification p95 fell from
0.95–0.96 ms to 0.35–0.40 ms. All four sustained candidate phases met
the keyboard budget. First idle dispatch and mixed pointer qualification
remain open. This workload excludes physical input, vJoy and preview.

The candidate passed portal/MCP backup and full restart restore on
WAL/FULL and paired-key crash recovery. All 205 Windows desktop-output
tests and 73 persistence tests passed, with one existing persistence
ignore. The tests rejected an equal-content
replacement file, a junction that resolved to the same file, and a
directory at the rollback sidecar path. LLVM measured 95.95% lines and
92.31% functions in the new Windows path module and 98.97% lines and
100% functions in the live evidence module. These checks detect path
changes at each verification point. They do not make external namespace
changes atomic with SQLite commit.

Evidence is under `.research/c193-reconnect/verified-wal-path/`.
The dev executable SHA-256 is
`69f274fa6795682b90f34604b0c1c91bebb3b8893c08289a69ffa9ccf5a4ab81`.

### First-press queue boundary

The first idle miss in each exact-path trial came from the second key
down. Its frame was in the native companion queue before the first
request issued, then waited 0.917–0.920 ms for dequeue. The second
delivery bound after the first request was in progress. It waited
another 1.77–1.80 ms from binding to its own request, then spent
1.38–1.40 ms persisting the new intent before enqueue. Both measured
misses were acquisitions. The release path and receiver teardown passed.

An experimental build opened the existing FIFO successor slot before the
worker started. A deterministic mapping-host test exercised pairing and
independent successor dispatch after the first producer was withdrawn.
All 185 mapping-host tests passed on that build. The timing runs matched
all 806 candidate transitions and proved clean Stop.

| Run | First idle / Profiles p95 | Sustained idle / Profiles p95 | Confirmed native pairs |
| --- | ---: | ---: | ---: |
| Current main build | 5.143 / 4.353 ms | 4.541 / 4.294 ms | 87 |
| Experimental run 1 | 4.342 / 3.999 ms | 4.776 / 4.587 ms | 87 |
| Experimental run 2 | 4.019 / 3.491 ms | 4.740 / 4.420 ms | 93 |

All 180 candidate pairs had both native dequeues before the first
delivery bound. They were handled by the existing same-pass path. The
experimental cross-pass path was never exercised in either live run,
so the first-dispatch improvement cannot be attributed to it. The
prototype was discarded. The next timing boundary is the interval from
native enqueue to reducer dequeue while the first delivery is prepared.

A read-only Windows desktop check found the same PickerHost process in
the foreground and at all nine sampled points across the 2560 by 1440
desktop. Mixed pointer qualification remains open. No pointer input was
attempted in this check.

Local evidence is under `.research/c193-reconnect/queued-key-head/`.

### Instrumented profile poll

A private build of `f993800a` added QPC stamps around the profile poll,
runtime mutex and companion read. The running Cockpit received input
from two authenticated WSS peers and delivered all 402 expected keyboard
edges to the exact Windows receiver. Stop cleared owned output and both
receivers exited cleanly. The audit checkpoint had zero drops or errors.

| Phase | Edges | Admission to SendInput p95 / p99 | Enqueue to dequeue p95 |
| --- | ---: | ---: | ---: |
| First idle | 4 | 5.608 / 5.608 ms | 0.380 ms |
| Sustained idle | 196 | 4.351 / 4.813 ms | 0.745 ms |
| First with Profiles open | 4 | 4.754 / 4.754 ms | 0.412 ms |
| Sustained with Profiles open | 198 | 4.593 / 5.543 ms | 0.776 ms |

The slowest first-idle edge shared one paired request with the other
key down. Its 0.380 ms queue interval included 0.304 ms inside the
companion read, 0.016 ms before the worker poll began and 0.002 ms
acquiring the runtime mutex. The paired request spent 1.792 ms writing
the recovery intent, including 1.678 ms inside the live evidence write.
The helper then took 2.139 ms from request dequeue to `SendInput` return. That interval
includes helper preparation and the native call. The slowest sustained
queue samples overlapped work in the previous profile poll.

The first-idle budget remains open. The stamps were diagnostic only and
the fixture covered keyboard output. Local evidence, instrumentation and
the isolated executable are under `.research/c193-reconnect/profile-poll-trace/`.
Executable SHA-256 is `c306ecdac484ea4b56aa1667ba518949ffec4ef19676e688d4c508b0747f837e`.
