# Windows controller Raw Input reads

The Windows capture owner now supplies a reusable, aligned 256 KiB buffer to
`GetRawInputData` and parses the returned byte count. This removes the separate
size query and allocation per Windows message. The HID decoder reads each
validated report in place and reuses its result array. It clears that array
before decode because a button can write only the one-byte `On` member. The
decoded map is prepared before taking the registry lock. Ordered batches and
the existing packet and report limits still govern admission.
[GetRawInputData](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getrawinputdata)
· [HidP_GetData](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/hidpi/nf-hidpi-hidp_getdata)
· [HIDP_DATA](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/hidpi/ns-hidpi-_hidp_data)

The Windows fixtures prove one API call per message into the same buffer,
ordered reports across distinct report IDs, a clean button value after a
wide axis, and rejection of a truncated packet after a successful read.
All 176 controller tests and 183 mapping-host tests passed. The changed
production module reached 98.25% line and 86.21% function coverage.

## Running Windows Cockpit

The isolated dev app passed the Devices preview hide and reveal check,
visibility reader retirement, and shared capture with an applied output-free
profile. The profile continued to receive the known value with its preview
closed. Reopening took a fresh baseline. Profile replacement and Stop cleared
the retired owner. The profile fixture used revision 1 with semantic hash
`sha256:8c220d7f7c771227a8980cc2bca60e8af986336de02aea1cca56fb100b9f2879`.

The stationary VKBsim Gladiator EVO R exposed 137 controls. Over ten seconds
the Devices preview matched 125 consecutive reports, with 80.004 ms median
between journal commits. Journal-to-DOM p50/p95/p99 was 7.04/8.18/8.75 ms with
a ±2.15 ms clock bracket. This check starts after native decode and reports
DOM updates. A deliberate physical sweep, screen presentation and
raw-input-to-virtual receipt remain separate in #199.

The dev app SHA-256 is
`ff97e6af856398060cf39c0e0e83ba41baec761423a22587abce5b20f7517740`.
The local timing file is
`.research/c193-reconnect/raw-input-read/controller-preview-latency.json`,
SHA-256 `4af3757cedd6d0e2531f485c6395c56bac0d3ce0d108caec2816eed962b6e926`.
The profile evidence is in
`.research/reactive/profile-source-wakes-native/1794199561897/evidence.json`.

## Time from native read completion

The Windows owner records a monotonic instant immediately after
`GetRawInputData` returns. Valid reports from the same raw packet share that
read instant. The registry converts it into the controller journal clock and
retains it on the ordered report and on the matching preview sequence. The
journal commit governs ordering and admission. An unavailable
source and an authored report omit the native read field. A snapshot for a
newly applied profile keeps its own baseline boundary.

The preview diagnostic joins the raw read, journal, observer and Devices DOM using the exact
generation and report sequence. The raw-read-to-journal interval uses one
native clock. DOM intervals use the measured WebView clock bracket.
[Preview diagnostic](../../scripts/reactive/controller-preview-latency.mjs)

Two stationary ten-second traces used the same Windows dev executable.
Each matched 125 consecutive reports and reported zero sequence gaps.

| Device | Controls | Median read interval | Read to journal p50/p95/p99 | Read to DOM p50/p95/p99 | DOM clock bracket |
| --- | ---: | ---: | ---: | ---: | ---: |
| VKBSim T-Rudder | 10 | 79.994 ms | 29/45/52 µs | 2.79/3.47/3.65 ms | ±2.15 ms |
| VKBsim Gladiator EVO R | 137 | 80.003 ms | 272/395/410 µs | 7.73/22.19/23.77 ms | ±2.25 ms |

On the Gladiator, the diagnostic observer to matching DOM interval had a
14.60 ms p95. The two paths share the same controller report. This locates
the longer wait after native decode in this run. Movement-to-screen and
raw-input-to-virtual receipt still require the physical checks in #199.

The Windows dev build SHA-256 is
`03d39004dcb3d6c7e75c06f201f4b26a94bbc043fe75a32ff5f9b3dbcf909136`.
The profile and preview lifecycle fixtures passed again. Profile capture
evidence is under
`.research/reactive/profile-source-wakes-native/1794200119208/`.
The following files are under `.research/c193-reconnect/raw-read-clock/`.

| Artifact | SHA-256 |
| --- | --- |
| `controller-preview-latency.json` | `44b5f0b8b82855c3e3cadd1974557b51441e8b9443f7ab810a08463345adb2f7` |
| `gladiator/controller-preview-latency.json` | `e0c3dfecbd8f49cb9787813359cd3e373f89e0a1cb8fbbe0e8bd7a642242ed5c` |

All 177 controller, 183 mapping-host and 1,754 portal/script tests passed.
The production frontend build passed and Svelte reported zero diagnostics.
Changed controller production modules reached at least
94.24% line and 74.55% function coverage. Controller preview validation
reached 98.74% line, 96.93% branch and 97.44% function coverage.

The full Windows app test binary passed 2,490 tests across a two-thread run
and one isolated rerun, with 18 existing ignores. The first eight-thread
run hit disk exhaustion when several tests copied the executable at once.
The two-thread run passed 2,489 tests. After reclaiming an obsolete build
archive, the Assetto executable bounds test passed by itself with room for
its 512 MiB fixture. Both runs used test executable SHA-256
`91afbb3532c2ea8db30ab3324a91e26a582d4449bd96006f9c49ced80121b39c`.
