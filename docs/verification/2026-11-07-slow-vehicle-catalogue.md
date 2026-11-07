# Slow vehicle catalogue observations

An admitted passive telemetry reader could lose its generation when a
catalogue observation finished after the two-second proof window, even with
an exact native process watch still confirming continuity. After that window
elapses, the observation now discards its result and any unadmitted reader.
The admitted reader, generation, and original capture remain in place.
Existing process and capture expiry rules still govern the visible values.
Confirmed process loss or replacement still invalidates the reader and any
detached I/O.

The regression covers an attached reader and one detached for I/O. In both
cases the catalogue result misses its deadline. The test verifies the same
capture and generation, discards the late catalogue, and checks that a
subsequent confirmed process loss withdraws the capture. The existing opener
regression verifies that a late first open still cannot publish a reader.

## Running Windows Cockpit

The authored `tests/ui/dcs-physics-native.mjs` fixture passed against the
rebuilt desktop Cockpit in 51.76 seconds. It used a private DCS installation,
pinned DCS-BIOS catalogue, two compiled browser panels, a held MCP reader,
and the Windows Raw Input probe for vJoy slot 1. The executable SHA-256 was
`27637826c895b1975c5e5b801e15fca4c763c2f520d6f55c65f41c225055c868` and
the compiled publication was `scripts/fixtures/compiled-flight-publication.json`
at SHA-256 `2364ecb2441a8417141b515bdd96c0f698b669df2c43cb1c1fa986d85485e58f`.

| Check | Result |
| --- | --- |
| DCS quality and ownership | Pause, active pause, export denial, frozen time, respawn, BIOS expiry and process exit produced the expected independent transitions. A competing process withdrew passive telemetry and the sole process recovered. |
| Ordered authored debug input | Windows Raw Input received 65 of 65 transitions at a measured 124.60 Hz and the panel press. The largest pending input count was one. |
| Debug submission to Raw Input | p50/p95/p99 were 2.97/7.28/12.57 ms. Both Stop releases were received after 44.40 ms. |
| Stalled and closed observers | The independent telemetry reader advanced 39 times with one held delivery. Three readers then retired. The panel still received fresh feedback and Windows received both further transitions, with zero preview reopen or read requests. |
| Two browser panels | 24 authored UDP-to-canvas transitions per panel. p50/p95/p99 were 5.68/7.63/7.73 ms and 7.65/9.88/9.89 ms. |

The fixture reported zero configured timing misses, restored the private
installation, and reported no cleanup errors. The raw record is
`.research/dcs-physics-native/1794074773967/evidence.json`. The initial
launch needed a noninteractive Vite readiness check and the bundled
Playwright module specified explicitly. The passing run is the authored DCS
desktop fixture. Cross-adapter contention, native Windows process consent,
and physical controller/simulator qualification remain open.

## Local suites and coverage

Windows passed 2,420 native tests with 18 default ignores. Both pinned
DCS-BIOS archive tests passed separately. The changed observation module
reached 98.63% production line coverage by the repository counter and 100%
function coverage by LLVM. The Rust build did not instrument branches.
All 1,700 frontend tests passed, and Svelte reported zero errors or warnings.
The inventory accounted for all 450 current timing sites and the frontend
timer guard passed. Local suite logs and coverage data are under
`.research/reactive/vehicle-slow-catalogue/`.
