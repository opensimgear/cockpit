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
