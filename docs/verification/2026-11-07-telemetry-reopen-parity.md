# Debug telemetry changes and reopen on both transports

`tests/ui/game-telemetry-observation-native.mjs` uses private game
installations and changes the name of the exact game selected in Debug.
The source summary and selector follow the visible commit without reopening
the stream. The fixture commits another name while the user is on a
different Debug tab, then a third while the document is hidden. Neither
hidden period issues telemetry inspection requests. Each reopen obtains a
new reader and shows the current committed name. The browser writer uses a
separate tab.

| Running transport | Visible write to current summary | Tab reopen | Document reopen | Opens and retirements |
| --- | ---: | ---: | ---: | ---: |
| Native WebView | 343.0 ms | 62.0 ms | 63.9 ms | 3 and 3 |
| Edge browser-dev | 203.6 ms | 58.9 ms | 62.7 ms | 3 and 3 |

The Windows desktop executable was SHA-256
`0be5a759a05f8f4475d7fb27bb6d4a92089bf57492f5dde82e47c0749a34b8dc`.
The freshly linked browser-dev executable was SHA-256
`2272c6b0035edb1470da3155b733c9b3a6bbccd14d996660a41b353fdba66242`.
Cargo linked that executable successfully. Windows denied replacement of the
existing top-level gateway executable, so the fixture launched the linked
artifact directly.

The authored DCS fixture changed the native airspeed from 210.25 to 241.5
m/s after Debug's telemetry tab retired its reader. An independent check
confirmed the new native capture before the tab reopened. The visible
inspector showed 241.5 m/s after 62.6 ms in the WebView and 62.2 ms in Edge.
Each transport recorded two distinct reader opens, two retirements and zero
hidden inspection queries. Both fixtures also passed DCS setup repair,
competing process withdrawal, pause, export denial, stale telemetry,
respawn and process exit, with no cleanup errors.

All 1,705 frontend and timer-guard tests passed. The telemetry controller
reached 100% lines, 89.26% branches and 92.86% functions. Logs are under
`.research/reactive/telemetry-reopen-parity/`. The authored capture evidence
is in `.research/dcs-physics-native/1794080022781/evidence.json` and
`.research/dcs-physics-native/1794080099226/evidence.json`. The other nested
surfaces in #196 and physical simulator qualification remain open.
