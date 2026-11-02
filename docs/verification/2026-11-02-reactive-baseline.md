# Reactive architecture baseline, 2026-11-02

R0 #189 / parent #188 remain in progress. User-operated acceptance is #199,
placed first in Plan's In progress lane. This record is a baseline, not acceptance.

## Identity and workload

- Source HEAD: `8d84eb6824dcb7b6040b21fb99bcd13fe772649d`.
- Pre-existing dirty profile-editor work was present. The existing executable
  was started; this was not a rebuild of that dirty source tree.
- Executable: `src-tauri/target/debug/osg-cockpit.exe`.
- Executable SHA-256:
  `72037475f20915934429b649524b25f3c9854ae204a18bf29fb0ddc1b3ceab66`.
- Vite at `127.0.0.1:1420`; native WebView DevTools at `127.0.0.1:9222`.
- Started trace at `2026-11-02T12:04:18.352Z`; 60.177 s observation.
- Overview visible; no game, no active profile; previous run needs explicit
  recovery. No profile activation/recovery or physical/simulator input performed.

`node scripts/reactive/baseline.mjs` records actual `gateway_request` network
traffic and the visible view, source/executable identities and raw timestamps in
`.research/reactive/baseline.json`. It makes one explicit read-only `profile_status`
request to validate instrumentation. Do not count that request as periodic work.
An earlier attempted invoke wrapper was ineffective because Tauri's invoke
property is immutable. Its empty trace was discarded, not used as idle proof.

## Observed gateway traffic

| Command | Requests |
| --- | ---: |
| profile_activation_current | 401 |
| profile_status | 81 (one self-check) |
| game_get_state | 80 |
| profile_local_settings_get | 60 |
| profile_automatic_status | 60 |
| controller_list | 20 |
| module_usb_list | 20 |
| companion_inventory | 20 |
| ports | 20 |
| system_inventory | 20 |
| profile_resources_list | 20 |
| ui_preferences_save | 20 |

Thirteen further gateway URL network records had no decoded command and are
retained as `unparsed` in the raw trace. They are excluded from command counts;
the capture does not establish whether they were preflight/transport records.
This baseline demonstrates periodic application queries in an idle visible app,
including hidden activation progress. It establishes no capture/output latency.

## Source audit and qualification gaps

`node scripts/reactive/audit.mjs --check` confirms 233 timing sites across Cockpit,
device-gateway, device-template and firmware-common at the time of this record.
The generated inventory intentionally includes legitimate clocks. See
`../reactive/acquisition.md` and `../reactive/contract.md` for owners, replacement
boundaries, proposed budgets and safety invariants. No sampling exception has
been approved, and proposed budgets have not been confirmed on a representative
active rig.

Not measured: physical device cadence/range; capture-to-reducer/destination/UI
latency; actual DCS effects; panel feedback/presentation; owner/heartbeat health
under routing; overflow/load; unplug/replug; other platforms; 30-minute soak.
R0 needs conformance fixtures and representative active baseline evidence. R1–R8
implementation and integrated software R9 qualification remain outstanding.
