# Stream burst and fault scenarios

The Windows dev executable from `d56551c3` had SHA-256
`0be5a759a05f8f4475d7fb27bb6d4a92089bf57492f5dde82e47c0749a34b8dc`.
Each live run used a disposable Cockpit database. The workload uses authored
debug, telemetry and encoder traffic. The controller preview test used a
passively connected native HID controller with ten controls.

| Scenario | Current evidence |
| --- | --- |
| Delayed controller callbacks | `controller-delayed-callback-native.mjs` held an actual Tauri channel frame, unsubscribed the old preview and reopened the same topic. Releasing the old frame did not publish or acknowledge it. A third subscription stayed ready with its baseline while the native broker rejected the second subscription's delayed receipt. All three subscriptions retired. |
| Dual-rate authored sources and encoder | `companion-encoder-vjoy-native.mjs` measured 1,000.98 and 125.14 Hz native source offers and 124.78 encoder detents/s. Windows Raw Input received all 193 fast edges and 48 encoder edges in order, plus the Stop release. The pending peak was three frames, with an independent MCP reader and one unread reader. |
| DCS pause, context and stalled telemetry | `dcs-physics-native.mjs` ran at 1,000.53 and 125.08 Hz of authored offers with two panels. Windows received all 218 ordered edges and both Stop releases. An independent telemetry reader advanced 36 times during one held delivery. Pause, export denial, frozen time, respawn, competing process, BIOS expiry and process exit produced the expected separate quality and withdrawal changes. |
| Companion transport and publication replacement | `profile-companion-compiled-native.mjs` cut the real TLS connection while the panel held a mapped button and a feedback receipt. The affected vJoy contribution released within 2.03 ms of the cut, the independent button remained held, and the new session rearmed from a neutral baseline. It ignored the old acknowledgement. Publication replacement and restoration each required a fresh neutral baseline. |
| Authored controller removal and overflow | The current Windows native test `paced_hid_sources_route_together_with_a_stalled_preview_and_recover_after_loss` preserved ordered reports at both rates, exercised journal overflow, withdrew the removed source while its peer stayed held and fenced the replacement generation. |

The encoder fixture initially tried to read a live port from retained
instance metadata. It now verifies the selected publication contract and
waits for the matching current port on the paired resource before applying
the profile. The rerun passed with the original rate and edge assertions.

The raw records are under `.research/reactive/controller-delayed-callback/`
and `.research/dcs-physics-native/1794078799490/evidence.json`. Earlier
running-Cockpit load and latency measurements are in the
[DCS panel and vJoy report](2026-11-07-dcs-panel-vjoy.md).

All 1,705 frontend and timer-guard tests passed. The stream transport
coverage was 98.28% lines, 93.06% branches and 92.86% functions. Controller
observations reached 98.68%, 96.74% and 97.37%. Browser feedback reached
100% on all three measures. The production executable was unchanged.

Real controller unplug and replug, held-input rearming, physical input
latency, and simulator effects remain operator acceptance in
[#199](https://github.com/opensimgear/cockpit/issues/199). The broader
cross-platform matrix, full timing budget and CI gates in #198 remain open.

## Browser-dev gateway restart with held vJoy

Two Windows dev runs started from a private Cockpit database and a free
vJoy slot. Each held an authored button for more than one feeder lease.
The fixture terminated only the exact native gateway child of its private
Vite process. The Raw Input receiver recorded neutral output within
10.24 ms and 9.19 ms of the respective kill requests. The QPC brackets
for process termination were 12.72 and 12.01 ms wide. These are upper
bounds measured from before the kill request to the neutral report.

The same page observed loss of its runtime subscription. A new gateway
process reported recovery needed with no active owner. Explicit recovery
freed the slot, and the replacement observation began with an ownerless
snapshot in a new observation session. The gateway rejected a press from
the old owner. A deliberate reapply acquired a fresh owner and a neutral
baseline. The receiver's exact sequence was the original press, the
release on process loss, a fresh press, and its release. Stop freed the
slot again. The final fixture verified a single native vJoy identity.

The final tracked fixture reached 100% lines, 88.71% branches and
95.83% functions. It is opt-in on Windows. Run
`node --test tests/ui/gateway-held-restart.mjs` against a
private browser-dev instance with `OSG_GATEWAY_RESTART_PRIVATE=1`.
Supply its loopback URL, exact Vite PID, native service executable and
Raw Input probe through `OSG_GATEWAY_RESTART_URL`,
`OSG_GATEWAY_RESTART_ROOT_PID`, `OSG_GATEWAY_RESTART_EXE` and
`OSG_GATEWAY_RESTART_PROBE`. The fixture checks the gateway process
identity before the kill. Give the instance its own `OSG_SQLITE_DATABASE`.

The native gateway executable SHA-256 is
`a7164c3d58fe28a90c5707d82c9b2f4fd82247141f32565c8241534340ce96e8`.
The Raw Input probe SHA-256 is
`6ea93864c16d2897aedc150df5addf45fd65a3dcd89a0f40773a779c88f35e9e`.
The logs are under `.research/c193-reconnect/dcs-native-stage-timing/`.
This qualifies authored browser-dev gateway restart. Physical input
replacement and actual simulator effects stay in #199.

| Local artifact | SHA-256 |
| --- | --- |
| `browser-gateway-held-restart-1.log` | `2712ff771a87ccb5dc9f6c29c89485b0f0c41799d0c3b0c0e6faf2f809b22fc6` |
| `browser-gateway-held-restart-coverage.log` | `1f025e4dc18efd07b02ef1f2657f015667444e3ea84d26b6947cb17b01b98fc4` |

## Native Cockpit parent restart with held vJoy

The Windows native mode launched the disposable Cockpit parent with a
private SQLite database and WebView directory. It verified the database
through the running app before applying the authored vJoy profile. The
fixture held one button across multiple feeder leases and terminated
only the exact Cockpit parent it had started. A separate Raw Input
receiver stayed registered through the crash and recovery.

| Trial | Kill request to neutral report, upper bound | Process termination bracket |
| --- | ---: | ---: |
| Native 1 | 10.56 ms | 14.88 ms |
| Native 2 | 11.13 ms | 16.15 ms |

The old DevTools target disappeared. A new process and WebView reopened
the same database with recovery needed and no active owner. Explicit
recovery freed the slot. The runtime observation started with an ownerless
snapshot in a fresh session. The new process rejected the old input owner.
After reapply, the new owner began neutral and accepted a deliberate press
and release. The receiver matched the old press, crash release, fresh
press and fresh release on one vJoy device. Final Stop freed the slot.

The native fixture met the coverage gates with 85.87% lines, 70.11%
branches and 91.23% functions. The browser-dev branch also passed after
the fixture was shared, with a 10.93 ms release upper bound. Its first
launch stopped before the test because a scratch directory name had been
reused. The rerun used a unique directory.

For native mode, set `OSG_GATEWAY_RESTART_NATIVE=1` and
`OSG_GATEWAY_RESTART_PRIVATE_ROOT` to the disposable directory that
contains the copied app and the private database and WebView paths.
Supply a private loopback `OSG_NATIVE_CDP_URL` and the usual dev frontend
asset server. The fixture owns the native process launch and restart.
The native executable SHA-256 is
`0a722a56cb4b1726d516245bbb7f925114c2c37e455fd7c6fcb4df0f8a3d1c47`.
These trials cover loss of the native parent with its feeder left to
release the output. Physical input and mixed desktop timing remain open.

| Local artifact | SHA-256 |
| --- | --- |
| `native-gateway-held-restart-1.log` | `f7086412b6ce12420d3448898473ca1ed50b7b3f56f9faf873c62e341f934e30` |
| `native-gateway-held-restart-final.log` | `aaf5cad84dd57b263dfe669a1945669f4127c2125971b2580bf08399ea8ff38b` |
| `browser-gateway-held-restart-final-retry.log` | `5f371b98bd81900a7e9f2dbee4cdfe8aa07066bae42e1a3b014edffe0b69c15c` |
