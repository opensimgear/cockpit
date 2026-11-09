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
This run proves the replacement can reattach under the same owner.

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

### Preserve the last committed inventory reader

A second diagnostic run reproduced the stalled profile. The first returning
reader reached the profile while its native module metadata was unavailable.
The inventory rejected the claim before committing its reader ID. The host
then used that uncommitted ID as the expected predecessor, and 67 subsequent
claims were rejected during the recovery window.

The publication lane now seeds its predecessor during activation, records
successful initial and returning reader claims, and uses the last committed
reader for the next claim. It records a successful claim before refreshing
delivery, so cancelling a pending plan cannot discard the inventory receipt.
The prepared reader and the inventory claim each retain their exact ownership
check. The focused regressions cover the missing-metadata error, cancellation
before and after commit, and an independent reader intervening between attempts.

Three runs of the rebuilt Windows dev Cockpit recovered BIOS and ownship
on the same profile owner. Each run retired the old helper and bound the
replacement process, kept the service interval off for 1,100 ms without
gateway reads, and completed cleanup with no errors or timing-budget misses.
The final build passed the same checks after removing a redundant claim
record from the activation phase.
The runs used neutral desktop controls. Held-key exit and actual simulator
acceptance remain separate.

The final Windows library suite passed 2,501 tests with 18 default ignores.
Both pinned DCS archive checks passed when run separately. The three changed
production modules reached 77.25–95.00% line coverage and 74.68–93.94%
function coverage. LLVM emitted no branch counters for these files.

| Local artifact | SHA-256 |
| --- | --- |
| `.research/c193-reconnect/dcs-native-stage-timing/native-50092-native.err` | `0b4ee317febf7acce62130075f96c0d7ebe9e338787f442d172b75e89801c89e` |
| `dcs-committed-reader/committed-reader.exe` | `3d590d3fc104b691d6ca71559238005e1bee595dd0d1ad533373789355ba608e` |
| `.research/dcs-physics-native/1794240418915/evidence.json` | `b3d574b249ddb402d34bd073fc925a860f8b0609b30eec2702b044fe43113d28` |
| `.research/dcs-physics-native/1794240506611/evidence.json` | `633d929701c8b4eaebbeb91d0327c195d846ef757f9960ec1a0dbd1205c6c89f` |
| `.research/dcs-physics-native/1794240564250/evidence.json` | `5dcdadec0c76458d41ed37f81a60fdbb7abe6b7b7c80aa17a13a8fa34c9a57ee` |
| `dcs-committed-reader/committed-reader-final.exe` | `8241baca6621b207652fbf2d464bfd2a6d22b8c4fede54fa133d8446b3f826bf` |
| `.research/dcs-physics-native/1794240968311/evidence.json` | `4e28050ddc2b6d297e906c791267d00cbefbf7144abf6ab899e9eceeeb7f656c` |

### Process replacement during browser panel and vJoy load

The Windows dev Cockpit held one profile owner while two authenticated
browser panels rendered the authored DCS ownship lamp and two debug sources
drove vJoy at 1,000 and 125 Hz. The two lanes overlapped for 18 seconds.
Both panels withdrew the lit value and painted the unlit state after the
fixture process exited. They then received a fresh unlit baseline from the
replacement, with no replay of the old lit value. A deliberate on/off pair
changed both canvases while the two input lanes were still running.

The first fixture attempt expected the fresh unlit baseline to repaint a
canvas that was already unlit. The revised check confirms the fresh
feedback and current pixel separately, then requires new canvas writes for
the deliberate color changes.

| Run | Native lane | Measured offer rate | Exact offered edges | Offer to Raw Input p50 / p95 / p99 | Queue peak |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 | Fast | 999.999 Hz | 18,001 | 2.7 / 6.4 / 11.9 ms | 17 |
| 1 | Peer | 125.000 Hz | 2,251 | 2.4 / 6.3 / 10.1 ms | 11 |
| 2 | Fast | 1,000.000 Hz | 18,001 | 2.8 / 6.6 / 11.8 ms | 17 |
| 2 | Peer | 125.000 Hz | 2,251 | 2.5 / 6.3 / 9.4 ms | 11 |

Capture-to-reducer p95 was 1.3–1.4 ms. In each run the receiver matched all
20,252 offered edges and both Stop releases. The releases arrived 2.566–6.288 ms
after the Stop marker, the slot returned to free, and cleanup completed.
Panel presentation was observed at the canvas write boundary. Physical
controller, display scan-out and actual simulator effects stay in #199.

The private fixture and logs are in
`.research/c193-reconnect/dcs-native-stage-timing/dcs-loaded-replacement/`.

| Local evidence | SHA-256 |
| --- | --- |
| `.research/dcs-physics-native/1794242001934/evidence.json` | `12da9e91ce435763841691a597cacc9891c6f8b4f7603b225773a00ff807c8fe` |
| `.research/dcs-physics-native/1794242109596/evidence.json` | `fa3b5fb0a14d3a55386ebb11cc8233274e2a68191c07954b3148dfa2b4b2788b` |
