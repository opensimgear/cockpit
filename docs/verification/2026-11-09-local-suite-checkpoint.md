# Local reactive verification on Windows and WSL

Cockpit production sources are from `af78067d1f53e0c5c47a913c3a0a899ef67aba42`.
The clean frontend export used the pinned installed dependencies and a private
WebView and database. This checkpoint adds the MCP topic documentation and the
Nx test ownership correction to that export.

## Cockpit

All applicable `check`, `test` and `build` targets passed across 19 Nx
projects with caching disabled. They include the portal, browser and
mobile companion frontend checks, MCP client tests and configured
TypeScript coverage. The first run found that the Devices coverage
task and the shell test list both ran the visibility test. Assigning
the test to Devices leaves one coverage owner. The release tooling
and shared fixture checks passed once the installed Python runtime
preceded the Windows Store alias on PATH.

The existing [instrumented Windows suite](2026-11-09-dual-rate-recheck.md#debug-input-capture-to-reducer)
passed 2,490 tests with 18 existing ignores. The changed native modules met
the line and function coverage floors. The same dev executable passed the
[30-minute dual-rate qualification](2026-11-09-dual-rate-recheck.md#sustained-debug-capture-to-reducer-measurement).
Its SHA-256 is
`4f66a66d36f49f8aff26ca7cb9bf9da5a6d1107a8bb28ce70a231261132cb31b`.

The live UI fixture passed preference fan-out to two readers, disposal and
three reloads without a gateway refresh. The live MCP SDK fixture verified
observation baselines, UI and MCP commits, dirty draft isolation, Activity
updates and control parity. Its first run found three advertised topics
missing from the parity table. The documented topic set now matches the
runtime list and the live rerun passed.

| Live evidence under the local export | SHA-256 |
| --- | --- |
| `source/.research/reactive/native-transport.json` | `39416fd82d20e543f19430a535fe2cb12ed895b54ef88060bdbaa4539f560ac5` |
| `source/.research/reactive/mcp-native-observation.json` | `a410969103d75c2ae19190cd065fac3f51be2cbe974159186f458a57a27d11f` |

## Firmware

| Host suite | Local result |
| --- | --- |
| Firmware common `64025cce` | 10 CTest cases passed with the pinned TinyUSB descriptors. Portable core coverage was 99.78% lines. |
| Device template `c5457884` | 24 CTest cases passed, including native USB event and metadata delivery. |
| Device gateway `dfd83084` | Six CTest cases passed. Gateway publication passed ASan, UBSan and the per-module GCC coverage gates. |
| Pico Debug at the same gateway revision | Protocol-3 host tests passed under ASan and UBSan. The debug gateway and virtual device measured 100.00% and 99.38% line coverage. |
| ESP32 companion | 13 portable suites, 14 packaging and provisioning tests and four export tests passed. Export coverage was 96.83% lines, 95.59% branches and 92.86% functions. |

The [pinned Sunton and Waveshare builds](2026-11-08-esp32-current-qualification.md#waveshare-touch-interrupt-and-held-report-deadline)
identify the firmware images. The ESP32 sources and shared font assets
match `2effc3e85bce63c64c1f36388f471e3fdeb299a0`. The locked LVGL tests
passed for the rich and compact render profiles, and the firmware
software-client sanitizer smoke passed. The whole-panel compositor
reached 100% line, 84.31% branch and 100% function coverage. This run
used portable fixtures and made no writes to physical boards.

The local logs are under `.research/reactive/local-suite-af78067d-42912/`.
Linux desktop dependency resolution stopped offline on `arbitrary 1.5.0`.
The macOS desktop and signed Android and iOS applications were unavailable
in this run. Actual simulator and hardware checks remain in #199 and
mobile prerequisites remain in #55 and #56. CI remains deferred.

## Current Linux and Windows native suite

The exact `c7bf381d` export with the two fixture corrections used the
unchanged Cargo lockfile. The Linux dependencies resolved under Ubuntu
WSL2. The full native suite ran offline with an isolated D-Bus session and
temporary Secret Service store.

The first build found that the desktop lifecycle fixture lacked the tap
clock required by the new desktop adapter. After adding the fixture clock,
all 2,417 Linux library tests passed, with nine existing ignores. The real
gateway test then found that the large editor request referenced devices
absent from its inventory. The fixture now seeds 16 retained controller
identities through the native registry and launches the gateway against that
private SQLite database. The gateway passed the 65,536-byte framing boundary,
profile CRUD, the large editor save with all 16 assignments, the ordinary
profile payload limit, and clean service exit. The standalone framing test
also passed. The final locked Linux run passed all 2,419 tests with nine
existing ignores. The gateway integration fixture passed separately with
97.27% line and 75.00% function coverage.

The instrumented Windows library run passed 2,493 tests with 18 existing
ignores. The desktop lifecycle fixture reached 100% line and function
coverage. Its production desktop adapter reached 86.63% lines and 72.73%
functions. This build measured regions, lines and functions.

The mixed keyboard and pointer retry used the Windows dev Cockpit at
`ec7348da`. The fixture still found PickerHost PID 17540 under the pointer
and stopped before attempting native input. The receiver exited normally,
and the wrapper confirmed stopped ownership and empty cleanup errors.
Mixed pointer receipt remains unqualified. The Linux run exercised a real
gateway process under WSL2. Physical Linux, macOS and mobile qualification
remain open.

Local results are in `.research/c193-reconnect/r9-linux-current/`. The
test and service executable hashes identify the tested artifacts.

| Executable | SHA-256 |
| --- | --- |
| Linux library tests | `afc49b8c5e82ce647d7bc201ef92a48fa1ee2fce137a32001e8718a12a51998c` |
| Linux gateway service | `b4ac9156c592a18e47c7a87389fbf740a9a4b4502aa11b87f31ef7ddfa975a45` |
| Windows library tests | `2048d8b825cbbca6eabc6968f2e3b5b9a11e90fbc79bbd8ee5257bf2bf856136` |

The guarded pointer evidence is in
`.research/c193-reconnect/required-journal-leaf/mixed-ec7348da-24832/evidence.json`,
SHA-256 `7daa656bf4a9a63e97f55216e8c694a9254d9cd7c6661ff95245424c87b79ebb`.

## Current frontend and companion release guard

The clean `88ee864e` export used the pinned pnpm lockfile and Node 24.20.0.
The initial uncached Nx run reached all 19 applicable projects and failed
the companion release-tooling target. Its checker still expected the old
mobile authentication guard. It now verifies that Android and iOS can enter
qualification only with both debug assertions and the explicit qualification
feature, and that the authentication helper receives those inputs in order.
The regression test covers removing either build condition, swapping the
arguments, and bypassing the mobile branch.

All nine focused guard tests passed. Python line tracing measured 99% for
the release checker. `--require-release` exited with code 2 and reported
`releaseEligible: false` with zero qualified client targets. The signed
Android and iOS shell checks still require their native prerequisites.

The uncached `check`, `test`, and `build` rerun passed all 19 applicable
Nx projects. The exact final release-tooling target passed all 42 tests.
The changed controller observation module reached 98.76% lines, 96.98%
branches and 97.50% functions. Logs and the blocked release report are in
`.research/c193-reconnect/current-ui-88ee864e/`.

## Current Windows preview and observation transport recheck

Both Windows transports used the clean `88ee864e` frontend. The existing
native dev executable contains the `cc68085b` production sources. The
commits through `88ee864e` changed only verification documents. The
browser-dev gateway was built offline with the same native sources and
locked dependencies. Each mode used a separate private database.

The live Cockpit paired each of 64 authored controller transitions with
its preview mutation, next frame opportunity, and Windows Raw Input vJoy
receipt. Both transports opened and retired exactly two subscriptions,
reopened with the current value, issued zero runtime inspection requests,
and released the slot at Stop. Times are from authored submission and
reported as p50 / p95 / p99 in milliseconds.

| Transport | vJoy receipt upper bound | Preview DOM | Next preview frame opportunity |
| --- | ---: | ---: | ---: |
| Windows native | 1.73 / 2.16 / 2.51 | 4.70 / 5.30 / 6.00 | 4.80 / 17.60 / 21.70 |
| Windows browser-dev | 2.22 / 2.62 / 4.20 | 6.00 / 6.90 / 7.60 | 6.20 / 18.50 / 23.40 |

Each transport also recovered from an injected acknowledgement failure and
opened a fresh observation for existing and new listeners. Physical device
acquisition and display scanout remain separate measurements.

Devices and Profiles each recorded zero gateway requests and zero observation
protocol requests in 60-second idle windows on both transports. All four
windows were at least 60,101 ms. The complete logs and paired samples are
under `.research/c193-reconnect/current-ui-88ee864e/`.

| Artifact | SHA-256 |
| --- | --- |
| Native executable | `d452dd8ef5126feb889a37fe9c4725549b45cd1dc01f13ae30b7932c9d6726c3` |
| Browser-dev gateway executable | `a0a5d4572968e1e4af7b6c8381b8962e3004fdb825bf5c273e662a99ece9dbf7` |
| Raw Input probe | `6ea93864c16d2897aedc150df5addf45fd65a3dcd89a0f40773a779c88f35e9e` |
| `native-verified.log` | `0f6537bcf393981c624d9cac826b5c76afab7880abeed371bfe043e63121f220` |
| `browser-verified.log` | `d4a819033d95a4ea6c2847c1776ba585519f9f92d5482713c41b4dd0aad5cdac` |

## Linux native suite at the frontend queue fix

The clean export of `e0dcdced9a0f91c69146e7e5c7df62d45d18ea26` passed
the full locked and offline Ubuntu WSL2 suite using a private D-Bus
session and Secret Service store. The library passed 2,422 tests with
nine existing ignores. The native gateway framing unit test and
separate real gateway integration test also passed. The integration
exercised a real service process, including the editor payload limit,
correlated replies, profile CRUD and clean service exit.

An instrumented rerun of the integration passed with 97.27% line,
75.00% function and 95.15% region coverage. This build measured regions,
lines and functions. Physical Linux controller input and desktop
presentation remain separate checks.

Artifacts and logs are under `.research/c193-reconnect/r9-linux-e0dcdced/`.

| Artifact | SHA-256 |
| --- | --- |
| Exact source archive | `045ac77b53ddf77971c60b09cb5feea1bbac385a4de1ad4d00c6041457e5cbbf` |
| Library test binary | `680ba5fd0dca24f94854b18a5edd59e2b546a925374808f847864c3da2e38609` |
| Native gateway service | `68d9237c2e359b32630434ae53659e6ffe32ca47e8422b829e6602256e29934b` |
| Instrumented gateway integration binary | `6526fb65b8e42b43ea24505fb8ef9616bcdc5109a27e5a4514191d0158259673` |
| Full test log | `ca8e0dbc0950f35cf8b3049800952e31e31685ecd3495b1518bd3c9e53b99a22` |
