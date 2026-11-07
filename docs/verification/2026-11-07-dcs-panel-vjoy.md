# Combined DCS pause, browser panel and vJoy

One running desktop Cockpit profile routed authored DCS ownship telemetry
to a paired compiled browser panel and routed the panel press and an
independent debug source to separate buttons on the same vJoy slot.
The DCS observer held one acknowledgement while its independent reader
received 36 updates. The real browser worker delivered host lamp feedback
that went fresh, stale on ownship pause, and fresh again on resume. The
applied profile owner stayed the same.

Windows Raw Input received all 65 ordered debug transitions and the panel
press. Both buttons stayed held until Stop and released 1.91 ms after its
submission on the Windows performance counter. The slot became free and
the fixture restored the original private installation and provider state
with no cleanup errors.

Run `node --test tests/ui/dcs-physics-native.mjs` against a disposable native
Cockpit with a seeded private DCS installation and a valid DCS-BIOS copy.
Set `OSG_DCS_PHYSICS_NATIVE=1` and `OSG_DCS_COMBINED_VJOY_PROBE` to the
rebuilt Raw Input probe. The evidence is in
`.research/dcs-physics-native/1794038075714/evidence.json`. The dev build
from `3592e502` has SHA-256
`16781f2732f42e6d689ec9c740489c8d08a8bbb5cd13f5c78b5a3db575802ac5`.
The compiled publication is `scripts/fixtures/compiled-flight-publication.json`.

The separate older DCS-BIOS browser protocol fixture failed the current
compiled-panel asset negotiation. The combined run uses the compiled
publication and authored UDP. The 30-minute rig soak and real simulator
effects remain in #199.

## Routing with previews closed

The follow-up opened the Cockpit Debug telemetry inspector and observed its
native `games.telemetry` subscription and visible channel table. After the
stalled-reader phase it closed the two test readers and navigated from Debug
to Overview. The DevTools trace confirmed successful unsubscribe responses
for all three tokens. It recorded zero new preview subscriptions and zero
inspection reads during the closed phase.

The same applied owner sent a fresh host lamp update to the browser panel.
Windows Raw Input recorded both additional vJoy transitions after the readers
retired. The original 65 debug transitions and the panel press arrived in
order. During the withheld acknowledgement the independent reader received 31
notifications. Stop released both held buttons in 3.11 ms and the slot became
free. The fixture passed in 40.08 seconds with no cleanup errors.

The follow-up used the same native executable and compiled publication as
above, and the committed `3893eb7b` frontend in the native WebView. The
Raw Input probe SHA-256 was
`5ed395550241f052ac39fff604c347e5d0fe90efd97e8ba2df9b4f47b5d86947`.
Evidence is in `.research/dcs-physics-native/1794039524953/evidence.json`.
An earlier attempt timed out while detecting the inspector. The first
DevTools run reached the vJoy phase but used an older Raw Input probe that
exited before reporting readiness. The recorded pass uses the current
dual-button probe.

## Authored UDP to browser canvas timing

The timing publication adds two compiled lamp frames with distinct colors.
Each of 24 alternating ownship values was timestamped at the first UDP
submission carrying that value. The production browser panel recorded the
matching fresh host feedback and the actual canvas write of the expected
color. The test aligned the Node and browser clocks before and after the
sample. A native telemetry observer held its first acknowledgement throughout.

| Stage | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| Authored UDP submission to browser worker delivery | 30.83 ms | 48.69 ms | 49.02 ms |
| Authored UDP submission to canvas write | 31.23 ms | 49.19 ms | 49.52 ms |
| Browser worker delivery to canvas write | 0.50 ms | 0.60 ms | 0.60 ms |

Clock alignment uncertainty was 0.18 ms. The p99 is the maximum of the 24
samples. The test gates the p95 upper bound at 150 ms. The fixture publishes
ownship at 40 ms intervals and measures from submission on loopback. The
measurement ends at the canvas write and covers one panel. LAN delivery,
display scan-out, simulator export and the full simultaneous R0 workload
remain separate qualifications.

The full rerun passed in 41.11 seconds with no cleanup errors. After the
fixture process exited, the same generation-2 profile owner remained active
in Degraded with only game route and telemetry notices and no fault. The
native executable and Raw Input probe identities match the preceding run.
Evidence is in `.research/dcs-physics-native/1794040014188/evidence.json`.
The publication hash is `sha256:e2e2deb75ccfb202722302ea4ca9491d02a5bca3a09982276e648ba88114621a`.
[Timing publication](../../scripts/fixtures/compiled-flight-timing-publication.json)

The first run measured 49.15 ms p95 to the canvas, then reported a mapping
heartbeat expiry from the final Stop after process exit. The feeder fallback
released ownership, while recovery guards rejected independent cleanup. That
failure is preserved in `.research/dcs-physics-native/1794039894784/evidence.json`.
The rerun adds explicit owner and fault snapshots before and after process
exit.

## Stop releases vJoy before game teardown

The previous host stopped virtual output after releasing game and companion
resources. A regression held the host at the start of game cleanup with
the feeder's button held and axis deflected. The old order still had the
slot busy at that point. The fixed host releases desktop bindings, records
the reducer's fallback, revokes peripheral work, and releases the virtual
feeder before script, game, and companion teardown. With game cleanup held, the test observed the
feeder's Stop, neutral button and axis report, and slot release.

The final Windows native suite passed 2,394 tests with 18 default ignores.
Both pinned DCS archive tests passed separately. The repository's line counter
measured 80.33% production line coverage for the changed host. LLVM measured
75.55% function coverage. The build did not instrument branches.

Two live runs against separate private Cockpit databases passed with the
compiled browser panel and Windows vJoy. The first Stop released both held
buttons in 1.30 and 1.49 ms respectively. After reapply and fixture process
exit, the same profile owner remained active with only game degradation and
no fault. The final Stop and teardown completed cleanly in both runs.
The fixture uses authored DCS packets. The dev executable SHA-256 was
`a5a5100a615f96d5b114772994aa0f57ba6441b137f830a92045871880645888`.
Evidence is in `.research/dcs-physics-native/1794041818786/evidence.json`,
`.research/dcs-physics-native/1794041878860/evidence.json`, and
`.research/reactive/vjoy-stop-order/`.

## Two browser panels and dual-rate vJoy in one service

The browser-dev variant runs authored DCS ownship and BIOS packets, two
paired browser panels, the real Cockpit Debug inspector, two paced synthetic
button sources, and a deliberately stalled telemetry reader against one native
gateway service. The stalled reader holds one acknowledgement on the shared
browser connection. An independent reader received 32 updates while the same
profile owner stayed active. Navigation away from Debug retired all three
telemetry readers, with zero preview opens or reads afterward. Both panel
canvases painted a correlated DCS change inside the 190.21 ms overlap between
the two button sources.

Windows Raw Input received all 218 ordered vJoy transitions. Both lanes
peaked at two pending frames and the held buttons released 1.83 ms after
Stop. The input-to-receiver timestamps share the Windows performance counter.

| Target | Actual | HTTP round trip p50 | Submission to Raw Input p95 | Submission to Raw Input p99 |
| ---: | ---: | ---: | ---: | ---: |
| 1,000 Hz | 442.04 Hz | 1.73 ms | 4.97 ms | 10.84 ms |
| 125 Hz | 126.18 Hz | 2.19 ms | 5.62 ms | 6.49 ms |

Each lane awaits its HTTP response before sending the next transition. The
1,000 Hz lane missed the offered-rate target. The measured rate describes the
closed-loop browser-dev test source. In standalone runs, the previous harness
also missed the fast target at 845.07 Hz. The extracted harness subsequently
passed at 980.84 Hz and 124.94 Hz using the same service binary. The physical
capture path needs its own measurement.

The panel distributions use 24 alternating ownship changes per panel with
the same profile and stalled observer, before the short dual-rate burst.

| Panel | UDP to page receipt p95 | UDP to canvas p95 | Page receipt to canvas p95 | Alignment uncertainty |
| --- | ---: | ---: | ---: | ---: |
| 1 | 48.93 ms | 49.23 ms | 0.50 ms | 0.25 ms |
| 2 | 268.66 ms | 269.16 ms | 0.50 ms | 0.27 ms |

Panel 2 missed the 150 ms budget. The two preceding runs measured 188.58 ms
and 190.62 ms for panel 2 and 419.26 Hz and 423.77 Hz on the fast lane. The
measured panel tail occurred before page receipt. The combined test
preserves the full timing and lifecycle evidence
before failing its budget assertion. The native single-panel regression
passed on the same product revision. These runs completed cleanup.

To repeat, point `OSG_DCS_PHYSICS_BROWSER_URL` at a disposable loopback
browser-dev Cockpit with the managed DCS fixture configured. Set
`OSG_DCS_COMBINED_VJOY_PROBE` and run
`node --test tests/ui/dcs-physics-native.mjs`. The browser-dev gateway has
no MCP listener. This evidence covers authored packets and loopback panels.
Same-service MCP, the full R0 soak, actual simulator export, physical
capture, LAN and display scan-out remain separate.

The product revision was `027ee955`. Evidence is in
`.research/dcs-physics-native/1794045080235/evidence.json`,
`.research/dcs-physics-native/1794044739216/evidence.json` and
`.research/dcs-physics-native/1794044555463/evidence.json`. The native
regression evidence is in `.research/dcs-physics-native/1794044792093/evidence.json`.

## Retry a contended panel feedback commit

The slow panel advanced fewer host feedback sequences than its peer over the
same authored DCS changes. A successful panel commit wakes its socket reader,
which can briefly hold the shared Hub mutex when the next panel tries to
publish. Previously, a busy commit went through the full 34 ms publication
cooldown. The background feedback worker now gives a still-current projection
one retry after a 2 ms contention backoff. Both attempts check the same
assignment, session, binding generation, cancellation and age fences. A
second miss returns to the existing cooldown and latest projection.

An experimental batch commit put both panels under the timing budget, but
required a ready panel to wait for every later panel's validation. The final
change keeps the original per-panel commit order. Ready publications continue
to commit before the worker starts validating a later panel.

| Run | Panel 1 p95 | Panel 2 p95 | Panel 2 p99 | Fast synthetic source |
| --- | ---: | ---: | ---: | ---: |
| Before, `1794045080235` | 49.23 ms | 269.16 ms | 311.93 ms | 442.04 Hz |
| Retry, `1794046215504` | 49.35 ms | 49.35 ms | 64.28 ms | 412.62 Hz |
| Retry, `1794046263204` | 20.68 ms | 20.76 ms | 27.25 ms | 428.48 Hz |

Both retry runs met the 150 ms panel budget over 24 changes per panel.
Page receipt to canvas stayed at or below 0.50 ms p95. Both panels also
painted during the separate short dual-source burst. Each run delivered all
218 ordered vJoy transitions, released both held buttons within 1.03 ms
of Stop, retired all three telemetry readers, and completed cleanup.
Submission to Windows Raw Input remained below 6 ms p95 on both lanes.
The combined test failed only its 1,000 Hz synthetic HTTP source assertion
in both runs. Same-service MCP, sustained load and physical qualification
remain open.

The Windows native suite passed 2,398 tests with 18 default ignores. The
two pinned DCS archive tests passed separately. The changed production
module reached 98.05% line coverage by the repository counter and 97.06%
function coverage by LLVM. The build did not instrument branches. New
regressions cover immediate success, a single retry, repeated contention,
and cancellation before retry.

The retry runs used Windows browser-dev gateway SHA-256
`1f8f19e397d91e7ea2a8878860c5beb18401d8350671d971bea8bc55986bf1bb`.
The local build selected only the Rust library needed for the gateway
binary to fit on disk. The runnable artifact linked in Cargo's dependency
directory. Windows refused to replace a separate stale top-level alias, so
the test launcher selected the linked artifact by hash.
Raw evidence is in `.research/dcs-physics-native/1794046215504/evidence.json`
and `.research/dcs-physics-native/1794046263204/evidence.json`.

## Desktop MCP during DCS, paired panels and vJoy

The desktop variant pairs two browser panels and opens two authenticated MCP
subscriptions to the active profile's vJoy samples. One MCP client leaves its
response unread after the baseline. The other receives an authored debug
button press and a real panel press on separate vJoy ports, with advancing
revisions in the same observation generation. Both canvases paint the DCS
change and MCP sees the panel press inside the 125 Hz debug burst. The
native Debug inspector is open, and a separate native telemetry reader
withholds its acknowledgement while another continues to receive updates.

Windows Raw Input checks all 65 authored debug edges, the panel edge and two
additional debug edges after the UI previews close. Both held buttons stay
held until MCP Stop. The fast MCP client receives the retirement event, and
the unread client receives it when reading resumes. Stop releases the vJoy
slot. The temporary credential is revoked and listener settings are restored.
The unread application stream and the withheld native acknowledgement are
separate stressors.

| Run | Debug rate | Panel 1 p95 | Panel 2 p95 | MCP unread | Raw Input Stop release | MCP Stop reply |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `1794047182905` | 122.87 Hz | 18.56 ms | 21.08 ms | 3.45 s | 42.17 ms | 96.32 ms |
| `1794047289747` | 121.97 Hz | 7.14 ms | 46.20 ms | 3.76 s | 43.93 ms | 156.69 ms |
| `1794047419528` | 125.25 Hz | 18.00 ms | 20.31 ms | 3.46 s | 37.59 ms | 117.17 ms |

The panel distributions use 24 changes with both MCP readers open. The
short routing burst has a separately correlated canvas update per panel.
In the final run the two paints and MCP's panel-press sample landed within
the same 510.99 ms debug burst. The 65 matched Windows performance-counter
timestamps gave 2.80 ms p50, 9.24 ms p95 and 10.26 ms p99 from debug
submission to Raw Input. The input queue peaked at one frame.
Each run uses authored simulator packets and loopback panel traffic. The
additional 1 kHz source under this workload, a sustained soak, LAN and
physical/simulator acceptance remain open.

For a repeat, set `OSG_DCS_PHYSICS_NATIVE=1`, `OSG_DCS_COMBINED_MCP=1` and
`OSG_DCS_COMBINED_VJOY_PROBE` and run `node --test tests/ui/dcs-physics-native.mjs`
against a disposable running desktop Cockpit with managed DCS fixture
configuration. The product revision was `67756783`, with desktop SHA-256
`40f67438269b28d3f4f51f94adead43a85b367d924efe60b6ed95af778e480c8`.
Raw evidence is in the corresponding `.research/dcs-physics-native/<run>/evidence.json`.
The existing single-panel desktop DCS case and standalone MCP/vJoy case
also passed. The stream and browser-feedback unit coverage gates passed.
The browser-dev regression completed cleanup and missed only its existing
1,000 Hz synthetic source gate at 446.28 Hz. Its evidence is in
`.research/dcs-physics-native/1794047498183/evidence.json`.

## In-process desktop dual-rate source

The opt-in Windows qualification build schedules two authored debug sources
against the Windows performance counter. It submits each edge through the
ordinary native gateway with an exact applied owner and a separate ordered
stream ID per source. One IPC call starts the fixed burst. All 218 edges
still go through ordinary owner checks, queue admission, reducer delivery
and the vJoy feeder. The test keeps the same DCS profile, two browser
panels, the Debug inspector, the stalled native acknowledgement and both
MCP readers.

| Target | Actual submission rate | Native gateway call p50 | Submission to Raw Input p50 / p95 / p99 |
| ---: | ---: | ---: | ---: |
| 1,000 Hz | 442.78 Hz | 1.69 ms | 3.16 / 5.10 / 7.48 ms |
| 125 Hz | 55.46 Hz | 1.68 ms | 3.15 / 4.23 / 7.78 ms |

The fixture uses one native worker for both lanes. The slower call cadence
therefore stretched both schedules. The native gateway time persisted
after removing per-edge WebView and HTTP transport from this test. The
trace does not separate coordinator lock wait from gateway and reducer
work. The test retains the original rate and delivery thresholds.

Windows Raw Input received all 193 fast-lane and 25 peer-lane edges in
order, followed by both Stop releases. Each lane's reported queue peak was
one frame. Both panel canvases painted during the measured 430.99 ms
overlap and the 24-change presentation phase measured 7.02 and 9.67 ms
p95. The fast MCP client saw both held values in one snapshot. The second
MCP response went unread for 3.41 s and both readers observed retirement.
Windows received the releases 39.49 ms after MCP Stop submission. The HTTP
reply arrived at 120.92 ms. Cleanup completed without errors.

A second fresh-owner run reproduced both misses at 430.06 and 53.74 Hz.
Native gateway medians were 1.71 and 1.70 ms. All 218 edges arrived in
order, with 5.20 and 4.36 ms p95 to Raw Input. Both panels painted during
overlap, both MCP readers retired, and cleanup completed. The same
desktop executable was used for both runs.

Build desktop Cockpit with Cargo feature `reactive-qualification` in a
Windows debug build. Start a disposable instance with
`OSG_REACTIVE_QUALIFICATION=1`, then run the desktop MCP fixture with
`OSG_DCS_COMBINED_NATIVE_RATES=1`. The source admits one fixed burst at a
time, requires two neutral applied debug buttons, and stops submitting
when a gateway call fails or the time window expires. It attempts to
release partial input through the same owner fence on failure. The
fixture also stops the disposable profile during cleanup.

The feature-enabled Windows test binary passed 2,399 tests with the
qualification source disabled by default. The explicit enabled test and
both pinned DCS archive tests passed separately. The qualification module
reached 90.15% line coverage by the repository counter and 78.57% function
coverage by LLVM. This build did not instrument branches.

An earlier attempt waited for separate MCP messages when one snapshot
could carry both values. Exposing the native reply immediately showed
that the in-process gateway requests lacked the required client ID. The
corrected runs are `.research/dcs-physics-native/1794048557207/evidence.json`
and `.research/dcs-physics-native/1794048975685/evidence.json`,
using desktop SHA-256
`d517018db2e4becaf6fa2cff8fc3582a68bb356456c3f4fcdf20ec87b39f0ceb`.

## Independent 1 kHz and 125 Hz offers with bounded native admission

The follow-up timestamps each offered edge before handing it to a bounded
lane queue. The scheduler never waits for gateway completion. Each lane
keeps its stream ID and order, and enters the normal coordinator and
debug frame queue under the usual owner checks. The portal and MCP debug
requests also skip the redundant profile guard lock. The exact owner check
still runs under the coordinator lock.

With one coordinator acquisition per edge, run `1794049524376` offered
1,000.00 and 125.01 Hz. The fast lane completed at 493.44 Hz, with
1.61 ms median coordinator wait and 0.031 ms median coordinator work.
It reached Windows Raw Input at 194.98 ms p95 from offer. The
source-to-dispatch queue alone was 190.68 ms p95. All 218 edges arrived
in order, and the 125 Hz lane met its latency budget.

The final qualification worker drains up to eight already available
edges in one acquisition. It never waits for more edges to fill a batch.
It validates each exact owner and ordered frame, then wakes the profile
worker and publishes the debug state change after unlocking. The wake
is shared by the batch. Each edge still goes through the real reducer
and vJoy path.

| Run | Offered rates | Offer to Windows Raw Input p50 | p95 | p99 |
| --- | --- | --- | --- | --- |
| `1794049824258` | 999.81 / 124.99 Hz | 3.94 / 4.19 ms | 8.46 / 10.06 ms | 9.77 / 10.45 ms |
| `1794049920834` | 999.64 / 124.96 Hz | 3.63 / 3.66 ms | 5.77 / 5.33 ms | 6.51 / 5.43 ms |
| `1794049969060` | 1,001.22 / 125.16 Hz | 3.81 / 3.85 ms | 9.41 / 9.03 ms | 13.37 / 12.72 ms |

The fast lane used 54–64 acquisitions for 193 edges, with a peak batch
of eight and a peak of ten pending native frames. Its first-run median
acquisition wait was 2.19 ms. Wait is measured once per batch. Each edge
has a separate offer, dispatch, admission and Raw Input timestamp. The
acquisition wait excludes the outer profile and game admission reads.

All three fresh-owner runs delivered all 218 ordered edges. Both panels
painted during the 191.75–192.05 ms overlap. Their separate 24-change
presentation p95 stayed below 10 ms. The fast MCP reader observed both
held values and both readers retired after Stop. Windows received both
releases 38.58–44.00 ms after MCP Stop submission, and cleanup completed.
The ordinary single-source MCP/vJoy regression passed with qualification
disabled. Sustained load, actual simulator, LAN and physical-device
qualification remain open.

The batch build used desktop SHA-256
`9b69eea9eb3edeaa2b4356ef5d29a01e335993f986737e45d52580096ffd8881`.
Raw evidence is in the corresponding `.research/dcs-physics-native/<run>/evidence.json`.

The final Windows suite passed 2,400 tests with 18 default ignores. The
explicitly enabled qualification test and both pinned DCS archive tests
passed separately. An earlier high-parallelism run exhausted disk space
while three X-Plane tests copied their fixture executable. The same
instrumented binary passed at four test threads.

Repository production line coverage was 82.14% for the qualification
module and 97.37% for the shared gateway module. LLVM module function
coverage was 75.00% and 87.50% respectively. This build did not instrument
branches. The stream and browser feedback coverage checks also passed.

## 30-minute desktop soak

The disposable Windows Cockpit at `926cec91` passed **1,800.007 seconds** of
combined load with authored DCS ownship and BIOS, an applied profile, two
separately paired loopback browser panels, and a real Windows vJoy slot.
Before measurement, the fixture exercised deliberate DCS withdrawal and rearm,
warmed both panel renderers and let automatic startup discovery finish.

- The same profile owner and both armed panel sessions and bindings survived
  **919 health checks**, **60 inventory refreshes**, **60 completed discovery
  scans**, and repeated navigation through Devices, Games, Debug and Overview.
  Both panels painted each authored lamp change. The panel observer recorded no
  new non-fresh lamp values, feedback closures or binding changes during the soak.
- One native observer held its first vJoy node sample acknowledgement for the
  measured interval. The independent fast observer received **919** known
  samples during that hold. Releasing the acknowledgement delivered the next
  sample to the stalled observer.
- Across 60 memory samples, Cockpit private bytes ranged from **73.14 to
  89.43 MiB**. The five-minute peak increased from **81.38 to 88.67 MiB**
  between the early window after warmup and the final window. Browser panel
  JS heap peaks fell from **14.99 to 8.45 MiB** and **15.86 to 8.76 MiB**.
  The fixture allows up to 64 MiB of peak growth between those windows to
  accommodate cache warming and garbage collection.
- The post-soak fixture captured 65 authored debug transitions at **124.08 Hz**
  and two additional transitions after preview teardown. Both MCP readers
  retired on Stop, Windows reported the slot free, and cleanup reported no
  errors. Transport stream coverage was **98.28% lines / 93.06% branches /
  92.86% functions**. Browser feedback reached **100%** on all three metrics.

To repeat, use a disposable running Windows Cockpit with the same authored DCS
seed and Raw Input probe described above. Set `OSG_DCS_COMBINED_SOAK_SECONDS=1800`
and `OSG_DCS_SOAK_COCKPIT_PID` to that Cockpit process ID. Enable the native
DCS and combined MCP modes with `OSG_DCS_PHYSICS_NATIVE=1` and
`OSG_DCS_COMBINED_MCP=1`. Shorter runs exercise the fixture without satisfying
the duration gate. This fixture exercises real Windows output and browser
rendering with authored inputs. Physical-controller latency and live simulator
effects are tracked in [#199](https://github.com/opensimgear/cockpit/issues/199).
