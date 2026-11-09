# Launched game profile notifications, 9 November 2026

A hardware-first profile can acquire a game context after it starts. Once the exact launched child belongs to the active hardware owner and the provider has installed a watched reader, the profile host can service it on notifications. The 5 ms interval remains available while the profile is waiting for a watched context, including after the child exits. A mismatched child, missing context, or loss of the provider watch keeps the fallback eligible.

## Running Windows dev Cockpit

The isolated native Cockpit invoked `profile_game_launch` against the private DCS fixture and received authored DCS-BIOS and ownship telemetry. It kept hardware owner `a315d621-5a1e-4288-8d1b-1b2e79e20906`, generation 3, with profile `52dc1874-e6eb-46ed-af6c-1c06965b038c`, revision 2, and a null root game ID. The selected-game desktop helper tracked the exact fixture process, PID 40448, creation token `134387026226149615`.

`serviceIntervalMs` was `null` on admission and after 1,100 ms without gateway reads. The helper watch stayed healthy and the held-output list stayed empty. After the fixture process exited, the profile retained its hardware owner, withdrew the game context and telemetry, retired the helper and returned to the 5 ms waiting interval. The fixture reported no cleanup errors. The same run exercised setup and repair, independent inspection, competing process withdrawal, pause and expiry, and fresh baselines after respawn.

This run used authored telemetry and a neutral desktop mapping. Physical controls, desktop injection timing, and simulator effects remain separate checks in [#193](https://github.com/opensimgear/cockpit/issues/193), [#198](https://github.com/opensimgear/cockpit/issues/198), and [#199](https://github.com/opensimgear/cockpit/issues/199).

## Local evidence

The locked Windows dev build used the three changed Rust files with the native sources from `f8428056` and frontend from `88ee864e`. The parallel library run passed 2,490 tests with 18 existing ignores. Eight X-Plane bridge tests hit Windows `StorageFull` while copying their fixture executable. The same binary passed 2,497 tests with 18 ignores in the serial run, including all eight bridge tests. The only serial failure was an Assetto fingerprint test that hit `StorageFull` while allocating its 512 MiB fixture. That test passed in the parallel run. All 2,498 non-ignored tests passed across the two runs.

The focused profile-session and automatic-launch runs passed 499 tests with two existing ignores. The scheduling regression covers stale owner generation, mismatched game, missing installed context, and watch loss. Production line coverage reached 81.33% for `host_scheduling.rs` and 94.52% for `profile_session_launch_host.rs`. Function coverage was 72.73% and 73.33% respectively. LLVM emitted no branch counters for these files.

| Artifact | SHA-256 |
| --- | --- |
| `launch-child-wake.exe` | `4933f3a9c9bea2da4106484d24248e39f644095676473091109b7076a7ad4050` |
| `dcs-physics-native/1794229000138/evidence.json` | `e8090d0623946d2d164559ac6ec0113ba6a482c9894ae5d92530f55e0b8e4b6c` |
| `launch-child-live.log` | `08843cd4be309e92d6845aa806f1835ed3ecff6f72928216143431e9aeaf34fd` |
| Private `dcs-physics-native.mjs` fixture | `4b9aedd1ed9500b7b618d05150b5e0249ca14596d3c054e0615b8a0a1adc4ca4` |

## Selected-game process replacement

The Windows neutral fixture exercised process exit and replacement under
the same profile owner. One stock dev run withdrew both telemetry families,
retired the desktop helper, and resumed profile telemetry on the new PID.
In another stock run, the inspector reported fresh ownship and BIOS from a
verified replacement, while all six assigned profile inputs stayed stale
through the 8.5-second recovery check. The profile retained its owner and
reported that game routes were waiting for a fresh context. Both runs
cleaned up successfully.

A private diagnostic build recovered after two successive reader claims
were accepted. It bound a healthy helper to the replacement PID and kept
the profile service interval off through 1,100 ms without gateway reads.
This run proves the replacement can reattach under the same owner. The
reason the earlier stock run stayed stale is still under investigation.

The held-key variant stopped before sending input when PickerHost took
foreground. The exact receiver recorded zero marked keyboard events and
cleanup completed. Release with a held key and no replay after replacement
remain open in #198.

| Local artifact | SHA-256 |
| --- | --- |
| `.research/dcs-physics-native/1794239592182/evidence.json` | `c81d06806ef970984645a433f50f420d7e016579c619c1ef22ba824940f63ba3` |
| `.research/dcs-physics-native/1794239709617/evidence.json` | `0fb33f92085c4166c8175f63f4bb5180f0a6a41edd1df7eedfa0e63c1b1a6a8b` |
| `.research/dcs-physics-native/1794239959823/evidence.json` | `ba0e8d5922ca94501b6a920c18cea41b4ed3df7fd33911aac80cbc68f40ec2bc` |
| `.research/c193-reconnect/dcs-native-stage-timing/native-45904-native.err` | `e54ad8f581b9a6fb766fe7a8ecb491881b18e655d87e2e87291ffe52e0d6d4a1` |
