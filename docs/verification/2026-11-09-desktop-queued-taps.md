# Queued desktop tap verification

The Windows dev fixture authors one rising-edge rule and a KeyA tap with 8 ms
down and 1 ms up. It sends 23 alternating levels through the normal debug
controller gateway, then stops sending input. The fixture verifies the exact
receiver foreground, all 12 down/up pairs in order, queued tap-down work,
and a clean Stop. `OSG_DESKTOP_DEADLINES_BURST=1` selects this workload in
`tests/ui/desktop-deadlines-native.mjs`. It requires the fixture's native
injection opt-ins and a configured desktop audit collector.

Two runs on the `3dab3b33` candidate each observed all 24 keyboard edges.
The scheduler reported 11 queued successors and the native audit placed
15 edges after the marker following the final source update in each run.
The externally acknowledged parent checkpoint and private helper footer
validated. No queue fault, native gap or dropped delivery appeared between
the input and output markers. Stop cleared all held controls.

| Run | Down dwell min / p50 / max | Up gap min / p50 / max | Down dwells below 8 ms |
| --- | ---: | ---: | ---: |
| 1 | 5.815 / 8.575 / 8.778 ms | 2.503 / 6.201 / 10.823 ms | 2 of 12 |
| 2 | 6.380 / 8.435 / 8.803 ms | 2.551 / 6.393 / 6.901 ms | 3 of 12 |

The intervals use `SubmitReturned` QPC stamps from the exact helper, matched
to the parent channel, request, logical operation and receiver order. They
measure the time between native call returns. The source and output markers
bound the workload on the same clock. All five short down intervals ended
in an ordinary parent-requested release. All autonomous releases met 8 ms
in these runs. The parent scheduler starts its down deadline before native
dispatch, and an ordinary release can remove the guarded contribution before
the helper reaches its separately anchored deadline.

The baseline verified order and quiet-source drain and exposed the early down
release. The fixture uses authored software input and exercises keyboard output.
Both the queued mode and the existing single-tap mode passed on the running
dev Cockpit with 100% fixture line coverage in each run.
Local artifacts and the QPC comparison are under
`.research/c193-reconnect/desktop-tap-burst/` in `timing-1928-1` and
`timing-1928-2`. The dev executable SHA-256 is
`0cdb1a51132dd7180237ba9a91cecf1fb658a12cca9e887935a0ccdcf24d1878`.

## Native acknowledgements and profile deadlines

The scheduler waits for the exact native acknowledgement before starting each
authored tap dwell. Loss, Stop and rejected operations clear the pending tap
and its queue. Other actions can dispatch while a tap awaits its acknowledgement.
The helper still owns the native release timer, so it can release while the
parent handles the acknowledgement. The scheduler records acknowledgements in
its deterministic trace.

With acknowledgement timing alone, the two queued runs met both minimums
but the median up gap rose to 21.99 and 21.83 ms. The trace placed roughly
14 ms between the parent up acknowledgement and the next down issue after
the source went quiet. The Windows profile worker now uses the existing
one-shot high-resolution wait with its existing notification signal.

| Final run | Down dwell min / p50 / max | Up gap min / p50 / max |
| --- | ---: | ---: |
| 1 | 8.355 / 8.531 / 9.215 ms | 4.720 / 5.238 / 6.104 ms |
| 2 | 8.337 / 8.535 / 8.910 ms | 5.269 / 5.662 / 7.431 ms |

All 24 down dwells and 22 up gaps met the authored minimums. Both runs
delivered all 12 taps in order, queued 11 successors, recorded 18 native
edges after the final source marker, and proved clean Stop. The single-tap
fixture also passed on the final build.

The separate immediate KeyHold fixture delivered all 402 edges from two WSS
peers and met both timing targets. Admission to native return p95 was
3.805 / 3.582 ms on first dispatch and 3.871 / 3.836 ms on sustained idle
and Profiles phases. The mixed pointer workload is measured separately below.

All 61 stream, 211 mapping, 184 mapping-host and 206 desktop-output tests
passed. Line coverage was 100% for the wake module, 91.59% for the Windows
wait module, 88.89% for tap timing, and 89.19% for mapping-host dispatch.

Local final evidence is under `.research/c193-reconnect/native-tap-precise-wait/`.
The dev executable SHA-256 is
`42797065eaf7cf2a185168af58808904690cf896517157524e93eb51a067857a`.

## Mixed keyboard and pointer delivery

Two runs on Windows dev Cockpit at `11bf59e5` passed the exact foreground
and pointer preflight that previously stopped at PickerHost. Two
authenticated WSS peers each supplied a keyboard button and pointer axis
for six seconds at a requested 80 Hz. Each peer delivered 384–386 axis
updates per phase. The UI-load phases recorded 78 and 54 transitions
between Overview and Profiles. The receiver kept its application scope.

| Run and phase | Exact key edges | Marked pointer moves | Key p50 / p95 / p99 |
| --- | ---: | ---: | ---: |
| 1, idle | 194 | 92 | 3.262 / 4.638 / 5.953 ms |
| 1, Profiles | 192 | 184 | 3.338 / 4.806 / 17.398 ms |
| 2, idle | 192 | 102 | 3.340 / 4.608 / 5.219 ms |
| 2, Profiles | 194 | 114 | 3.233 / 4.357 / 5.499 ms |

The QPC audit measures WSS admission to successful `SendInput` return.
The four warmup edges per phase were matched separately. The slowest
warmup edge across both runs was 4.140 ms. There were no dropped deliveries,
native gaps or queue faults. The native queue peaked at two frames and
each audit transport peaked at one outstanding reservation. The receive-only
window recorded marked movement in every full second of each phase, and the
recent native outcome buffer contained confirmed motion from both axes.
The pointer count includes settlement after the source stops sending.
Pointer moves and key receipt are independent of the timing endpoint.
Both runs proved clean Stop, restored preferences and closed both receivers.

The first run missed the strict 10 ms Profiles p99 budget. Request 294
joined two key edges and spent 14.030 ms in its durable intent write
before enqueue. The edges took 17.398 and 17.518 ms end to end. The
same workload passed on the second run, with no slow write of that
size. Mixed delivery and cleanup passed. The repeatability check below
uses the committed build. Physical input timing remains in #199.

All 31 desktop audit and fixture contract tests passed. Local audit,
receiver and ownership evidence is under
`.research/c193-reconnect/required-journal-leaf/` in
`mixed-11bf59e5-49188` and `mixed-11bf59e5-repeat-18116`.
The dev executable SHA-256 is
`c628a650ddad47381842f0c8f681469cb03a98ef79cd3667624acd027522476a`.

### Durable write timing by stage

A private Windows dev build timestamped path checks, the record read,
`BEGIN IMMEDIATE`, identity and payload checks, `commit()`, and the final
path check inside each durable write. It used the existing bounded audit
collector. Each diagnostic row matched one `JournalPersist` row by parent
process, channel, request sequence and both exact QPC write bounds. The
neutral DCS restart check passed and matched all 34 writes. The checkpoint
recorded 628 events with zero drops or errors.

Two isolated replays used the same retained `Evidence::write` path on
private local WAL/FULL databases. Each measured 1,536 one-kibibyte writes
after warm-up, at a requested 64 writes per second.

| Run | Write p50 / p95 / p99 | Commit p99 | Slowest write | Commit in slowest write |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.814 / 1.033 / 1.174 ms | 0.422 ms | 1.740 ms | 1.178 ms |
| 2 | 0.794 / 1.045 / 1.161 ms | 0.411 ms | 8.785 ms | 8.106 ms |

The slowest replay write spent 92% of its time in `commit()`. The trace
measures wall time, so identifying a flush, checkpoint or scheduling delay
would require lower-level evidence. The later guarded runs used the original
mixed-input workload on an unobstructed desktop. [SQLite WAL performance notes](https://sqlite.org/wal.html#performance_considerations)

The private build, replay CSVs, source and neutral audit are in
`.research/c193-reconnect/dcs-native-stage-timing/desktop-write-stages/`.
The private dev executable SHA-256 is
`4ddd71b8d6c29d2344324418a934aa8091ffb4d5ba753a20d2ee05b2c80b0737`.

### Repeat mixed-load timing on the committed Windows build

Two stock dev runs at `52927823` passed the strict keyboard budget under
the original two-peer WSS workload. Each peer offered 384–386 pointer
updates in each six-second phase. The receiver matched every offered key
edge and marked pointer movement under the exact application scope. The
audit matched all 774 measured key edges and 16 separate warm-up edges to
successful Windows `SendInput` returns. Each phase peaked at two queued
native frames and one outstanding reservation per audit transport.

| Run and phase | Measured key edges | Marked pointer moves | Admission to `SendInput` return p50 / p95 / p99 |
| --- | ---: | ---: | ---: |
| 1, idle | 194 | 190 | 2.562 / 3.596 / 3.886 ms |
| 1, Profiles | 194 | 101 | 3.291 / 4.156 / 5.198 ms |
| 2, idle | 194 | 97 | 3.339 / 4.581 / 5.365 ms |
| 2, Profiles | 192 | 96 | 3.424 / 4.439 / 5.439 ms |

The earlier retry read a partially written receiver report as empty.
The fixture now waits for a complete report. Another retry recorded all
expected keys and 283 exact pointer moves during Profiles, but its proof
required a separate native event for each axis. The fixture now matches
each participant to the first operation of its acknowledged batch. Both
axes receive delivery credit while the native event is counted once.
All 46 focused audit and fixture tests passed. The pointer proof reached
94% line coverage, and the incomplete receiver report branch was exercised.

A separate instrumented idle attempt correlated 169 durable writes. Total
write p99 was 1.219 ms and the largest write was 2.361 ms. The prior
14.030 ms write remains recorded above as an observed outlier. These
desktop timings start at native WSS admission. Physical controller and
display timing remain in #199.

Local artifacts are in `.research/c193-reconnect/required-journal-leaf/`.

| Local evidence | SHA-256 |
| --- | --- |
| `mixed-batch-fixed-56680/evidence.json` | `bae44f3f788064d96f99e1633c95cf6a38abd8b7cacdec457f2ec867e5daf157` |
| `mixed-batch-fixed-56680/analysis.json` | `3ee1ad31c7d8d016dfe9bd4e137648ee0147678c8cd8187a4a23fa490517d1e5` |
| `mixed-batch-repeat-42684/evidence.json` | `e90955ec2fa7274edb7f97bc64507fc7321073ea70f84f6b5fba3848a23c41a5` |
| `mixed-batch-repeat-42684/analysis.json` | `3d909e9fcc7798829225e739634b934db77736af488448e76437ad696bb7dac6` |
| `mixed-stages-fixed-38232/journal-write-stages.json` | `7b8f1ad3aecf2e2f71aaff2fce952f6ffb92a9cbe744bc2b90b015df23dabe84` |
