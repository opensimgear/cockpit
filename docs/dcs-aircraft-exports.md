# DCS aircraft mod exports

Games → DCS World → Integration setup installs optional aircraft hooks alongside
the checksum-pinned DCS-BIOS 0.11.7 runtime. Close DCS before running setup.
Expand **Aircraft mod export hooks** to see detection and installation status.
Launch DCS afterwards to generate the aircraft JSON catalogues and aliases.

In Profiles, add an aircraft tab to browse that aircraft's installed controls and
telemetry. Base exposes CommonData shared flight telemetry. The loaded aircraft
selects the live DCS-BIOS module automatically through the generated aliases;
browsing a tab does not change that selection or provider settings.

| Aircraft | Capability | Audited mod definitions |
| --- | --- | --- |
| MH-6J | CommonData flight telemetry, with its exact aircraft identity; no aircraft commands | MH-6J declared by the AH-6J mod |
| AH-6J | 127 clickable control channels and 82 unique gauge outputs, plus CommonData | AH-6J 1.0 cockpit definitions |
| OH-6A Cayuse | 61 clickable control channels and 91 unique gauge outputs, plus CommonData | OH-6A 1.7 cockpit definitions |

AH-6J covers electrical, fuel, lighting, armament/AMS, countermeasures, RWR,
ARC-210/182/186 radios, intercom, clock, TNL3100 and Argus controls. Cayuse covers
its electrical, engine/fuel, lighting, weapons, countermeasures, RWR,
ARC-51/54/83 radios and cockpit instrument controls. Every clickable channel and
unique main-panel gauge in the audited source is represented. Duplicate gauge
declarations with identical argument, parameter and range are exported once.
Rendered display textures and undocumented device internals are not inferred.

Gauge ports contain normalized cockpit argument values, not physical-unit
instrument readings. Buttons accept state 1 to hold and 0 to release, including
spring switches whose native press value is negative. Relative knobs accept
INC/DEC. Other axes and selectors use the normal DCS-BIOS control interfaces.

## Detection and ownership

Detection looks only in the selected Saved Games folder and selected DCS
installation's `Mods/aircraft` directories. Saved Games takes precedence. It
requires the exact `make_flyable` aircraft declaration in `entry.lua`; comments
and string examples do not count. AH-6J and Cayuse additionally require matching
SHA-256 fingerprints of `devices.lua`, `command_defs.lua`, `clickable_defs.lua`,
`clickabledata.lua` and `mainpanel_init.lua`. Unknown definitions are reported
as needing mapping review and do not receive a control hook. No mod Lua executes
during discovery or setup.

Setup writes a single owned `Scripts/OpenSimGear/DCSAircraft.lua` containing only
the applicable aircraft configurations, and a marked preloader at the start of
`Export.lua`. Existing bytes outside that block are preserved, including a BOM
and legacy comments. If no supported mods are detected, no aircraft hook or
preloader is installed. Running setup after mod removal removes the obsolete
owned configuration; the runtime also skips a mod whose entry file disappeared
between setup and DCS launch. Recheck and run setup after mod changes.

The official runtime files remain unchanged. The legacy bundled AH-6 module's
AH-6/BlackHawk aliases are retained under `AH-6-Legacy` in memory; the modern
AH-6J module uses its exact identity and a separate address range. Aircraft
exports share the existing DCS-BIOS state machine and transport.

Setup journals both previous Export.lua and aircraft hook bytes. Cancellation,
write failures and interrupted recovery restore only matching owned bytes.
Edited hooks, foreign scripts, and concurrent changes are preserved and require
review. A previous temporary `MH6CommonData` loader must be restored using its
original ownership receipt before installing the managed replacement.

## Validation and maintenance

The mapping data in `examples/dcs-bios/aircraft/*.json` records the audited source
hashes. `scripts/dcs-aircraft-extract.lua` extracts declarations from trusted mod
sources in an offline Lua 5.1 environment with cockpit stubs; it is a developer
tool and is never executed by setup. Supply MOD_ROOT, BIOS_ROOT and OUTPUT before
loading it; compare generated mappings and fingerprints when updating a mod.

Run the native filesystem and integration service tests:

```text
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib dcs_setup:: -- --test-threads=1
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib game_integration_setup::tests -- --test-threads=1
```

Exercise every mapped command and gauge through the actual pinned module API:

```text
python scripts/test-dcs-aircraft-exports.py --bios-source "<DCS Saved Games>" --dll "<DCS installation>/bin/lua.dll"
```

CI uses `--archive <checksum-pinned release zip> --lua lua5.1`. The driver verifies
the archive SHA-256 before extracting its module API into a temporary folder.
Alternatively use `--lua <Lua 5.1 executable>` with a folder containing
`Scripts/DCS-BIOS`. The harness isolates all writes in temporary folders and
starts no game, service, socket or device. It checks the absent, MH-6J-only,
AH-6J-only, Cayuse-only and combined configurations, exact device/command/value
dispatch, releases, generated gauge metadata, and preservation of legacy aliases.
These checks validate setup and the module API; simulator flight acceptance is
a separate check.
