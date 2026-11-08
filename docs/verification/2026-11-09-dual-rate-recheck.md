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
