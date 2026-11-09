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

An early held-key attempt stopped before sending input when PickerHost took
foreground. The exact receiver recorded zero marked keyboard events and
cleanup completed.

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

### Held keyboard output on selected-game exit

Windows dev Cockpit dispatched an authored key hold to the exact private
DCS receiver. Its marked key-down and the Windows asynchronous key state
confirmed the key was held when the fixture was terminated. After exit,
Windows reported the key up, the desktop helper retired, and the output
backend reported complete cleanup with empty held and possibly held lists.

The replacement process recovered both telemetry families on the same
profile owner. The source stayed pressed while the replacement waited
for neutral. It received no marked keys during the 350 ms replay check.
A fresh neutral transition armed it, then a deliberate press and release
produced exactly one marked key-down and key-up. The isolated fixture
finished with no cleanup errors or timing-budget misses.

After PickerHost was dismissed, the dev Cockpit window still owned the
foreground. The private receiver build allowed a focus handoff only from
the known isolated Cockpit PID and executable. The first attempt with the
handoff stopped at neutral arming because debug input publishes value
changes. The passing run verified that an initial pre-neutral press
produced no key before the falling edge armed output. The desktop fixture
reached 97.91% line and 87.04% branch coverage. The native application was
the same `committed-reader-final.exe` build used above.

| Local evidence | SHA-256 |
| --- | --- |
| `.research/dcs-physics-native/1794243239025/evidence.json` | `2b7043e37963a4f8f3366baf8f27108e44ae8431c72eaf1fe8077287f560b829` |
| `dcs-held-exit-9628/retry-fresh-neutral.log` | `fcab54c4f419fb3d1e892d84aeced3584910536e4948e769d33cffcef1033374` |
| `dcs-held-exit-9628/native_receiver.exe` | `aeb6ddfef463c171e9d7836e25fa82690c6719b629e36bef7308e938ab5dd675` |

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

### Browser gateway replacement with two input lanes

Browser-dev sent two authored streams through the ordinary HTTP gateway
while two authenticated browser panels rendered the DCS lamp. The 125 Hz
and 50 Hz lanes overlapped for 14.009 seconds. Both panels withdrew the old
lit state on exit, received an unlit baseline from the replacement without
replaying the old value, and painted a deliberate on/off pair under load.
The profile retained the same owner.

| HTTP target | Measured offer rate | Exact offered edges | Offer to Raw Input p50 / p95 / p99 | Peak pending frames |
| --- | ---: | ---: | ---: | ---: |
| 125 Hz | 124.915 Hz | 1,751 | 1.97 / 4.87 / 6.73 ms | 2 |
| 50 Hz | 49.967 Hz | 701 | 2.11 / 5.06 / 7.21 ms | 2 |

Raw Input matched all 2,452 offered edges and both Stop releases. The
releases arrived 3.752 ms after the Stop marker, the slot returned to free,
and cleanup completed. The stop-controlled recorder replaced the short
warm-up recorder so both captures stayed active through Stop.

The replacement workload met its configured timing checks. The encompassing
test still failed because its separate short warm-up offered 590.67 Hz
against a 1,000 Hz HTTP target. These rates describe authored HTTP input
through Windows Raw Input. Physical preview and controller latency remain
in #199.

The warm-up awaited each HTTP acknowledgement before offering the next
value on that stream. Its fast lane measured 1.09 / 3.45 / 10.22 ms
HTTP round-trip p50 / p95 / p99 and 1.81 / 4.08 / 5.61 ms offer-to-Raw
Input p50 / p95 / p99. The offered and received sequence matched, including
the release on Stop, and peak pending input was two frames. The serial
request cadence is therefore a separate limit from Cockpit's output
timing. The browser gateway dispatches independent requests on separate
threads, so overlapping same-stream requests would require ordered
admission to preserve sequence validation.

| Local evidence | SHA-256 |
| --- | --- |
| `dcs-browser-replacement/osg-gateway-service.exe` | `822f93b0aa37a2f84402dc45ae6c6d982652e09e1480533273dcd5cd1de227b7` |
| `.research/dcs-physics-native/1794243040777/evidence.json` | `93e949d6433661a10bb408f7508cf5e2fb7581ce87e465194531597f4e09314d` |
| `1794243040777/loaded-replacement/browser-rate-raw.json` | `7fd7934a35714087140008c697e164c047417280a2faae6120d903b23972f335` |
| `1794243040777/loaded-replacement/browser-rate-offers.json` | `a3f3c77e0dd7d5d5a2abc8ef036559fe449c911ba318579741277529e6d09e35` |

### Browser gateway timing under light and DCS load

The same native gateway binary sustained 1,001.25 Hz and 125.01 Hz in
the browser-dev vJoy fixture without DCS or active panels. Windows Raw
Input matched all 218 offered edges and both Stop releases. The fast lane
had 0.68 ms median HTTP round trip and 1.00 ms p95 to Raw Input.

With two DCS panels active, a private Vite bridge trace measured the fast
lane at 597.02 Hz. The gateway and Raw Input received every offered edge.
For each request, the bridge measured request-body parsing, the interval
from before its write to native stdin until its native reply, and the
remainder of the client HTTP round trip outside the bridge callback.

| Fast-lane interval under DCS load | p50 / p95 / p99 |
| --- | ---: |
| Full HTTP round trip | 1.11 / 3.64 / 6.14 ms |
| Native pipe and service round trip | 0.76 / 3.18 / 4.35 ms |
| HTTP round trip outside the bridge callback | 0.29 / 0.46 / 2.39 ms |
| Vite request-body parsing | 0.044 / 0.056 / 0.065 ms |
| Offer to Windows Raw Input | 1.84 / 4.39 / 5.81 ms |

The native round trip includes pipe scheduling, gateway dispatch and
response writing. The bridge trace gives an upper bound for their combined
cost. Each lane had at most two pending frames and both buttons released
0.94 ms after the short warm-up Stop marker. The 125 Hz and 50 Hz
replacement workload again matched all 2,452 edges and both releases,
kept the profile owner and had no cleanup errors. The integrated 1 kHz
HTTP check remains open. All 25 local gateway bridge and observation
transport tests passed.

A second private trace joined Vite timing with native gateway timestamps.
The fast lane offered 475.66 Hz and kept every edge. Native request
handling measured 0.72 / 4.10 / 5.59 ms at p50 / p95 / p99, while the
request thread's enqueue-to-dispatch interval measured 0.09 / 0.11 /
0.13 ms. Native request handling includes gateway admission, session lock
waits and the debug dispatch. The trace isolates those together and leaves
their individual contributions unmeasured. It points to measuring within
the request before changing worker scheduling. The replacement workload
again passed at 125 Hz and 50 Hz with clean teardown.

| Local evidence | SHA-256 |
| --- | --- |
| `dcs-browser-replacement/current-light.json` | `7d81a464879c9d774e999f76636d1c1b51294971b5b27d2b0883a66aaec92881` |
| `.research/dcs-physics-native/1794244459214/evidence.json` | `96b0f2f770f93483fb79cf874a4ce95367a5cfc4aa2f056af52aa9f70f26e722` |
| `.research/dcs-physics-native/1794244640227/evidence.json` | `e1df5facd4a2661f4e86f50f34dea215cf41d9a74136de29a22d56a16c20edfe` |
| `dcs-browser-replacement/instrumented-gateway/osg-gateway-service.exe` | `344ca6e35f67289658c695445f911190d19eefe1fb7ad00c40f2e19972c0adf1` |

### Debug admission under the active session lock

A private gateway trace joined the same browser request, bridge and native
timestamps with the admission guards, the active profile session mutex
and the debug handler. Both sources preserved all 218 ordered offers and
both Stop releases. The fast lane offered 481.29 Hz, so the full test still
failed its 1 kHz cadence check. The 125 Hz and 50 Hz process replacement
scenario passed again with no cleanup errors.

| Fast-lane native stage | p50 / p95 / p99 |
| --- | ---: |
| Whole gateway request | 0.71 / 3.75 / 5.62 ms |
| Waiting for the active profile session mutex | 0.55 / 3.59 / 5.46 ms |
| Admitting the input under that mutex | 0.03 / 0.04 / 0.04 ms |
| Waking the reducer and publishing the debug observation | 0.03 / 0.03 / 0.04 ms |

The profile and game admission guards each took under 0.005 ms at p99.
The native profile worker takes the session mutex while it processes
telemetry, effects and output health. The debug request needs the same
mutex to verify the exact owner and append a frame. The trace measures
the wait, and does not identify the mutex owner for each individual wait.
An admission change must preserve the active owner, variant and Stop
fences, as well as the existing ordering and queue bounds.

| Local evidence | SHA-256 |
| --- | --- |
| `.research/dcs-physics-native/1794244913157/evidence.json` | `bd76c49ffc7749f636e6199d7a7b04d411cca39463b2679963ab7daad8e5078a` |
| `dcs-browser-replacement/request-trace/osg-gateway-service.exe` | `43d0f7f8f53e1d0b7a7b4affb20bbdd42a999628e8d90c5e6135bfa51a09bbc9` |

### Separate debug admission from the active session lock

Gateway debug requests append to the active owner's existing bounded queue
under a short mutex. The native profile worker takes the frames, releases
the mutex and processes them with the other sources. The gateway still uses
its normal profile and game admission guards. The queue is published only
after the new profile owner commits. Takeover, Stop, recovery and vehicle
pause retire the old queue and its stream state. The reducer still applies
authored freshness to the original monotonic capture timestamps.

Two stock candidate runs passed the loaded browser fixture with two live
DCS panels and one stalled telemetry reader. In each run Raw Input matched
all 218 warm-up offers and both Stop releases. The fast source went from
590.67–597.02 Hz in earlier stock runs to 975.63–1,000.70 Hz, within the
fixture's 5% tolerance.

| Run | Browser HTTP lane | Measured rate | HTTP round trip p50 / p95 / p99 | Offer to Raw Input p50 / p95 / p99 | Peak pending frames |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 | Fast | 1,000.70 Hz | 0.77 / 0.87 / 1.83 ms | 1.75 / 2.57 / 3.21 ms | 3 |
| 1 | Peer | 125.00 Hz | 0.81 / 1.23 / 1.28 ms | 1.87 / 2.52 / 2.61 ms | 2 |
| 2 | Fast | 975.63 Hz | 0.78 / 0.94 / 11.05 ms | 1.87 / 2.86 / 5.07 ms | 3 |
| 2 | Peer | 125.01 Hz | 0.82 / 1.11 / 1.57 ms | 1.91 / 2.71 / 2.93 ms | 3 |

Warm-up releases arrived 0.97–2.99 ms after Stop. The 125 Hz and 50 Hz
replacement workload also matched all 2,452 edges and both releases in
each run. Both panels recovered on the same profile owner, the slot
returned to free, and fixture cleanup passed.

The Windows dev WebView also passed with two panels, native qualification
sources at 1,000 Hz and 125 Hz, and a stalled MCP reader. Raw Input matched
5,402 authored transitions and both Stop releases. The fast lane measured
1.22 / 1.96 / 2.48 ms offer-to-receipt p50 / p95 / p99. Its wait for the
new ingress mutex was 0.0005 ms p95. Both lanes peaked at four pending
frames and cleanup passed.

The instrumented Windows library suite passed 2,503 tests with 18 existing
ignores. A targeted regression held the session mutex while both the
ordinary gateway and the native qualification batch admitted ordered input.
The worker then drained the queue, and Stop rejected the retired owner.
All 69 focused frontend tests passed. The new ingress and gateway debug
modules reached 89.86% and 81.20% line coverage, with 71.43% function
coverage in each. The changed gateway constructor lines were exercised.
LLVM emitted no branch counters for those files.

| Local evidence | SHA-256 |
| --- | --- |
| `debug-ingress-candidate/osg-gateway-service.exe` | `d2673413d32815e2d248196308581c8c2bd35366b700e68c600a29f6f8454f6a` |
| `.research/dcs-physics-native/1794245423664/evidence.json` | `bf455f9242e3c9b2570e13050efdd52e3b8cd3a0381a3e3339c676f2568e3093` |
| `.research/dcs-physics-native/1794245740173/evidence.json` | `26d36cdd6e3d2d7a40423675a039dd0df03ecc0924ab0d7913e9af84c5c096d0` |
| `debug-ingress-candidate/osg-cockpit.exe` | `db4b6a2204507162d4829f3a23c1d55c0477a1a24e0a474f79d6d805b4baa1d1` |
| `.research/dcs-physics-native/1794245990424/evidence.json` | `d7a1416cfea82b4f9e120a4fa551fe9788b4e3dbf926e5c35578a63630362159` |

## Telemetry and browser panel replacement on both Cockpit transports

The private DCS fixture used the Windows Cockpit and browser-dev gateway
binaries from `2f175ef0` above and the same Cockpit frontend from
`e82f44d9` for both runs. One profile routed the authored ownship
`inAir` value to two authenticated browser panels. The Cockpit telemetry
reader used the same shared observation client on native IPC and browser
HTTP. It read the selected value under its exact observation scope after
each committed invalidation. Panel feedback carried value deltas over WSS.

Both clients observed the same false, true, false, true value sequence.
The test retired the Cockpit reader and navigated one panel away while
the value was true. The old observation scope rejected reads. The second
panel received the change to false in its existing session. On reopen,
the Cockpit reader received a fresh snapshot under a new scope. The
reopened panel received a snapshot under a new session, then a fresh
false value without replaying the old true value. A new true change
reached both panels and the reopened Cockpit reader.

| Cockpit transport | Reader baseline, change, reopened baseline, next change | Raw Input edges including Stop | Stop release |
| --- | --- | ---: | ---: |
| Native IPC | 519, 521, 528, 533 | 194 + 26 | 37.79 ms |
| Browser-dev HTTP | 518, 520, 527, 533 | 194 + 26 | 1.14 ms |

Both runs matched the expected edges and finished with no cleanup errors.
The replacement panel first reported an unknown value in its snapshot.
The first fresh value was a false delta under that same new session.
An earlier browser attempt exhausted the short Raw Input capture before
Stop. The final run verified the capture continued for 7.40 seconds after
Stop and matched both releases. The lifecycle helper reached 100% lines,
88.46% branches and 95.24% functions on both transports. All 85 focused
frontend and companion tests passed.

| Local trace | SHA-256 |
| --- | --- |
| `.research/dcs-physics-native/1794250408930/evidence.json` | `77879018154836c102fda5d2d4944e00a30daf748f98ec5fbcfcf74f27fcc72b` |
| `.research/dcs-physics-native/1794250350770/evidence.json` | `7b6a1dce460c37cfdda65ef37e687fe6fbc41e8cc7d7f17deb861dc1267b6df2` |

The fixture runs panel replacement after the existing presentation and
dual-rate checks. Its Raw Input probe must capture through Stop. To run
the lifecycle variant, enable `OSG_DCS_TRANSPORT_LIFECYCLE=1` and set
`OSG_DCS_COMBINED_VJOY_PROBE` to the two-button probe. For native dev
Cockpit, enable `OSG_DCS_PHYSICS_NATIVE=1`, `OSG_DCS_COMBINED_MCP=1` and
`OSG_DCS_COMBINED_NATIVE_RATES=1`. For browser-dev, set
`OSG_DCS_PHYSICS_BROWSER_URL` to the private browser-dev server URL.
Run `node --experimental-test-coverage --test tests/ui/dcs-physics-native.mjs`
against the isolated dev session with the private DCS seed. The physical
panel, signed mobile and actual simulator checks remain separate.
