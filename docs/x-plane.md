# X-Plane 11 and 12 telemetry

Games has separate X-Plane 11 and X-Plane 12 installation/provider identities. The
`x-plane` provider uses read-only RREF UDP subscriptions, shared with both simulator
generations. Unified profiles select its exact telemetry ports, visible
processing and supported companion/physical actions. Native endpoint ownership
and stale/disconnected fallback rules remain required. Saving/importing a graph
or setting up a provider never activates outputs.

## Support and qualification

| Environment | Editing and importing | Native launch/telemetry |
| --- | --- | --- |
| Windows, X-Plane 11 | Yes | Implemented; live qualification pending |
| Windows, X-Plane 12 | Yes | Implemented; live qualification pending |
| Linux/macOS | Yes | Unavailable: native game process monitoring requires Windows |
| Remote simulator | Yes | Unavailable: local process identity is required |

The intended protocol baseline is X-Plane 11.00 and 12.00. These are **not claimed
as tested minimum builds**. No installed simulator, aircraft, physical peripheral
or Windows telemetry session has been qualified for this change. Authored fixtures
and a local UDP simulator exercise production parsing and transport independently
of native acquisition. Fixture reception never establishes native verification.
Live qualification for **each** simulator version remains an open acceptance item
in [issue #45](https://github.com/opensimgear/cockpit/issues/45).

RREF is used instead of relying on the newer
[Web API](https://developer.x-plane.com/article/x-plane-web-api/), which requires
X-Plane 12.1.1 or newer. The subscription wire format follows the simulator's
`Instructions/X-Plane SPECS from Austin/Sending Data to X-Plane` documentation;
[Laminar's support answer](https://questions.x-plane.com/6880/where-can-find-the-complete-protocol-specification-dataref)
identifies that installed specification. Dataref availability is checked against
the selected installation's `Resources/plugins/DataRefs.txt`; see the
[official dataref catalogue](https://developer.x-plane.com/datarefs/).

## Set up and play

1. Rescan Games or use **Browse…** to select the installation's `X-Plane.exe` in
   the native desktop file picker. Steam libraries and
   the user's `x-plane_install_11.txt`/`x-plane_install_12.txt` lists are searched.
   Multiple detected copies appear in Installation. Select the desired copy,
   then **Use this path**. The installation's DataRefs header must match the tile's
   major version. The selected path is displayed read-only; cancelling the picker
   keeps the current installation. Selection is validated before it is saved.
2. Choose **Set up telemetry**. Cockpit persists local RREF defaults beside its
   other game setup settings. Setup is repeatable, preserves previous overrides
   and changes no simulator files; no plugin, backup or restoration is needed.
   Recheck observes configuration readiness, independently of live data.
3. In that game's details, edit its provider connection settings. Defaults are loopback
   `127.0.0.1:49000`, an automatically allocated reply port, 10 Hz and 1500 ms
   freshness. Set the simulator's UDP receive port to match, allow local UDP in
   the firewall, and run only one X-Plane process. No Data Output checkbox is
   required for RREF subscriptions. Fixed reply ports must be free.
4. Open **Profiles** and select the matching X-Plane resource. Choose exact
   catalogue outputs and explicitly assigned companion/physical destination
   ports. Add visible conversion, conditions, formatting and invocation packing
   as required by their contracts. Inspect draft preflight, then **Save profile**.
5. Use Games **Launch** with the saved Manual pin or Automatic rule/default, or
   start the selected installation externally and Apply the saved profile with
   its matching verified context. Native code verifies process path, PID and
   creation time; the profile/rule never stores a durable PID. Enter a flight and
   leave pause/replay to establish a fresh baseline. Inspect the applied port and
   path samples in Profiles. Wrong/no game leaves the X-Plane branch dormant.
6. **Stop**, **Pause** or game-context loss withdraws affected output. Interrupted
   ownership uses explicit profile recovery; recovery does not replay a process,
   commands or old telemetry. Unrelated admitted hardware branches can continue
   after game-context loss.

Host, simulator port, reply port, update rate and freshness persist in the game's
provider preferences. Repeat setup preserves those choices. Switching
installations requires setup for the new copy and native revalidation of captured
ports/plans. Old DCS/ACR/X-Plane game/integration configurations are not imported
or migrated into the fresh unified profile library.

## Starter catalogue

| Channel | Type / unit | Dataref |
| --- | --- | --- |
| `airspeed_kts` | number / kt | `sim/flightmodel/position/indicated_airspeed` |
| `altitude_ft` | number / ft | `sim/cockpit2/gauges/indicators/altitude_ft_pilot` |
| `heading_deg` | number / deg magnetic | `sim/flightmodel/position/mag_psi` |
| `pitch_deg` | number / deg | `sim/flightmodel/position/theta` |
| `roll_deg` | number / deg | `sim/flightmodel/position/phi` |
| `vertical_speed_fpm` | number / ft/min | `sim/cockpit2/gauges/indicators/vvi_fpm_pilot` |
| `engine_1_rpm`, `engine_2_rpm` | number / rpm | `sim/cockpit2/engine/indicators/engine_speed_rpm[0]`, `[1]` |
| `gear_ratio` | number / 0–1 | `sim/flightmodel2/gear/deploy_ratio[0]` |
| `flaps_ratio` | number / 0–1 | `sim/cockpit2/controls/flap_ratio` |
| `gear_unsafe` | boolean | `sim/cockpit2/annunciators/gear_unsafe` |
| `stall_warning` | boolean | `sim/cockpit2/annunciators/stall_warning` |
| `low_fuel` | boolean | `sim/cockpit2/annunciators/fuel_quantity` |
| `landing_lights` | boolean switch | `sim/cockpit/electrical/landing_lights_on` |
| `beacon_lights` | boolean switch | `sim/cockpit/electrical/beacon_lights_on` |

Engine indices are zero based in datarefs, one based in channel names. Engine 2
stays unavailable on single-engine aircraft. Gear ratio reports the **first gear**,
not a synthesized all-gears-down indication. Annunciators follow aircraft support;
custom aircraft may require other datarefs. Switch channels describe switch state,
not electrical lamp illumination. These are stock datarefs, not aircraft-specific
guarantees. Absent catalogue entries, missing updates, invalid ranges and missing
engines remain explicitly unavailable rather than fabricated zeroes.

For a C172 graph, select `gear_unsafe` or indicated airspeed and the actual
destination contracts. A C172's fixed gear normally leaves gear-unsafe off; use
`stall_warning` in a safely controlled simulator scenario or choose retractable
gear when checking an indicator. Author the schema-2 graph through Profiles with
exact local assignments; retired integration documents are not importable
unified profiles.

## Freshness, bounds and troubleshooting

Subscriptions use random per-run integer indices. The connected UDP socket only
accepts the selected loopback address and simulator port. The monitored process
must retain its executable path/PID/creation time, and other X-Plane processes
are rejected. Local UDP has no cryptographic producer authentication; these checks
do not make it a security protocol.

The adapter limits packets to 4096 bytes, work to 64 datagrams per poll, rates to
1–20 Hz, socket receive buffering to a requested 64 KiB, and subscriptions to a
fixed 148 datarefs (including guard and aircraft-path bytes). Subscriptions renew
every two seconds; Stop sends zero-frequency unsubscriptions. Missing catalogued
optional channels are not subscribed. No unbounded application queue is retained.

The simulator major version, pause/replay state, engine count, advancing simulator
clock and a complete NUL-terminated aircraft-relative-path prefix gate telemetry.
Paths longer than 127 bytes fail closed. Each channel expires independently.
Aircraft changes, backwards/stalled clocks, stale guards, pause/replay and malformed
or non-finite packets clear pending values and require a new baseline. Subscription
indices rotate after resets so late prior-session packets cannot refill outputs.
RREF has no atomic multi-dataref frame or aircraft-change event; transitions are
recognized when their guard updates arrive. Qualification must include real
aircraft switches, pause/replay and restart on each simulator build.

If configured but waiting, check the selected installation/version, close other
X-Plane copies, confirm UDP receive port/firewall, start a flight and exit pause
and replay. A port-bind error means another listener owns the reply port; choose
0 or a free fixed port. Missing guard/catalogue errors require repairing the
installation. A disappeared optional channel drives its configured fallback.
After process exit/restart, a fresh native context and admission are required;
Automatic selection follows its configured rules. Old process identity is rejected.
