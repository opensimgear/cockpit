# DCS examples

## Feature Lab profile

The [WIN Companion panel](win-companion/README.md) provides minimal compiled
controls and feedback widgets sized for the desktop destination.

[feature-lab.profile.json](feature-lab.profile.json) restores the saved
**DCS Feature Lab - AH-6J + F/A-18C** demo (archived revision 11, reset to revision 1
for a fresh seed). It includes both aircraft tabs, controller processing, telemetry,
virtual and desktop outputs, companion routes and the saved canvas layout.
Debug builds include it in a fresh development database. Existing databases keep
their saved contents; use **Profiles → Import profile** to add this document.
Import creates a new profile identity without local hardware assignments or runtime
ownership. The referenced companion publication and DCS setup are not included.
The AH-6J tab demonstrates the original AH-6 catalogue; it does not qualify MH-6J
commands for the distinct aircraft mod described below.

## MH-6J read-only CommonData

[MH6CommonData.lua](MH6CommonData.lua) adds the exact **MH-6J** aircraft identity to the checksum-pinned DCS-BIOS **0.11.7** runtime. It activates the existing CommonData telemetry module and a self-named module with no controls. It provides no cockpit commands and does not change the aircraft name received by Cockpit.

The installed AH-6J/MH-6J mod inspected for this example registers `MH-6J` as its actual aircraft type. The pinned DCS-BIOS AH-6J module recognizes the older `AH-6` and `BlackHawk` names, and its cockpit device IDs and arguments differ from this mod. Do not alias MH-6J to that command module.

## Install separately from the official runtime

Close DCS before changing its export setup. Keep the original Export.lua bytes and SHA-256, record whether each new file existed, and preserve other export integrations. Review the current DCS-BIOS version and this exact aircraft identity before using the example. An updated runtime or mod needs a fresh check.

1. Copy the example to `<DCS Saved Games>/Scripts/OpenSimGear/MH6CommonData.lua`. Preserve an existing file instead of overwriting it.
2. Immediately before the single existing canonical DCS-BIOS loader in Export.lua, add:

   ```lua
   -- OpenSimGear MH-6J CommonData-only extension
   package.path = lfs.writedir() .. "?.lua;" .. package.path
   require("Scripts.OpenSimGear.MH6CommonData")
   ```

   Keep the original loader unchanged:

   ```lua
   dofile(lfs.writedir() .. [[Scripts\DCS-BIOS\BIOS.lua]])
   ```

Do not edit BIOS.lua, BIOSConfig.lua, AircraftList.lua, CommonData.lua, AH-6J.lua or the official release archive. The preloader must run before DCS-BIOS registers its modules; it refuses an existing MH-6J definition. Lua `require` prevents duplicate registration in the same runtime.

During ordinary DCS-BIOS initialization, the official writer generates an empty-object `doc/json/MH-6J.json` and the exact alias `["MH-6J", "CommonData"]` in AircraftAliases.json. It also attempts the reference file `doc/doc_assets/MH-6J.jsonp` when that normal reference directory exists. Do not create that directory just for this extension. Preserve the other aliases; do not copy a reduced test alias table into the installation. Record each generated file's presence/hash and the alias file's after hash for restoration.

In Cockpit's **Games → DCS details**, retain the actual Saved Games folder and catalogue connection, then select `MH-6J` as the aircraft module after its catalogue exists. Native capture still requires the actual DCS process, matching MH-6J metadata and advancing export counters. Missing or changed files remain a preparation error. The extension does not supply virtual-controller or hiding qualification.

## Route reported altitude to a browser panel

Select the native read-only source `channel:common:CommonData:ALT_MSL_FT:0` in **Profiles**. Its schema remains raw integer `0..65535` with no unit. Use **Declare quantity** to interpret it as *Reported MSL altitude*, `ft`, before a visible feet-to-metres conversion and any comparison, hysteresis or formatting. Connect that graph to captured browser companion feedback inputs. See [explicit quantities](../../docs/profile-quantities.md) and [browser pairing and feedback](../../docs/companion-guide.md).

The [pinned CommonData implementation](https://github.com/DCS-Skunkworks/dcs-bios/blob/30125bd4231f87769f46bacd6050f25e2d25d1f8/Scripts/DCS-BIOS/lib/modules/common_modules/CommonData.lua) computes `floor(altitude_metres / 0.304)`. Cockpit preserves those reported counts. They are MSL, not AGL; the stream has no measurement-validity bit. Continuing counters alone cannot prove a new altitude measurement when ownship export is denied or self data is absent.

## Restore only owned changes

With DCS closed, restore Export.lua only if its current bytes match the recorded edited version. Remove the preloader and generated MH-6J JSON/optional JSONP files only if their current hashes match the recorded owned files; preserve files that existed before installation. Restore AircraftAliases.json only if its current bytes still match the recorded generated version; otherwise preserve the external changes and review the conflict. Other modules' catalogue files, Addresses.h and the DCS-BIOS log are normal runtime outputs, outside this extension's restoration ownership. Keep backups and the ownership record until restoration is verified. A failed partial install uses the same comparisons.

## Evidence and limits

The local proof loads the actual installed DCS Lua 5.1 DLL and 35 pinned official DCS-BIOS modules, executes this registration mechanism and the official BIOSStateMachine, and records its real generated JSON and binary frames. DCS globals and the in-memory connection are fixtures. The proof has zero command processors and no game, network listener, service, controller, driver or physical I/O. All 109 installed Lua files, Export.lua and checked catalogue documents remain unchanged. Exact source/runtime hashes and raw evidence are retained privately under `.research/p9-mh6-commondata`.

The native `dcs_profile::common_tests` regressions check selected-aircraft admission, the exact CommonData port, absent command inputs and changed-alias rejection. The explicit `pinned_official_archive_commondata_normalization_and_native_guard` check repeats the MH-6J case against the complete official catalogue documents from `OSG_DCS_SETUP_ARCHIVE`, whose SHA-256 is `3272bc07210fa6c560f115cfb5dc8881accf7af5e7a5e218e2c46e1fd89e3292`. These are software proofs; real MH-6J execution and browser feedback remain separately observed qualification work.
