# Active telemetry proof continuity

Active game routing keeps separate proofs for process identity and the DCS
catalogue. A successful worker result can arrive after the old proof has
expired, before the routing worker has observed expiry. A prior successful
probe followed by a delayed successful probe could therefore hide the gap.
The previous check only caught gaps after a probe explicitly returned no
evidence.

The validator now compares the original capture time of every successful
replacement against the previous proof's expiry. A capture after expiry
withdraws the affected game branches once. The exact reader remains open
and can establish a fresh baseline from the replacement proof. A capture
at the expiry boundary is contiguous. The 100 ms process and 250 ms
catalogue horizons, capture timestamps and command validation are unchanged.

The deterministic regression drives both proof types. It first replaces
a proof at its expiry boundary, then lets a pending successful probe supply
evidence one nanosecond after expiry. The old policy failed the withdrawal
assertion in an isolated comparison. The new policy passes it and all 17
focused native validation tests, including the existing fresh-baseline and
reader-retirement checks.

## Local suite and coverage

Both checksum-pinned official DCS-BIOS archive checks passed. The changed
validator reached 96 of 97 production lines (98.97%) and 95.65% LLVM function
coverage. LLVM did not emit branch counters for this module. The parallel
Windows run had one companion reconnect test failure when its fixture tried
to read a panel assignment. The same test passed in isolation and in the
serial Windows rerun. The serial run passed 2,470 tests, with 18 default
ignores and zero failures.

The suite and coverage records are under `.research/reactive/active-proof-gap/`.

## Running Cockpit

The rebuilt Windows dev Cockpit and Edge browser-dev each passed the DCS
fixture with authored packets and a stand-in simulator process. With one
subscriber stalled, BIOS expiry withdrew only the BIOS values. Ownship
remained current. Pause, export denial, frozen ownship, respawn and process
exit also exercised withdrawal and recovery. Both fixtures restored their
private installation and profile state without cleanup errors. This exercises
the runtime on both transports. The deterministic regression controls the
exact timing of the late proof separately.

Native executable SHA-256
`bf10476d53bebdbec5613adb35eb2726c64dae6a4e4b3e0e07da4687d46dd76e`.
Browser-dev gateway SHA-256
`49aa914f0138c4447712d330e882e390deabb39ad253ffcfd61b9b8a1d5f275f`.
Raw fixture records are under `.research/dcs-physics-native/1794159535733/`
and `.research/dcs-physics-native/1794159610102/`.

The local Windows build linked the existing isolated `rlib` candidate with
debug info disabled for Cockpit. The committed manifest retains all crate
types. The source comparison checked 1,883 tracked source and manifest files
against main plus the two intended edits, allowing those local link settings.

The wider #192 criteria remain open for the other telemetry providers.
The process and catalogue validators still use their scheduled checks.
Actual simulator and physical timing qualification remain in #199.
