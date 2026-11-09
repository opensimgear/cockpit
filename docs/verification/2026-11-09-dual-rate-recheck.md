# Windows dual-rate software check

The isolated Windows dev Cockpit ran the authored DCS ownship and BIOS
fixture with native 1 kHz and 125 Hz debug sources, vJoy slot 1, two
Edge panel clients, and fast and stalled MCP subscribers. The Windows
Raw Input receiver recorded ordered vJoy button transitions on a shared
QPC clock. The sources overlapped for 192.11 ms and both canvases updated
during the overlap.

| Target | Source offers | Dispatch / completion | Ordered edges including Stop | Offer to Raw Input p50 / p95 / p99 | Peak dispatch batch |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1,000 Hz | 193 at 999.38 Hz | 986.13 / 986.39 Hz | 194 | 3.89 / 8.84 / 12.89 ms | 8 |
| 125 Hz | 25 at 124.93 Hz | 124.93 / 123.40 Hz | 26 | 3.73 / 7.31 / 9.30 ms | 1 |

Both buttons stayed held until MCP Stop and were released 39.43 ms after
submission. The slot returned to free. The receiver confirmed it kept
capturing for 8.55 s after Stop. The previous five-second capture ended
without the final release. The fixture now requests 15 s for the native
dual-rate check and requires proof that capture extended through Stop.

The two panels recorded authored UDP-to-canvas p50 / p95 / p99 of
4.93 / 6.98 / 10.87 ms and 6.69 / 8.47 / 9.36 ms, respectively. The
clock-alignment uncertainties were 0.25 and 0.24 ms. A separate
48-sample measurement in the same run put the upper bound on authored
UDP-to-inspection p50 / p95 / p99 at 25.61 / 46.95 / 50.56 ms while a
subscriber withheld acknowledgement. Fast notifications continued and
the stalled reader resumed after acknowledgement. Both MCP observers
retired at Stop. DCS withdrawal and recovery, process exit and teardown
passed with no timing misses or cleanup errors.

The dev-only build used `--features reactive-qualification` and ran with
`OSG_REACTIVE_QUALIFICATION=1`, `OSG_DCS_PHYSICS_NATIVE=1`,
`OSG_DCS_COMBINED_MCP=1`, `OSG_DCS_COMBINED_NATIVE_RATES=1` and
`OSG_DCS_COMBINED_VJOY_PROBE` pointing to the two-button probe built from
[`RawInputProbe.cs`](../../tests/controller-mapping-m0/RawInputProbe.cs).
Run `node --test tests/ui/dcs-physics-native.mjs` against an isolated
running dev Cockpit. The native app code matches `08142f5b` through
`534876ad`. The dev binary SHA-256 is
`351d7be47fcee93cc848c00ab956593108fdd1c35d2a200bdee25bb24595a35e`.
The local evidence is `.research/dcs-physics-native/1794180093199/evidence.json`
with SHA-256 `f943965f3ddfd3bc7d7bd821bf44a8516d701f3e56fb3f7cff31942f0fe18694`.
The fixture syntax check and both focused vJoy transport and native burst
module checks passed.

This is a short authored burst through the running app. Sustained
dual-rate load, the complete R0 baseline and the integrated timing
matrix remain open in [#189](https://github.com/opensimgear/cockpit/issues/189)
and [#198](https://github.com/opensimgear/cockpit/issues/198). Physical
controller-to-preview and controller-to-virtual latency are in
[#199](https://github.com/opensimgear/cockpit/issues/199).

## Extended 4.8-second run

With `OSG_DCS_COMBINED_EXTENDED_RATES=1` and a rebuilt two-button
receiver, the two input schedules overlapped for 4,800.15 ms. The
receiver checks its event bound and capture end timestamp. Both
panels painted during the overlap.

| Target | Offers and actual rate | Dispatch / completion | Offer to Raw Input p50 / p95 / p99 | Peak pending frames / batch |
| --- | ---: | ---: | ---: | ---: |
| 1,000 Hz | 4,801 at 999.97 Hz | 999.87 / 999.88 Hz | 3.66 / 7.97 / 10.53 ms | 12 / 8 |
| 125 Hz | 601 at 125.00 Hz | 125.00 / 124.98 Hz | 3.59 / 8.27 / 11.91 ms | 10 / 2 |

Raw Input received all 5,402 source edges in order and both Stop
releases. Both releases arrived 47.38 ms after Stop submission. The
receiver remained active for another 4.52 s and the vJoy slot returned
to free. In the same run, the panels measured UDP-to-canvas p95 of
20.52 and 22.85 ms with 0.22 and 0.20 ms alignment uncertainty.

The first extended attempt admitted both schedules but its fast MCP
test client read only one sample and later saw an unexpected EOF. The
fixture now drains the fast reader continuously after the first matched
update. On rerun, it received updates during the burst, drained 896
updates in total and received retirement. The unread reader's HTTP
response closed. DCS withdrawal and recovery, process exit and cleanup
passed with no timing misses or cleanup errors.

The extended Windows dev binary SHA-256 is
`116ade662033ba4423ca1ee13dba0ee7c148f150fc148d952f355f8d01994af6`.
The receiver SHA-256 is
`8ce5a3fb5fa814048be555d08b31682f51e75fd9050deb04983b9a4d9094d754`.
The trace is `.research/dcs-physics-native/1794180557421/evidence.json`
with SHA-256 `d5489804f8d9c7d0acb7db6506baf4f84a90b1167098d2c308e03d145ba3b68f`.
The native qualification tests passed with the dev source enabled and
disabled. LLVM measured 80.88% line and 76.00% function coverage for
the native qualification module. Branch instrumentation was unavailable.
The 30-minute soak uses a different input workload. Sustained dual-rate
load and the physical latency in #199 remain open.

## Sustained dual-rate fixture

Set `OSG_DCS_COMBINED_SOAK_SECONDS=1800` in addition to the native
dual-rate flags above. Use `15` for the live smoke. Set
`OSG_DCS_SOAK_COCKPIT_PID` to the disposable Cockpit PID for private-memory
samples, and use the current `RawInputProbe.cs` built with .NET Framework
`csc.exe` and `System.Web.Extensions.dll`. The existing one-source soak
runs when `OSG_DCS_COMBINED_NATIVE_RATES` is off.

The two native lanes write 32-byte records into separate, exclusively
created files. The independent Windows receiver writes 14-byte Raw Input
records for the selected vJoy slot and signals its QPC start and end. The
fixture joins the traces in bounded chunks, checks every ordered edge and
both Stop releases, and summarizes timing in 0.1 ms bins. Processing uses
fixed-size buffers, and the duration and event limits cap disk use. Panel identities,
owner health, gateway attribution, telemetry and readers use the existing
soak checks.

The 15-second live smoke overlapped the two schedules for 15,007.98 ms.
Windows received all 16,886 offered edges and both Stop releases. The
offer-to-Raw-Input p95 / p99 was 9.40 / 17.20 ms at 1 kHz and
9.60 / 15.20 ms at 125 Hz. Both releases arrived 43.25 ms after Stop.
Both panels stayed armed, 2,686 fast MCP updates arrived, the held
native reader resumed, and one inventory and discovery cycle completed.
There were no budget misses or cleanup errors. The smoke trace is
`.research/dcs-physics-native/1794181213067/evidence.json`, SHA-256
`87069e6a5af51f25ea5c21fab343ae5bcc0c3e3c3224388ceac128bc8b841ed6`.

For this fixture the Windows dev executable SHA-256 is
`0f3a320ccb98dddc80fb995fdc2eef94e5245b3363ba79281a8e2273c701f58e`.
The Raw Input receiver SHA-256 is
`0a66dca7c5355cafe6e1bc12fc52e00e06af9fda9b16baa1c27eea6f44a0bee5`.

### 30-minute Windows baseline

The same build ran the two native sources together for 1,800,000.20 ms
under authored DCS BIOS and ownship telemetry, two loopback Edge panels,
the Cockpit WebView, and fast and stalled subscribers. The working
profile was `53352cfe-d34f-4043-91bc-cb9e8a623e67` revision 2, owner
generation 1. Both panels retained publication
`sha256:e2e2deb75ccfb202722302ea4ca9491d02a5bca3a09982276e648ba88114621a`.

| Source | Offered / received including Stop | Actual offer / dispatch / admission rate | Offer to Raw Input p50 / p95 / p99 | Peak pending frames / batch |
| --- | ---: | ---: | ---: | ---: |
| 1 kHz | 1,800,001 / 1,800,002 | 1,000.00 / 1,000.00 / 1,000.00 Hz | 3.90 / 8.70 / 14.80 ms | 28 / 8 |
| 125 Hz | 225,001 / 225,002 | 125.00 / 125.00 / 125.00 Hz | 4.10 / 9.00 / 13.60 ms | 25 / 5 |

The receiver matched every transition in order. Both releases reached
Raw Input 41.99 ms after MCP Stop and the vJoy slot returned to free.
The separate offer-to-dispatch p95 was 5.30 / 0.10 ms for the two lanes.
Dispatch-to-admission p95 was 4.50 / 6.30 ms. The independent receiver
can observe an edge before admission returns, so those timings are
reported as separate segments.

| Presentation | Samples | p50 / p95 / p99 | Clock uncertainty |
| --- | ---: | ---: | ---: |
| Authored UDP to panel 1 canvas, upper bound | 906 | 4.67 / 12.03 / 44.31 ms | 0.26 ms |
| Authored UDP to panel 2 canvas, upper bound | 906 | 7.40 / 42.65 / 48.31 ms | 0.26 ms |
| Stationary VKB axis-capable reports, journal to visible Devices DOM | 125 | 9.92 / 10.85 / 11.32 ms | ±2.75 ms |
| Same controller reports, journal to next render opportunity | 125 | 14.36 / 17.98 / 18.72 ms | ±2.75 ms |

The VKB measurement opened the preview during a normal gap between soak
navigation cycles. It correlates native capture sequence to the visible
DOM, starting at the journal commit. Movement, hardware capture latency,
display scanout and LAN presentation remain separate acceptance.

The same owner and both armed panels stayed healthy through 906 checks,
60 inventory reads and 60 discovery scans. The fast native and MCP
readers received 335,917 and 336,306 updates. The held native reader
resumed after acknowledgement. The fast MCP reader received retirement
at Stop and the unread response closed. Sixty memory samples showed a
1.45 MiB increase between the measured Cockpit private-memory peaks and
lower closing peaks for both panel heaps. The trace recorded no timing
misses, owner or panel fault, or cleanup error.

The active gateway trace attributed 1,812 profile status reads, 906
runtime inspections and 906 companion status reads to the fixture. App
traffic included 1,619 game page reads, 1,510 of them in Games during
discovery. The separate preview diagnostic accounts for the 20 app-tagged
controller-list reads. The raw evidence contains the remaining per-view
counts and the timing distributions.

This confirms the R0 Windows software baseline budgets of 20 ms p95 and
50 ms p99 to vJoy receipt, 100 ms p95 for foreground UI presentation,
and 150 ms p95 for the loopback panels. The full R9 timing matrix still
requires capture-to-reducer and lease-margin evidence across its paths.
Real simulator, physical controller and LAN checks remain in #199.

Local evidence is under `.research/dcs-physics-native/1794181324677/`.
`evidence.json` SHA-256 is
`3ef50665db787dc4de2bfac3b8ec61f4d5433bfd7d62975217a5104b65334449`.
`controller-preview-latency.json` SHA-256 is
`1cc8ebb590379c2d1848f329e727198d483763394fdc99169f1667cdff43fdc9`.

The three focused native tests passed with the dev source both enabled
and disabled. The full Windows native suite passed 2,486 tests, with 18
existing explicit ignores. LLVM measured 81.53% line and 72.73% function coverage for
the qualification module. The build did not instrument branches. The
binary trace decoder regression, fixture module checks, and UI timer
tests passed locally.
