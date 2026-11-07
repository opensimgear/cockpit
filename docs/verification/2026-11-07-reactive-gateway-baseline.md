# Reactive gateway baseline, 2026-11-07

This is the current idle gateway evidence for #189. The representative active
workload and the proposed resource and latency budgets remain open.

## Native trace

The Windows dev Cockpit ran with a private SQLite database and WebView data
directory. The WebView served the working-tree Vite frontend, which contained
unrelated local changes. The native executable was built from `55a21c9a`.
Its SHA-256 was
`1bc4039fb1b88ec72d7df26dbafdfae0eb748cd3dedb45fdd1c51138b9d5241f`.
The repository HEAD at capture was `69d4043f`.

The measured window ran for 60,002 ms. Devices was visible and the profile
session reported stopped with no owner or fault at both endpoints. The
collector waited for the main WebView and a successful read-only status probe
before measuring, then validated a second probe after measurement. Both probes
were observed and excluded from app traffic. The capture counts POST requests
to the exact gateway command endpoint in that WebView.

| App command | Requests |
| --- | ---: |
| game_read_model_page | 181 |
| ui_preferences_get | 5 |
| ui_preferences_save | 5 |
| system_inventory | 2 |
| companion_inventory | 1 |
| module_usb_list | 1 |
| profile_resources_list | 1 |
| profile_status | 0 |
| Unparsed | 0 |

The 196 requests include the view's startup and resource loading after the
first status probe. The game pages arrived in bursts, including groups about
half a second apart. This shows read-model page traffic on Devices. The trace
does not identify the triggering producer or establish recurring behavior on
committed frontend sources. The November 2 baseline used Overview with a
faulted profile session and a different collector, so the command counts
describe separate workloads.

For a repeat, set `OSG_BASELINE_EXECUTABLE` to the executable being launched
and `OSG_BASELINE_OUTPUT` to a fresh evidence path before running
`node scripts/reactive/baseline.mjs`. This run's raw events and endpoint samples
are in `.research/reactive/r0-baseline-current/idle.json` locally. An initial
capture hit the WebView before Tauri installed its invoke API. The collector
now waits for that API and the app's main element before its first probe.

## Remaining R0 measurements

The [stationary VKB trace](2026-11-06-controller-preview-reducer.md) gives
journal-to-DOM p95 of 15.49 ms with 2.70 ms clock-alignment uncertainty.
The [DCS heartbeat soak](2026-10-29-profile-heartbeat-companion.md) and
[feedback publication checks](2026-11-05-feedback-publication.md) provide
separate owner and browser panel evidence. These runs cover different loads
and builds. The idle gateway trace does not measure owner continuity, panel
presentation, queue peaks, or capture-to-destination latency under concurrent
sources and stalled consumers. Physical input and output acceptance stays in
#199.

The panel trace adds 24 clock-aligned samples from authored ownship UDP to the real browser
canvas while a native telemetry reader withholds its acknowledgement. The
passing run measured 49.19 ms p95 with 0.18 ms alignment uncertainty. This
covers one loopback panel. The concurrent dual-rate, two-panel, UI and MCP
workload and full budget gate remain open.
[DCS panel canvas trace](2026-11-07-dcs-panel-vjoy.md#authored-udp-to-browser-canvas-timing)

The same-service browser-dev run adds two panels and ordered Windows vJoy
delivery while one telemetry reader holds its acknowledgement. Both panels
painted during the 190.21 ms dual-source overlap. The synthetic 1,000 Hz
source sustained 442.04 Hz, and the second panel exceeded the 150 ms p95
budget in the 24-change panel measurement. The combined budget stays open,
including same-service MCP and sustained load.
[Combined browser-dev trace](2026-11-07-dcs-panel-vjoy.md#two-browser-panels-and-dual-rate-vjoy-in-one-service)

The desktop trace adds two MCP readers to the same owner as DCS, both
panels, the Debug inspector and vJoy. It verifies live output changes and
retirement after MCP Stop while one MCP response is unread. This run uses
the 125 Hz authored lane and a real panel press. The full 125 Hz and 1 kHz
workload with MCP and the sustained load gate remain open.
[Desktop MCP trace](2026-11-07-dcs-panel-vjoy.md#desktop-mcp-during-dcs-paired-panels-and-vjoy)
