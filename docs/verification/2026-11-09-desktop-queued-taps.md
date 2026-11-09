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

The order and quiet-source drain are verified. The down dwell gate remains
open. The fixture uses authored software input and exercises keyboard output.
Both the queued mode and the existing single-tap mode passed on the running
dev Cockpit with 100% fixture line coverage in each run.
Local artifacts and the QPC comparison are under
`.research/c193-reconnect/desktop-tap-burst/` in `timing-1928-1` and
`timing-1928-2`. The dev executable SHA-256 is
`0cdb1a51132dd7180237ba9a91cecf1fb658a12cca9e887935a0ccdcf24d1878`.
