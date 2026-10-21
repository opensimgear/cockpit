# Game telemetry

Games publish read-only, typed telemetry through the same game integration resources used by Profiles. Fields do not need an active binding to appear in **Profiles → Debug → Incoming telemetry**. Select a game, category, or search term to inspect its current values and native field names. Profile inspection and runtime recovery have separate Debug tabs.

The inspector reports source state, session context, packet progression, and sample age. Unknown, paused, stale, unsupported, and closed-game readings are distinct from numeric zero, false, and empty text. Inspection never applies a profile or commands a device.

## Coverage

| Integration | Available surface |
| --- | --- |
| Assetto Corsa | Documented shared-memory physics, graphics, and static fields, including scalar components of vectors and wheel arrays. |
| Assetto Corsa Competizione | Its own documented physics, graphics, and static layout. SDK fields explicitly marked unused remain unavailable. |
| Assetto Corsa EVO | Its documented physics, nested graphics, and static layout, including forces, moments, wheel data, and vibration fields. |
| Assetto Corsa Rally | The previously qualified live prefix and maximum-RPM data. Unverified extended fields remain unavailable until a compatible layout can be established. |
| DCS-BIOS | Every installed export output for the freshly loaded aircraft and its authorized CommonData module, with the captured catalogue revision. Encoded gauge values retain their raw units and ranges. |
| DCS ownship | A managed 101-field selection of the installed DCS ownship export API: flight parameters, motion, attitude, environment, engines/fuel, position, identity, simulation state and mechanics. This supplements DCS-BIOS; it does not enumerate other objects or expose weapon targeting. |
| X-Plane 11 and 12 | Numeric scalar and array components described by the selected installation's `Resources/plugins/DataRefs.txt`. RREF requests cover the selected processing inputs and visible inspection page. |

Assetto layouts are version-qualified. Missing optional tails and invalid components become unknown independently; they do not invalidate unrelated valid fields. Native strings and boolean fields retain their types. Large integer identifiers use text where a JavaScript number could lose precision.

X-Plane RREF transports float32 values, including integer and double DataRefs. Its subscription sender follows the simulator's documented request pacing. Very large or unspecified arrays have explicit unavailable metadata; the bounded component catalogue currently supports arrays up to 8,192 elements and at most 65,536 published ports. Large airfoil lookup tables are not streamed. The execution engine retains its separate limit of 1,024 selected ports. DataRefs absent from the installed SDK, byte/data blocks unsupported by this numeric transport, and plugin-defined DataRefs absent from the installed list are not invented.

## DCS ownship export

Close DCS and run **Games → DCS World → Set up integration** or **Repair setup** to install the managed `Scripts/OpenSimGear/DCSPhysics.lua` extension in the chosen Saved Games folder. Older DCS-BIOS-only setups need Repair to add it. Setup preserves existing exporters and chains `LuaExportStart`, `LuaExportStop` and `LuaExportAfterNextFrame`; the extension reads ownship state and sends no simulator commands. Its source, loader and backups participate in the same digest-checked setup, recovery and removal transaction as the DCS-BIOS integration.

The exporter sends complete, bounded JSON packets at up to 20 Hz to `127.0.0.1:7781`. One native receiver owns that socket; Debug and profile inputs share its captures. Fresh DCS-BIOS aircraft identity, the verified game process, ownship export permission, a stable ownship identity and advancing model time qualify live values. Pause, active pause, permission loss, stop, stale packets and changed source identity withdraw physics values. The ownship receive lease is one second; this is a telemetry input contract, not a motor watchdog.

The current selection contains 101 native fields, before common-channel projections:

| Group | Fields | Examples and units |
| --- | ---: | --- |
| Flight parameters | 14 | True airspeed and vertical velocity in m/s; altitude in m; attack/sideslip angles in rad; Mach and deviation ratios. |
| Motion | 9 | Native x/y/z acceleration in g, world velocity in m/s and angular velocity in rad/s. |
| Environment | 4 | Native world wind vector in m/s and atmospheric pressure in mmHg. |
| Attitude | 7 | ADI pitch/bank/yaw, magnetic yaw and ownship heading/pitch/bank in rad. |
| Engines and fuel | 10 | Left/right speed percentage, temperature in °C, native hydraulic pressure in kg/cm², consumption in kg/s and internal/external fuel mass in kg. |
| Position | 6 | Geographic latitude/longitude in degrees, altitude in m and native DCS world coordinates in m. |
| Identity | 17 | Aircraft, unit/group/pilot names, ownship IDs, country/coalition and ownship flags. |
| Simulation state | 3 | Model time, mission start model time in seconds and ownship in-air state. |
| Mechanics | 31 | Gear/flaps/brake and other native positions/statuses, gear rods and relative control-surface values. |

These are the selected documented ownship getters, not a claim that every DCS export API is supported. Missing functions, nil returns, unsupported aircraft fields, invalid components and values outside the finite protocol bounds remain unknown independently. Numeric safety bounds are not aircraft operating limits. Mechanical status values whose enum is undocumented remain text. Indicated airspeed, shake and several native mechanical positions have no unit declared in the installed reference; their native values and IDs remain available with **Not specified**, rather than an invented conversion. Native axes retain their coordinate frame and signs.

The primary API reference is Eagle Dynamics' installed `<DCS installation>/Scripts/Export.lua`: ownship permissions and Boolean pause queries at lines 433–441, flight/engine getters at 449–487, ownship data at 615–647, mechanics at 782–797 and velocity/wind getters at 854–856 in the audited installation. `LoSimulationOnPause()` and `LoSimulationOnActivePause()` are documented Boolean status queries; the independent model-time guard also detects a frozen simulation. Field provenance is retained in the [typed catalogue](../src-tauri/src/dcs_physics_channels.json) and [read-only exporter](../examples/dcs-bios/DCSPhysics.lua).

## Shared channel names and units

The `opensimgear.telemetry` schema, version 1, adds common channels alongside native fields and legacy IDs. A common channel has the same meaning and unit in each adapter that provides it. Conversion happens before a value enters processing and uses the same projection in Debug.

| Channel | Unit | Meaning |
| --- | --- | --- |
| `engine.rpm` | rpm | Actual speed of the primary engine. |
| `engine.max_rpm` | rpm | Reported maximum engine speed. |
| `engine.rev_ratio` | ratio | Engine speed divided by reported maximum. |
| `engine.rpm_percent`, `engine.2.rpm_percent` | % | DCS left/right engine speed percentages; these do not supply `engine.rpm`. |
| `vehicle.speed_mps` | m/s | Ground speed; distinct from indicated airspeed. |
| `vehicle.g_force.magnitude` | g | Euclidean magnitude of the reported three-axis g vector. Native gravity conventions remain documented at the source. |
| `controls.throttle`, `controls.brake`, `controls.clutch` | ratio | Reported control application. |
| `transmission.gear` | — | Reverse −1, neutral 0, forward gears 1 and above. |
| `vehicle.fuel_l` | L | Reported fuel volume. |
| `vehicle.fuel_kg` | kg | Reported fuel mass; ACC is not converted to litres without density. |
| `aircraft.indicated_airspeed_mps` | m/s | Indicated airspeed. |
| `aircraft.true_airspeed_mps` | m/s | True airspeed from X-Plane's SDK or the documented DCS ownship getter. |
| `aircraft.altitude_msl_m` | m | Altitude above sea level; distinct from indicated altitude. |
| `aircraft.indicated_altitude_m` | m | Indicated altitude. |
| `aircraft.vertical_speed_mps` | m/s | Indicated vertical speed. |

Native names, types, units, source documentation, array indices, and qualification metadata remain in the provider catalogue. Canonical channels record their source channels and conversion. Existing bindings such as `rpm`, `speed_kmh`, and X-Plane's original channel IDs keep their previous contracts.

RPM percentage is a separate quantity from actual RPM. Signed axes are only converted when their frame and sign are known; a common label does not establish a shared frame. Raw DCS-BIOS encoded values require a documented physical interpretation before they can become unitful quantities.

## Processing and future force feedback

Select any advertised channel as a Profile input and connect it to processing. Freshness, game process identity, vehicle/session changes, and source epochs still qualify the sample. Pausing, exiting, stale packets, or source replacement withdraw live values rather than repeating a previous sample.

Physics acquisition provides inputs for future force-feedback processing. Motor torque delivery still needs a dedicated device backend, rate contract, calibrated units, limits, and a much shorter device watchdog. The inspection view and current profile freshness policy are not a real-time motor control loop.

Layout references: [Kunos EVO shared-memory guide](https://steamcommunity.com/sharedfiles/filedetails/?id=3707421508), [X-Plane DataRefs](https://developer.x-plane.com/datarefs/), [X-Plane SDK data access](https://developer.x-plane.com/sdk/XPLMDataAccess/), and [DCS-BIOS documentation](https://dcs-skunkworks.github.io/dcs-bios-docs/). The Assetto native schema tables also retain the source of each transcribed field.
