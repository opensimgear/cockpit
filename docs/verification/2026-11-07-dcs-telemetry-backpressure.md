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
