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
size. Mixed delivery and cleanup pass. Repeatable mixed-load timing
remains open in #193, and physical input timing remains in #199.

All 31 desktop audit and fixture contract tests passed. Local audit,
receiver and ownership evidence is under
`.research/c193-reconnect/required-journal-leaf/` in
`mixed-11bf59e5-49188` and `mixed-11bf59e5-repeat-18116`.
The dev executable SHA-256 is
`c628a650ddad47381842f0c8f681469cb03a98ef79cd3667624acd027522476a`.
