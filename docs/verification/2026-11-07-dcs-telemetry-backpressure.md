# Independent DCS telemetry expiry under backpressure

The running Cockpit fixture holds one real `games.telemetry` receipt while a
second subscriber continues to acknowledge notifications. An authored DCS-BIOS
sender first sends counter-only packets, then goes quiet. The managed ownship
sender continues independently.

The fixture requires the passive BIOS value to become stale and null. In the
applied profile, the BIOS output becomes stale while the selected ownship
outputs stay known and the original ownship capture timestamp advances. The
aircraft and applied profile owner stay the same. The fast observer receives
more notifications while the slow observer still has only one delivery. The
slow observer resumes once its first receipt is released.

The test uses a private installation, database, exporter and revision-2
temporary profile. It also verifies competing-process withdrawal, recovery on
the sole remaining process, independent source recovery, and final process-exit
withdrawal. The tests use authored UDP packets and a stand-in DCS process.

## Results

| Transport | Fast notifications during BIOS expiry | Slow deliveries before release | BIOS / ownship | Cleanup errors |
| --- | --- | --- | --- | --- |
| Native WebView | 18 | 1 | Stale / live | 0 |
| Browser-dev | 14 | 1 | Stale / live | 0 |

The frontend was at `a145a9b2`. Native used the Windows dev executable
with SHA-256 `0be5a759a05f8f4475d7fb27bb6d4a92089bf57492f5dde82e47c0749a34b8dc`.
Browser-dev used the directly linked gateway with SHA-256
`2272c6b0035edb1470da3155b733c9b3a6bbccd14d996660a41b353fdba66242`.
Both runs generated exporter SHA-256
`61292175eb7a053873003382b5adb03cc01a8c87b924663f9d99cd79f0cf3cc5`.
The raw local records are `.research/dcs-physics-native/1794081586609/evidence.json`
and `.research/dcs-physics-native/1794081486568/evidence.json`, with exact
temporary profile IDs and captured sample times.

All 12 focused Windows native tests passed from the Cargo package directory.
The 31-test stream coverage gate passed at 98.28% lines, 93.06% branches
and 92.86% functions. This change modifies the fixture only.

## Corrections during qualification

The initial assertion treated the passive DCS-BIOS epoch as an ownship
identifier. The test now compares the ownship input's observed timestamp and
checks its live state, aircraft and applied owner directly.

Earlier runs exhausted local disk space copying the DCS-BIOS documentation
assets and initializing browser-dev. The fixture skips the unused
`doc/doc_assets` tree, keeps the runtime files and complete JSON catalogue,
and removes its private catalogue copy after restoring the original settings.

A focused native run from the repository root failed to load its relative
fixture. It passed from the package directory, though LLVM could not write
that run's extra profile file due to disk space.

The broader cross-adapter gate remains open in #192. Actual simulator and rig
acceptance remains in [#199](https://github.com/opensimgear/cockpit/issues/199).

## Observation-driven read timing

The fixture now times 24 distinct ownship wind values after BIOS has expired.
The existing fast reader drives the same filtered telemetry client and
controller as the inspector. Each value is matched to the first authored UDP
packet carrying it, and the receipt timestamp is taken when the controller
exposes the matching known value. The slow reader holds its original receipt
throughout. A notification can cover several captures, so the test measures
the matching read and never assumes that the notification identifies a packet.

| Transport | Authored packet rate | UDP to readable value p50 / p95 / p99 | Inspection request p95 / p99 | Client projection p95 | Clock bracket width |
| --- | ---: | ---: | ---: | ---: | ---: |
| Native WebView | 21.45 Hz | 22.90 / 28.74 / 183.29 ms | 20.90 / 175.70 ms | 0.20 ms | 0.22 ms |
| Browser-dev | 21.33 Hz | 23.31 / 40.88 / 186.93 ms | 22.80 / 181.10 ms | 0.20 ms | 0.37 ms |

The UDP-to-value figures use the conservative end of the clock bracket. With
24 samples, p99 is the maximum. Both runs met the 100 ms read p95 budget and
matched every marker. Most of the longest sample on each transport was spent
inside one inspection request. The request trace includes gateway transport
and native work. It does not identify which native step caused the delay.
The fixture stops at controller state and does not measure DOM paint, physical
input, or simulator export time.

Both dev runs completed profile and installation cleanup with no errors. The
raw local records are `.research/dcs-physics-native/1794130619178/evidence.json`
and `.research/dcs-physics-native/1794130674675/evidence.json`.
The native and browser-dev binaries were built from the current Rust sources
with the previous driver fixture's private-directory override disabled.
Their SHA-256 hashes were
`7E53AA4BFEE5B09A4D643C7FC47E72C2F2C01237AE17250B35F5D2822D2D9B8E`
and `C70F6C55B0EA7C6EE8B6C61D56A0546956CD864525451E3AC97A22B96A87F624`.
All 14 telemetry and 34 stream frontend tests passed. Telemetry coverage was
100.00% lines, 89.26% branches and 92.86% functions. Stream coverage was
98.75% lines, 93.68% branches and 94.59% functions.

## Reuse the watched DCS catalogue

The inspection request could rebuild the full DCS-BIOS provider catalogue
after two seconds, even if the files had stayed unchanged. Successful reads
now reuse the snapshot while the native watcher covers the exact configured
catalogue directory. A callback advances the watched revision before the
context worker runs. A read crossed by that callback cannot reuse or publish
the old snapshot. Worker rearm advances the catalogue generation and clears
the affected entries. Failed captures and unavailable watches keep the
two-second request-time limit. Alias resolution and strict native source
checks retain their existing paths.

The full instrumented Windows suite passed 2,462 tests with 18 default
ignores. The two checksum-pinned DCS-BIOS archive checks passed separately.
The four changed native modules measured 93.60% to 97.69% production line
coverage. LLVM measured 78.43% to 96.67% function coverage and emitted no
branch counters for these modules.

The two dev clients each read a new CommonData altitude descriptor after an
edit to the private JSON file and read the original descriptor after the
fixture restored it. The first changed descriptor was readable in 152 ms
on native and 320 ms on browser-dev, including the fixture's observation
checks. Both restored their private profile, installation and provider
settings without cleanup errors.

| Transport | Authored packet rate | UDP to readable value p50 / p95 / p99 | Inspection request p95 / p99 |
| --- | ---: | ---: | ---: |
| Native WebView | 21.52 Hz | 21.91 / 31.62 / 68.29 ms | 16.50 / 22.50 ms |
| Browser-dev | 21.40 Hz | 23.70 / 46.74 / 123.33 ms | 22.30 / 22.40 ms |

Both clients matched all 24 values while the other receipt remained held.
The inspection request maximum fell from 175.70 to 22.50 ms on native
and from 181.10 to 22.40 ms on browser-dev. Both met the 100 ms read p95
budget. The longest remaining browser-dev sample spent about 107 ms before
the matching request began. The trace does not separate source processing
from notification delivery during that interval. These results cover
authored loopback packets and client state.

Raw evidence is in `.research/dcs-physics-native/1794131624844/evidence.json`
and `.research/dcs-physics-native/1794131681429/evidence.json`.
The native and browser-dev binary hashes are
`AB2570F8A65839D47D2CD51E009F7D557504ADB1C7A614858E773BD9036822BB`
and `55355AB70D3B102049289C338432E151475E0AF5B5F6D0717C119EE9A0FA520C`.

## Native publication spacing with stale BIOS

The follow-up uses 48 distinct ownship markers and records the two most recent
fast notifications at the start of each matching read. The notification
records include their native publication timestamps and the client's receipt
timestamps. Differences between consecutive revisions on the same clock can
be compared without assigning a notification to a particular UDP packet.

On the previous browser-dev binary, consecutive native publications were
130.41 and 132.79 ms apart in two slow samples. Their client arrivals were
130.60 and 132.80 ms apart. The client began inspection within 0.10 ms of
the last notification in every sample. These gaps existed before delivery
to the browser. The trace does not isolate the native operation between
publications.

While BIOS is stale, the passive DCS reader now reuses the channel IDs
prepared for its aircraft catalogue. It prepares a new set when the aircraft
module changes. Previously every ownship update rebuilt and sorted the full
JSON channel descriptors to recover the same IDs. The resulting stale BIOS
channels and independently fresh ownship values keep the same semantics.

| Fixture | Authored packet rate | UDP to readable value p50 / p95 / p99 | Inspection request max |
| --- | ---: | ---: | ---: |
| Previous browser-dev | 21.54 Hz | 22.92 / 63.09 / 108.70 ms | 22.70 ms |
| Current native WebView | 21.44 Hz | 22.45 / 43.50 / 121.89 ms | 23.90 ms |
| Current browser-dev | 21.44 Hz | 18.32 / 43.79 / 72.05 ms | 21.90 ms |

Both current clients received all 48 values with one receipt held, showed the
edited and restored catalogue descriptor, and completed cleanup. The largest
browser-dev sample still followed a 147.67 ms interval between consecutive
native publications and a 147.80 ms interval between the corresponding
arrivals. Native also recorded a 143.75 ms publication interval. The
repeated browser gaps on the previous build occurred roughly 2.2 seconds
apart. The passive catalogue guard still performs a full content check
inline when its two-second interval elapses. The trace establishes the
publication gap and does not time that check separately. A future change
must preserve exact selected-file validation and avoid rebinding the
reader for unrelated aircraft catalogue edits.

Raw records are `.research/dcs-physics-native/1794132061341/evidence.json`,
`.research/dcs-physics-native/1794132316519/evidence.json` and
`.research/dcs-physics-native/1794132371556/evidence.json`.
The current native and browser-dev binaries had SHA-256 hashes
`D6E8CC38E179409157E90235108A8D73B8EE5288FF2FD1374A538E1C101D13A7`
and `1F48C2E525B47490B49A11E04DE289F850C9266DC64B6FDF13A86F35B677038A`.

The serial Windows suite passed 2,462 tests with 18 default ignores. Both
pinned archive checks passed separately. Production line coverage for the
changed vehicle source module was 88.89% and LLVM function coverage was
93.02%. The initial parallel run hit an X-Plane lease count assertion and
the second parallel run hit a process freshness assertion. Both checks
passed in isolation and in the serial suite.

## Exact file notifications for the passive reader

The passive reader now arms a native watch for the exact paths used by its
prepared catalogue. It confirms the semantic baseline under that watch
before accepting the reader. A selected-file event or lost notification
wakes the reader and fences publication. Reopening requires a new baseline.
A healthy watch replaces the recurring full catalogue load on the capture
path. The short metadata check remains, and failed watch registration
retains the previous bounded validation. The command lane keeps its strict
file and content checks.

The focused tests exercised a real sibling write and selected-file write.
The sibling left the exact wake quiet. The selected file woke the reader,
and a watch bound after invalidation still received a wake. A separate
test verified that registration cannot certify a previously captured
catalogue after a file has changed.

The serial Windows suite passed 2,464 tests with 18 default ignores.
Both checksum-pinned archive checks passed separately. All six changed
native modules passed their production line gates, with 88.86% to 100%
coverage. LLVM did not emit branch counters for these modules.

The running dev clients kept A-10C telemetry live during an unselected
F-16C catalogue edit and restore, even as the broad provider generation
advanced. After a 140 ms quiet UDP window with a still-live capture, a
selected-file edit woke the reader and withdrew its aircraft in 14.28 ms
on native and 11.35 ms on browser-dev. Restoring the file with UDP still
paused left the old value withdrawn through the reader retry interval.
Both clients then recovered from fresh packets. The first native fixture
incorrectly expected the aircraft-specific channel to stay listed after
withdrawal. The corrected assertion accepts a removed channel or an
unavailable channel with no value.

| Transport | Authored packet rate | UDP to readable value p50 / p95 / p99 | Largest sampled consecutive publication interval |
| --- | ---: | ---: | ---: |
| Native WebView | 21.48 Hz | 17.34 / 40.36 / 47.56 ms | 73.34 ms |
| Browser-dev | 21.44 Hz | 18.37 / 45.67 / 64.50 ms | 95.45 ms |

Both current runs matched all 48 markers with one receipt held and reported
no cleanup errors. The preceding fixtures measured 121.89 ms and 72.05 ms
for the largest UDP-to-readable samples and sampled publication intervals
of 143.75 ms and 147.67 ms respectively. The intervals compare consecutive
revisions on the same clock. They cover the authored workload and do not
qualify simulator, physical controller or display timing.

The local records are `.research/dcs-physics-native/1794133924404/evidence.json`
and `.research/dcs-physics-native/1794133981077/evidence.json`.
The native and browser-dev binary hashes were
`14DB1C1E8FE9065A42558F00F04936E0648E473A6D63CF83F5862B5DE1389F59`
and `F36023CC415E49F7B7A9FF3DBFE5ECFBD3A1013C5AA7E375D24732A246EFDAAA`.
