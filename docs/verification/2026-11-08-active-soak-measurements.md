# Active Windows soak measurements

The existing DCS Feature Lab fixture now counts gateway POSTs from the
main WebView during the active navigation and discovery workload. It
separates requests sent by the app from explicit fixture probes by client
ID and uses the status probes to verify the collector. The per-view counts
identify the current navigation step. They do not identify the producer of
an asynchronous request.

The fixture timestamps each authored ownship UDP submission and each
panel's first matching canvas transition. Clock brackets at both ends of
the soak give an upper bound for p50, p95 and p99. The existing 150 ms
panel p95 gate applies to both loopback panels. The fixture also maintains
two authenticated MCP subscriptions during the soak. One checks advancing
vJoy sample revisions in the same generation, and the other leaves the
response unread until Stop. The fast reader must receive retirement. The
long soak records a closed unread response separately from a retirement
frame. The held native observation acknowledgement is a separate
stressor. A failure retains the owner degradation,
both telemetry branch statuses, partial paint samples and request counts.

## Short Windows checks

The Windows dev executable used production code at `b72c1d12` with SHA-256
`d61daa22b7976ed20d4427fa87389efd03ca1cd16cd92052c064261e8252b2b0`.
The profile ran against authored DCS telemetry, a stand-in simulator
process, two loopback panels and a Windows vJoy slot.

- The first instrumented run saw a new stale panel value after warmup. Its
  reported source age was 11 ms. The run stopped before completing its
  30-second window. Evidence is in `.research/dcs-physics-native/1794170459895/`.
- A second run saw the native ownship sample change from known to stale
  and back in about 16 ms. The same owner briefly reported Degraded with
  no fault. The fast MCP subscription had received all 15 authored debug
  transitions up to that point. The app had issued 21 gateway requests,
  including 15 game read-model pages. Explicit fixture traffic was
  counted separately. Evidence is in
  `.research/dcs-physics-native/1794170636831/soak-failure.json`.
- The original committed fixture completed 90 seconds on the same
  executable with 47 health checks and three completed discovery scans.
  Its later Raw Input stage selected an older probe that lacked the
  two-button mode and did not run. The short soak result is recorded in
  `.research/dcs-physics-native/1794170758983/progress.json`.

The corrected instrumented run completed **90.002 seconds** with the
same owner, 46 healthy checkpoints, three completed discovery scans and
46 advancing samples at both the fast native and MCP readers. The stalled
native subscriber received its next delivery after acknowledging. The
app issued 75 gateway requests during the measured interval. Of 69 game
read-model pages, 64 arrived during the Games step, four during Debug and
one during Overview. The other six app requests were companion inventory
and preference calls. The collector saw all 92 explicit status probes
and attributed them separately.

| Loopback panel | Samples | UDP submission to canvas upper p50 / p95 / p99 | Clock uncertainty |
| --- | ---: | ---: | ---: |
| 1 | 46 | 4.61 / 7.74 / 49.08 ms | 0.23 ms |
| 2 | 46 | 6.44 / 38.51 / 48.25 ms | 0.26 ms |

Both canvas distributions met the 150 ms p95 budget for this workload.
The later Stop received retirement on the fast reader. The long-unread
reader ended before delivering retirement, which the first version of
the extended fixture treated as a test failure. Cleanup reported no
errors.
Evidence is in `.research/dcs-physics-native/1794170942876/`.

A 45-second run with explicit unread-stream closure handling received all
23 measured MCP updates and reached the Windows output check. The legacy
five-second Raw Input probe missed the final three expected debug-button
edges. The soak now requests a bounded 15-second probe window and verifies
the receiver's monotonic end timestamp against Stop. The probe's other
callers still use the five-second default.

The rebuilt probe passed the entire fixture. The 35-second target took
44.434 seconds including the last discovery scan. All 82 expected MCP
samples advanced. The fast reader received retirement, the unread
response closed, both vJoy buttons released 41.03 ms after MCP Stop
submission and Windows reported the slot free. Process-exit withdrawal
and cleanup also passed. Evidence is in
`.research/dcs-physics-native/1794171394805/evidence.json`.

## Thirty-minute Windows run

The same Windows executable passed 1,800.013 measured seconds on the
extended fixture. One profile owner, two armed loopback panels, a
withheld native acknowledgement and two authenticated MCP subscriptions
remained under test while Cockpit navigated and refreshed discovery.
The fast native and MCP readers each advanced through 912 changes.
All 60 discovery scans and 60 inventory checks completed. The main
WebView made 1,714 app gateway calls, including 1,708 game read-model
pages. The collector counted explicit fixture requests separately.

| Loopback panel | Canvas changes | UDP submission to canvas upper p50 / p95 / p99 |
| --- | ---: | ---: |
| 1 | 912 | 4.61 / 7.92 / 48.21 ms |
| 2 | 912 | 6.85 / 39.02 / 48.45 ms |

Both panels stayed within the 150 ms p95 budget. The native subscriber
resumed after acknowledgement. The fast MCP reader received retirement,
the unread response closed, and Windows Raw Input observed both vJoy
releases 42.83 ms after MCP Stop submission. Windows reported the slot
free. Process-exit withdrawal and teardown passed with no errors.
Evidence is in `.research/dcs-physics-native/1794171621628/evidence.json`.

The two earlier intermittent failures did not recur during this run.
Their cause and the full dual-rate timing budget remain open. Real
raw-input, physical display, LAN and simulator effects are tracked
separately in [#199](https://github.com/opensimgear/cockpit/issues/199).

To repeat, use the isolated Windows dev Cockpit described in
[the original DCS and vJoy verification](2026-11-07-dcs-panel-vjoy.md#30-minute-desktop-soak).
Recompile the two-button Raw Input probe from
[`tests/controller-mapping-m0/RawInputProbe.cs`](../../tests/controller-mapping-m0/RawInputProbe.cs)
for the explicit capture window and end timestamp.
Set `OSG_DCS_COMBINED_SOAK_SECONDS=90` for the short diagnostic run or
`1800` for the duration gate.
