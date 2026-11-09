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

## Immediate preview after an idle frame

The Devices and Profile inspectors present the first ready report immediately
when no frame is pending. Further reports replace a single pending value.
The next display callback presents the latest value and holds the next
frame slot. An empty frame reopens immediate presentation. A hidden view,
lost source or replaced generation cancels pending work. Other consumers
receive the shared report independently of the preview frame.

Four alternating Windows runs used the same native executable and stationary
137-control Gladiator. Each run matched 125 consecutive report sequences,
with zero gaps. The baseline was frontend revision `21ecd7d1`. The tested
candidate `controller-observations.ts` SHA-256 was
`f195c1b8806818452ec2423fa3ca5320bd18eae7cbeb7354dff326d586562f73`.

| Run order | Read to DOM p50/p95/p99 | Observer to matching DOM p95 | DOM clock bracket |
| --- | ---: | ---: | ---: |
| Baseline 1 | 7.78/21.81/24.08 ms | 14.10 ms | ±2.25 ms |
| Candidate 1 | 7.47/9.24/9.77 ms | 0.40 ms | ±2.30 ms |
| Candidate 2 | 7.55/9.10/10.11 ms | 0.40 ms | ±2.20 ms |
| Baseline 2 | 7.38/19.04/21.44 ms | 11.90 ms | ±2.35 ms |

These traces measure the Devices DOM. The physical motion-to-screen and
raw-input-to-virtual checks stay in #199. The local traces are under
`.research/c193-reconnect/preview-first-report/`.

| Run | Trace SHA-256 |
| --- | --- |
| `baseline-1` | `39191eaeca117f1e4a8fde26c28f7337b8598f52eb32a361f34cec11fd455ee8` |
| `candidate-1` | `52e50c008631ae1c46df3ee75c6abf97a624d2cbcdb2d3c44c9eba9c6c23f34e` |
| `candidate-2` | `f730ba5d37927b642afbdb23a6a814d404e668024b92224d478559bc73c6aff7` |
| `baseline-2` | `9212bd51daab284a9712ef5eb85260ea75954ef8a3b79bb6342c87b622a5f982` |

The running Cockpit passed Devices hide/reveal and visibility retirement,
the passive Profile inspector check, and shared capture through preview
closure and profile replacement. All 1,754 portal/script tests passed.
The production frontend build passed and Svelte reported zero diagnostics.
Controller preview coverage reached 98.76% line, 96.98% branch and 97.50%
function coverage.

## Watched HID routing pages

The capture journal takes ownership of each report's value map and gives
preview publication its commit timestamps and cursor. The profile reader
starts at the unread suffix and copies only reports selected by its native
watch. It advances the global cursor even when that suffix contains only
unrelated controllers. An unread eviction still invalidates the page and
requires a fresh baseline. A watch from a different capture clock cannot
supply a page. The HID adapter looks up routed values in the original
report map.

The running Windows dev app passed Devices preview lifecycle, visibility
reader retirement and shared capture through profile replacement. The
output-free profile retained its physical value with the preview closed.
In a separate test, the same app sent all 65 authored vJoy transitions
and the Stop release to Windows Raw Input while a preview held its receipt.
Authored submission to Raw Input receipt p50/p95/p99 was
1.26/1.52/2.61 ms. Physical raw-input-to-virtual timing remains in #199.

The dev app SHA-256 is
`ed43c0ead1eeb6d41a06aaee13d1b36a4bbcdb409d07b30235b71863d63536ff`.
The receiver SHA-256 is
`5ed395550241f052ac39fff604c347e5d0fe90efd97e8ba2df9b4f47b5d86947`.
The authored trace is
`.research/c193-reconnect/hid-route-copy/vjoy-raw-burst-result.json`,
SHA-256 `8834b0e25f4a1af66fbfeaf129f705d97b6ff51d0be28de032b20f6d9f363e73`.
The physical profile evidence is under
`.research/reactive/profile-source-wakes-native/1794201400008/`.

The Windows tests passed 179 controller-input, 183 mapping-host, and
475 profile-session cases. The profile-session filter had two existing
ignores. Instrumented line coverage was 100% for the controller journal
and snapshot reader, 95.00% for registry observation, and 86.97% for the
native profile HID input adapter. The adapter reached 89.66% function
coverage.

## Physical button to vJoy receipt fixture

The Windows dev fixture saves a temporary profile with one observed HID
button wired directly to button 1 of an explicitly selected free vJoy
slot. The source trace reads the same ordered native journal as the
profile. It retains only the selected button's changes and rejects
missing reads, report gaps and generation replacement. The separate
Raw Input receiver verifies the slot's neutral baseline and exact
ordered transitions. Windows QPC brackets the journal clock before and
after the trace. The result reports lower and upper latency percentiles
and checks the R0 destination budget using the upper bounds.

Start a private Windows dev Cockpit with the `reactive-qualification`
Cargo feature and `OSG_REACTIVE_QUALIFICATION=1`. Give that process a
disposable `OSG_SQLITE_DATABASE` under `.research` and a loopback WebView
DevTools port. Stop any active profile and close simulators. Compile
the [receive-only Raw Input probe](../../tests/controller-mapping-m0/README.md).
In Devices, use the controller inspector's local source ID and the
chosen button's `data-control-id`. Use the IDs shown by this dev session.

```powershell
$env:OSG_NATIVE_CDP_URL = 'http://127.0.0.1:9222'
$env:OSG_PHYSICAL_VJOY_PRIVATE_DATABASE = '1'
$env:OSG_PHYSICAL_VJOY_APP = '<absolute path to the running dev osg-cockpit.exe>'
$env:OSG_VJOY_RAW_PROBE = '<absolute path to RawInputProbe.exe>'
$env:OSG_PHYSICAL_VJOY_SOURCE_ID = '<local controller source ID>'
$env:OSG_PHYSICAL_VJOY_CONTROL_ID = '<physical button control ID>'
$env:OSG_PHYSICAL_VJOY_SLOT = '1'
node scripts/reactive/physical-vjoy-latency.mjs --preflight
node scripts/reactive/physical-vjoy-latency.mjs
```

Keep the button released during preflight. In the measurement, wait for
the tap cue, press and release the selected button at least ten times,
and finish released. The fixture stops the temporary profile, confirms
the slot is free and deletes the profile. It writes the exact identities,
native source edges, receiver timestamps, clock bounds and percentiles
under `.research/reactive/physical-vjoy-latency/`.

The timed interval starts at Cockpit's physical `GetRawInputData`
completion and ends at the receiver's virtual `WM_INPUT` callback.
This includes Windows destination delivery and receiver scheduling.
Physical actuation, pre-read OS/HID time, display presentation, and
simulator effects require separate measurements. The qualification trace
adds one temporary passive journal reader. A stationary preflight
reports no timing samples.

The live stationary preflight used a VKBsim Gladiator EVO R on
`hid:0009:0001:1:0:8` and vJoy slot 1, receiver device
`EC4DE84FA125EBA8`. The exact profile became active, the native trace
retained nine reports, and the receiver saw its neutral baseline.
The conservative clock uncertainty was 2.3 µs. Stop cleared the owner,
the slot returned to free, and the fixture deleted its profile with
zero cleanup errors. The first setup attempt confused the local source
ID with the persistent profile resource ID and stopped before Apply.
The fixture now resolves the exact resource through its HID connection.

The native trace passed four focused tests, including a stationary
capture with the real controller. LLVM measured 94.92% line and 87.50%
function coverage for its module. Branch instrumentation was
unavailable. All five matcher tests passed with 100% line and function
coverage and 97.73% branch coverage.

The dev executable SHA-256 is
`787824c23889b119d5feee75d2b729d1d140ae7e0acfc075be1ea108674ca2c9`.
The receiver SHA-256 is
`c0075da5274d9f1f0532e7b07dd7293a0f91c8c40ec98227cee8db27eb21f71c`.
The local evidence is
`.research/reactive/physical-vjoy-latency/51f89263-c1e6-48fe-9134-54b9ae27c975/evidence.json`,
SHA-256 `ad429ad5ebbaf7cda5ab000290ad9655486718b608fd05c8ae90ea8c3a5a2522`.
Physical taps, motion-to-preview, and simulator acceptance stay in #199.

## Current rig and Profiles timing preflight

The Windows dev build from `44217e01` repeated the stationary vJoy
preflight with the attached VKBsim Gladiator EVO R. It observed nine
physical reports, the neutral baseline on slot 1, and 2.3 µs clock
uncertainty. Stop freed the slot and the fixture deleted its profile
without cleanup errors. All five receipt matcher tests passed.

A separate unrouted profile opened the Gladiator inspector in the
foreground with the frontend from `6beee318`. The new preview preflight
received 38 consecutive reports for the six axes and checked a valid
Raw Input read timestamp for each report. The stationary report interval
was 80.00 ms at p50 and 80.07 ms at p95. Native read completion to journal
commit measured 0.329 ms p50, 0.383 ms p95 and 0.400 ms p99. These two
segments use the native clock. The browser clock bracket was ±2.80 ms.

With the assigned controller selected in the visible Profiles
inspector, set `OSG_CONTROLLER_SOURCE_ID` to the current local source ID.
Run `node scripts/reactive/profile-preview-latency.mjs --preflight` with
the controller stationary. Then run the same command without
`--preflight` and sweep an axis during its 15-second capture. The
sweep must yield at least 20 unambiguous visible transitions. The probe
reports native read completion to DOM and the next frame opportunity
separately from report cadence. Display scan-out, deliberate button
delivery to vJoy, and simulator response have separate acceptance steps.

| Local trace | SHA-256 |
| --- | --- |
| `.research/reactive/profile-preview-latency/1794251645252.json` | `b4da6820104cbdbfa38219b237c057317ccbca14dc37667a805803039b3c8a0f` |
| `.research/reactive/physical-vjoy-latency/5f6fa5ab-8efc-4d27-ac32-67722b0267c5/evidence.json` | `b899d5e8660abb06cb1042c6cac52278ae73e9b6cffce93927e517e62c69394c` |

## Operator Gladiator sweep and vJoy receipts

The same Windows dev executable received the operator's physical VKB
Gladiator input. Two main axes spanned the normalized presentation
range from 0 through 32,767. The foreground Profiles inspector matched
1,624 unambiguous changes out of 2,814 changed reports. Its passive reader
received 2,857 reports. The median interval between adjacent native
reports was 4.00 ms. The p95 interval was 4.04 ms. The full run includes
the quiet period before and after movement.

The earlier probe treated any forward jump as loss. The current probe
counts skipped reports when a complete snapshot advances within the
same generation. It still rejects loss, identity replacement, backwards
progress, and nonconsecutive deltas. The final sweep counted 19 reports
coalesced by the preview subscription. The ordered routing journal
has its own reader and evidence.

| Segment | Samples | p50 | p95 | p99 |
| --- | ---: | ---: | ---: | ---: |
| Raw Input read to journal during sweep | 2,857 | 0.187 ms | 0.286 ms | 0.332 ms |
| Raw Input read to Profiles DOM | 1,624 | 8.35 ms | 10.25 ms | 14.34 ms |
| Raw Input read to next frame opportunity | 1,624 | 15.58 ms | 17.20 ms | 20.19 ms |
| Raw Input read to vJoy Raw Input receipt, upper bound | 44 | 0.760 ms | 1.013 ms | 1.046 ms |

The preview clock bracket was ±2.45 ms. The separate button run matched
all 44 ordered press and release events on HID button 3 and vJoy slot 1.
Its native clock uncertainty was 2.3 µs. Stop freed the slot and the
temporary profile was deleted. The first button run matched eight edges,
below the minimum sample count. The final run used a private build of
the same receive-only probe with its maximum observation window extended
to 30 seconds so the operator cue and Stop fit inside the capture.
The native button trace ran for ten seconds.

The measured segments begin at Windows Raw Input read completion.
Physical actuation before that point, display scan-out, and simulator
effects remain separate.

| Local trace or binary | SHA-256 |
| --- | --- |
| `.research/reactive/profile-preview-latency/1794252869754.json` | `c9049598e1d5781832bb194b2cbc09c254808a79e73355db3624f595a811b34c` |
| `.research/reactive/physical-vjoy-latency/8d07844b-e250-4d19-a280-d1acf608e4f2/evidence.json` | `163fde17708d09979241059a1e8e1e7d4d4cf1c6524452a8a486e3431671fb31` |
| `OperatorRawInputProbe.exe` | `bb31c37369b97e467ce4a26a45000108ec1d2aa94a43d6156b5512c88ccba0f5` |

## Native axis range evidence

The Profiles probe now records each axis's declared HID and presentation
ranges alongside the observed native extrema. Each extremum carries its
normalized value and native report sequence. The probe checks the native
to presentation conversion before discarding repeated display signatures,
so two raw samples that display the same value still count toward the
observed span. These witnesses reflect the controller's reported values.

On the attached Gladiator, both main axes declared 0–4,095 native and
0–32,767 in the preview. In the stationary preflight the six axes each
had 39 valid observations, including the initial snapshot. All values
agreed with the declared conversion. The 38 continuing reports carried
Raw Input read timestamps, with no gaps or coalesced reports. The run
establishes the probe and descriptor bounds. A deliberate sweep is still
needed for measured raw travel.

The preflight trace is
`.research/reactive/profile-preview-latency/1794255066025.json`, SHA-256
`d7f9d59d39342656e202599228bb9c328d3443c24c1308cbb2f5605037d271ed`.
The two focused tests exercised all lines, branches and functions of the
range collector.
