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
