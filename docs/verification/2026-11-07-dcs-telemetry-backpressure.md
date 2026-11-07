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
